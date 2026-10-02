//! Every command the world applied, kept 48 hours: a snapshot plus the
//! commands after it replays any recent tick exactly.

use super::{DAY_MS, Result, Store, now_ms};

pub const COMMANDS_TTL_MS: i64 = 2 * DAY_MS;

/// One applied command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRecord {
    pub tick: u64,
    /// order within the tick
    pub seq: u32,
    /// who sent it: "brain", "admin", "import", …
    pub source: String,
    /// the command as JSON
    pub body: String,
}

impl Store {
    /// Stores the commands applied in one or more ticks, in one transaction.
    pub async fn record_commands(&self, world_id: i64, commands: Vec<CommandRecord>) -> Result<()> {
        if commands.is_empty() {
            return Ok(());
        }
        self.write(move |conn| {
            let tx = conn.transaction()?;
            {
                let mut stmt = tx.prepare_cached(
                    "INSERT INTO commands (world_id, tick, seq, source, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?;
                let now = now_ms();
                for c in &commands {
                    stmt.execute((world_id, c.tick as i64, i64::from(c.seq), &c.source, &c.body, now))?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Commands applied after tick `after` (exclusive), in the order they were applied.
    pub async fn commands_after(&self, world_id: i64, after: u64) -> Result<Vec<CommandRecord>> {
        self.read(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT tick, seq, source, body FROM commands WHERE world_id = ?1 AND tick > ?2 ORDER BY tick, seq, id",
            )?;
            let rows = stmt.query_map((world_id, after as i64), |r| {
                Ok(CommandRecord {
                    tick: r.get::<_, i64>(0)? as u64,
                    seq: r.get::<_, i64>(1)? as u32,
                    source: r.get(2)?,
                    body: r.get(3)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing;
    use super::*;

    #[tokio::test]
    async fn commands_replay_in_order() {
        let (store, _dir) = testing::store().await;
        let world = store.create_world("Vale", 1, true).await.unwrap();
        let cmd = |tick, seq, body: &str| CommandRecord { tick, seq, source: "brain".into(), body: body.into() };
        store.record_commands(world.id, vec![cmd(5, 1, "b"), cmd(5, 0, "a"), cmd(3, 0, "old")]).await.unwrap();
        store.record_commands(world.id, vec![cmd(6, 0, "c")]).await.unwrap();
        let after = store.commands_after(world.id, 3).await.unwrap();
        assert_eq!(after.iter().map(|c| c.body.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    }
}
