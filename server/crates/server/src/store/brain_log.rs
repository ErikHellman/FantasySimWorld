//! The brain's decision log and its daily usage counters.
//!
//! Full prompts and responses (zstd-compressed) are kept for the newest
//! [`KEEP_FULL`] rows; the metadata for [`LOG_TTL_MS`]. Usage is counted per
//! UTC day and request kind, so the daily caps survive restarts.

use rusqlite::Connection;

use super::{DAY_MS, Result, Store, enum_parse, enum_text, now_ms, utc_day, zstd_compress, zstd_decompress};
use crate::api::wire::{BrainLogRecord, LlmUsage, LogStatus, RequestKind};

pub const KEEP_FULL: i64 = 5000;
pub const LOG_TTL_MS: i64 = 30 * DAY_MS;

/// The usage kind for embedding requests, which the language model's daily caps leave out.
pub const EMBED_USAGE: &str = "embed";

/// One answered (or failed) request.
#[derive(Clone, Debug)]
pub struct BrainLogWrite {
    pub world_id: i64,
    pub tick: u64,
    /// the character the request was about
    pub character: String,
    pub kind: RequestKind,
    pub status: LogStatus,
    pub latency_ms: u32,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub summary: String,
    pub prompt: String,
    pub response: String,
    /// one line per command the answer produced
    pub commands: Vec<String>,
}

/// One row of `brain_usage`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageRow {
    /// days since the epoch (UTC)
    pub day: i64,
    pub kind: String,
    pub requests: u64,
    pub failures: u64,
    pub tokens: u64,
}

fn text_blob(text: &str) -> Result<Vec<u8>> {
    zstd_compress(text.as_bytes())
}

fn blob_text(blob: Option<Vec<u8>>) -> Result<String> {
    match blob {
        None => Ok(String::new()),
        Some(bytes) => Ok(String::from_utf8_lossy(&zstd_decompress(&bytes, bytes.len() * 4)?).into_owned()),
    }
}

fn record(conn: &Connection, before: Option<u64>, limit: u32) -> Result<Vec<BrainLogRecord>> {
    let before = before.map_or(i64::MAX, |id| id.min(i64::MAX as u64) as i64);
    let mut stmt = conn.prepare(
        "SELECT id, tick, created_at, character_id, kind, status, latency_ms, prompt_tokens, completion_tokens,
                summary, prompt, response, commands
         FROM brain_log WHERE id < ?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map((before, i64::from(limit.clamp(1, 500))), |r| {
        Ok((
            [r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?],
            r.get::<_, String>(3)?,
            [r.get::<_, String>(4)?, r.get(5)?],
            [r.get::<_, i64>(6)?, r.get(7)?, r.get(8)?],
            r.get::<_, String>(9)?,
            [r.get::<_, Option<Vec<u8>>>(10)?, r.get(11)?],
            r.get::<_, String>(12)?,
        ))
    })?;
    rows.map(|row| {
        let (
            [id, tick, at],
            character,
            [kind, status],
            [latency, prompt_tokens, completion_tokens],
            summary,
            [prompt, response],
            commands,
        ) = row?;
        Ok(BrainLogRecord {
            id: id as u64,
            tick: tick as u64,
            at: at as u64,
            character,
            kind: enum_parse(&kind)?,
            status: enum_parse(&status)?,
            latency_ms: latency as u32,
            prompt_tokens: prompt_tokens as u32,
            completion_tokens: completion_tokens as u32,
            summary,
            prompt: blob_text(prompt)?,
            response: blob_text(response)?,
            commands: serde_json::from_str(&commands)?,
        })
    })
    .collect()
}

impl Store {
    /// Stores one log row; returns its id.
    pub async fn insert_brain_log(&self, log: BrainLogWrite) -> Result<u64> {
        let (prompt, response) = (text_blob(&log.prompt)?, text_blob(&log.response)?);
        self.write(move |conn| {
            conn.execute(
                "INSERT INTO brain_log (world_id, tick, created_at, character_id, kind, status, latency_ms, prompt_tokens,
                                        completion_tokens, summary, prompt, response, commands)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                rusqlite::params![
                    log.world_id,
                    log.tick as i64,
                    now_ms(),
                    log.character,
                    enum_text(&log.kind),
                    enum_text(&log.status),
                    log.latency_ms,
                    log.prompt_tokens,
                    log.completion_tokens,
                    log.summary,
                    prompt,
                    response,
                    serde_json::to_string(&log.commands)?,
                ],
            )?;
            Ok(conn.last_insert_rowid() as u64)
        })
        .await
    }

    /// Newest first, rows with an id below `before` (all when `None`), at most `limit` (≤ 500).
    /// Rows past the newest 5,000 come back with empty prompt and response.
    pub async fn brain_log(&self, before: Option<u64>, limit: u32) -> Result<Vec<BrainLogRecord>> {
        self.read(move |conn| record(conn, before, limit)).await
    }

    /// Adds to today's (UTC) counters for one request kind.
    pub async fn add_usage(&self, kind: &str, requests: u64, failures: u64, tokens: u64) -> Result<()> {
        let kind = kind.to_owned();
        self.write(move |conn| {
            conn.execute(
                "INSERT INTO brain_usage (day, kind, requests, failures, tokens) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (day, kind) DO UPDATE SET requests = requests + excluded.requests,
                     failures = failures + excluded.failures, tokens = tokens + excluded.tokens",
                (utc_day(now_ms()), kind, requests as i64, failures as i64, tokens as i64),
            )?;
            Ok(())
        })
        .await
    }

    /// Counters for the UTC days `from..=to`, by day then kind.
    pub async fn usage(&self, from_day: i64, to_day: i64) -> Result<Vec<UsageRow>> {
        self.read(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT day, kind, requests, failures, tokens FROM brain_usage WHERE day BETWEEN ?1 AND ?2 ORDER BY day, kind",
            )?;
            let rows = stmt.query_map((from_day, to_day), |r| {
                Ok(UsageRow {
                    day: r.get(0)?,
                    kind: r.get(1)?,
                    requests: r.get::<_, i64>(2)? as u64,
                    failures: r.get::<_, i64>(3)? as u64,
                    tokens: r.get::<_, i64>(4)? as u64,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    /// Today's (UTC) totals over all kinds; what the daily caps compare against.
    pub async fn usage_today(&self) -> Result<LlmUsage> {
        let today = utc_day(now_ms());
        let rows = self.usage(today, today).await?;
        let rows: Vec<UsageRow> = rows.into_iter().filter(|r| r.kind != EMBED_USAGE).collect();
        Ok(LlmUsage {
            requests_24h: rows.iter().map(|r| r.requests).sum::<u64>().min(u64::from(u32::MAX)) as u32,
            failures_24h: rows.iter().map(|r| r.failures).sum::<u64>().min(u64::from(u32::MAX)) as u32,
            tokens_24h: rows.iter().map(|r| r.tokens).sum(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing;
    use super::*;

    fn log(world_id: i64, n: u64) -> BrainLogWrite {
        BrainLogWrite {
            world_id,
            tick: n,
            character: "c1".into(),
            kind: RequestKind::PlanDay,
            status: LogStatus::Ok,
            latency_ms: 900,
            prompt_tokens: 1200,
            completion_tokens: 80,
            summary: format!("plan {n}"),
            prompt: "You are Mira, the smith.".repeat(20),
            response: "{\"plan\":[]}".into(),
            commands: vec!["plan c1".into()],
        }
    }

    #[tokio::test]
    async fn log_rows_round_trip_newest_first() {
        let (store, _dir) = testing::store().await;
        let world = store.create_world("Vale", 1, true).await.unwrap();
        let first = store.insert_brain_log(log(world.id, 1)).await.unwrap();
        let second = store.insert_brain_log(log(world.id, 2)).await.unwrap();
        let rows = store.brain_log(None, 10).await.unwrap();
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), [second, first]);
        assert_eq!(rows[0].kind, RequestKind::PlanDay);
        assert_eq!(rows[0].prompt, "You are Mira, the smith.".repeat(20));
        assert_eq!(rows[0].commands, ["plan c1"]);
        assert_eq!(store.brain_log(Some(second), 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn usage_accumulates_per_day_and_kind() {
        let (store, _dir) = testing::store().await;
        store.add_usage("decide", 1, 0, 300).await.unwrap();
        store.add_usage("decide", 1, 1, 200).await.unwrap();
        store.add_usage("plan_day", 1, 0, 1000).await.unwrap();
        let today = store.usage_today().await.unwrap();
        assert_eq!((today.requests_24h, today.failures_24h, today.tokens_24h), (3, 1, 1500));
        let day = utc_day(now_ms());
        let rows = store.usage(day, day).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, "decide");
        assert_eq!(rows[0].requests, 2);
    }

    #[tokio::test]
    async fn embedding_usage_is_kept_apart_from_the_models() {
        let (store, _dir) = testing::store().await;
        store.add_usage("decide", 2, 0, 100).await.unwrap();
        store.add_usage(EMBED_USAGE, 5, 1, 900).await.unwrap();
        let today = store.usage_today().await.unwrap();
        assert_eq!((today.requests_24h, today.failures_24h, today.tokens_24h), (2, 0, 100));
    }
}
