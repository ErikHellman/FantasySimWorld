//! Language-model settings (port of `LlmSettings`, its defaults and `parseHeaders`
//! from `src/persist/settings.ts`).
//!
//! The JSON shape matches the browser's (camelCase, every field optional and
//! filled from the defaults), except that the API key is not part of it: the
//! server stores the key on its own and never sends it to a client. A stored
//! browser blob that still has `apiKey` parses; the key is ignored.

use std::fmt;

use serde::{Deserialize, Serialize};

/// How tool calls travel: native tool calls, JSON-schema output, or learn which works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireProtocol {
    #[default]
    Auto,
    Tools,
    Json,
}

/// Whether a conversation is written in one request or one line per request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConversationMode {
    #[default]
    Auto,
    Batched,
    Turns,
}

/// Whether models that can "think" before answering should: off is much faster and enough for a villager.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningMode {
    #[default]
    Off,
    Low,
    Default,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LlmSettings {
    pub enabled: bool,
    /// OpenAI-compatible base URL, e.g. https://api.openai.com/v1
    pub base_url: String,
    pub model: String,
    /// requests in flight at once; 1 suits a local model
    pub concurrency: u32,
    pub protocol: WireProtocol,
    pub conversations: ConversationMode,
    pub reasoning: ReasoningMode,
    pub temperature: f64,
    pub max_tokens: u32,
    /// extra request headers, one "Name: value" per line
    pub headers: String,
    /// hard cap on requests per real minute (0 = no cap)
    pub requests_per_minute: u32,
    /// requests per UTC day, spread through the day (0 = no cap); new on the server
    pub max_requests_per_day: u64,
    /// prompt plus completion tokens per UTC day, spread through the day (0 = no cap); new on the server
    pub max_tokens_per_day: u64,
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            model: String::new(),
            concurrency: 2,
            protocol: WireProtocol::Auto,
            conversations: ConversationMode::Auto,
            reasoning: ReasoningMode::Off,
            temperature: 0.8,
            max_tokens: 600,
            headers: String::new(),
            requests_per_minute: 40,
            max_requests_per_day: 0,
            max_tokens_per_day: 0,
        }
    }
}

/// The endpoint's API key. Kept apart from [`LlmSettings`] and masked in `Debug`.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ApiKey(pub String);

impl ApiKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_empty() { "ApiKey(none)" } else { "ApiKey(***)" })
    }
}

/// Reads "Name: value" lines. Lines without a name or value are skipped; a
/// repeated name keeps its first position and its last value, like a JS object.
pub fn parse_headers(text: &str) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        let Some(index) = line.find(':') else { continue };
        if index == 0 {
            continue;
        }
        let name = js_trim(&line[..index]);
        let value = js_trim(&line[index + 1..]);
        if name.is_empty() || value.is_empty() {
            continue;
        }
        match headers.iter_mut().find(|(existing, _)| existing == name) {
            Some(entry) => entry.1 = value.to_string(),
            None => headers.push((name.to_string(), value.to_string())),
        }
    }
    headers
}

/// Whitespace as JavaScript's `String.prototype.trim` sees it.
pub(crate) fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// JavaScript's `trim`: unlike `str::trim` it removes U+FEFF and keeps U+0085.
pub(crate) fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_space)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_browser() {
        let settings: LlmSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, LlmSettings::default());
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["baseUrl"], "");
        assert_eq!(json["requestsPerMinute"], 40);
        assert_eq!(json["maxTokens"], 600);
        assert_eq!(json["temperature"], 0.8);
        assert_eq!(json["protocol"], "auto");
        assert_eq!(json["reasoning"], "off");
        assert_eq!(json["maxRequestsPerDay"], 0);
        assert!(json.get("apiKey").is_none());
    }

    #[test]
    fn reads_a_stored_browser_blob_and_drops_the_key() {
        let stored = r#"{"enabled":true,"baseUrl":"http://localhost:11434/v1","apiKey":"sk-x","model":"m",
            "concurrency":1,"protocol":"json","reasoning":"default","headers":"X-A: 1"}"#;
        let settings: LlmSettings = serde_json::from_str(stored).unwrap();
        assert!(settings.enabled);
        assert_eq!(settings.concurrency, 1);
        assert_eq!(settings.protocol, WireProtocol::Json);
        assert_eq!(settings.reasoning, ReasoningMode::Default);
        assert_eq!(settings.max_tokens, 600);
        assert!(!serde_json::to_string(&settings).unwrap().contains("sk-x"));
    }

    #[test]
    fn parses_header_lines() {
        let headers = parse_headers("X-Title: Sim\nbad line\n: no name\nEmpty:\n  A : b:c \nX-Title: Again\r");
        assert_eq!(headers, vec![("X-Title".into(), "Again".into()), ("A".into(), "b:c".into())]);
    }

    #[test]
    fn masks_the_key() {
        assert_eq!(format!("{:?}", ApiKey::new("sk-secret")), "ApiKey(***)");
    }
}
