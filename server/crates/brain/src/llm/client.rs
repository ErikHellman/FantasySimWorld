//! Talks to an OpenAI-compatible `/chat/completions` endpoint (port of
//! `src/brain/llm/client.ts`).
//!
//! Tool calling is the contract; it is carried either as native tool calls
//! or, for servers where that is unreliable, as JSON-schema-constrained
//! output. The client adapts to each server's quirks as it meets them, and
//! what it learned can be exported and restored so a restart does not repeat
//! the dance.
//!
//! Intentional differences from the TypeScript:
//! - one timeout bounds the whole call, reading the body included (the browser's
//!   abort only covered the fetch);
//! - a reply is parsed with the protocol its request was sent with, not whatever
//!   a concurrent request switched the client to meanwhile;
//! - a 2xx body that is not JSON is an `http` error carrying the status and body
//!   (the browser threw a bare `SyntaxError`);
//! - `list_models` has the 45 s timeout too, and its failures are `LlmError`s;
//! - JSON object keys (tool schemas, bodies) are serialised sorted by key.

use std::future::pending;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::parse::{calls_from_message, calls_from_text, strip_thinking};
use super::transport::{HttpRequest, LlmTransport, Method};
use super::types::{LlmError, LlmErrorKind, LlmRequest, LlmResponse, Protocol, ToolCall, ToolDef};
use crate::settings::{ApiKey, LlmSettings, ReasoningMode, WireProtocol, parse_headers};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);
/// The first request may wait for a local model to load.
pub const WARMUP_TIMEOUT: Duration = Duration::from_secs(150);
/// Retries within one call, for adaptations and truncation.
const MAX_ADAPTATIONS: u32 = 4;
const MAX_TOKEN_SCALE: f64 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MaxTokensField {
    #[default]
    #[serde(rename = "max_tokens")]
    MaxTokens,
    #[serde(rename = "max_completion_tokens")]
    MaxCompletionTokens,
}

impl MaxTokensField {
    fn key(self) -> &'static str {
        match self {
            Self::MaxTokens => "max_tokens",
            Self::MaxCompletionTokens => "max_completion_tokens",
        }
    }
}

/// What we have learned about this endpoint's quirks; each starts optimistic.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Capabilities {
    pub protocol: Protocol,
    pub tool_choice: bool,
    pub max_tokens_field: MaxTokensField,
    pub temperature: bool,
    pub json_schema: bool,
    /// the server accepts reasoning_effort
    pub reasoning_effort: bool,
    /// consecutive replies in the tools protocol that contained no usable call
    pub tool_misses: u32,
    /// multiplier on the output token limit, raised when answers get cut off
    pub token_scale: f64,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            protocol: Protocol::Tools,
            tool_choice: true,
            max_tokens_field: MaxTokensField::MaxTokens,
            temperature: true,
            json_schema: true,
            reasoning_effort: true,
            tool_misses: 0,
            token_scale: 1.0,
        }
    }
}

/// Capabilities tagged with the endpoint and model they were learned from, for the settings table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedCapabilities {
    pub base_url: String,
    pub model: String,
    pub capabilities: Capabilities,
}

fn trim_slash(url: &str) -> &str {
    url.trim_end_matches('/')
}

/// JSON Schema asking for {"calls": [{"tool": ..., "args": {...}}]} with one branch per tool.
fn calls_schema(tools: &[ToolDef], max_calls: usize) -> Value {
    let branches: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({
                "type": "object",
                "properties": { "tool": { "type": "string", "enum": [tool.name] }, "args": tool.parameters },
                "required": ["tool", "args"],
                "additionalProperties": false,
            })
        })
        .collect();
    json!({
        "type": "object",
        "properties": {
            "calls": { "type": "array", "minItems": 1, "maxItems": max_calls, "items": { "anyOf": branches } },
        },
        "required": ["calls"],
        "additionalProperties": false,
    })
}

fn describe_tools(tools: &[ToolDef], max_calls: usize) -> String {
    let mut lines = vec![
        "Reply with JSON only, no prose, in exactly this shape:".to_string(),
        format!(
            "{{\"calls\": [{{\"tool\": \"<tool name>\", \"args\": {{ ... }}}}]}}  (1 to {max_calls} calls, in the order they should happen)"
        ),
        "Tools:".to_string(),
    ];
    for tool in tools {
        lines.push(format!("- {}: {} Arguments (JSON Schema): {}", tool.name, tool.description, tool.parameters));
    }
    lines.join("\n")
}

/// The last `count` characters of a text.
fn tail(text: &str, count: usize) -> &str {
    if count == 0 {
        return "";
    }
    text.char_indices().rev().nth(count - 1).map_or(text, |(index, _)| &text[index..])
}

fn token_count(usage: Option<&Value>, key: &str) -> u64 {
    usage.and_then(|usage| usage.get(key)).and_then(Value::as_f64).filter(|n| *n > 0.0).map_or(0, |n| n.round() as u64)
}

/// Seconds in a Retry-After header, read like JavaScript's `Number()`.
fn retry_after(value: Option<&str>) -> Option<Duration> {
    let text = value?.trim();
    let seconds: f64 = if text.is_empty() { 0.0 } else { text.parse().ok()? };
    (seconds.is_finite() && seconds > 0.0).then(|| Duration::from_secs_f64(seconds.min(1e9)))
}

async fn cancelled(token: Option<&CancellationToken>) {
    match token {
        Some(token) => token.cancelled().await,
        None => pending().await,
    }
}

/// An OpenAI-compatible chat client that learns each server's quirks. Shared
/// by every job in flight; the learned state sits behind a short-lived lock.
pub struct LlmClient {
    settings: LlmSettings,
    api_key: ApiKey,
    transport: Arc<dyn LlmTransport>,
    capabilities: Mutex<Capabilities>,
    warmed_up: AtomicBool,
}

impl LlmClient {
    pub fn new(settings: LlmSettings, api_key: ApiKey, transport: Arc<dyn LlmTransport>) -> Self {
        let protocol = if settings.protocol == WireProtocol::Json { Protocol::Json } else { Protocol::Tools };
        Self {
            settings,
            api_key,
            transport,
            capabilities: Mutex::new(Capabilities { protocol, ..Capabilities::default() }),
            warmed_up: AtomicBool::new(false),
        }
    }

    pub fn settings(&self) -> &LlmSettings {
        &self.settings
    }

    fn caps(&self) -> MutexGuard<'_, Capabilities> {
        self.capabilities.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn protocol(&self) -> Protocol {
        self.caps().protocol
    }

    pub fn capabilities(&self) -> Capabilities {
        *self.caps()
    }

    /// What this client has learned, to store with the settings.
    pub fn learned(&self) -> LearnedCapabilities {
        LearnedCapabilities {
            base_url: self.settings.base_url.clone(),
            model: self.settings.model.clone(),
            capabilities: self.capabilities(),
        }
    }

    /// Restores what an earlier client learned. Ignored (returns false) when it
    /// was learned from another endpoint or model; a protocol forced in the
    /// settings wins over a learned one.
    pub fn restore(&self, learned: &LearnedCapabilities) -> bool {
        if learned.base_url != self.settings.base_url || learned.model != self.settings.model {
            return false;
        }
        let mut restored = learned.capabilities;
        restored.tool_misses = 0;
        restored.token_scale = restored.token_scale.clamp(1.0, MAX_TOKEN_SCALE);
        restored.protocol = match self.settings.protocol {
            WireProtocol::Auto => restored.protocol,
            WireProtocol::Tools => Protocol::Tools,
            WireProtocol::Json => Protocol::Json,
        };
        *self.caps() = restored;
        true
    }

    fn headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        let mut set = |name: String, value: String| match headers.iter_mut().find(|(key, _)| *key == name) {
            Some(entry) => entry.1 = value,
            None => headers.push((name, value)),
        };
        for (name, value) in parse_headers(&self.settings.headers) {
            set(name, value);
        }
        if !self.api_key.is_empty() {
            set("Authorization".to_string(), format!("Bearer {}", self.api_key.expose()));
        }
        headers
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", trim_slash(&self.settings.base_url))
    }

    /// The model ids the server offers, sorted.
    pub async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        let request =
            HttpRequest { method: Method::Get, url: self.url("/models"), headers: self.headers(), body: None };
        let response = tokio::time::timeout(DEFAULT_TIMEOUT, self.transport.send(request))
            .await
            .map_err(|_| LlmError::new(LlmErrorKind::Timeout, "No model list within 45 s."))?
            .map_err(|reason| self.network_error(&reason))?;
        if !response.ok() {
            let mut error = LlmError::new(
                LlmErrorKind::Http,
                format!("The server answered {} to the model list request.", response.status),
            );
            error.status = Some(response.status);
            return Err(error);
        }
        let body: Value = serde_json::from_str(&response.body).unwrap_or(Value::Null);
        let entries = [body.get("data"), body.get("models")].into_iter().flatten().find(|value| !value.is_null());
        let mut ids: Vec<String> = entries
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let id = [entry.get("id"), entry.get("name")].into_iter().flatten().find(|value| !value.is_null());
                id.and_then(Value::as_str).filter(|id| !id.is_empty()).map(str::to_string)
            })
            .collect();
        ids.sort();
        Ok(ids)
    }

    fn body(&self, request: &LlmRequest, caps: &Capabilities) -> Value {
        let settings = &self.settings;
        let mut body = Map::new();
        body.insert("model".into(), json!(settings.model));
        body.insert("stream".into(), json!(false));
        let limit = (f64::from(request.max_tokens.unwrap_or(settings.max_tokens)) * caps.token_scale).round();
        body.insert(caps.max_tokens_field.key().into(), json!(limit as u64));
        if caps.temperature {
            body.insert("temperature".into(), json!(request.temperature.unwrap_or(settings.temperature)));
        }
        // Models that reason before answering are asked not to: a villager's next step does not need it.
        if caps.reasoning_effort && settings.reasoning != ReasoningMode::Default {
            let effort = if settings.reasoning == ReasoningMode::Off { "none" } else { "low" };
            body.insert("reasoning_effort".into(), json!(effort));
        }

        if caps.protocol == Protocol::Tools {
            body.insert("messages".into(), json!(request.messages));
            let tools: Vec<Value> = request
                .tools
                .iter()
                .map(|tool| {
                    json!({ "type": "function", "function": {
                        "name": tool.name, "description": tool.description, "parameters": tool.parameters,
                    } })
                })
                .collect();
            body.insert("tools".into(), Value::Array(tools));
            if caps.tool_choice {
                body.insert("tool_choice".into(), json!("required"));
            }
            return Value::Object(body);
        }

        // JSON protocol: the tools are described in the prompt and the output is constrained to calls.
        // The description goes in the system message, ahead of anything that varies, so servers can cache it.
        let mut messages = request.messages.clone();
        let instruction = describe_tools(&request.tools, request.max_calls);
        match messages.iter_mut().find(|message| message.role == super::types::Role::System) {
            Some(system) => system.content = format!("{}\n\n{instruction}", system.content),
            None => messages.insert(0, super::types::ChatMessage::system(instruction)),
        }
        body.insert("messages".into(), json!(messages));
        let format = if caps.json_schema {
            json!({ "type": "json_schema", "json_schema": {
                "name": "tool_calls", "strict": true, "schema": calls_schema(&request.tools, request.max_calls),
            } })
        } else {
            json!({ "type": "json_object" })
        };
        body.insert("response_format".into(), format);
        Value::Object(body)
    }

    /// Learns from a 400: which parameter did the server not like? Returns true if a retry is worthwhile.
    fn adapt(&self, error_text: &str) -> bool {
        let settings = &self.settings;
        let text = error_text.to_lowercase();
        let mut caps = self.caps();
        if caps.tool_choice && text.contains("tool_choice") {
            caps.tool_choice = false;
            return true;
        }
        if caps.max_tokens_field == MaxTokensField::MaxTokens
            && text.contains("max_tokens")
            && text.contains("max_completion_tokens")
        {
            caps.max_tokens_field = MaxTokensField::MaxCompletionTokens;
            return true;
        }
        if caps.reasoning_effort
            && settings.reasoning != ReasoningMode::Default
            && (text.contains("reasoning_effort") || text.contains("reasoning"))
        {
            caps.reasoning_effort = false;
            return true;
        }
        if caps.temperature && text.contains("temperature") {
            caps.temperature = false;
            return true;
        }
        if caps.protocol == Protocol::Json
            && caps.json_schema
            && (text.contains("response_format") || text.contains("json_schema"))
        {
            caps.json_schema = false;
            return true;
        }
        if caps.protocol == Protocol::Tools
            && settings.protocol == WireProtocol::Auto
            && (text.contains("tools") || text.contains("function"))
            && text.contains("support")
        {
            caps.protocol = Protocol::Json;
            return true;
        }
        false
    }

    fn network_error(&self, reason: &str) -> LlmError {
        LlmError::new(LlmErrorKind::Network, format!("Could not reach {}: {reason}.", self.settings.base_url))
    }

    /// Sends one request, adapting and retrying within a single time limit:
    /// 150 s until the endpoint has answered once (a local model may be
    /// loading), 45 s after, unless the request sets its own.
    pub async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let started = Instant::now();
        let timeout = request.timeout.unwrap_or(if self.warmed_up.load(Ordering::Relaxed) {
            DEFAULT_TIMEOUT
        } else {
            WARMUP_TIMEOUT
        });
        let tokens = AtomicU64::new(0);
        let outcome = tokio::select! {
            biased;
            outcome = tokio::time::timeout(timeout, self.attempts(request, started, &tokens)) => outcome.unwrap_or_else(|_| {
                Err(LlmError::new(LlmErrorKind::Timeout, format!("No answer within {} s.", timeout.as_secs_f64().round())))
            }),
            () = cancelled(request.cancel.as_ref()) => Err(LlmError::new(LlmErrorKind::Aborted, "The request was cancelled.")),
        };
        outcome.map_err(|mut error| {
            error.tokens_used = tokens.load(Ordering::Relaxed);
            error
        })
    }

    async fn attempts(
        &self,
        request: &LlmRequest,
        started: Instant,
        tokens: &AtomicU64,
    ) -> Result<LlmResponse, LlmError> {
        let mut attempt = 0;
        loop {
            let caps = self.capabilities();
            let protocol = caps.protocol;
            let http = HttpRequest {
                method: Method::Post,
                url: self.url("/chat/completions"),
                headers: self.headers(),
                body: Some(self.body(request, &caps).to_string()),
            };
            let response = self.transport.send(http).await.map_err(|reason| self.network_error(&reason))?;

            if !response.ok() {
                let text = response.body.as_str();
                if response.status == 400 && attempt < MAX_ADAPTATIONS && self.adapt(text) {
                    attempt += 1;
                    continue;
                }
                let excerpt: String = text.chars().take(300).collect();
                let mut error =
                    LlmError::new(LlmErrorKind::Http, format!("The server answered {}: {excerpt}", response.status));
                error.status = Some(response.status);
                error.retry_after = retry_after(response.header("retry-after"));
                error.raw = Some(text.to_string());
                return Err(error);
            }

            let Ok(payload) = serde_json::from_str::<Value>(&response.body) else {
                let excerpt: String = response.body.chars().take(300).collect();
                let mut error = LlmError::new(
                    LlmErrorKind::Http,
                    format!("The server answered {} with something other than JSON: {excerpt}", response.status),
                );
                error.status = Some(response.status);
                error.raw = Some(response.body);
                return Err(error);
            };
            self.warmed_up.store(true, Ordering::Relaxed);
            let usage = payload.get("usage");
            let prompt_tokens = token_count(usage, "prompt_tokens");
            let completion_tokens = token_count(usage, "completion_tokens");
            tokens.fetch_add(prompt_tokens + completion_tokens, Ordering::Relaxed);

            let choice = payload.get("choices").and_then(|choices| choices.get(0));
            let empty = Map::new();
            let message = choice.and_then(|choice| choice.get("message")).and_then(Value::as_object).unwrap_or(&empty);
            let content = message.get("content").and_then(Value::as_str).unwrap_or("");
            // Some servers put the whole constrained answer in the reasoning channel and leave content empty.
            let reasoning = [message.get("reasoning_content"), message.get("reasoning")]
                .into_iter()
                .flatten()
                .find_map(Value::as_str)
                .unwrap_or("");
            let mut calls: Vec<ToolCall> = match protocol {
                Protocol::Tools => calls_from_message(message, &request.tools),
                Protocol::Json => calls_from_text(content, &request.tools),
            };
            if calls.is_empty() && !reasoning.is_empty() {
                calls = calls_from_text(reasoning, &request.tools);
            }
            calls.truncate(request.max_calls);
            let native = match message.get("tool_calls") {
                Some(Value::Array(list)) => !list.is_empty(),
                Some(Value::String(text)) => !text.is_empty(),
                _ => false,
            };
            let raw = if native {
                json!({ "content": message.get("content").cloned().unwrap_or(Value::Null), "tool_calls": message.get("tool_calls") })
            } else {
                json!(if content.is_empty() { tail(reasoning, 600) } else { content })
            }
            .to_string();

            if calls.is_empty() {
                let truncated =
                    choice.and_then(|choice| choice.get("finish_reason")).and_then(Value::as_str) == Some("length");
                if truncated {
                    // Cut off (often by a model thinking aloud first): give it more room and try once more.
                    let raised = {
                        let mut caps = self.caps();
                        let raised = caps.token_scale < MAX_TOKEN_SCALE;
                        if raised {
                            caps.token_scale = MAX_TOKEN_SCALE.min(caps.token_scale * 2.0);
                        }
                        raised
                    };
                    if raised && attempt < MAX_ADAPTATIONS {
                        attempt += 1;
                        continue;
                    }
                    return Err(LlmError::new(
                        LlmErrorKind::Truncated,
                        "The answer was cut off before it finished (raise the output token limit).",
                    )
                    .with_raw(raw));
                }
                // A model that answers in prose despite its tools is switched to constrained JSON, which
                // servers enforce with a grammar (measured on a local model: 85% usable natively, 100% as JSON).
                if protocol == Protocol::Tools && self.settings.protocol == WireProtocol::Auto {
                    let switched = {
                        let mut caps = self.caps();
                        caps.tool_misses += 1;
                        let switched = caps.tool_misses >= 1;
                        if switched {
                            caps.protocol = Protocol::Json;
                        }
                        switched
                    };
                    if switched && attempt < MAX_ADAPTATIONS {
                        attempt += 1;
                        continue;
                    }
                }
                return Err(
                    LlmError::new(LlmErrorKind::NoCall, "The model answered without calling a tool.").with_raw(raw)
                );
            }
            self.caps().tool_misses = 0;
            return Ok(LlmResponse {
                calls,
                text: strip_thinking(content),
                raw,
                prompt_tokens,
                completion_tokens,
                tokens_used: tokens.load(Ordering::Relaxed),
                latency: started.elapsed(),
                protocol,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use super::*;
    use crate::llm::transport::{FakeTransport, HttpResponse};
    use crate::llm::types::ChatMessage;

    fn tools() -> Vec<ToolDef> {
        vec![ToolDef::new(
            "go_to",
            "Walk to a place.",
            json!({ "type": "object", "properties": { "place": { "type": "string" } }, "required": ["place"] }),
        )]
    }

    fn request() -> LlmRequest {
        LlmRequest {
            messages: vec![ChatMessage::system("s"), ChatMessage::user("u")],
            tools: tools(),
            max_calls: 2,
            ..LlmRequest::default()
        }
    }

    fn settings() -> LlmSettings {
        LlmSettings {
            enabled: true,
            base_url: "http://llm.test/v1/".into(),
            model: "m".into(),
            ..LlmSettings::default()
        }
    }

    fn client(settings: LlmSettings, transport: &Arc<FakeTransport>) -> LlmClient {
        LlmClient::new(settings, ApiKey::default(), transport.clone())
    }

    fn tool_reply() -> Value {
        json!({
            "choices": [{ "message": { "content": null, "tool_calls": [
                { "type": "function", "function": { "name": "go_to", "arguments": "{\"place\":\"tavern\"}" } },
            ] } }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 5 },
        })
    }

    fn go_to_tavern() -> Vec<ToolCall> {
        vec![ToolCall::new("go_to", json!({ "place": "tavern" }))]
    }

    /// A transport that answers each request body with `answer(body)`.
    fn scripted(answer: impl Fn(&Value) -> HttpResponse + Send + Sync + 'static) -> Arc<FakeTransport> {
        FakeTransport::new(move |request| {
            let response = answer(&request.json());
            async move { Ok(response) }
        })
    }

    fn error_400(message: &str) -> HttpResponse {
        HttpResponse::json(400, &json!({ "error": { "message": message } }))
    }

    #[tokio::test]
    async fn sends_native_tools_and_reads_the_call_back() {
        let transport = FakeTransport::always(200, tool_reply());
        let llm = LlmClient::new(
            LlmSettings { headers: "X-Title: Sim".into(), ..settings() },
            ApiKey::new("k"),
            transport.clone(),
        );
        let response = llm.complete(&request()).await.unwrap();
        assert_eq!(response.calls, go_to_tavern());
        assert_eq!(response.prompt_tokens, 12);
        assert_eq!(response.tokens_used, 17);
        let seen = &transport.requests()[0];
        assert_eq!(seen.url, "http://llm.test/v1/chat/completions");
        assert_eq!(seen.method, Method::Post);
        let body = seen.json();
        assert_eq!(body["tool_choice"], "required");
        assert_eq!(body["tools"].as_array().unwrap().len(), 1);
        assert_eq!(body["max_tokens"], 600);
        assert_eq!(body["reasoning_effort"], "none");
        assert_eq!(body["stream"], false);
        assert_eq!(seen.header("Authorization"), Some("Bearer k"));
        assert_eq!(seen.header("X-Title"), Some("Sim"));
        assert_eq!(seen.header("Content-Type"), Some("application/json"));
    }

    #[tokio::test]
    async fn omits_the_authorization_header_when_there_is_no_key() {
        let transport = FakeTransport::always(200, tool_reply());
        client(settings(), &transport).complete(&request()).await.unwrap();
        assert_eq!(transport.requests()[0].header("Authorization"), None);
    }

    #[tokio::test]
    async fn adapts_when_the_server_rejects_tool_choice_max_tokens_or_temperature() {
        let transport = scripted(|body| {
            if body.get("tool_choice").is_some() {
                error_400("tool_choice is not supported")
            } else if body.get("max_tokens").is_some() {
                error_400("Unsupported parameter: 'max_tokens'. Use 'max_completion_tokens' instead.")
            } else if body.get("temperature").is_some() {
                error_400("temperature does not support 0.8 with this model")
            } else {
                HttpResponse::json(200, &tool_reply())
            }
        });
        let llm = client(settings(), &transport);
        let response = llm.complete(&request()).await.unwrap();
        assert_eq!(response.calls.len(), 1);
        let bodies = transport.bodies();
        let last = bodies.last().unwrap();
        assert!(last.get("tool_choice").is_none());
        assert!(last.get("max_completion_tokens").is_some());
        assert!(last.get("temperature").is_none());
        // The lessons stick for the next request.
        let before = bodies.len();
        llm.complete(&request()).await.unwrap();
        assert_eq!(transport.requests().len(), before + 1);
    }

    #[tokio::test]
    async fn switches_to_json_output_when_a_model_ignores_its_tools() {
        let transport = scripted(|body| {
            let content = if body.get("tools").is_some() {
                "I shall wander to the tavern."
            } else {
                r#"{"calls":[{"tool":"go_to","args":{"place":"tavern"}}]}"#
            };
            HttpResponse::json(200, &json!({ "choices": [{ "message": { "content": content } }] }))
        });
        let llm = client(LlmSettings { protocol: WireProtocol::Auto, ..settings() }, &transport);
        let response = llm.complete(&request()).await.unwrap();
        assert_eq!(response.protocol, Protocol::Json);
        let bodies = transport.bodies();
        assert_eq!(bodies.len(), 2);
        assert_eq!(response.calls, go_to_tavern());
        let last = bodies.last().unwrap();
        assert!(last.get("tools").is_none());
        assert_eq!(last["response_format"]["type"], "json_schema");
        assert!(last["messages"].to_string().contains("Reply with JSON only"));
        assert_eq!(last["messages"].as_array().unwrap().len(), 2);
        assert_eq!(llm.protocol(), Protocol::Json);
    }

    #[tokio::test]
    async fn asks_reasoning_models_not_to_think_and_stops_asking_if_the_server_objects() {
        let transport = scripted(|body| {
            if body.get("reasoning_effort").is_some() {
                error_400("Unrecognized request argument supplied: reasoning_effort")
            } else {
                HttpResponse::json(200, &tool_reply())
            }
        });
        client(settings(), &transport).complete(&request()).await.unwrap();
        let bodies = transport.bodies();
        assert_eq!(bodies[0]["reasoning_effort"], "none");
        assert!(bodies[1].get("reasoning_effort").is_none());

        let untouched = FakeTransport::always(200, tool_reply());
        let llm = client(LlmSettings { reasoning: ReasoningMode::Default, ..settings() }, &untouched);
        llm.complete(&request()).await.unwrap();
        assert!(untouched.bodies()[0].get("reasoning_effort").is_none());

        let low = FakeTransport::always(200, tool_reply());
        client(LlmSettings { reasoning: ReasoningMode::Low, ..settings() }, &low).complete(&request()).await.unwrap();
        assert_eq!(low.bodies()[0]["reasoning_effort"], "low");
    }

    #[tokio::test]
    async fn finds_the_answer_when_a_server_puts_it_in_the_reasoning_channel() {
        let transport = FakeTransport::always(
            200,
            json!({ "choices": [{ "finish_reason": "stop", "message": {
                "content": "",
                "reasoning_content": r#"{"calls": [{"tool": "go_to", "args": {"place": "tavern"}}]}"#,
                "tool_calls": [],
            } }] }),
        );
        let llm = client(LlmSettings { protocol: WireProtocol::Json, ..settings() }, &transport);
        assert_eq!(llm.complete(&request()).await.unwrap().calls, go_to_tavern());
    }

    #[tokio::test]
    async fn gives_a_cut_off_answer_more_room_and_tries_again_without_changing_protocol() {
        let transport = scripted(|body| {
            if body["max_tokens"].as_u64().unwrap() < 1000 {
                HttpResponse::json(
                    200,
                    &json!({ "choices": [{ "finish_reason": "length", "message": {
                        "content": "", "reasoning_content": "Thinking Process: 1. Analyse",
                    } }] }),
                )
            } else {
                HttpResponse::json(200, &tool_reply())
            }
        });
        let llm = client(LlmSettings { protocol: WireProtocol::Auto, max_tokens: 300, ..settings() }, &transport);
        let response = llm.complete(&request()).await.unwrap();
        let limits =
            |t: &FakeTransport| t.bodies().iter().map(|b| b["max_tokens"].as_u64().unwrap()).collect::<Vec<_>>();
        assert_eq!(limits(&transport), vec![300, 600, 1200]);
        assert_eq!(response.protocol, Protocol::Tools);
        // The larger limit is remembered.
        llm.complete(&request()).await.unwrap();
        assert_eq!(*limits(&transport).last().unwrap(), 1200);
    }

    #[tokio::test]
    async fn reports_truncation_once_the_room_runs_out() {
        let transport = FakeTransport::always(
            200,
            json!({ "choices": [{ "finish_reason": "length", "message": { "content": "" } }], "usage": { "prompt_tokens": 10, "completion_tokens": 10 } }),
        );
        let llm = client(settings(), &transport);
        let error = llm.complete(&request()).await.unwrap_err();
        assert_eq!(error.kind, LlmErrorKind::Truncated);
        // 1x, 2x, 4x, 6x, then give up.
        assert_eq!(transport.requests().len(), 4);
        assert_eq!(error.tokens_used, 80);
        assert_eq!(llm.capabilities().token_scale, 6.0);
    }

    #[tokio::test]
    async fn reports_http_errors_with_retry_after_and_network_failures_clearly() {
        let limited = FakeTransport::new(|_| async {
            Ok(HttpResponse::json(429, &json!({ "error": "slow down" })).with_header("Retry-After", "7"))
        });
        let error = client(settings(), &limited).complete(&request()).await.unwrap_err();
        assert_eq!(error.kind, LlmErrorKind::Http);
        assert_eq!(error.status, Some(429));
        assert_eq!(error.retry_after, Some(Duration::from_secs(7)));

        let down = FakeTransport::new(|_| async { Err("connection refused".to_string()) });
        let error = client(settings(), &down).complete(&request()).await.unwrap_err();
        assert_eq!(error.kind, LlmErrorKind::Network);
        assert!(error.message.contains("connection refused"), "{}", error.message);
    }

    #[test]
    fn reads_retry_after_like_javascript_number() {
        assert_eq!(retry_after(Some(" 1.5 ")), Some(Duration::from_millis(1500)));
        assert_eq!(retry_after(Some("0")), None);
        assert_eq!(retry_after(Some("")), None);
        assert_eq!(retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")), None);
        assert_eq!(retry_after(None), None);
    }

    #[tokio::test(start_paused = true)]
    async fn times_out() {
        let transport = FakeTransport::new(|_| pending());
        let llm = client(settings(), &transport);
        let error =
            llm.complete(&LlmRequest { timeout: Some(Duration::from_millis(20)), ..request() }).await.unwrap_err();
        assert_eq!(error.kind, LlmErrorKind::Timeout);
    }

    #[tokio::test(start_paused = true)]
    async fn waits_longer_until_the_endpoint_has_answered_once() {
        let answered = Arc::new(AtomicBool::new(false));
        let flag = answered.clone();
        let transport = FakeTransport::new(move |_| {
            let answer = flag.load(Ordering::Relaxed);
            async move { if answer { Ok(HttpResponse::json(200, &tool_reply())) } else { pending().await } }
        });
        let llm = client(settings(), &transport);
        let started = Instant::now();
        assert_eq!(llm.complete(&request()).await.unwrap_err().kind, LlmErrorKind::Timeout);
        assert_eq!(started.elapsed(), WARMUP_TIMEOUT);

        answered.store(true, Ordering::Relaxed);
        llm.complete(&request()).await.unwrap();
        answered.store(false, Ordering::Relaxed);
        let started = Instant::now();
        let error = llm.complete(&request()).await.unwrap_err();
        assert_eq!(started.elapsed(), DEFAULT_TIMEOUT);
        assert_eq!(error.message, "No answer within 45 s.");
    }

    #[tokio::test(start_paused = true)]
    async fn stops_when_cancelled() {
        let transport = FakeTransport::new(|_| pending());
        let llm = client(settings(), &transport);
        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            cancel.cancel();
        });
        let error = llm.complete(&LlmRequest { cancel: Some(token), ..request() }).await.unwrap_err();
        assert_eq!(error.kind, LlmErrorKind::Aborted);
    }

    #[tokio::test]
    async fn gives_up_on_a_body_that_is_not_json() {
        let transport = FakeTransport::new(|_| async {
            Ok(HttpResponse { status: 200, headers: vec![], body: "<html>gateway</html>".into() })
        });
        let error = client(settings(), &transport).complete(&request()).await.unwrap_err();
        assert_eq!(error.kind, LlmErrorKind::Http);
        assert_eq!(error.status, Some(200));
    }

    #[tokio::test]
    async fn falls_back_from_json_schema_to_json_object() {
        let transport = scripted(|body| {
            if body["response_format"]["type"] == "json_schema" {
                error_400("response_format json_schema is not supported")
            } else {
                HttpResponse::json(
                    200,
                    &json!({ "choices": [{ "message": { "content": "{\"calls\":[{\"tool\":\"go_to\",\"args\":{\"place\":\"tavern\"}}]}" } }] }),
                )
            }
        });
        let llm = client(LlmSettings { protocol: WireProtocol::Json, ..settings() }, &transport);
        assert_eq!(llm.complete(&request()).await.unwrap().calls, go_to_tavern());
        assert!(!llm.capabilities().json_schema);
        let first = &transport.bodies()[0];
        let schema = &first["response_format"]["json_schema"]["schema"];
        assert_eq!(schema["properties"]["calls"]["maxItems"], 2);
        assert_eq!(schema["properties"]["calls"]["items"]["anyOf"][0]["properties"]["tool"]["enum"][0], "go_to");
        assert!(first["messages"][0]["content"].as_str().unwrap().starts_with("s\n\nReply with JSON only"));
    }

    #[tokio::test]
    async fn switches_protocol_when_the_server_says_tools_are_unsupported() {
        let transport = scripted(|body| {
            if body.get("tools").is_some() {
                error_400("This model does not support tools")
            } else {
                HttpResponse::json(
                    200,
                    &json!({ "choices": [{ "message": { "content": "{\"calls\":[{\"tool\":\"go_to\",\"args\":{\"place\":\"tavern\"}}]}" } }] }),
                )
            }
        });
        let llm = client(LlmSettings { reasoning: ReasoningMode::Default, ..settings() }, &transport);
        let response = llm.complete(&request()).await.unwrap();
        assert_eq!(response.protocol, Protocol::Json);
        // The message names neither tool_choice nor another parameter, so the protocol changes at once.
        assert_eq!(transport.requests().len(), 2);
    }

    #[tokio::test]
    async fn exports_and_restores_what_it_learned() {
        let transport = scripted(|body| {
            if body.get("tool_choice").is_some() {
                error_400("tool_choice is not supported")
            } else {
                HttpResponse::json(200, &tool_reply())
            }
        });
        let first = client(settings(), &transport);
        first.complete(&request()).await.unwrap();
        let learned = first.learned();
        let stored = serde_json::to_string(&learned).unwrap();
        assert!(stored.contains("\"toolChoice\":false"), "{stored}");
        assert!(stored.contains("\"maxTokensField\":\"max_tokens\""), "{stored}");

        let second = client(settings(), &transport);
        assert!(second.restore(&serde_json::from_str(&stored).unwrap()));
        let before = transport.requests().len();
        second.complete(&request()).await.unwrap();
        assert_eq!(transport.requests().len(), before + 1);

        let other = client(LlmSettings { model: "other".into(), ..settings() }, &transport);
        assert!(!other.restore(&learned));
        assert!(other.capabilities().tool_choice);

        let forced = client(LlmSettings { protocol: WireProtocol::Tools, ..settings() }, &transport);
        let json_learned = LearnedCapabilities {
            capabilities: Capabilities { protocol: Protocol::Json, ..Capabilities::default() },
            ..learned
        };
        assert!(forced.restore(&json_learned));
        assert_eq!(forced.protocol(), Protocol::Tools);
    }

    #[tokio::test]
    async fn lists_models() {
        let transport = FakeTransport::always(
            200,
            json!({ "data": [{ "id": "b" }, { "id": "a" }, { "name": "c" }, { "id": "" }] }),
        );
        let llm = client(settings(), &transport);
        assert_eq!(llm.list_models().await.unwrap(), vec!["a", "b", "c"]);
        assert_eq!(transport.requests()[0].url, "http://llm.test/v1/models");
        assert_eq!(transport.requests()[0].method, Method::Get);
        let ollama = FakeTransport::always(200, json!({ "models": [{ "name": "q:8b" }] }));
        assert_eq!(client(settings(), &ollama).list_models().await.unwrap(), vec!["q:8b"]);
        let refused = FakeTransport::always(401, json!({}));
        assert_eq!(client(settings(), &refused).list_models().await.unwrap_err().status, Some(401));
    }

    #[test]
    fn takes_the_tail_of_a_text_by_characters() {
        assert_eq!(tail("héllo", 3), "llo");
        assert_eq!(tail("hé", 5), "hé");
        assert_eq!(tail("abc", 0), "");
    }
}
