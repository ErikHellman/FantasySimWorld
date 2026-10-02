//! Snapshots of the world: the sim state JSON plus a small brain sidecar,
//! zstd-compressed, in tiers with their own retention:
//!
//! | tier | what | kept |
//! |---|---|---|
//! | 0 | interval save (every 30 s) | newest 20 per world |
//! | 1 | save point (and shutdown, import, …) | 24 h |
//! | 2 | first save of each game day | 14 days |
//! | 3 | first tier-2 save after 00:00 UTC each real day | forever |
//!
//! The newest snapshot of each world is never pruned, whatever its tier.
//!
//! Body encoding [`ENCODING`]: zstd over `state_len: u32 LE`, the state bytes,
//! then the brain sidecar bytes.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use super::{DAY_MS, Result, Store, StoreError, World, now_ms, utc_day, worlds, zstd_compress, zstd_decompress};

pub const ENCODING: &str = "zstd+sidecar/1";
pub const KEEP_INTERVAL: usize = 20;
pub const SAVE_POINT_TTL_MS: i64 = DAY_MS;
pub const DAILY_TTL_MS: i64 = 14 * DAY_MS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Interval = 0,
    SavePoint = 1,
    Daily = 2,
    Keeper = 3,
}

impl Tier {
    fn from_i64(n: i64) -> Result<Self> {
        Ok(match n {
            0 => Self::Interval,
            1 => Self::SavePoint,
            2 => Self::Daily,
            3 => Self::Keeper,
            _ => return Err(StoreError::Corrupt(format!("snapshot tier {n}"))),
        })
    }
}

/// Why a snapshot was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotReason {
    /// the 30-second autosave
    Interval,
    /// after a `save_point` event
    SavePoint,
    Shutdown,
    NewWorld,
    Import,
    /// an admin asked for one
    Manual,
}

impl SnapshotReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interval => "interval",
            Self::SavePoint => "save_point",
            Self::Shutdown => "shutdown",
            Self::NewWorld => "new_world",
            Self::Import => "import",
            Self::Manual => "manual",
        }
    }
}

/// What the tier decision needs to know about a world's earlier snapshots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TierHistory {
    /// highest game day that already has a tier-2 or tier-3 snapshot
    pub last_daily_game_day: Option<u32>,
    /// when the newest tier-3 snapshot was taken (ms)
    pub last_keeper_at: Option<i64>,
}

/// Picks the tier of a new snapshot. Pure.
pub fn tier_for(reason: SnapshotReason, game_day: u32, now_ms: i64, history: TierHistory) -> Tier {
    let first_of_game_day = history.last_daily_game_day.is_none_or(|day| game_day > day);
    if first_of_game_day {
        let keeper_today = history.last_keeper_at.is_some_and(|at| utc_day(at) == utc_day(now_ms));
        return if keeper_today { Tier::Daily } else { Tier::Keeper };
    }
    match reason {
        SnapshotReason::Interval => Tier::Interval,
        _ => Tier::SavePoint,
    }
}

/// The columns retention looks at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotMeta {
    pub id: i64,
    pub world_id: i64,
    pub tier: Tier,
    pub created_at: i64,
}

/// Ids of the snapshots that retention removes at `now_ms`, ascending. Pure.
pub fn expired(snapshots: &[SnapshotMeta], now_ms: i64) -> Vec<i64> {
    let mut newest: HashMap<i64, i64> = HashMap::new();
    for s in snapshots {
        let id = newest.entry(s.world_id).or_insert(s.id);
        *id = (*id).max(s.id);
    }
    let mut intervals: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut out = Vec::new();
    for s in snapshots {
        if s.tier == Tier::Interval {
            // Sorted below; the world's newest id always lands among the kept ones.
            intervals.entry(s.world_id).or_default().push(s.id);
            continue;
        }
        if newest[&s.world_id] == s.id {
            continue;
        }
        match s.tier {
            Tier::SavePoint if now_ms - s.created_at > SAVE_POINT_TTL_MS => out.push(s.id),
            Tier::Daily if now_ms - s.created_at > DAILY_TTL_MS => out.push(s.id),
            _ => {}
        }
    }
    for mut ids in intervals.into_values() {
        ids.sort_unstable_by(|a, b| b.cmp(a));
        out.extend(ids.into_iter().skip(KEEP_INTERVAL));
    }
    out.sort_unstable();
    out
}

/// A snapshot to store.
#[derive(Clone, Debug)]
pub struct SnapshotWrite {
    pub world_id: i64,
    pub tick: u64,
    pub game_day: u32,
    pub reason: SnapshotReason,
    /// the sim's `SAVE_VERSION`
    pub save_version: u32,
    /// `SimState` as JSON
    pub state: Vec<u8>,
    /// brain state that is not in the sim state (may be empty)
    pub brain: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotSaved {
    pub id: i64,
    pub tier: Tier,
    pub size_raw: usize,
    pub size_stored: usize,
}

#[derive(Clone, Debug)]
pub struct LoadedSnapshot {
    pub id: i64,
    pub world: World,
    pub tick: u64,
    pub game_day: u32,
    pub reason: String,
    pub tier: Tier,
    pub save_version: u32,
    pub created_at: i64,
    pub state: Vec<u8>,
    pub brain: Vec<u8>,
}

fn encode(state: &[u8], brain: &[u8]) -> Result<(Vec<u8>, usize)> {
    let len = u32::try_from(state.len()).map_err(|_| StoreError::Refused("state larger than 4 GiB"))?;
    let mut raw = Vec::with_capacity(4 + state.len() + brain.len());
    raw.extend_from_slice(&len.to_le_bytes());
    raw.extend_from_slice(state);
    raw.extend_from_slice(brain);
    Ok((zstd_compress(&raw)?, raw.len()))
}

fn decode(encoding: &str, body: &[u8], size_raw: usize) -> Result<(Vec<u8>, Vec<u8>)> {
    if encoding != ENCODING {
        return Err(StoreError::Corrupt(format!("snapshot encoding {encoding:?}")));
    }
    let mut raw = zstd_decompress(body, size_raw)?;
    let Some((len, _)) = raw.split_first_chunk::<4>() else {
        return Err(StoreError::Corrupt("snapshot shorter than its header".into()));
    };
    let len = u32::from_le_bytes(*len) as usize;
    if raw.len() < 4 + len {
        return Err(StoreError::Corrupt("snapshot state is cut short".into()));
    }
    let brain = raw.split_off(4 + len);
    raw.drain(..4);
    Ok((raw, brain))
}

fn history(conn: &Connection, world_id: i64) -> Result<TierHistory> {
    Ok(conn.query_row(
        "SELECT (SELECT max(game_day) FROM snapshots WHERE world_id = ?1 AND tier >= 2),
                (SELECT max(created_at) FROM snapshots WHERE world_id = ?1 AND tier = 3)",
        [world_id],
        |r| {
            Ok(TierHistory {
                last_daily_game_day: r.get::<_, Option<i64>>(0)?.map(|d| d as u32),
                last_keeper_at: r.get(1)?,
            })
        },
    )?)
}

pub(crate) fn metas(conn: &Connection) -> Result<Vec<SnapshotMeta>> {
    let mut stmt = conn.prepare("SELECT id, world_id, tier, created_at FROM snapshots")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)?, r.get(3)?)))?;
    rows.map(|row| {
        let (id, world_id, tier, created_at) = row?;
        Ok(SnapshotMeta { id, world_id, tier: Tier::from_i64(tier)?, created_at })
    })
    .collect()
}

impl Store {
    /// Compresses and stores a snapshot; the tier follows from the world's history.
    pub async fn write_snapshot(&self, snapshot: SnapshotWrite) -> Result<SnapshotSaved> {
        self.write_snapshot_at(snapshot, now_ms()).await
    }

    /// [`Store::write_snapshot`] with an explicit wall clock (tests, imports).
    pub async fn write_snapshot_at(&self, snapshot: SnapshotWrite, at_ms: i64) -> Result<SnapshotSaved> {
        // Compress on the blocking pool so the store thread only does the insert.
        let (snapshot, body, size_raw) = tokio::task::spawn_blocking(move || -> Result<_> {
            let (body, size_raw) = encode(&snapshot.state, &snapshot.brain)?;
            Ok((snapshot, body, size_raw))
        })
        .await
        .map_err(|_| StoreError::Closed)??;
        self.write(move |conn| {
            let tx = conn.transaction()?;
            let tier = tier_for(snapshot.reason, snapshot.game_day, at_ms, history(&tx, snapshot.world_id)?);
            tx.execute(
                "INSERT INTO snapshots (world_id, tick, game_day, reason, tier, save_version, encoding, size_raw, body, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                (
                    snapshot.world_id,
                    snapshot.tick as i64,
                    i64::from(snapshot.game_day),
                    snapshot.reason.as_str(),
                    tier as i64,
                    i64::from(snapshot.save_version),
                    ENCODING,
                    size_raw as i64,
                    &body,
                    at_ms,
                ),
            )?;
            let id = tx.last_insert_rowid();
            tx.commit()?;
            Ok(SnapshotSaved { id, tier, size_raw, size_stored: body.len() })
        })
        .await
    }

    /// The newest snapshot of the active world, decompressed; `None` without an active world or snapshot.
    pub async fn load_latest_snapshot(&self) -> Result<Option<LoadedSnapshot>> {
        self.read(|conn| {
            let Some(world) = worlds::active(conn)? else { return Ok(None) };
            let id = world.id;
            load(conn, world, "WHERE world_id = ?1 ORDER BY id DESC LIMIT 1", id)
        })
        .await
    }

    /// One snapshot by id (any world).
    pub async fn load_snapshot(&self, id: i64) -> Result<Option<LoadedSnapshot>> {
        self.read(move |conn| {
            let Some(world_id) =
                conn.query_row("SELECT world_id FROM snapshots WHERE id = ?1", [id], |r| r.get(0)).optional()?
            else {
                return Ok(None);
            };
            load(conn, worlds::get(conn, world_id)?, "WHERE id = ?1", id)
        })
        .await
    }

    /// When the active world's newest snapshot was taken (ms), for the health report.
    pub async fn latest_snapshot_at(&self) -> Result<Option<i64>> {
        self.read(|conn| {
            Ok(conn.query_row(
                "SELECT max(s.created_at) FROM snapshots s JOIN worlds w ON w.id = s.world_id WHERE w.is_active = 1",
                [],
                |r| r.get(0),
            )?)
        })
        .await
    }
}

fn load(conn: &Connection, world: World, filter: &str, param: i64) -> Result<Option<LoadedSnapshot>> {
    let row = conn
        .query_row(
            &format!(
                "SELECT id, tick, game_day, reason, tier, save_version, created_at, encoding, size_raw, body FROM snapshots {filter}"
            ),
            [param],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, Vec<u8>>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((id, tick, game_day, reason, tier, save_version, created_at, encoding, size_raw, body)) = row else {
        return Ok(None);
    };
    let (state, brain) = decode(&encoding, &body, size_raw as usize)?;
    Ok(Some(LoadedSnapshot {
        id,
        world,
        tick: tick as u64,
        game_day: game_day as u32,
        reason,
        tier: Tier::from_i64(tier)?,
        save_version: save_version as u32,
        created_at,
        state,
        brain,
    }))
}

#[cfg(test)]
mod tests {
    use super::super::testing;
    use super::*;

    const HOUR: i64 = 3600 * 1000;
    /// Some day at 00:00 UTC.
    const MIDNIGHT: i64 = 20_000 * DAY_MS;

    #[test]
    fn first_save_ever_is_kept_forever() {
        let tier = tier_for(SnapshotReason::Interval, 1, MIDNIGHT, TierHistory::default());
        assert_eq!(tier, Tier::Keeper);
    }

    #[test]
    fn tiers_follow_game_and_real_days() {
        let seen = TierHistory { last_daily_game_day: Some(3), last_keeper_at: Some(MIDNIGHT + HOUR) };
        // Same game day: interval saves are tier 0, everything else tier 1.
        assert_eq!(tier_for(SnapshotReason::Interval, 3, MIDNIGHT + 2 * HOUR, seen), Tier::Interval);
        assert_eq!(tier_for(SnapshotReason::SavePoint, 3, MIDNIGHT + 2 * HOUR, seen), Tier::SavePoint);
        assert_eq!(tier_for(SnapshotReason::Shutdown, 3, MIDNIGHT + 2 * HOUR, seen), Tier::SavePoint);
        // A new game day on the same real day: tier 2 (there already is a keeper today).
        assert_eq!(tier_for(SnapshotReason::Interval, 4, MIDNIGHT + 2 * HOUR, seen), Tier::Daily);
        // The first new game day after 00:00 UTC: tier 3.
        assert_eq!(tier_for(SnapshotReason::Interval, 4, MIDNIGHT + DAY_MS, seen), Tier::Keeper);
        // A new real day alone does not promote a save within the same game day.
        assert_eq!(tier_for(SnapshotReason::Interval, 3, MIDNIGHT + DAY_MS, seen), Tier::Interval);
    }

    fn meta(id: i64, world_id: i64, tier: Tier, created_at: i64) -> SnapshotMeta {
        SnapshotMeta { id, world_id, tier, created_at }
    }

    #[test]
    fn retention_keeps_twenty_intervals_per_world() {
        let now = MIDNIGHT;
        let mut snapshots: Vec<_> = (1..=25).map(|id| meta(id, 1, Tier::Interval, now)).collect();
        snapshots.extend((26..=28).map(|id| meta(id, 2, Tier::Interval, now)));
        assert_eq!(expired(&snapshots, now), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn retention_by_age_and_tier() {
        let now = MIDNIGHT + 100 * DAY_MS;
        let snapshots = [
            meta(1, 1, Tier::Keeper, now - 90 * DAY_MS),
            meta(2, 1, Tier::Daily, now - 15 * DAY_MS),
            meta(3, 1, Tier::Daily, now - 13 * DAY_MS),
            meta(4, 1, Tier::SavePoint, now - 25 * HOUR),
            meta(5, 1, Tier::SavePoint, now - 23 * HOUR),
            meta(6, 1, Tier::Interval, now - 30 * DAY_MS),
            // another world, long stopped: its newest snapshot stays whatever its age
            meta(7, 2, Tier::SavePoint, now - 50 * DAY_MS),
            meta(8, 2, Tier::SavePoint, now - 40 * DAY_MS),
        ];
        assert_eq!(expired(&snapshots, now), vec![2, 4, 7]);
    }

    #[test]
    fn newest_interval_counts_towards_twenty() {
        let snapshots: Vec<_> = (1..=21).map(|id| meta(id, 1, Tier::Interval, 0)).collect();
        assert_eq!(expired(&snapshots, 0), vec![1]);
    }

    #[test]
    fn body_round_trips() {
        let (body, raw) = encode(b"{\"tick\":1}", b"brain").unwrap();
        assert_eq!(raw, 4 + 10 + 5);
        let (state, brain) = decode(ENCODING, &body, raw).unwrap();
        assert_eq!(state, b"{\"tick\":1}");
        assert_eq!(brain, b"brain");
        assert!(decode("gzip", &body, raw).is_err());
    }

    fn write(world_id: i64, tick: u64, game_day: u32, reason: SnapshotReason) -> SnapshotWrite {
        SnapshotWrite {
            world_id,
            tick,
            game_day,
            reason,
            save_version: 7,
            state: format!("{{\"tick\":{tick}}}").into_bytes(),
            brain: Vec::new(),
        }
    }

    #[tokio::test]
    async fn write_and_load_the_newest() {
        let (store, _dir) = testing::store().await;
        assert!(store.load_latest_snapshot().await.unwrap().is_none());
        let world = store.create_world("Vale", 42, true).await.unwrap();
        let first = store.write_snapshot_at(write(world.id, 10, 1, SnapshotReason::Interval), MIDNIGHT).await.unwrap();
        assert_eq!(first.tier, Tier::Keeper);
        let second =
            store.write_snapshot_at(write(world.id, 20, 1, SnapshotReason::Interval), MIDNIGHT + 1).await.unwrap();
        assert_eq!(second.tier, Tier::Interval);
        let third =
            store.write_snapshot_at(write(world.id, 30, 2, SnapshotReason::SavePoint), MIDNIGHT + 2).await.unwrap();
        assert_eq!(third.tier, Tier::Daily);

        let mut sidecar = write(world.id, 40, 2, SnapshotReason::Shutdown);
        sidecar.brain = br#"{"minds":{}}"#.to_vec();
        store.write_snapshot_at(sidecar, MIDNIGHT + 3).await.unwrap();
        let loaded = store.load_latest_snapshot().await.unwrap().unwrap();
        assert_eq!((loaded.tick, loaded.game_day, loaded.save_version), (40, 2, 7));
        assert_eq!(loaded.reason, "shutdown");
        assert_eq!(loaded.tier, Tier::SavePoint);
        assert_eq!(loaded.state, b"{\"tick\":40}");
        assert_eq!(loaded.brain, br#"{"minds":{}}"#);
        assert_eq!(loaded.world.id, world.id);

        // Another active world has no snapshot yet.
        store.create_world("New", 1, true).await.unwrap();
        assert!(store.load_latest_snapshot().await.unwrap().is_none());
        assert_eq!(store.load_snapshot(first.id).await.unwrap().unwrap().tick, 10);
    }
}
