//! The chronicle: the village's full history, append-only.
//!
//! Appends go through [`Store::append_chronicle`] (batched by the store thread);
//! pages are read newest-first by tick.

use rusqlite::Connection;

use super::{Result, Store, enum_parse, enum_text};
use crate::api::wire::{ChronicleEntry, ChroniclePage};

/// Most entries one page returns (a single tick with more is returned whole).
pub const MAX_PAGE: u32 = 500;

/// Writes a batch in one transaction (called by the store thread).
pub(crate) fn insert(conn: &mut Connection, entries: &[(i64, ChronicleEntry)]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt =
            tx.prepare_cached("INSERT INTO chronicle (world_id, tick, kind, text, ids) VALUES (?1, ?2, ?3, ?4, ?5)")?;
        for (world_id, entry) in entries {
            stmt.execute((
                world_id,
                entry.tick as i64,
                enum_text(&entry.kind),
                &entry.text,
                serde_json::to_string(&entry.ids)?,
            ))?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn rows(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<Vec<(i64, ChronicleEntry)>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    rows.map(|row| {
        let (id, tick, kind, text, ids) = row?;
        Ok((id, ChronicleEntry { tick: tick as u64, kind: enum_parse(&kind)?, text, ids: serde_json::from_str(&ids)? }))
    })
    .collect()
}

/// Entries older than `before` (all when `None`), at most `limit`, oldest first.
/// A page never splits a tick, so the next page is simply `before = entries[0].tick`.
pub(crate) fn page(conn: &Connection, world_id: i64, before: Option<u64>, limit: u32) -> Result<ChroniclePage> {
    let limit = limit.clamp(1, MAX_PAGE);
    let before = before.map_or(i64::MAX, |t| t.min(i64::MAX as u64) as i64);
    let mut got = rows(
        conn,
        "SELECT id, tick, kind, text, ids FROM chronicle WHERE world_id = ?1 AND tick < ?2
         ORDER BY tick DESC, id DESC LIMIT ?3",
        (world_id, before, i64::from(limit) + 1),
    )?;
    let mut more = got.len() > limit as usize;
    if more {
        let cut = got[limit as usize].1.tick;
        got.truncate(limit as usize);
        if got.last().is_some_and(|(_, e)| e.tick == cut) {
            // The boundary falls inside one tick: leave that tick for the next page,
            // unless it is all this page has, in which case return the tick whole.
            got.retain(|(_, e)| e.tick != cut);
            if got.is_empty() {
                got = rows(
                    conn,
                    "SELECT id, tick, kind, text, ids FROM chronicle WHERE world_id = ?1 AND tick = ?2 ORDER BY id DESC",
                    (world_id, cut as i64),
                )?;
                more = conn.query_row(
                    "SELECT EXISTS (SELECT 1 FROM chronicle WHERE world_id = ?1 AND tick < ?2)",
                    (world_id, cut as i64),
                    |r| r.get(0),
                )?;
            }
        }
    }
    got.reverse();
    Ok(ChroniclePage { entries: got.into_iter().map(|(_, e)| e).collect(), more })
}

impl Store {
    /// `GET /api/chronicle?before=&limit=`: entries older than tick `before`, oldest first.
    /// Entries appended in the last second may still be buffered (see [`Store::flush`]).
    pub async fn chronicle_page(&self, world_id: i64, before: Option<u64>, limit: u32) -> Result<ChroniclePage> {
        self.read(move |conn| page(conn, world_id, before, limit)).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing;
    use crate::api::wire::{ChronicleEntry, ChronicleKind};

    fn entry(tick: u64, text: &str) -> ChronicleEntry {
        ChronicleEntry { tick, kind: ChronicleKind::Death, text: text.into(), ids: vec!["c1".into(), "m2".into()] }
    }

    fn texts(page: &crate::api::wire::ChroniclePage) -> Vec<&str> {
        page.entries.iter().map(|e| e.text.as_str()).collect()
    }

    #[tokio::test]
    async fn pages_never_split_a_tick() {
        let (store, _dir) = testing::store().await;
        let world = store.create_world("Vale", 1, true).await.unwrap();
        let other = store.create_world("Other", 2, false).await.unwrap();
        let entries = vec![entry(1, "a"), entry(2, "b1"), entry(2, "b2"), entry(3, "c"), entry(4, "d")];
        store.append_chronicle(world.id, entries).await.unwrap();
        store.append_chronicle(other.id, vec![entry(5, "elsewhere")]).await.unwrap();
        store.flush().await.unwrap();

        let page = store.chronicle_page(world.id, None, 3).await.unwrap();
        // b1, b2, c, d would split tick 2; tick 2 waits for the next page.
        assert_eq!(texts(&page), ["c", "d"]);
        assert!(page.more);
        assert_eq!(page.entries[1].ids, ["c1", "m2"]);
        assert_eq!(page.entries[1].kind, ChronicleKind::Death);

        let page = store.chronicle_page(world.id, Some(3), 3).await.unwrap();
        assert_eq!(texts(&page), ["a", "b1", "b2"]);
        assert!(!page.more);

        // A tick bigger than the page comes whole.
        let page = store.chronicle_page(world.id, Some(3), 1).await.unwrap();
        assert_eq!(texts(&page), ["b1", "b2"]);
        assert!(page.more);
    }
}
