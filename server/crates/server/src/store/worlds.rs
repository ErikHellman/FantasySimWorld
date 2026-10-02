//! Worlds: one row per world ever started; exactly one is active.

use rusqlite::{Connection, OptionalExtension, Row};

use super::{Result, Store, StoreError, now_ms};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct World {
    pub id: i64,
    pub name: String,
    pub seed: u32,
    pub created_at: i64,
    pub active: bool,
}

const COLUMNS: &str = "id, name, seed, created_at, is_active";

fn from_row(row: &Row) -> rusqlite::Result<World> {
    Ok(World {
        id: row.get(0)?,
        name: row.get(1)?,
        seed: row.get::<_, i64>(2)? as u32,
        created_at: row.get(3)?,
        active: row.get::<_, i64>(4)? == 1,
    })
}

pub(crate) fn active(conn: &Connection) -> Result<Option<World>> {
    Ok(conn.query_row(&format!("SELECT {COLUMNS} FROM worlds WHERE is_active = 1"), [], from_row).optional()?)
}

pub(crate) fn get(conn: &Connection, id: i64) -> Result<World> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM worlds WHERE id = ?1"), [id], from_row)
        .optional()?
        .ok_or(StoreError::NotFound("world"))
}

impl Store {
    /// The world that runs now, if any.
    pub async fn active_world(&self) -> Result<Option<World>> {
        self.read(active).await
    }

    /// Adds a world; with `activate` it becomes the active one in the same transaction.
    pub async fn create_world(&self, name: &str, seed: u32, activate: bool) -> Result<World> {
        let name = name.to_owned();
        self.write(move |conn| {
            let tx = conn.transaction()?;
            if activate {
                tx.execute("UPDATE worlds SET is_active = 0 WHERE is_active = 1", [])?;
            }
            tx.execute(
                "INSERT INTO worlds (name, seed, created_at, is_active) VALUES (?1, ?2, ?3, ?4)",
                (&name, i64::from(seed), now_ms(), i64::from(activate)),
            )?;
            let world = get(&tx, tx.last_insert_rowid())?;
            tx.commit()?;
            Ok(world)
        })
        .await
    }

    /// Makes `id` the active world (and no other).
    pub async fn set_active_world(&self, id: i64) -> Result<()> {
        self.write(move |conn| {
            let tx = conn.transaction()?;
            tx.execute("UPDATE worlds SET is_active = 0 WHERE is_active = 1", [])?;
            if tx.execute("UPDATE worlds SET is_active = 1 WHERE id = ?1", [id])? != 1 {
                return Err(StoreError::NotFound("world"));
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    pub async fn worlds(&self) -> Result<Vec<World>> {
        self.read(|conn| {
            let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM worlds ORDER BY id"))?;
            let rows = stmt.query_map([], from_row)?.collect::<rusqlite::Result<_>>()?;
            Ok(rows)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing;
    use super::*;

    #[tokio::test]
    async fn switching_the_active_world() {
        let (store, _dir) = testing::store().await;
        assert_eq!(store.active_world().await.unwrap(), None);
        let first = store.create_world("First", 1, true).await.unwrap();
        let second = store.create_world("Second", u32::MAX, false).await.unwrap();
        assert_eq!(second.seed, u32::MAX);
        assert_eq!(store.active_world().await.unwrap().unwrap().id, first.id);
        store.set_active_world(second.id).await.unwrap();
        assert_eq!(store.active_world().await.unwrap().unwrap().id, second.id);
        let third = store.create_world("Third", 3, true).await.unwrap();
        assert!(third.active);
        let all = store.worlds().await.unwrap();
        assert_eq!(all.iter().filter(|w| w.active).count(), 1);
        assert!(matches!(store.set_active_world(999).await, Err(StoreError::NotFound(_))));
        // A failed switch leaves the active world as it was.
        assert_eq!(store.active_world().await.unwrap().unwrap().id, third.id);
    }
}
