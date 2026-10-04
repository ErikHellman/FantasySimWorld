//! Housekeeping: retention pruning, the nightly backup, the database size.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::brain_log::{KEEP_FULL, LOG_TTL_MS};
use super::commands::COMMANDS_TTL_MS;
use super::credits::PURCHASES_TTL_MS;
use super::snapshots::{expired, metas};
use super::vectors::VECTORS_TTL_MS;
use super::{DAY_MS, Result, Store, StoreError, db, now_ms};

/// Refused sign-ins are kept this long.
pub const ATTEMPTS_TTL_MS: i64 = 90 * DAY_MS;
pub const BACKUP_DIR: &str = "backup";
pub const BACKUP_FILE: &str = "latest.db";
/// A backup older than this is replaced on the next maintenance run.
pub const BACKUP_EVERY: Duration = Duration::from_secs(23 * 3600);
const RUN_EVERY: Duration = Duration::from_secs(3600);
/// The first run waits a little so a crash loop does not prune or back up on every start.
const FIRST_RUN_AFTER: Duration = Duration::from_secs(600);

/// What one pruning run removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub snapshots: usize,
    pub commands: usize,
    /// brain log rows deleted (older than 30 days)
    pub brain_log_deleted: usize,
    /// brain log rows that lost their prompt and response (past the newest 5,000)
    pub brain_log_trimmed: usize,
    pub sessions: usize,
    pub attempts: usize,
    /// viewers' purchases older than 90 days
    pub purchases: usize,
    /// memory vectors unused for a week
    pub vectors: usize,
}

impl Store {
    /// Applies every retention rule as of `now_ms`, in one transaction, then
    /// returns free pages to the file system (`auto_vacuum = INCREMENTAL`).
    pub async fn prune(&self, now_ms: i64) -> Result<PruneReport> {
        self.write(move |conn| {
            let tx = conn.transaction()?;
            let mut report = PruneReport::default();
            {
                let mut delete = tx.prepare_cached("DELETE FROM snapshots WHERE id = ?1")?;
                for id in expired(&metas(&tx)?, now_ms) {
                    report.snapshots += delete.execute([id])?;
                }
            }
            report.commands = tx.execute("DELETE FROM commands WHERE created_at < ?1", [now_ms - COMMANDS_TTL_MS])?;
            report.brain_log_deleted =
                tx.execute("DELETE FROM brain_log WHERE created_at < ?1", [now_ms - LOG_TTL_MS])?;
            report.brain_log_trimmed = tx.execute(
                "UPDATE brain_log SET prompt = NULL, response = NULL
                 WHERE (prompt IS NOT NULL OR response IS NOT NULL)
                   AND id <= (SELECT id FROM brain_log ORDER BY id DESC LIMIT 1 OFFSET ?1)",
                [KEEP_FULL],
            )?;
            report.sessions = tx.execute("DELETE FROM sessions WHERE expires_at <= ?1", [now_ms])?;
            report.attempts = tx.execute("DELETE FROM auth_attempts WHERE at < ?1", [now_ms - ATTEMPTS_TTL_MS])?;
            report.purchases = tx.execute("DELETE FROM credit_purchases WHERE at < ?1", [now_ms - PURCHASES_TTL_MS])?;
            report.vectors = tx.execute("DELETE FROM memory_vectors WHERE used_ms < ?1", [now_ms - VECTORS_TTL_MS])?;
            tx.commit()?;
            conn.execute_batch("PRAGMA incremental_vacuum")?;
            Ok(report)
        })
        .await
    }

    /// Writes a consistent copy of the database to `<data_dir>/backup/latest.db`
    /// (`VACUUM INTO` a temporary file, then a rename), on a side connection so writes go on.
    pub async fn backup(&self) -> Result<PathBuf> {
        let path = self.path().to_path_buf();
        let dir = self.data_dir().join(BACKUP_DIR);
        tokio::task::spawn_blocking(move || -> Result<PathBuf> {
            std::fs::create_dir_all(&dir)?;
            let target = dir.join(BACKUP_FILE);
            let partial = dir.join(format!("{BACKUP_FILE}.partial"));
            let _ = std::fs::remove_file(&partial);
            let conn = db::open_side(&path)?;
            let partial_text = partial.to_str().ok_or(StoreError::Refused("backup path is not UTF-8"))?;
            conn.execute("VACUUM INTO ?1", [partial_text])?;
            drop(conn);
            std::fs::rename(&partial, &target)?;
            Ok(target)
        })
        .await
        .map_err(|_| StoreError::Closed)?
    }

    /// Bytes on disk: the database file plus its WAL.
    pub async fn db_bytes(&self) -> Result<u64> {
        let path = self.path().to_path_buf();
        tokio::task::spawn_blocking(move || {
            let size = |p: PathBuf| std::fs::metadata(p).map_or(0, |m| m.len());
            let mut wal = path.clone().into_os_string();
            wal.push("-wal");
            Ok(size(path) + size(wal.into()))
        })
        .await
        .map_err(|_| StoreError::Closed)?
    }

    /// Age of `backup/latest.db`, if there is one.
    pub fn backup_age(&self) -> Option<Duration> {
        let modified = std::fs::metadata(self.data_dir().join(BACKUP_DIR).join(BACKUP_FILE)).ok()?.modified().ok()?;
        SystemTime::now().duration_since(modified).ok()
    }
}

/// Runs pruning every hour, and the backup when the last one is about a day old.
/// Stops when `shutdown` is cancelled.
pub fn spawn(store: Store, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut wait = FIRST_RUN_AFTER;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                () = tokio::time::sleep(wait) => {}
            }
            wait = RUN_EVERY;
            match store.prune(now_ms()).await {
                Ok(report) => tracing::info!(event = "prune", ?report, "retention applied"),
                Err(error) => tracing::error!(%error, "pruning failed"),
            }
            if store.backup_age().is_none_or(|age| age >= BACKUP_EVERY) {
                match store.backup().await {
                    Ok(path) => tracing::info!(event = "backup", path = %path.display(), "database backed up"),
                    Err(error) => tracing::error!(%error, "backup failed"),
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::super::snapshots::{SnapshotReason, SnapshotWrite};
    use super::super::{BrainLogWrite, CommandRecord, testing};
    use super::*;
    use crate::api::wire::{LogStatus, RequestKind};

    #[tokio::test]
    async fn prune_applies_every_rule() {
        let (store, _dir) = testing::store().await;
        let world = store.create_world("Vale", 1, true).await.unwrap();
        let now = now_ms();
        let snap = |tick| SnapshotWrite {
            world_id: world.id,
            tick,
            game_day: 1,
            reason: SnapshotReason::Interval,
            save_version: 1,
            state: b"{}".to_vec(),
            brain: vec![],
        };
        // 1 keeper + 25 intervals on the same game day.
        for tick in 0..26 {
            store.write_snapshot_at(snap(tick), now).await.unwrap();
        }
        store
            .record_commands(world.id, vec![CommandRecord { tick: 1, seq: 0, source: "t".into(), body: "{}".into() }])
            .await
            .unwrap();
        store
            .insert_brain_log(BrainLogWrite {
                world_id: world.id,
                tick: 1,
                character: "c1".into(),
                kind: RequestKind::Decide,
                status: LogStatus::Ok,
                latency_ms: 1,
                prompt_tokens: 1,
                completion_tokens: 1,
                summary: String::new(),
                prompt: "p".into(),
                response: "r".into(),
                commands: vec![],
            })
            .await
            .unwrap();

        let report = store.prune(now).await.unwrap();
        assert_eq!(report.snapshots, 5);
        assert_eq!(report.commands, 0);
        // Three days on, the commands are gone; the log row still has its metadata.
        let later = store.prune(now + 3 * DAY_MS).await.unwrap();
        assert_eq!(later.commands, 1);
        assert_eq!(later.brain_log_deleted, 0);
        assert_eq!(store.prune(now + 31 * DAY_MS).await.unwrap().brain_log_deleted, 1);
        // The newest snapshot is still there to resume from.
        assert_eq!(store.load_latest_snapshot().await.unwrap().unwrap().tick, 25);
    }

    #[tokio::test]
    async fn backup_is_a_working_database() {
        let (store, dir) = testing::store().await;
        store.create_world("Vale", 9, true).await.unwrap();
        let path = store.backup().await.unwrap();
        assert_eq!(path, dir.path().join("backup/latest.db"));
        assert!(store.backup_age().unwrap() < Duration::from_secs(60));
        // Twice in a row works too (the target exists).
        store.backup().await.unwrap();
        let copy = rusqlite::Connection::open(&path).unwrap();
        let seed: i64 = copy.query_row("SELECT seed FROM worlds", [], |r| r.get(0)).unwrap();
        assert_eq!(seed, 9);
        assert!(store.db_bytes().await.unwrap() > 0);
    }
}
