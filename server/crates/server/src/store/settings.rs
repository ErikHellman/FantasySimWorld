//! Settings: JSON values by key.
//!
//! The language model is configured from the environment (see `Config::llm`), not
//! here; only what the client learns about an endpoint is kept.

use rusqlite::OptionalExtension;
use serde_json::Value;

use super::{Result, Store, now_ms};

/// `llm.capabilities`: what the client learned from HTTP 400s, so the probing does not repeat on each boot.
pub const LLM_CAPABILITIES: &str = "llm.capabilities";
/// `sim`: speed and other world-wide switches.
pub const SIM: &str = "sim";

impl Store {
    pub async fn setting(&self, key: &str) -> Result<Option<Value>> {
        let key = key.to_owned();
        self.read(move |conn| {
            let text: Option<String> =
                conn.query_row("SELECT value FROM settings WHERE key = ?1", [&key], |r| r.get(0)).optional()?;
            Ok(text.map(|t| serde_json::from_str(&t)).transpose()?)
        })
        .await
    }

    pub async fn put_setting(&self, key: &str, value: &Value) -> Result<()> {
        let (key, value) = (key.to_owned(), value.to_string());
        self.write(move |conn| {
            conn.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                (&key, &value, now_ms()),
            )?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing;
    use super::*;

    #[tokio::test]
    async fn settings_round_trip() {
        let (store, _dir) = testing::store().await;
        assert_eq!(store.setting(SIM).await.unwrap(), None);
        store.put_setting(SIM, &serde_json::json!({"speed": 2})).await.unwrap();
        store.put_setting(SIM, &serde_json::json!({"speed": 4})).await.unwrap();
        assert_eq!(store.setting(SIM).await.unwrap(), Some(serde_json::json!({"speed": 4})));
    }
}
