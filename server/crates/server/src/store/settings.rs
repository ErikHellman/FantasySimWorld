//! Settings: JSON values by key.
//!
//! The LLM API key lives under its own key and has its own accessors; the
//! generic ones refuse it, and [`ApiKey`] cannot be serialised, so it never
//! reaches a client by accident.

use rusqlite::OptionalExtension;
use serde_json::Value;

use super::{Result, Store, StoreError, now_ms};

/// `llm`: the LLM settings without the key.
pub const LLM: &str = "llm";
/// `llm.api_key`: the key alone (see [`Store::api_key`]).
pub const LLM_API_KEY: &str = "llm.api_key";
/// `llm.capabilities`: what the client learned from HTTP 400s, so the probing does not repeat on each boot.
pub const LLM_CAPABILITIES: &str = "llm.capabilities";
/// `sim`: speed and other world-wide switches.
pub const SIM: &str = "sim";

/// The LLM API key. No `Serialize`, and `Debug` shows only the hint.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The last four characters, e.g. `…x7Qa`, for the admin panel.
    pub fn hint(&self) -> String {
        let tail: String = self.0.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        format!("…{tail}")
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ApiKey({})", self.hint())
    }
}

fn refuse_key(key: &str) -> Result<()> {
    if key == LLM_API_KEY { Err(StoreError::Refused("the API key has its own accessors")) } else { Ok(()) }
}

impl Store {
    pub async fn setting(&self, key: &str) -> Result<Option<Value>> {
        refuse_key(key)?;
        let key = key.to_owned();
        self.read(move |conn| {
            let text: Option<String> =
                conn.query_row("SELECT value FROM settings WHERE key = ?1", [&key], |r| r.get(0)).optional()?;
            Ok(text.map(|t| serde_json::from_str(&t)).transpose()?)
        })
        .await
    }

    pub async fn put_setting(&self, key: &str, value: &Value) -> Result<()> {
        refuse_key(key)?;
        self.put_raw(key.to_owned(), Some(value.to_string())).await
    }

    pub async fn api_key(&self) -> Result<Option<ApiKey>> {
        self.read(|conn| {
            let text: Option<String> =
                conn.query_row("SELECT value FROM settings WHERE key = ?1", [LLM_API_KEY], |r| r.get(0)).optional()?;
            Ok(text.map(|t| serde_json::from_str::<String>(&t)).transpose()?.filter(|k| !k.is_empty()).map(ApiKey))
        })
        .await
    }

    /// Stores the key; `None` or an empty string removes it.
    pub async fn set_api_key(&self, key: Option<&str>) -> Result<()> {
        let value = key.filter(|k| !k.is_empty()).map(|k| Value::String(k.to_owned()).to_string());
        self.put_raw(LLM_API_KEY.to_owned(), value).await
    }

    async fn put_raw(&self, key: String, value: Option<String>) -> Result<()> {
        self.write(move |conn| {
            match value {
                Some(value) => conn.execute(
                    "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    (&key, &value, now_ms()),
                )?,
                None => conn.execute("DELETE FROM settings WHERE key = ?1", [&key])?,
            };
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
    async fn settings_round_trip_and_the_key_stays_apart() {
        let (store, _dir) = testing::store().await;
        assert_eq!(store.setting(SIM).await.unwrap(), None);
        store.put_setting(SIM, &serde_json::json!({"speed": 2})).await.unwrap();
        store.put_setting(SIM, &serde_json::json!({"speed": 4})).await.unwrap();
        assert_eq!(store.setting(SIM).await.unwrap(), Some(serde_json::json!({"speed": 4})));

        assert!(matches!(store.setting(LLM_API_KEY).await, Err(StoreError::Refused(_))));
        assert!(store.put_setting(LLM_API_KEY, &Value::Null).await.is_err());

        assert_eq!(store.api_key().await.unwrap(), None);
        store.set_api_key(Some("sk-secret-x7Qa")).await.unwrap();
        let key = store.api_key().await.unwrap().unwrap();
        assert_eq!(key.expose(), "sk-secret-x7Qa");
        assert_eq!(key.hint(), "…x7Qa");
        assert!(!format!("{key:?}").contains("secret"));
        store.set_api_key(Some("")).await.unwrap();
        assert_eq!(store.api_key().await.unwrap(), None);
    }
}
