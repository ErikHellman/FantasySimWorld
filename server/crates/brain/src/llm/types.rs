//! Requests, replies and errors of the language-model client (port of `src/brain/llm/types.ts`).

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: content.into() }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into() }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: content.into() }
    }
}

/// A tool the model may call. `parameters` is a JSON Schema object
/// (`{"type": "object", "properties": {...}, "required": [...]}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolDef {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self { name: name.into(), description: description.into(), parameters }
    }

    /// Whether the schema names this property.
    pub fn has_property(&self, key: &str) -> bool {
        self.parameters.get("properties").and_then(Value::as_object).is_some_and(|props| props.contains_key(key))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub args: Map<String, Value>,
}

impl ToolCall {
    pub fn new(name: impl Into<String>, args: Value) -> Self {
        let args = match args {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        Self { name: name.into(), args }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LlmRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    /// most tool calls that will be used from one response
    pub max_calls: usize,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    /// cancelling it ends the request with [`LlmErrorKind::Aborted`]
    pub cancel: Option<CancellationToken>,
    pub timeout: Option<Duration>,
}

/// Which wire protocol carried the tool calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tools,
    Json,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    pub calls: Vec<ToolCall>,
    /// assistant text with any reasoning blocks removed
    pub text: String,
    /// the message as received, for the decision log
    pub raw: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// prompt plus completion tokens over every attempt this request took (for the daily cap)
    pub tokens_used: u64,
    pub latency: Duration,
    pub protocol: Protocol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmErrorKind {
    Network,
    Http,
    Timeout,
    Aborted,
    NoCall,
    Truncated,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmError {
    pub kind: LlmErrorKind,
    pub message: String,
    pub status: Option<u16>,
    pub retry_after: Option<Duration>,
    pub raw: Option<String>,
    /// tokens the failed request consumed anyway (for the daily cap)
    pub tokens_used: u64,
}

impl LlmError {
    pub fn new(kind: LlmErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), status: None, retry_after: None, raw: None, tokens_used: 0 }
    }

    pub fn with_raw(mut self, raw: impl Into<String>) -> Self {
        self.raw = Some(raw.into());
        self
    }
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LlmError {}
