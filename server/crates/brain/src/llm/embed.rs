//! Talks to an OpenAI-compatible `/embeddings` endpoint, so memories can be found by meaning.
//! Nothing here knows about the sim. One request at a time, a per-minute limit and a
//! breaker keep a struggling endpoint from being hammered; the chat queue is not involved.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::time::Instant;

use super::transport::{HttpRequest, LlmTransport, Method};
use crate::settings::{ApiKey, EmbedSettings};

/// Texts per request: small enough for any server, large enough to fill an index quickly.
pub const BATCH: usize = 32;
const TIMEOUT: Duration = Duration::from_secs(30);
/// Requests per real minute, cues and indexing together.
const PER_MINUTE: usize = 240;
/// After this many failures in a row the embedder rests for [`REST`].
const BREAK_AFTER: u32 = 4;
const REST: Duration = Duration::from_secs(60);

/// Vectors in the order of the texts, each scaled to unit length.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedded {
    /// One unit vector per text, in the order the texts were given.
    pub vectors: Vec<Vec<f32>>,
    /// The tokens the endpoint says the request used, or 0 when it does not say.
    pub tokens: u64,
}

/// Why [`Embedder::embed`] gave no vectors. A refusal sent nothing, so callers can tell a
/// resting embedder from a failing endpoint and count only the requests that were made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedError {
    /// Nothing was sent: the embedder is resting after repeated failures, or over its per-minute limit.
    Refused(String),
    /// The request failed, or could not be made at all.
    Failed(String),
}

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(message) | Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for EmbedError {}

#[derive(Default)]
struct Gate {
    sent: VecDeque<Instant>,
    failures: u32,
    resting_until: Option<Instant>,
}

/// The client for the embedding endpoint, shared by cues and the indexer.
pub struct Embedder {
    settings: EmbedSettings,
    key: ApiKey,
    transport: Arc<dyn LlmTransport>,
    /// Held for the whole request, so requests go one at a time.
    gate: tokio::sync::Mutex<Gate>,
}

impl Embedder {
    /// An embedder for these settings. The key comes apart from them, so the settings can be shown and logged
    /// without it.
    pub fn new(settings: EmbedSettings, key: ApiKey, transport: Arc<dyn LlmTransport>) -> Self {
        Self { settings, key, transport, gate: tokio::sync::Mutex::new(Gate::default()) }
    }

    /// The embedding model's name, which the cached vectors are keyed by.
    pub fn model(&self) -> &str {
        &self.settings.model
    }

    /// Embeds up to [`BATCH`] texts in one request.
    pub async fn embed(&self, texts: &[String]) -> Result<Embedded, EmbedError> {
        if texts.is_empty() {
            return Ok(Embedded { vectors: Vec::new(), tokens: 0 });
        }
        if texts.len() > BATCH {
            return Err(EmbedError::Failed(format!("at most {BATCH} texts per request")));
        }
        let mut gate = self.gate.lock().await;
        let now = Instant::now();
        if gate.resting_until.is_some_and(|until| now < until) {
            return Err(EmbedError::Refused("the embedding endpoint failed repeatedly and is resting".into()));
        }
        while gate.sent.front().is_some_and(|at| now.duration_since(*at) >= Duration::from_secs(60)) {
            gate.sent.pop_front();
        }
        if gate.sent.len() >= PER_MINUTE {
            return Err(EmbedError::Refused("over the embedding requests allowed per minute".into()));
        }
        gate.sent.push_back(now);
        let result = match tokio::time::timeout(TIMEOUT, self.send(texts)).await {
            Ok(result) => result.map_err(|error| EmbedError::Failed(self.redact(&error))),
            Err(_) => Err(EmbedError::Failed(format!("no answer within {} s", TIMEOUT.as_secs()))),
        };
        if result.is_ok() {
            gate.failures = 0;
        } else {
            gate.failures += 1;
            if gate.failures >= BREAK_AFTER {
                gate.failures = 0;
                gate.resting_until = Some(Instant::now() + REST);
            }
        }
        result
    }

    async fn send(&self, texts: &[String]) -> Result<Embedded, String> {
        let mut headers = vec![("Content-Type".to_owned(), "application/json".to_owned())];
        if !self.key.is_empty() {
            headers.push(("Authorization".to_owned(), format!("Bearer {}", self.key.expose())));
        }
        let request = HttpRequest {
            method: Method::Post,
            url: format!("{}/embeddings", self.settings.base_url),
            headers,
            body: Some(json!({ "model": self.settings.model, "input": texts }).to_string()),
        };
        let response = self.transport.send(request).await?;
        if !response.ok() {
            // Masked before it is cut short, so a key split by the cut cannot slip through in part.
            let body: String = self.redact(&response.body).chars().take(200).collect();
            return Err(format!("HTTP {}: {body}", response.status));
        }
        parse(&response.body, texts.len())
    }

    /// Masks the key in text from the endpoint, which may echo what it was sent, so the key reaches no log and
    /// no admin's test message.
    fn redact(&self, text: &str) -> String {
        if self.key.is_empty() { text.to_owned() } else { text.replace(self.key.expose(), "***") }
    }
}

/// Reads `data[].embedding` in `index` order and scales each vector to unit length.
/// Anything that does not fit (count, size, numbers) refuses the whole answer.
pub fn parse(body: &str, expected: usize) -> Result<Embedded, String> {
    let value: Value = serde_json::from_str(body).map_err(|_| "the answer was not JSON".to_owned())?;
    let data = value.get("data").and_then(Value::as_array).ok_or("the answer had no data")?;
    let mut rows: Vec<(usize, Vec<f32>)> = Vec::with_capacity(data.len());
    for (position, item) in data.iter().enumerate() {
        let index = item.get("index").and_then(Value::as_u64).map_or(position, |i| i as usize);
        let numbers = item.get("embedding").and_then(Value::as_array).ok_or("an item had no embedding")?;
        let vector: Vec<f32> = numbers
            .iter()
            .map(|n| n.as_f64().map(|n| n as f32))
            .collect::<Option<_>>()
            .ok_or("an embedding held something other than numbers")?;
        rows.push((index, normalise(vector)?));
    }
    rows.sort_by_key(|(index, _)| *index);
    if rows.len() != expected || rows.iter().enumerate().any(|(i, (index, _))| i != *index) {
        return Err(format!("expected {expected} embeddings, got {}", rows.len()));
    }
    let size = rows.first().map_or(0, |(_, v)| v.len());
    if rows.iter().any(|(_, v)| v.len() != size) {
        return Err("the embeddings were of different sizes".into());
    }
    let tokens = value
        .pointer("/usage/total_tokens")
        .or_else(|| value.pointer("/usage/prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Ok(Embedded { vectors: rows.into_iter().map(|(_, v)| v).collect(), tokens })
}

fn normalise(mut vector: Vec<f32>) -> Result<Vec<f32>, String> {
    let norm = vector.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>().sqrt();
    if vector.is_empty() || !norm.is_finite() || norm == 0.0 {
        return Err("an embedding was empty or all zeros".into());
    }
    for x in &mut vector {
        *x = (f64::from(*x) / norm) as f32;
    }
    Ok(vector)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::llm::transport::{FakeTransport, HttpRequest, HttpResponse};

    fn embedder(transport: Arc<FakeTransport>) -> Embedder {
        let settings = EmbedSettings { base_url: "http://embed.test/v1".into(), model: "nomic".into() };
        Embedder::new(settings, ApiKey::new("sk-e"), transport)
    }

    fn texts(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| (*t).to_owned()).collect()
    }

    #[tokio::test]
    async fn sends_the_model_and_texts_and_reads_vectors_in_index_order() {
        let transport = FakeTransport::always(
            200,
            json!({ "data": [
                { "index": 1, "embedding": [0.0, 2.0] },
                { "index": 0, "embedding": [3.0, 4.0] },
            ], "usage": { "prompt_tokens": 9, "total_tokens": 9 } }),
        );
        let embedded = embedder(transport.clone()).embed(&texts(&["fire", "bread"])).await.unwrap();
        assert_eq!(embedded.vectors, vec![vec![0.6, 0.8], vec![0.0, 1.0]]);
        assert_eq!(embedded.tokens, 9);
        let request = &transport.requests()[0];
        assert_eq!(request.url, "http://embed.test/v1/embeddings");
        assert_eq!(request.header("Authorization"), Some("Bearer sk-e"));
        assert_eq!(request.json(), json!({ "model": "nomic", "input": ["fire", "bread"] }));
    }

    #[test]
    fn refuses_answers_that_do_not_fit() {
        let fewer = json!({ "data": [{ "index": 0, "embedding": [1.0] }] }).to_string();
        assert!(parse(&fewer, 2).unwrap_err().contains("expected 2"));
        let zero = json!({ "data": [{ "index": 0, "embedding": [0.0, 0.0] }] }).to_string();
        assert!(parse(&zero, 1).is_err());
        let words = json!({ "data": [{ "index": 0, "embedding": ["a"] }] }).to_string();
        assert!(parse(&words, 1).is_err());
        let sizes = json!({ "data": [
            { "index": 0, "embedding": [1.0] }, { "index": 1, "embedding": [1.0, 0.0] },
        ] })
        .to_string();
        assert!(parse(&sizes, 2).is_err());
        assert!(parse("not json", 1).is_err());
    }

    #[tokio::test]
    async fn reports_an_error_status_with_its_body() {
        let transport = FakeTransport::always(404, json!({ "error": "no such model" }));
        let error = embedder(transport).embed(&texts(&["fire"])).await.unwrap_err();
        assert!(matches!(error, EmbedError::Failed(_)), "{error:?}");
        let error = error.to_string();
        assert!(error.contains("404") && error.contains("no such model"), "{error}");
    }

    #[tokio::test]
    async fn never_puts_the_key_in_an_error() {
        let echo = FakeTransport::new(|request: HttpRequest| {
            let said = request.header("Authorization").unwrap_or_default().to_owned();
            async move { Ok(HttpResponse::json(401, &json!({ "error": format!("bad token: {said}") }))) }
        });
        let error = embedder(echo).embed(&texts(&["fire"])).await.unwrap_err().to_string();
        assert!(!error.contains("sk-e"), "{error}");
        assert!(error.contains("401") && error.contains("***"), "{error}");
    }

    #[tokio::test(start_paused = true)]
    async fn rests_after_four_failures_in_a_row() {
        let transport = FakeTransport::always(500, json!({}));
        let embedder = embedder(transport.clone());
        for _ in 0..4 {
            assert!(matches!(embedder.embed(&texts(&["x"])).await, Err(EmbedError::Failed(_))));
        }
        let resting = embedder.embed(&texts(&["x"])).await.unwrap_err();
        assert!(matches!(&resting, EmbedError::Refused(why) if why.contains("resting")), "{resting:?}");
        assert_eq!(transport.requests().len(), 4);
        tokio::time::advance(Duration::from_secs(61)).await;
        assert!(matches!(embedder.embed(&texts(&["x"])).await, Err(EmbedError::Failed(_))));
        assert_eq!(transport.requests().len(), 5);
    }

    #[tokio::test(start_paused = true)]
    async fn refuses_without_sending_over_the_per_minute_limit() {
        let transport = FakeTransport::always(200, json!({ "data": [{ "index": 0, "embedding": [1.0] }] }));
        let embedder = embedder(transport.clone());
        for _ in 0..PER_MINUTE {
            assert!(embedder.embed(&texts(&["x"])).await.is_ok());
        }
        assert!(matches!(embedder.embed(&texts(&["x"])).await, Err(EmbedError::Refused(_))));
        assert_eq!(transport.requests().len(), PER_MINUTE);
    }

    #[tokio::test]
    async fn sends_nothing_for_no_texts_and_refuses_too_many() {
        let transport = FakeTransport::always(200, json!({ "data": [] }));
        let embedder = embedder(transport.clone());
        assert!(embedder.embed(&[]).await.unwrap().vectors.is_empty());
        let many: Vec<String> = (0..=BATCH).map(|i| i.to_string()).collect();
        assert!(embedder.embed(&many).await.is_err());
        assert!(transport.requests().is_empty());
    }
}
