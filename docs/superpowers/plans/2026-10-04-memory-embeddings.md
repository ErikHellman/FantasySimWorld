# Memory by Meaning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Villagers recall memories by meaning when an OpenAI-compatible embeddings endpoint is configured, and by keywords exactly as today when it is not.

**Architecture:** The server reads `EMBED_*` settings. A background indexer embeds the memory texts of the living and keeps the vectors in SQLite (migration 4) and in an in-memory cache, outside `SimState`. When the brain builds a decision, a reply or a day plan, it first embeds a short cue (2 s limit). It then passes a `Recall` (the cache plus the cue vector) to `retrieve`, which swaps the shared-words part of relevance for cosine closeness. Saves, replay, determinism and the prompt fixture do not change.

**Tech Stack:** Rust 2024 (tokio, rusqlite, serde_json, ts-rs), the existing `LlmTransport` for HTTP, TypeScript and Preact for the admin panel, Node for the mock.

**Spec:** `docs/superpowers/specs/2026-10-04-memory-embeddings-design.md`

## Global Constraints

- The settings are `EMBED_BASE_URL` and `EMBED_MODEL` (both or neither; the URL must start with `http://` or `https://`, and a trailing `/` is trimmed), plus an optional `EMBED_API_KEY`. They are read once at start.
- The key is never sent to a browser, a log, an export or a test message, and is masked in `Debug` (reuse `ApiKey`).
- Vectors never enter `SimState`. The prompt fixture `server/crates/brain/tests/fixtures/prompts.json` must not change.
- Batches hold at most 32 texts. The cue wait is 2 seconds. Unused vectors are deleted after 7 days. The semantic score is `clamp((cosine − 0.2) / 0.6, 0, 1) × 1.5`.
- The embedder makes one request at a time, with its own per-minute limit and breaker. It is separate from the chat `JobQueue` and the governor.
- Usage is recorded under the kind `embed`, which `usage_today` (the chat caps and the admin's 24-hour count) leaves out.
- The schema only grows: add migration `(4, V4)`; never edit V1–V3.
- Wire changes start in `wire.rs`. Run `cargo run --bin export-api` in `server/` and never edit `src/generated/api.ts` by hand. The new fields are additive and admin-only, so `PROTOCOL_VERSION` stays as it is.
- Style: match the surrounding code, comments in full sentences saying why, British spelling in docs, a 120-column rustfmt width, no `unsafe`, and never hold a sync lock across an `.await`.
- Commit subjects are one imperative sentence. End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Before calling a task done, run:
  - `cd server && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
  - and, for the client, `pnpm typecheck && pnpm test`
  - When piping test output, use `set -o pipefail`.

## Review Focus

1. **An embedding endpoint that hangs** must not hold up a villager's decision: the decision goes out with keyword recall once 2 s have passed (test in Task 5).
2. **Vectors of different sizes** (a model changed, a stale row) must never zero out relevance: the memory falls back to shared words (test in Task 4).
3. **A malformed answer** (fewer embeddings than inputs, an all-zero vector, non-numbers) must be refused whole and counted as a failure, never half-stored (tests in Task 2 and Task 3).
4. **The `EMBED_API_KEY`** must not appear in the admin view, the connection-test message or `Config`'s `Debug` (tests in Task 1 and Task 8).
5. **Switching `EMBED_MODEL`** must not load the old model's vectors or compare across models (tests in Task 3 and Task 6).

---

## File map

| File | Responsibility |
| --- | --- |
| `server/crates/brain/src/settings.rs` | `EmbedSettings` (base URL and model) |
| `server/crates/server/src/config.rs` | `EMBED_*` → `Config.embed`, `Config.embed_api_key` |
| `server/crates/brain/src/llm/embed.rs` (new) | `Embedder`: HTTP to `/embeddings`, parse, normalise, gate (one at a time, per-minute limit, breaker) |
| `server/crates/brain/src/recall.rs` (new) | `text_key`, `cosine`, `VectorLookup`, `VectorCache`, `Recall`, `MemoryIndex` (cue and index) |
| `server/crates/brain/src/retrieval.rs` | `retrieve(..., recall)` scores by meaning when it can |
| `server/crates/brain/src/prompts.rs` | `decide_cue`, `plan_cue`, `reply_cue`; decide, plan and reply prompts take `Option<&Recall>` |
| `server/crates/brain/src/brain.rs` | `Brain::set_memory`; `request` embeds the cue before building the prompt |
| `server/crates/server/src/store/migrations.rs`, `store/vectors.rs` (new), `store/maintenance.rs`, `store/brain_log.rs` | V4 table, load/put/touch, the sweep, `embed` usage left out of `usage_today` |
| `server/crates/server/src/brain_task.rs`, `main.rs` | load vectors at start, the indexer loop, wiring |
| `server/crates/server/src/api/{wire,llm}.rs`, `src/ui/Admin.tsx` | read-only view of the embedding settings and the connection test |
| `tools/mock-llm.mjs`, `server/crates/brain/src/bin/probe.rs` | the mock `/v1/embeddings`; `probe --embed` |
| `README.md`, `docs/deploy.md`, `docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md` | docs |

---

### Task 1: Embedding settings from the environment

**Files:**
- Modify: `server/crates/brain/src/settings.rs` (after `ApiKey`)
- Modify: `server/crates/server/src/config.rs` (the `Config` struct near line 181, `from_vars` near line 287, the new `embed_from` after `llm_from`, and the tests)

**Interfaces:**
- Produces: `fsw_brain::settings::EmbedSettings { pub base_url: String, pub model: String }` with `pub fn enabled(&self) -> bool`. `Config.embed: EmbedSettings` and `Config.embed_api_key: ApiKey`.

- [ ] **Step 1: Write the failing test** in `config.rs`'s `mod tests`:

```rust
    #[test]
    fn embeddings_are_off_unless_both_settings_are_given() {
        let token = ("AUTH_TOKEN", "0123456789abcdef");
        let config = Config::from_vars(vars(&[token])).unwrap();
        assert!(!config.embed.enabled());
        assert!(config.embed_api_key.is_empty());

        let config = Config::from_vars(vars(&[
            token,
            ("EMBED_BASE_URL", "http://localhost:1234/v1/"),
            ("EMBED_MODEL", "nomic-embed-text"),
            ("EMBED_API_KEY", "sk-embed-1234"),
        ]))
        .unwrap();
        assert!(config.embed.enabled());
        assert_eq!(config.embed.base_url, "http://localhost:1234/v1");
        assert_eq!(config.embed.model, "nomic-embed-text");
        assert_eq!(config.embed_api_key.expose(), "sk-embed-1234");
        assert!(!format!("{config:?}").contains("sk-embed-1234"));

        let only_model = Config::from_vars(vars(&[token, ("EMBED_MODEL", "m")])).unwrap_err();
        assert!(only_model.contains("EMBED_BASE_URL"), "{only_model}");
        let only_url = Config::from_vars(vars(&[token, ("EMBED_BASE_URL", "http://x/v1")])).unwrap_err();
        assert!(only_url.contains("EMBED_MODEL"), "{only_url}");
        let bad = Config::from_vars(vars(&[token, ("EMBED_BASE_URL", "ftp://x"), ("EMBED_MODEL", "m")])).unwrap_err();
        assert!(bad.contains("EMBED_BASE_URL"), "{bad}");
    }
```

- [ ] **Step 2: Run it and check that it fails**

Run: `cd server && cargo test -p fsw-server --lib config::tests::embeddings_are_off_unless_both_settings_are_given`
Expected: a compile error: no field `embed` on `Config`.

- [ ] **Step 3: Implement the change.** In `settings.rs`, after `impl fmt::Debug for ApiKey`:

```rust
/// The embedding model that lets villagers recall memories by meaning. Off unless both are set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmbedSettings {
    /// OpenAI-compatible base URL, e.g. http://localhost:1234/v1
    pub base_url: String,
    pub model: String,
}

impl EmbedSettings {
    pub fn enabled(&self) -> bool {
        !self.base_url.is_empty() && !self.model.is_empty()
    }
}
```

In `config.rs`:
- Import `EmbedSettings` next to `ApiKey`.
- Add these fields after `llm_api_key` in `Config`:

```rust
    /// The embedding model, from `EMBED_BASE_URL` and `EMBED_MODEL` (see `embed_from`). Off unless both are set.
    pub embed: EmbedSettings,
    /// `EMBED_API_KEY`: the embedding endpoint's key, if it needs one (masked in `Debug`)
    pub embed_api_key: ApiKey,
```

- In `from_vars`, after `let llm = llm_from(&get)?;`, add `let embed = embed_from(&get)?;`.
- In the `Ok(Self { ... })`, after `llm_api_key`, add:

```rust
            embed,
            embed_api_key: ApiKey::new(get("EMBED_API_KEY").unwrap_or("")),
```

- After `llm_from`, add:

```rust
/// The embedding model. Both variables or neither, like the language model's.
fn embed_from<'a>(get: &impl Fn(&str) -> Option<&'a str>) -> Result<EmbedSettings, String> {
    let base_url = get("EMBED_BASE_URL").unwrap_or("").trim_end_matches('/').to_owned();
    let model = get("EMBED_MODEL").unwrap_or("").to_owned();
    match (base_url.is_empty(), model.is_empty()) {
        (false, true) => return Err("EMBED_BASE_URL is set but EMBED_MODEL is not; set both or neither".into()),
        (true, false) => return Err("EMBED_MODEL is set but EMBED_BASE_URL is not; set both or neither".into()),
        _ => {}
    }
    if !(base_url.is_empty() || base_url.starts_with("https://") || base_url.starts_with("http://")) {
        return Err(format!("EMBED_BASE_URL must start with https:// or http://, got {base_url:?}"));
    }
    Ok(EmbedSettings { base_url, model })
}
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cd server && cargo test -p fsw-server --lib config::tests`
Expected: PASS, including the existing config tests.

- [ ] **Step 5: Commit**

```bash
git add server/crates/brain/src/settings.rs server/crates/server/src/config.rs
git commit -m "Read the embedding model's settings from EMBED_* environment variables" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: The embeddings client

**Files:**
- Create: `server/crates/brain/src/llm/embed.rs`
- Modify: `server/crates/brain/src/llm/mod.rs` (add `pub mod embed;`)

**Interfaces:**
- Consumes: `EmbedSettings`, `ApiKey` (Task 1), `LlmTransport`, `HttpRequest`, `Method` (`llm/transport.rs`).
- Produces:
  - `pub const BATCH: usize = 32`
  - `pub struct Embedded { pub vectors: Vec<Vec<f32>>, pub tokens: u64 }`
  - `pub struct Embedder` with `pub fn new(settings: EmbedSettings, key: ApiKey, transport: Arc<dyn LlmTransport>) -> Self`, `pub fn model(&self) -> &str` and `pub async fn embed(&self, texts: &[String]) -> Result<Embedded, String>`
  - `pub fn parse(body: &str, expected: usize) -> Result<Embedded, String>`

- [ ] **Step 1: Write the failing tests.** Create `embed.rs` with only the test module and the module header:

```rust
//! Talks to an OpenAI-compatible `/embeddings` endpoint, so memories can be found by meaning.
//! Nothing here knows about the sim. One request at a time, a per-minute limit and a
//! breaker keep a struggling endpoint from being hammered; the chat queue is not involved.

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::llm::transport::{FakeTransport, HttpResponse};

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
        assert!(error.contains("404") && error.contains("no such model"), "{error}");
    }

    #[tokio::test(start_paused = true)]
    async fn rests_after_four_failures_in_a_row() {
        let transport = FakeTransport::always(500, json!({}));
        let embedder = embedder(transport.clone());
        for _ in 0..4 {
            assert!(embedder.embed(&texts(&["x"])).await.is_err());
        }
        assert!(embedder.embed(&texts(&["x"])).await.unwrap_err().contains("resting"));
        assert_eq!(transport.requests().len(), 4);
        tokio::time::advance(Duration::from_secs(61)).await;
        assert!(!embedder.embed(&texts(&["x"])).await.unwrap_err().contains("resting"));
        assert_eq!(transport.requests().len(), 5);
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
```

Add `pub mod embed;` to `llm/mod.rs`.

- [ ] **Step 2: Run the tests and check that they fail**

Run: `cd server && cargo test -p fsw-brain --lib llm::embed`
Expected: a compile error: `Embedder`, `parse` and `BATCH` are not defined.

- [ ] **Step 3: Implement the client.** Put this above the test module in `embed.rs`:

```rust
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
    pub vectors: Vec<Vec<f32>>,
    pub tokens: u64,
}

#[derive(Default)]
struct Gate {
    sent: VecDeque<Instant>,
    failures: u32,
    resting_until: Option<Instant>,
}

pub struct Embedder {
    settings: EmbedSettings,
    key: ApiKey,
    transport: Arc<dyn LlmTransport>,
    /// Held for the whole request, so requests go one at a time.
    gate: tokio::sync::Mutex<Gate>,
}

impl Embedder {
    pub fn new(settings: EmbedSettings, key: ApiKey, transport: Arc<dyn LlmTransport>) -> Self {
        Self { settings, key, transport, gate: tokio::sync::Mutex::new(Gate::default()) }
    }

    pub fn model(&self) -> &str {
        &self.settings.model
    }

    /// Embeds up to [`BATCH`] texts in one request.
    pub async fn embed(&self, texts: &[String]) -> Result<Embedded, String> {
        if texts.is_empty() {
            return Ok(Embedded { vectors: Vec::new(), tokens: 0 });
        }
        if texts.len() > BATCH {
            return Err(format!("at most {BATCH} texts per request"));
        }
        let mut gate = self.gate.lock().await;
        let now = Instant::now();
        if gate.resting_until.is_some_and(|until| now < until) {
            return Err("the embedding endpoint failed repeatedly and is resting".into());
        }
        while gate.sent.front().is_some_and(|at| now.duration_since(*at) >= Duration::from_secs(60)) {
            gate.sent.pop_front();
        }
        if gate.sent.len() >= PER_MINUTE {
            return Err("over the embedding requests allowed per minute".into());
        }
        gate.sent.push_back(now);
        let result = match tokio::time::timeout(TIMEOUT, self.send(texts)).await {
            Ok(result) => result,
            Err(_) => Err(format!("no answer within {} s", TIMEOUT.as_secs())),
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
            let body: String = response.body.chars().take(200).collect();
            return Err(format!("HTTP {}: {body}", response.status));
        }
        parse(&response.body, texts.len())
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
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cd server && cargo test -p fsw-brain --lib llm::embed`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add server/crates/brain/src/llm/embed.rs server/crates/brain/src/llm/mod.rs
git commit -m "Add a client for OpenAI-compatible embedding endpoints" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: The vector cache and the memory index

**Files:**
- Create: `server/crates/brain/src/recall.rs`
- Modify: `server/crates/brain/src/lib.rs` (add `pub mod recall;` after `pub mod prompts;`)

**Interfaces:**
- Consumes: `Embedder`, `BATCH`, `Embedded` (Task 2).
- Produces:
  - `pub fn text_key(model: &str, text: &str) -> i64`
  - `pub fn cosine(a: &[f32], b: &[f32]) -> f64`
  - `pub trait VectorLookup: Send + Sync { fn vector(&self, text: &str) -> Option<Arc<[f32]>>; }`
  - `pub struct Recall<'a> { pub lookup: &'a dyn VectorLookup, pub cue: Option<&'a [f32]> }`, with `pub fn meaning(&self, text: &str) -> Option<f64>`
  - `pub struct VectorCache`, with `new(model)`, `model()`, `key(text) -> i64`, `insert(key, Arc<[f32]>)`, `contains(key) -> bool`, `retain(&HashSet<i64>)`, `len()` and `is_empty()`, and `impl VectorLookup`
  - `pub struct MemoryIndex`, with `new(Embedder)`, `cache() -> &VectorCache`, `async cue(&self, text: &str, wait: Duration) -> Option<Arc<[f32]>>` and `async index(&self, texts: &[String], max_batches: usize) -> Indexed`
  - `pub struct Indexed { pub added: Vec<(i64, Arc<[f32]>)>, pub requests: u64, pub failures: u64, pub tokens: u64, pub error: Option<String> }`

- [ ] **Step 1: Write the failing tests.** Create `recall.rs` with the header and tests:

```rust
//! Memories found by meaning: the vectors the brain has for memory texts, and the
//! cue a prompt compares them with. None of this enters the sim's state; it only
//! changes which memories a prompt recalls, so saves and replay never see it.

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::llm::transport::{FakeTransport, HttpRequest, HttpResponse};
    use crate::settings::{ApiKey, EmbedSettings};

    /// An endpoint that answers every text with [1, 0], or fails when told to.
    fn endpoint(fail: bool) -> Arc<FakeTransport> {
        FakeTransport::new(move |request: HttpRequest| {
            let count = request.json()["input"].as_array().map_or(0, Vec::len);
            let data: Vec<_> = (0..count).map(|i| json!({ "index": i, "embedding": [1.0, 0.0] })).collect();
            let response = if fail {
                HttpResponse::json(500, &json!({}))
            } else {
                HttpResponse::json(200, &json!({ "data": data, "usage": { "total_tokens": 3 } }))
            };
            async move { Ok(response) }
        })
    }

    fn index(transport: Arc<FakeTransport>) -> MemoryIndex {
        let settings = EmbedSettings { base_url: "http://embed.test/v1".into(), model: "m1".into() };
        MemoryIndex::new(Embedder::new(settings, ApiKey::default(), transport))
    }

    #[test]
    fn keys_depend_on_model_and_text_and_are_stable() {
        assert_eq!(text_key("m1", "Åsa lit the bonfire 🔥"), text_key("m1", "Åsa lit the bonfire 🔥"));
        assert_ne!(text_key("m1", "a"), text_key("m2", "a"));
        assert_ne!(text_key("m1", "ab"), text_key("m1a", "b"));
    }

    #[test]
    fn meaning_needs_both_vectors_of_the_same_size() {
        let cache = VectorCache::new("m1");
        cache.insert(cache.key("fire"), Arc::from(vec![1.0f32, 0.0]));
        cache.insert(cache.key("bread"), Arc::from(vec![0.0f32, 1.0]));
        cache.insert(cache.key("odd"), Arc::from(vec![1.0f32, 0.0, 0.0]));
        let cue = [1.0f32, 0.0];
        let recall = Recall { lookup: &cache, cue: Some(&cue) };
        assert_eq!(recall.meaning("fire"), Some(1.5));
        assert_eq!(recall.meaning("bread"), Some(0.0));
        assert_eq!(recall.meaning("odd"), None);
        assert_eq!(recall.meaning("unknown"), None);
        assert_eq!(Recall { lookup: &cache, cue: None }.meaning("fire"), None);
    }

    #[tokio::test]
    async fn indexes_only_what_it_has_not_seen_and_each_text_once() {
        let transport = endpoint(false);
        let index = index(transport.clone());
        let texts: Vec<String> = ["a", "b", "a"].iter().map(|t| (*t).to_owned()).collect();
        let indexed = index.index(&texts, 4).await;
        assert_eq!((indexed.added.len(), indexed.requests, indexed.tokens), (2, 1, 3));
        assert!(index.cache().vector("a").is_some() && index.cache().vector("b").is_some());
        let again = index.index(&texts, 4).await;
        assert!(again.added.is_empty());
        assert_eq!(transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn stops_at_the_batch_limit_and_reports_a_failure_without_storing() {
        let many: Vec<String> = (0..(BATCH * 3)).map(|i| format!("memory {i}")).collect();
        let index = index(endpoint(false));
        assert_eq!(index.index(&many, 2).await.added.len(), BATCH * 2);

        let failing = self::index(endpoint(true));
        let indexed = failing.index(&many, 2).await;
        assert!(indexed.added.is_empty() && indexed.error.is_some());
        assert_eq!((indexed.requests, indexed.failures), (0, 1));
        assert!(failing.cache().is_empty());
    }

    #[tokio::test]
    async fn a_cue_is_embedded_or_given_up_on() {
        let index = index(endpoint(false));
        assert_eq!(index.cue("smoke", Duration::from_secs(2)).await.as_deref(), Some(&[1.0f32, 0.0][..]));
        assert_eq!(index.cue("   ", Duration::from_secs(2)).await, None);
        let failing = self::index(endpoint(true));
        assert_eq!(failing.cue("smoke", Duration::from_secs(2)).await, None);
    }

    #[test]
    fn retain_forgets_the_rest() {
        let cache = VectorCache::new("m1");
        cache.insert(cache.key("keep"), Arc::from(vec![1.0f32]));
        cache.insert(cache.key("drop"), Arc::from(vec![1.0f32]));
        cache.retain(&HashSet::from([cache.key("keep")]));
        assert_eq!(cache.len(), 1);
        assert!(cache.vector("keep").is_some());
    }
}
```

Add `pub mod recall;` to `lib.rs`.

- [ ] **Step 2: Run the tests and check that they fail**

Run: `cd server && cargo test -p fsw-brain --lib recall`
Expected: a compile error: the items are not defined.

- [ ] **Step 3: Implement the module.** Put this above the tests:

```rust
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use crate::llm::embed::{BATCH, Embedder};

/// The cache key for a text under a model: FNV-1a over the model name, a separator byte
/// and the text, as a signed number so SQLite can keep it as an integer key.
pub fn text_key(model: &str, text: &str) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in model.bytes().chain([0xff]).chain(text.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash as i64
}

/// The cosine of two unit vectors, which is their dot product.
pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(x, y)| f64::from(*x) * f64::from(*y)).sum()
}

/// Finds the vector for a memory text, if there is one yet.
pub trait VectorLookup: Send + Sync {
    fn vector(&self, text: &str) -> Option<Arc<[f32]>>;
}

/// What a prompt recalls with: the vectors known so far and the cue's own vector.
pub struct Recall<'a> {
    pub lookup: &'a dyn VectorLookup,
    pub cue: Option<&'a [f32]>,
}

impl Recall<'_> {
    /// How close a memory is to the cue in meaning, as relevance from 0 to 1.5. `None`
    /// without both vectors, or when their sizes differ (another model's), so the
    /// caller falls back to shared words for that memory.
    pub fn meaning(&self, text: &str) -> Option<f64> {
        let cue = self.cue?;
        let vector = self.lookup.vector(text)?;
        (vector.len() == cue.len()).then(|| ((cosine(cue, &vector) - 0.2) / 0.6).clamp(0.0, 1.0) * 1.5)
    }
}

/// The vectors the brain has made under one model, by [`text_key`].
pub struct VectorCache {
    model: String,
    vectors: Mutex<HashMap<i64, Arc<[f32]>>>,
}

impl VectorCache {
    pub fn new(model: &str) -> Self {
        Self { model: model.to_owned(), vectors: Mutex::new(HashMap::new()) }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn key(&self, text: &str) -> i64 {
        text_key(&self.model, text)
    }

    pub fn insert(&self, key: i64, vector: Arc<[f32]>) {
        self.vectors.lock().insert(key, vector);
    }

    pub fn contains(&self, key: i64) -> bool {
        self.vectors.lock().contains_key(&key)
    }

    /// Keeps only these keys (the memories of the living), so the forgotten and the dead do not pile up.
    pub fn retain(&self, keys: &HashSet<i64>) {
        self.vectors.lock().retain(|key, _| keys.contains(key));
    }

    pub fn len(&self) -> usize {
        self.vectors.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.lock().is_empty()
    }
}

impl VectorLookup for VectorCache {
    fn vector(&self, text: &str) -> Option<Arc<[f32]>> {
        self.vectors.lock().get(&self.key(text)).cloned()
    }
}

/// What one indexing pass did: the new vectors to keep, and the usage to count.
#[derive(Debug, Default)]
pub struct Indexed {
    pub added: Vec<(i64, Arc<[f32]>)>,
    pub requests: u64,
    pub failures: u64,
    pub tokens: u64,
    pub error: Option<String>,
}

/// The embedder and the vectors it has made: what the brain recalls with.
pub struct MemoryIndex {
    embedder: Embedder,
    cache: VectorCache,
}

impl MemoryIndex {
    pub fn new(embedder: Embedder) -> Self {
        let cache = VectorCache::new(embedder.model());
        Self { embedder, cache }
    }

    pub fn cache(&self) -> &VectorCache {
        &self.cache
    }

    /// The vector for a prompt's cue. `None` when the cue is blank, or the endpoint fails
    /// or takes longer than `wait`; the prompt then recalls by shared words.
    pub async fn cue(&self, text: &str, wait: Duration) -> Option<Arc<[f32]>> {
        if text.trim().is_empty() {
            return None;
        }
        let embedded = tokio::time::timeout(wait, self.embedder.embed(&[text.to_owned()])).await.ok()?.ok()?;
        embedded.vectors.into_iter().next().map(Arc::from)
    }

    /// Embeds the texts that have no vector yet, [`BATCH`] at a time and at most
    /// `max_batches` requests, and adds them to the cache. Stops at the first failure.
    pub async fn index(&self, texts: &[String], max_batches: usize) -> Indexed {
        let mut seen = HashSet::new();
        let missing: Vec<String> = texts
            .iter()
            .filter(|text| {
                let key = self.cache.key(text);
                !self.cache.contains(key) && seen.insert(key)
            })
            .cloned()
            .collect();
        let mut indexed = Indexed::default();
        for chunk in missing.chunks(BATCH).take(max_batches) {
            match self.embedder.embed(chunk).await {
                Ok(embedded) => {
                    indexed.requests += 1;
                    indexed.tokens += embedded.tokens;
                    for (text, vector) in chunk.iter().zip(embedded.vectors) {
                        let (key, vector) = (self.cache.key(text), Arc::<[f32]>::from(vector));
                        self.cache.insert(key, vector.clone());
                        indexed.added.push((key, vector));
                    }
                }
                Err(error) => {
                    indexed.failures += 1;
                    indexed.error = Some(error);
                    break;
                }
            }
        }
        indexed
    }
}
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cd server && cargo test -p fsw-brain --lib recall`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add server/crates/brain/src/recall.rs server/crates/brain/src/lib.rs
git commit -m "Keep memory vectors in a cache the brain can recall with" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Score memories by meaning in retrieval and the prompts

**Files:**
- Modify: `server/crates/brain/src/retrieval.rs` (all of it: header, `retrieve`, and a new test module)
- Modify: `server/crates/brain/src/prompts.rs` (`decide_prompt` at line 478, `plan_prompt` at 630, `reply_prompt` at 861, and the other `retrieve` calls at 789 and 928)
- Modify: `server/crates/brain/tests/prompts.rs` (the decide, plan and reply calls), `server/crates/brain/src/bin/probe.rs` (lines 50 and 68)
- Modify: `server/crates/brain/src/brain.rs` (the three prompt calls get `None` for now; Task 5 replaces them)

**Interfaces:**
- Consumes: `Recall` (Task 3).
- Produces:
  - `retrieve(c, tick, cue, count, recall: Option<&Recall>) -> Vec<&Memory>`
  - `decide_prompt(state, c, max_tokens, recall: Option<&Recall>)`
  - `plan_prompt(state, c, max_tokens, recall: Option<&Recall>)`
  - `reply_prompt(state, convo, lines, me, other, max_tokens, recall: Option<&Recall>)`
  - `pub fn decide_cue(state: &SimState, c: &Character) -> String`
  - `pub fn plan_cue(c: &Character) -> String`
  - `pub fn reply_cue(lines: &[ConversationLine]) -> String`

- [ ] **Step 1: Write the failing tests** at the end of `retrieval.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fsw_sim::create_state;

    use super::*;
    use crate::recall::{Recall, VectorCache};

    const NOW: i64 = 10 * TICKS_PER_DAY;

    fn memory(days_ago: i64, text: &str, importance: i64) -> Memory {
        let tick = NOW - days_ago * TICKS_PER_DAY;
        Memory { tick, day: tick / TICKS_PER_DAY, text: text.into(), importance, kind: MemoryKind::Event, subjects: vec![] }
    }

    fn villager(memories: Vec<Memory>) -> Character {
        let state = create_state(1);
        let mut c = state.characters.values().next().unwrap().clone();
        c.memories = memories;
        c
    }

    const FLAMES: &str = "Carried water while the flames took the mill roof.";
    const BREAD: &str = "Bought fresh bread from the baker.";

    #[test]
    fn meaning_finds_an_older_memory_that_shares_no_words_with_the_cue() {
        let c = villager(vec![memory(3, FLAMES, 3), memory(0, BREAD, 5)]);
        let cue = Cue { subjects: &[], text: "Smoke over the village" };
        assert_eq!(retrieve(&c, NOW, cue, 1, None)[0].text, BREAD);

        let cache = VectorCache::new("m");
        cache.insert(cache.key(FLAMES), Arc::from(vec![1.0f32, 0.0]));
        cache.insert(cache.key(BREAD), Arc::from(vec![0.0f32, 1.0]));
        let vector = [1.0f32, 0.0];
        let recall = Recall { lookup: &cache, cue: Some(&vector) };
        assert_eq!(retrieve(&c, NOW, Cue { subjects: &[], text: "Smoke over the village" }, 1, Some(&recall))[0].text, FLAMES);
    }

    #[test]
    fn without_vectors_the_result_is_the_keyword_result() {
        let c = villager(vec![memory(0, "The mill fire spread fast.", 3), memory(0, BREAD, 5), memory(2, FLAMES, 7)]);
        let text = "the mill fire";
        let keyword: Vec<&str> = retrieve(&c, NOW, Cue { subjects: &[], text }, 2, None).iter().map(|m| m.text.as_str()).collect();

        let empty = VectorCache::new("m");
        let vector = [1.0f32, 0.0];
        let no_vectors = Recall { lookup: &empty, cue: Some(&vector) };
        let with_empty: Vec<&str> =
            retrieve(&c, NOW, Cue { subjects: &[], text }, 2, Some(&no_vectors)).iter().map(|m| m.text.as_str()).collect();
        assert_eq!(with_empty, keyword);

        // Vectors from another model (another size) count as none.
        let other = VectorCache::new("m");
        for m in &c.memories {
            other.insert(other.key(&m.text), Arc::from(vec![1.0f32, 0.0, 0.0]));
        }
        let mismatched = Recall { lookup: &other, cue: Some(&vector) };
        let with_other: Vec<&str> =
            retrieve(&c, NOW, Cue { subjects: &[], text }, 2, Some(&mismatched)).iter().map(|m| m.text.as_str()).collect();
        assert_eq!(with_other, keyword);
    }
}
```

- [ ] **Step 2: Run the tests and check that they fail**

Run: `cd server && cargo test -p fsw-brain --lib retrieval`
Expected: a compile error: `retrieve` takes 4 arguments, but 5 were supplied.

- [ ] **Step 3: Implement the change.** In `retrieval.rs`:
- Replace the two header lines with:

```rust
//! Which memories a prompt recalls (port of `src/brain/memory/retrieval.ts`).
//! Relevance is the people involved plus either meaning (with embeddings, see
//! [`crate::recall`]) or shared words, memory by memory.
```

- Add `use crate::recall::Recall;`.
- Change the signature to `pub fn retrieve<'c>(c: &'c Character, tick: i64, cue: Cue, count: usize, recall: Option<&Recall>) -> Vec<&'c Memory>`.
- Replace the `if !cue_words.is_empty() { ... }` block with:

```rust
            match recall.and_then(|recall| recall.meaning(&memory.text)) {
                Some(meaning) => relevance += meaning,
                None if !cue_words.is_empty() => {
                    let shared = words(&memory.text).iter().filter(|w| cue_words.contains(*w)).count();
                    relevance += (shared as f64 * 0.34).min(1.0);
                }
                None => {}
            }
```

In `prompts.rs`:
- Import `use crate::recall::Recall;` and add `ConversationLine` to the `fsw_sim::state` import if it is not there.
- Add, above `decide_prompt`:

```rust
/// What a decision is about, in words: recent events and where the villager stands.
pub fn decide_cue(state: &SimState, c: &Character) -> String {
    format!("{} {}", recent_events(state, c).join(" "), place_name_at(c.pos))
}

/// What a day's plan is about, for recalling by meaning only: the villager's goals.
pub fn plan_cue(c: &Character) -> String {
    c.profile.goals.join("; ")
}

/// What has been said so far in a conversation.
pub fn reply_cue(lines: &[ConversationLine]) -> String {
    lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>().join(" ")
}
```

- `decide_prompt(state: &SimState, c: &Character, max_tokens: usize, recall: Option<&Recall>)`: replace `let cue_text = format!(...)` with `let cue_text = decide_cue(state, c);`. Leave `let events = recent_events(state, c);` where it is, because the prompt uses it later. Pass `recall` as the last argument of its `retrieve` call.
- `plan_prompt(..., max_tokens: usize, recall: Option<&Recall>)`: its `retrieve` call gets `recall`. The cue text stays `""`, so keyword recall is unchanged.
- `reply_prompt(..., max_tokens: usize, recall: Option<&Recall>)`: its `retrieve` call gets `recall`. Replace the local `said` with `let said = reply_cue(lines);`.
- `conversation_prompt` (line 789) and `mercy_prompt` (line 928): add `None` as the last argument of their `retrieve` calls.
- Everywhere else that calls the three changed prompts gets `None` as the last argument:
  - `brain.rs`: `decide_prompt(state, me, budget, None)`, `plan_prompt(state, me, budget, None)`, and `reply_prompt(state, convo, &lines, me, other, budget, None)`
  - `bin/probe.rs`: lines 50 and 68
  - `tests/prompts.rs`: the `"decide"`, `"plan"` and `"reply"` arms

- [ ] **Step 4: Run the tests and check that they pass, with the prompt fixture unchanged**

Run: `cd server && cargo test -p fsw-brain`
Expected: all pass, including `tests/prompts.rs`. Then `git status --short server/crates/brain/tests/fixtures` should print nothing.

- [ ] **Step 5: Commit**

```bash
git add server/crates/brain
git commit -m "Score memories by meaning when the brain has vectors, by shared words otherwise" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: The brain embeds a cue before it builds a prompt

**Files:**
- Modify: `server/crates/brain/src/brain.rs`:
  - imports, constants and `BrainInner`
  - `Brain::new`
  - new `set_memory`
  - `request` (line 487)
  - every `self.request(` call: decide (767), react (818), plan (851), reflect (891), converse (~975), reply (1024), mercy (1100) and billboard (~1150)
- Test: `server/crates/brain/tests/brain.rs`

**Interfaces:**
- Consumes: `MemoryIndex`, `Recall` (Task 3); `decide_cue`, `plan_cue`, `reply_cue` and the new prompt signatures (Task 4).
- Produces: `Brain::set_memory(&self, memory: Option<Arc<MemoryIndex>>)`.

- [ ] **Step 1: Write the failing tests** at the end of `tests/brain.rs`. Also add these imports at the top:
  - `use fsw_brain::llm::embed::Embedder;`
  - `use fsw_brain::recall::MemoryIndex;`
  - `use fsw_brain::settings::EmbedSettings;`
  - `use fsw_sim::state::{Memory, MemoryKind};`

```rust
const FLAMES: &str = "Carried buckets until the flames at the red cottage died down.";

/// Gives Aldric twenty ordinary memories from yesterday and one old one about a fire.
fn remember_a_fire(h: &Harness) -> (String, Vec<String>) {
    let id = h.who("Aldric");
    let mut sim = h.link.sim.lock();
    let tick = sim.state.tick;
    let memory = |days: i64, text: String, importance: i64| Memory {
        tick: tick - days * TICKS_PER_DAY,
        day: (tick - days * TICKS_PER_DAY) / TICKS_PER_DAY,
        text,
        importance,
        kind: MemoryKind::Event,
        subjects: vec![],
    };
    let ordinary: Vec<String> = (0..20).map(|i| format!("Mended fence post number {i} on the farm.")).collect();
    let c = sim.state.characters.get_mut(&id).unwrap();
    c.memories = vec![memory(3, FLAMES.into(), 2)];
    c.memories.extend(ordinary.iter().map(|text| memory(1, text.clone(), 6)));
    (id, ordinary)
}

fn memory_index(endpoint: Arc<FakeTransport>) -> Arc<MemoryIndex> {
    let settings = EmbedSettings { base_url: "http://embed.test/v1".into(), model: "e".into() };
    Arc::new(MemoryIndex::new(Embedder::new(settings, ApiKey::default(), endpoint)))
}

#[tokio::test]
async fn recalls_a_memory_by_meaning_when_embeddings_are_on() {
    let mut h = harness(continue_otherwise(|_, _| None), ConversationMode::Batched);
    h.settle().await;
    let (id, ordinary) = remember_a_fire(&h);
    // Every cue means "fire"; the fire memory does too, the fences do not.
    let endpoint = FakeTransport::always(200, json!({ "data": [{ "index": 0, "embedding": [1.0, 0.0] }] }));
    let index = memory_index(endpoint.clone());
    index.cache().insert(index.cache().key(FLAMES), Arc::from(vec![1.0f32, 0.0]));
    for text in &ordinary {
        index.cache().insert(index.cache().key(text), Arc::from(vec![0.0f32, 1.0]));
    }
    h.brain.set_memory(Some(index));
    h.stimulate(&id, "Smoke is rising from a house nearby.");
    h.brain.update(true);
    h.wait().await;
    h.step(1);
    let log = h.log("Aldric", RequestKind::Decide).unwrap();
    assert!(log.prompt.contains(FLAMES), "{}", log.prompt);
    assert!(endpoint.requests().iter().any(|r| r.url.ends_with("/embeddings")));
}

#[tokio::test]
async fn recalls_by_words_when_embeddings_are_off() {
    let mut h = harness(continue_otherwise(|_, _| None), ConversationMode::Batched);
    h.settle().await;
    let (id, _) = remember_a_fire(&h);
    h.stimulate(&id, "Smoke is rising from a house nearby.");
    h.brain.update(true);
    h.wait().await;
    h.step(1);
    let log = h.log("Aldric", RequestKind::Decide).unwrap();
    assert!(!log.prompt.contains(FLAMES));
}

#[tokio::test(start_paused = true)]
async fn a_hung_embedding_endpoint_does_not_hold_up_a_decision() {
    let mut h = harness(continue_otherwise(|_, _| None), ConversationMode::Batched);
    h.settle().await;
    let (id, _) = remember_a_fire(&h);
    let hung = FakeTransport::new(|_| std::future::pending::<Result<HttpResponse, String>>());
    h.brain.set_memory(Some(memory_index(hung)));
    h.stimulate(&id, "Smoke is rising from a house nearby.");
    h.brain.update(true);
    tokio::time::sleep(Duration::from_secs(3)).await;
    h.wait().await;
    h.step(1);
    let log = h.log("Aldric", RequestKind::Decide).unwrap();
    assert_eq!(log.status, LogStatus::Ok);
    assert!(!log.prompt.contains(FLAMES));
}
```

- [ ] **Step 2: Run the tests and check that they fail**

Run: `cd server && cargo test -p fsw-brain --test brain recall`
Expected: a compile error: no method `set_memory` on `Brain`.

- [ ] **Step 3: Implement the change** in `brain.rs`:

Imports: add `use crate::recall::{MemoryIndex, Recall};` and add `decide_cue, plan_cue, reply_cue` to the `crate::prompts::{...}` import.

Constants, after `INITIAL_BUDGET`:

```rust
/// How long a request waits for its cue's vector before recalling by shared words instead.
const CUE_WAIT: Duration = Duration::from_secs(2);
```

`BrainInner`: add the field

```rust
    /// the embedding model and its vectors, when memories are recalled by meaning
    memory: Option<Arc<MemoryIndex>>,
```

and `memory: None,` in `Brain::new`.

After `set_initial_budget_for_tests`:

```rust
    /// Recalls memories by meaning with this index from now on (`None`: by shared words).
    pub fn set_memory(&self, memory: Option<Arc<MemoryIndex>>) {
        self.inner.state.lock().memory = memory;
    }
```

Next to `type OnFail`:

```rust
/// The text a request's memories should be close to, read from the world when the request is sent.
type CueFn = Box<dyn FnOnce(&SimState) -> Option<String> + Send>;
```

`request`: add the parameter `cue: Option<CueFn>` right after `ttl: Duration`. Change `build` to `impl FnOnce(&SimState, Option<&Recall>) -> Option<Built<T>> + Send + 'static`. Replace the first lines of the job body, from `let link = ...` through the `let Some(built) = read(&*link, build) else {` line, with:

```rust
            let link = brain.inner.link.clone();
            let memory = brain.inner.state.lock().memory.clone();
            let cue_vector = match (cue, &memory) {
                (Some(cue), Some(memory)) => match read(&*link, cue) {
                    Some(text) => memory.cue(&text, CUE_WAIT).await,
                    None => None,
                },
                _ => None,
            };
            let built = {
                let recall = memory.as_deref().map(|m| Recall { lookup: m.cache(), cue: cue_vector.as_deref() });
                read(&*link, |state| build(state, recall.as_ref()))
            };
            let Some(built) = built else {
```

(The rest of the job body is unchanged.)

Call sites. Every `self.request(` gets a cue argument right after its `Duration::from_secs(..)`. Every build closure takes a second parameter:
- **React, reflect, converse, mercy and billboard:** pass `None,` and change `move |state| {` to `move |state, _| {`.
- **Decide** (`request_decide`): add `let cue_id = id.clone();` with the other clones, pass

```rust
            Some(Box::new(move |state: &SimState| state.characters.get(&cue_id).map(|me| decide_cue(state, me)))),
```

  and change the build closure to `move |state, recall| {`, using `decide_prompt(state, me, budget, recall)`.
- **Plan** (`request_plan`): add `let cue_id = id.clone();` and pass `Some(Box::new(move |state: &SimState| state.characters.get(&cue_id).map(plan_cue))),`. The build becomes `move |state, recall|`, using `plan_prompt(state, me, budget, recall)`.
- **Reply** (`request_reply`): add `let (turns_for_cue, cue_id) = (self.clone(), convo_id.clone());` and pass

```rust
            Some(Box::new(move |_: &SimState| turns_for_cue.inner.state.lock().turns.get(&cue_id).map(|lines| reply_cue(lines)))),
```

  The build becomes `move |state, recall|`, using `reply_prompt(state, convo, &lines, me, other, budget, recall)`.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cd server && cargo test -p fsw-brain && cargo clippy -p fsw-brain --all-targets --locked -- -D warnings`
Expected: all pass (including the three new tests and `tests/prompts.rs`), and clippy is clean.

- [ ] **Step 5: Commit**

```bash
git add server/crates/brain
git commit -m "Embed a decision's, plan's or reply's cue before building its prompt" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Keep vectors in the database

**Files:**
- Modify: `server/crates/server/src/store/migrations.rs` (`MIGRATIONS` at line 10, plus a new `V4`)
- Create: `server/crates/server/src/store/vectors.rs`
- Modify: `server/crates/server/src/store/mod.rs` (add `pub mod vectors;`)
- Modify: `server/crates/server/src/store/maintenance.rs` (`PruneReport` and `prune`)
- Modify: `server/crates/server/src/store/brain_log.rs` (`usage_today`, plus a new constant and a test)

**Interfaces:**
- Produces:
  - `Store::load_vectors(&self, model: &str) -> Result<Vec<(i64, Vec<f32>)>>`
  - `Store::put_vectors(&self, model: &str, rows: Vec<(i64, Arc<[f32]>)>) -> Result<()>`
  - `Store::touch_vectors(&self, keys: Vec<i64>, at_ms: i64) -> Result<()>`
  - `pub const VECTORS_TTL_MS: i64`
  - `PruneReport.vectors: usize`
  - `pub const EMBED_USAGE: &str = "embed"` in `store::brain_log`, re-exported from `store`

- [ ] **Step 1: Write the failing tests.** Create `vectors.rs` with the header and tests:

```rust
//! Embedding vectors for memory texts (see `fsw_brain::recall`). They are a cache kept
//! apart from the world: saves and replay never see them, and losing them only costs
//! requests to the embedding endpoint.

#[cfg(test)]
mod tests {
    use super::super::testing;
    use super::*;

    #[tokio::test]
    async fn vectors_are_kept_by_model_and_swept_when_unused() {
        let (store, _dir) = testing::store().await;
        store.put_vectors("e1", vec![(1, Arc::from(vec![0.6f32, 0.8])), (2, Arc::from(vec![1.0f32, 0.0]))]).await.unwrap();
        store.put_vectors("e2", vec![(3, Arc::from(vec![0.0f32, 1.0, 0.0]))]).await.unwrap();
        let mut kept = store.load_vectors("e1").await.unwrap();
        kept.sort_by_key(|(key, _)| *key);
        assert_eq!(kept, vec![(1, vec![0.6, 0.8]), (2, vec![1.0, 0.0])]);
        assert_eq!(store.load_vectors("e2").await.unwrap().len(), 1);
        assert!(store.load_vectors("another").await.unwrap().is_empty());

        // A week on, only what was used lately is left.
        let later = now_ms() + VECTORS_TTL_MS + 1_000;
        store.touch_vectors(vec![1], later).await.unwrap();
        let report = store.prune(later).await.unwrap();
        assert_eq!(report.vectors, 2);
        assert_eq!(store.load_vectors("e1").await.unwrap(), vec![(1, vec![0.6, 0.8])]);
    }
}
```

In `brain_log.rs`'s tests module, add:

```rust
    #[tokio::test]
    async fn embedding_usage_is_kept_apart_from_the_models() {
        let (store, _dir) = testing::store().await;
        store.add_usage("decide", 2, 0, 100).await.unwrap();
        store.add_usage(EMBED_USAGE, 5, 1, 900).await.unwrap();
        let today = store.usage_today().await.unwrap();
        assert_eq!((today.requests_24h, today.failures_24h, today.tokens_24h), (2, 0, 100));
    }
```

Add `pub mod vectors;` to `store/mod.rs`.

- [ ] **Step 2: Run the tests and check that they fail**

Run: `cd server && cargo test -p fsw-server --lib store::`
Expected: a compile error: `put_vectors`, `VECTORS_TTL_MS` and `EMBED_USAGE` are not defined.

- [ ] **Step 3: Implement the change.**

In `migrations.rs`, after `V3`, add the following, and change `MIGRATIONS` to `&[(1, V1), (2, V2), (3, V3), (4, V4)]`:

```rust
/// Embedding vectors for memory texts, keyed by a hash of the model and the text
/// (`fsw_brain::recall::text_key`). A cache: losing it only costs requests.
const V4: &str = "
CREATE TABLE memory_vectors (
    key INTEGER PRIMARY KEY,
    model TEXT NOT NULL,
    dim INTEGER NOT NULL,
    vec BLOB NOT NULL,
    used_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX memory_vectors_by_model ON memory_vectors (model);
CREATE INDEX memory_vectors_by_use ON memory_vectors (used_ms);
";
```

In `vectors.rs`, above the tests:

```rust
use std::sync::Arc;

use super::{DAY_MS, Result, Store, now_ms};

/// Vectors nobody has used for this long are deleted by maintenance.
pub const VECTORS_TTL_MS: i64 = 7 * DAY_MS;

fn to_blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn from_blob(blob: &[u8]) -> Option<Vec<f32>> {
    (blob.len() % 4 == 0).then(|| blob.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

impl Store {
    /// Every kept vector made under this model, by key.
    pub async fn load_vectors(&self, model: &str) -> Result<Vec<(i64, Vec<f32>)>> {
        let model = model.to_owned();
        self.read(move |conn| {
            let mut stmt = conn.prepare("SELECT key, vec FROM memory_vectors WHERE model = ?1")?;
            let rows = stmt.query_map([&model], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
            let mut vectors = Vec::new();
            for row in rows {
                let (key, blob) = row?;
                if let Some(vector) = from_blob(&blob) {
                    vectors.push((key, vector));
                }
            }
            Ok(vectors)
        })
        .await
    }

    /// Keeps new vectors, marked as used now.
    pub async fn put_vectors(&self, model: &str, rows: Vec<(i64, Arc<[f32]>)>) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let model = model.to_owned();
        self.write(move |conn| {
            let tx = conn.transaction()?;
            {
                let mut put = tx.prepare_cached(
                    "INSERT INTO memory_vectors (key, model, dim, vec, used_ms) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (key) DO UPDATE SET model = excluded.model, dim = excluded.dim,
                         vec = excluded.vec, used_ms = excluded.used_ms",
                )?;
                let now = now_ms();
                for (key, vector) in &rows {
                    put.execute((key, &model, vector.len() as i64, to_blob(vector), now))?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Marks these vectors as still in use, so maintenance keeps them.
    pub async fn touch_vectors(&self, keys: Vec<i64>, at_ms: i64) -> Result<()> {
        self.write(move |conn| {
            let tx = conn.transaction()?;
            {
                let mut touch = tx.prepare_cached("UPDATE memory_vectors SET used_ms = ?2 WHERE key = ?1")?;
                for key in &keys {
                    touch.execute((key, at_ms))?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }
}
```

In `maintenance.rs`:
- Import `use super::vectors::VECTORS_TTL_MS;`.
- Add `/// memory vectors unused for a week` and `pub vectors: usize,` to `PruneReport`.
- In `prune`, after the `purchases` line, add:

```rust
            report.vectors = tx.execute("DELETE FROM memory_vectors WHERE used_ms < ?1", [now_ms - VECTORS_TTL_MS])?;
```

If the hourly run logs the report field by field, add `vectors` there too.

In `brain_log.rs`, near the top:

```rust
/// The usage kind for embedding requests, which the language model's daily caps leave out.
pub const EMBED_USAGE: &str = "embed";
```

In `usage_today`, bind `let rows: Vec<UsageRow> = rows.into_iter().filter(|r| r.kind != EMBED_USAGE).collect();` before summing. Then re-export it from `store/mod.rs`: `pub use brain_log::{BrainLogWrite, EMBED_USAGE, UsageRow};`.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cd server && cargo test -p fsw-server --lib store::`
Expected: all pass, including `migrations::tests::applies_once` (it counts `MIGRATIONS`).

- [ ] **Step 5: Commit**

```bash
git add server/crates/server/src/store
git commit -m "Keep memory vectors in the database and sweep those unused for a week" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: The indexer and the server wiring

**Files:**
- Modify: `server/crates/server/src/brain_task.rs` (imports, constants, and new functions after `start`)
- Modify: `server/crates/server/src/main.rs` (log line near 43, wiring near 62, the shutdown joins)

**Interfaces:**
- Consumes:
  - `MemoryIndex`, `Embedder` (Tasks 2 and 3)
  - `Brain::set_memory` (Task 5)
  - `Store::{load_vectors, put_vectors, touch_vectors, add_usage}` and `EMBED_USAGE` (Task 6)
  - `Config.embed` and `Config.embed_api_key` (Task 1)
- Produces:
  - `brain_task::start_memory(store: &Store, settings: EmbedSettings, key: ApiKey) -> Option<Arc<MemoryIndex>>`
  - `brain_task::index_once(index: &MemoryIndex, world: &World, store: &Store) -> usize`
  - `brain_task::spawn_indexer(index, world, store, shutdown) -> JoinHandle<()>`

- [ ] **Step 1: Write the failing test** at the end of `brain_task.rs`:

```rust
#[cfg(test)]
mod tests {
    use fsw_brain::llm::transport::{FakeTransport, HttpRequest, HttpResponse};
    use fsw_sim::state::{Memory, MemoryKind};
    use serde_json::json;

    use super::*;
    use crate::store::{testing, utc_day};

    #[tokio::test]
    async fn indexes_the_memories_of_the_living_once_and_keeps_them() {
        let (store, _dir) = testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = crate::world_task::start(store.clone(), 0, shutdown.clone()).await.unwrap();
        {
            let mut core = world.core.write();
            let c = core.sim.state.characters.values_mut().next().unwrap();
            let memory = Memory {
                tick: 0,
                day: 0,
                text: "Watched the bonfire on the green.".into(),
                importance: 4,
                kind: MemoryKind::Event,
                subjects: vec![],
            };
            c.memories.push(memory);
        }
        let endpoint = FakeTransport::new(|request: HttpRequest| {
            let count = request.json()["input"].as_array().map_or(0, Vec::len);
            let data: Vec<_> = (0..count).map(|i| json!({ "index": i, "embedding": [1.0, i as f64 + 1.0] })).collect();
            let response = HttpResponse::json(200, &json!({ "data": data, "usage": { "total_tokens": 7 } }));
            async move { Ok(response) }
        });
        let settings = EmbedSettings { base_url: "http://embed.test/v1".into(), model: "e1".into() };
        let index = MemoryIndex::new(Embedder::new(settings, ApiKey::default(), endpoint.clone()));

        let mut passes = 0;
        while index_once(&index, &world, &store).await > 0 {
            passes += 1;
            assert!(passes < 100, "the index never fills");
        }
        let requests = endpoint.requests().len();
        assert!(requests > 0);
        assert_eq!(index_once(&index, &world, &store).await, 0);
        assert_eq!(endpoint.requests().len(), requests, "nothing is embedded twice");
        assert!(index.cache().vector("Watched the bonfire on the green.").is_some());
        assert_eq!(store.load_vectors("e1").await.unwrap().len(), index.cache().len());

        let today = utc_day(now_ms());
        let usage = store.usage(today, today).await.unwrap();
        assert!(usage.iter().any(|row| row.kind == EMBED_USAGE && row.requests == requests as u64));
        assert_eq!(store.usage_today().await.unwrap().requests_24h, 0, "embeddings do not count against the model");

        shutdown.cancel();
        let _ = task.await;
    }
}
```

If `testing` and `utc_day` are not visible at that path, use `crate::store::testing` (it is `pub(crate)`) and `crate::store::utc_day` as `brain_task.rs` already imports it.

- [ ] **Step 2: Run the test and check that it fails**

Run: `cd server && cargo test -p fsw-server --lib brain_task`
Expected: a compile error: `index_once` is not defined.

- [ ] **Step 3: Implement the change** in `brain_task.rs`.

Imports:

```rust
use std::collections::HashSet;

use fsw_brain::llm::embed::Embedder;
use fsw_brain::recall::MemoryIndex;
use fsw_brain::settings::EmbedSettings;
use fsw_sim::util::living;
use crate::store::EMBED_USAGE;
```

Merge `EmbedSettings` into the existing `fsw_brain::settings::{ApiKey, LlmSettings}` import.

Constants:

```rust
/// How often the indexer looks for memories without a vector.
const INDEX_EVERY: Duration = Duration::from_secs(5);
/// Requests per pass, so a fresh index fills over a few minutes instead of in one burst.
const INDEX_BATCHES: usize = 4;
/// How often kept vectors are marked as in use and the cache is trimmed to the memories of the living.
const TOUCH_EVERY: Duration = Duration::from_secs(3600);
```

After `start`:

```rust
/// The embedding model with the vectors kept for it, or `None` when `EMBED_*` is not set.
pub async fn start_memory(store: &Store, settings: EmbedSettings, key: ApiKey) -> Option<Arc<MemoryIndex>> {
    if !settings.enabled() {
        return None;
    }
    let index = Arc::new(MemoryIndex::new(Embedder::new(settings, key, transport())));
    match store.load_vectors(index.cache().model()).await {
        Ok(rows) => {
            for (key, vector) in rows {
                index.cache().insert(key, Arc::from(vector));
            }
        }
        Err(error) => tracing::error!(%error, "cannot load the memory vectors; they will be made again"),
    }
    tracing::info!(vectors = index.cache().len(), "memory index loaded");
    Some(index)
}

/// The memory texts of the living: what the brain may recall.
fn memory_texts(world: &World) -> Vec<String> {
    let core = world.core.read();
    living(&core.sim.state).flat_map(|c| c.memories.iter().map(|m| m.text.clone())).collect()
}

/// One indexer pass: embeds what has no vector yet, keeps it and counts the usage. Returns how many were added.
pub async fn index_once(index: &MemoryIndex, world: &World, store: &Store) -> usize {
    let texts = memory_texts(world);
    let indexed = index.index(&texts, INDEX_BATCHES).await;
    if let Some(error) = &indexed.error {
        tracing::warn!(%error, "embedding memories failed; recall falls back to shared words meanwhile");
    }
    let added = indexed.added.len();
    if let Err(error) = store.put_vectors(index.cache().model(), indexed.added).await {
        tracing::error!(%error, "cannot keep memory vectors");
    }
    if indexed.requests + indexed.failures > 0
        && let Err(error) = store.add_usage(EMBED_USAGE, indexed.requests, indexed.failures, indexed.tokens).await
    {
        tracing::error!(%error, "cannot count embedding usage");
    }
    added
}

/// Marks the vectors of the living as used and forgets the rest from memory.
async fn touch(index: &MemoryIndex, world: &World, store: &Store) {
    let keys: HashSet<i64> = memory_texts(world).iter().map(|text| index.cache().key(text)).collect();
    index.cache().retain(&keys);
    if let Err(error) = store.touch_vectors(keys.into_iter().collect(), now_ms()).await {
        tracing::error!(%error, "cannot mark memory vectors as used");
    }
}

/// Keeps the index filled until shutdown.
pub fn spawn_indexer(
    index: Arc<MemoryIndex>,
    world: Arc<World>,
    store: Store,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(INDEX_EVERY);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut touching = tokio::time::interval_at(tokio::time::Instant::now() + TOUCH_EVERY, TOUCH_EVERY);
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                _ = every.tick() => { index_once(&index, &world, &store).await; }
                _ = touching.tick() => touch(&index, &world, &store).await,
            }
        }
    })
}
```

In `main.rs`:
- After the `language model on/off` log, add:

```rust
    if config.embed.enabled() {
        let embed = &config.embed;
        tracing::info!(base_url = %embed.base_url, model = %embed.model, key = !config.embed_api_key.is_empty(), "memory by meaning on");
    }
```

- After `let brain = brain_task::start(...)`, add:

```rust
    let memory = brain_task::start_memory(&store, config.embed.clone(), config.embed_api_key.clone()).await;
    brain.set_memory(memory.clone());
    let indexing = memory.map(|index| brain_task::spawn_indexer(index, world.clone(), store.clone(), shutdown.clone()));
```

- After `let _ = thinking.await;`, add:

```rust
    if let Some(indexing) = indexing {
        let _ = indexing.await;
    }
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cd server && cargo test -p fsw-server --lib brain_task && cargo clippy -p fsw-server --all-targets --locked -- -D warnings`
Expected: PASS, and clippy is clean (no lock held across an `.await`: `memory_texts` drops its guard before returning).

- [ ] **Step 5: Commit**

```bash
git add server/crates/server/src/brain_task.rs server/crates/server/src/main.rs
git commit -m "Embed the memories of the living in the background and hand the index to the brain" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: The admin view and the connection test

**Files:**
- Modify: `server/crates/server/src/api/wire.rs` (`LlmSettingsView`, around line 920)
- Modify: `server/crates/server/src/api/llm.rs` (`view`, `get_settings`, `test`, plus a new `test_embeddings`)
- Regenerate: `src/generated/api.ts` (`cd server && cargo run --bin export-api`)
- Modify: `src/ui/Admin.tsx` (`LlmSection`, lines 149–196)
- Test: `server/crates/server/tests/http.rs`

**Interfaces:**
- Consumes: `Config.embed`, `Config.embed_api_key`, `Embedder`.
- Produces: `LlmSettingsView.embed_base_url: Option<String>`, `embed_model: Option<String>` and `embed_api_key_set: bool` (TypeScript: `embedBaseUrl`, `embedModel`, `embedApiKeySet`).

- [ ] **Step 1: Write the failing test** in `tests/http.rs`, after `model_settings_come_from_the_environment_and_cannot_be_changed`:

```rust
#[tokio::test]
async fn embedding_settings_are_shown_without_their_key_and_tested() {
    let env = [
        ("AUTH_TOKEN", TOKEN),
        ("EMBED_BASE_URL", "http://127.0.0.1:9/v1"),
        ("EMBED_MODEL", "nomic-embed-text"),
        ("EMBED_API_KEY", "sk-embed-secret"),
    ];
    let (app, _, _data) = app_with("embed", &env).await;
    let current = text(app.clone().oneshot(authed("/api/admin/settings/llm")).await.unwrap()).await;
    assert!(current.contains(r#""embedModel":"nomic-embed-text""#), "{current}");
    assert!(current.contains(r#""embedBaseUrl":"http://127.0.0.1:9/v1""#), "{current}");
    assert!(current.contains(r#""embedApiKeySet":true"#) && !current.contains("sk-embed-secret"), "{current}");

    // Nothing listens there: the test says so, without the key.
    let test = text(app.oneshot(json_request("POST", "/api/admin/llm/test", "")).await.unwrap()).await;
    assert!(test.contains(r#""ok":false"#) && test.contains("Embeddings failed"), "{test}");
    assert!(!test.contains("sk-embed-secret"), "{test}");
}
```

In `the_model_is_off_without_its_environment`, add:

```rust
    assert!(current.contains(r#""embedModel":null"#) && current.contains(r#""embedApiKeySet":false"#), "{current}");
```

- [ ] **Step 2: Run the tests and check that they fail**

Run: `cd server && cargo test -p fsw-server --test http embed`
Expected: FAIL: `embedModel` is missing from the view.

- [ ] **Step 3: Implement the change.**

In `wire.rs`, append to `LlmSettingsView`:

```rust
    /// the embedding model's endpoint (`EMBED_BASE_URL`), when memories are recalled by meaning
    pub embed_base_url: Option<String>,
    /// `EMBED_MODEL`, or none when memories are recalled by shared words
    pub embed_model: Option<String>,
    pub embed_api_key_set: bool,
```

In `llm.rs`:
- Imports: `use fsw_brain::llm::embed::Embedder;` and add `EmbedSettings` to the settings import.
- Change `view` to `fn view(settings: &LlmSettings, key: &ApiKey, embed: &EmbedSettings, embed_key: &ApiKey) -> LlmSettingsView` and add to the struct literal:

```rust
        embed_base_url: embed.enabled().then(|| embed.base_url.clone()),
        embed_model: embed.enabled().then(|| embed.model.clone()),
        embed_api_key_set: !embed_key.is_empty(),
```

- `get_settings`: `Json(view(&app.config.llm, &app.config.llm_api_key, &app.config.embed, &app.config.embed_api_key))`.
- Replace `test` with:

```rust
/// `POST /api/admin/llm/test`: asks the configured model for one decision and the embedding
/// model for one vector. Changes nothing.
pub async fn test(State(app): State<Arc<AppState>>) -> Json<LlmTestResult> {
    let (settings, embed) = (&app.config.llm, &app.config.embed);
    if !settings.enabled && !embed.enabled() {
        return Json(LlmTestResult { ok: false, message: "Set LLM_BASE_URL and LLM_MODEL on the server.".into() });
    }
    let (mut ok, mut parts) = (true, Vec::new());
    if settings.enabled {
        let result = probe(settings, app.config.llm_api_key.clone(), transport()).await;
        ok &= result.ok;
        parts.push(result.message);
    }
    if embed.enabled() {
        let (embedded, message) = test_embeddings(embed, app.config.embed_api_key.clone()).await;
        ok &= embedded;
        parts.push(message);
    }
    Json(LlmTestResult { ok, message: parts.join(" ") })
}

/// Embeds one sentence; says how many numbers a vector has and how long it took.
async fn test_embeddings(settings: &EmbedSettings, key: ApiKey) -> (bool, String) {
    let embedder = Embedder::new(settings.clone(), key, transport());
    let started = std::time::Instant::now();
    match embedder.embed(&["The mill burned down in the night.".to_owned()]).await {
        Ok(embedded) => (
            true,
            format!(
                "Embeddings: {} numbers per memory, in {} ms.",
                embedded.vectors[0].len(),
                started.elapsed().as_millis()
            ),
        ),
        Err(error) => (false, format!("Embeddings failed: {error}")),
    }
}
```

Regenerate the client types: `cd server && cargo run --bin export-api`.

In `Admin.tsx`'s `LlmSection`:
- Before `return (`, add:

```tsx
  const embeddings = view.embedModel ? (
    <li>Memory by meaning: {view.embedModel} at {view.embedBaseUrl} · API key: {view.embedApiKeySet ? 'set' : 'none'}</li>
  ) : (
    <li>Memory by meaning: off (memories are matched by shared words; set <code>EMBED_BASE_URL</code> and <code>EMBED_MODEL</code> to turn it on)</li>
  );
```

- Add `{embeddings}` as the last `<li>` of the enabled `<ul class="slots">`. After the off-paragraph `<p>Off: …</p>`, render `<ul class="slots">{embeddings}</ul>` too, so it shows either way.
- Change the button's `disabled` to `busy || (!view.enabled && !view.embedModel)`.
- Change the busy message to `'Asking the model to make a decision…'` when `view.enabled`, otherwise `'Asking the embedding model for a vector…'`.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cd server && cargo test -p fsw-server && cd .. && pnpm typecheck && pnpm test`
Expected: all pass, including the `fsw-server` test that checks `src/generated/api.ts` is current.

- [ ] **Step 5: Commit**

```bash
git add server/crates/server/src/api src/generated/api.ts src/ui/Admin.tsx server/crates/server/tests/http.rs
git commit -m "Show the embedding model in the admin panel and test it with the connection test" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Mock embeddings, the probe and the docs

**Files:**
- Modify: `tools/mock-llm.mjs` (inside the `request.on('end', ...)` handler)
- Modify: `server/crates/brain/src/bin/probe.rs` (header comment, `main`)
- Modify: `README.md` (the settings table at line 98, plus a new subsection after the language-model notes at about line 137)
- Modify: `docs/deploy.md` (the table at line 42, the paragraph at line 47)
- Modify: `docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md` (a bullet in "Since the server migration")

**Interfaces:**
- Consumes: `Embedder`, `cosine`, `EmbedSettings`.
- Produces: no code interfaces.

- [ ] **Step 1: Add the mock's embeddings.** In `tools/mock-llm.mjs`:
- Add a line to the header comment: `//   POST /v1/embeddings answers at once with vectors from hashed words (fire, flames, smoke… count as one word)`.
- Above `http.createServer`, add:

```js
// Words that should mean the same to the mock embedding model, so recall by meaning can be seen working.
const SAME = { flames: 'fire', blaze: 'fire', burning: 'fire', burnt: 'fire', smoke: 'fire', skeleton: 'dead', skeletons: 'dead', undead: 'dead' };

function embed(text) {
  const vector = new Array(64).fill(0);
  for (const raw of text.toLowerCase().match(/[a-z]{4,}/g) ?? []) {
    const word = SAME[raw] ?? raw;
    let hash = 2166136261;
    for (const ch of word) hash = Math.imul(hash ^ ch.charCodeAt(0), 16777619) >>> 0;
    vector[hash % 64] += 1;
  }
  if (vector.every((x) => x === 0)) vector[0] = 1;
  return vector;
}
```

- In the `end` handler, right after the body parses:

```js
    if (request.url?.endsWith('/embeddings')) {
      const inputs = Array.isArray(body.input) ? body.input : [body.input ?? ''];
      const data = inputs.map((text, index) => ({ object: 'embedding', index, embedding: embed(String(text)) }));
      const tokens = Math.round(raw.length / 4);
      response.writeHead(200, { ...CORS, 'Content-Type': 'application/json' }).end(JSON.stringify({ object: 'list', data, model: body.model ?? 'mock-embed', usage: { prompt_tokens: tokens, total_tokens: tokens } }));
      return;
    }
```

Check it: run `node tools/mock-llm.mjs 8799 &`, then `curl -s localhost:8799/v1/embeddings -d '{"model":"e","input":["fire","flames"]}' | head -c 300`. Expect two `embedding` arrays with their 1 in the same place. Stop the mock afterwards.

- [ ] **Step 2: Add `--embed` to the probe.** In `bin/probe.rs`:
- Header: add `//!   cargo run --release -p fsw-brain --bin probe -- <base-url> <model> [runs] [api-key] --embed <embed-model>` and `//! Options (environment): EMBED_BASE_URL (defaults to <base-url>), EMBED_API_KEY (defaults to the model's key)`.
- Imports:
  - `use fsw_brain::llm::embed::Embedder;`
  - `use fsw_brain::recall::cosine;`
  - add `EmbedSettings` to the settings import
  - `use fsw_brain::llm::transport::LlmTransport;`
- In `main`, make `args` mutable and, before the usage check, take the flag out of it:

```rust
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let embed_model = args.iter().position(|arg| arg == "--embed").and_then(|at| {
        let model = args.get(at + 1).cloned();
        args.drain(at..(at + 2).min(args.len()));
        model
    });
```

- After the `transport` is built, add:

```rust
    let embeddings_ok = match &embed_model {
        Some(model) => {
            let base = std::env::var("EMBED_BASE_URL").unwrap_or_else(|_| base_url.clone());
            let embed_key = std::env::var("EMBED_API_KEY").unwrap_or_else(|_| key.clone());
            check_embeddings(&base, model, &embed_key, transport.clone()).await
        }
        None => true,
    };
```

- Change the final `ExitCode::SUCCESS` to `if embeddings_ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }`.
- Add this function below `main`:

```rust
/// Embeds three sentences and checks that the two about a fire are closer to each other than to bread.
async fn check_embeddings(base_url: &str, model: &str, key: &str, transport: Arc<dyn LlmTransport>) -> bool {
    let settings = EmbedSettings { base_url: base_url.trim_end_matches('/').to_owned(), model: model.to_owned() };
    let embedder = Embedder::new(settings, ApiKey::new(key), transport);
    let texts = [
        "The red cottage burned in the night.",
        "Flames rose from the roof while we carried water.",
        "I bought bread at the market.",
    ]
    .map(str::to_owned);
    let started = std::time::Instant::now();
    match embedder.embed(&texts).await {
        Ok(embedded) => {
            let near = cosine(&embedded.vectors[0], &embedded.vectors[1]);
            let far = cosine(&embedded.vectors[0], &embedded.vectors[2]);
            let verdict = if near > far { "" } else { " (this model does not tell them apart)" };
            println!(
                "embeddings: {} numbers, {} ms; fire~flames {near:.2}, fire~bread {far:.2}{verdict}",
                embedded.vectors[0].len(),
                started.elapsed().as_millis()
            );
            near > far
        }
        Err(error) => {
            println!("embeddings failed: {error}");
            false
        }
    }
}
```

`transport` is an `Arc<ReqwestTransport>`. Pass `transport.clone()`; it coerces to `Arc<dyn LlmTransport>` at the call.

Run: `cd server && cargo build -p fsw-brain --bin probe`
Expected: it builds.

- [ ] **Step 3: Write the docs.**

README, in the settings table after the `LLM_*` row:

```markdown
| `EMBED_*` | off | Memory by meaning ([below](#memory-by-meaning)) |
```

README, a new subsection after the language-model notes (before the next `##` or `###` heading):

```markdown
### Memory by meaning

Villagers recall memories by the people involved and the words they share with the moment. With an embedding model they recall by meaning instead, so a fire brings back "the flames at the mill" even when nobody said "fire". Any OpenAI-compatible `/v1/embeddings` endpoint works: LM Studio, Ollama or a hosted one. It can be a different server from the language model.

| Variable | Default | What it does |
| --- | --- | --- |
| `EMBED_BASE_URL` | none | The embeddings endpoint, e.g. `http://localhost:1234/v1`. Set it together with `EMBED_MODEL`. |
| `EMBED_MODEL` | none | The embedding model's name there, e.g. `text-embedding-nomic-embed-text-v1.5` |
| `EMBED_API_KEY` | none | Its key, if it needs one |

The server embeds each new memory in the background (a few hundred short texts a day for a village), keeps the vectors in the database, and embeds what a decision, a reply or a morning plan is about when it asks the model. If the endpoint is slow (over two seconds) or down, recall falls back to shared words, and nothing else changes. The vectors are a cache: changing the model starts a fresh set, and vectors unused for a week are deleted. Embedding requests do not count against `LLM_MAX_REQUESTS_PER_DAY` or `LLM_MAX_TOKENS_PER_DAY`; **🛠 Admin → Language model → Test connection** tests both models. To check an endpoint first: `cargo run --release -p fsw-brain --bin probe -- <base-url> <model> 4 --embed <embed-model>`.
```

`docs/deploy.md`, in the table at line 42, after the `LLM_API_KEY` row:

```markdown
| `EMBED_API_KEY` | Optional: the embedding endpoint's key. Read at every start and never stored in the database or sent to a browser. |
```

and, at the end of the paragraph at line 47:

```markdown
Memory by meaning is set the same way, with `EMBED_BASE_URL` and `EMBED_MODEL` (and `EMBED_API_KEY` as a secret); see the [README](../README.md#memory-by-meaning).
```

In the design doc, add a bullet to "Since the server migration", after the house fires bullet:

```markdown
- **Memory by meaning** (October 2026): with an embedding model (`EMBED_*`), memories are recalled by how close they are in meaning to the moment, not only by shared words. The vectors live in the server's database, never in the world. See [the memory design](2026-10-04-memory-embeddings-design.md).
```

- [ ] **Step 4: Run the whole check, then an end-to-end run**

```bash
set -o pipefail
cd server && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked
cd .. && pnpm typecheck && pnpm test
```

Expected: everything passes.

End to end, with a scratch data directory:
1. Run `node tools/mock-llm.mjs`.
2. Run `pnpm build`.
3. In `server/`: `AUTH_MODE=off PUBLIC_ORIGIN=http://localhost:8080 STATIC_DIR=../dist DATA_DIR=$(mktemp -d) LLM_BASE_URL=http://localhost:8787/v1 LLM_MODEL=mock EMBED_BASE_URL=http://localhost:8787/v1 EMBED_MODEL=mock-embed cargo run --release`.

Check:
- the log says `memory by meaning on` and, within about a minute, the mock prints no errors
- **🛠 Admin → Language model** shows "Memory by meaning: mock-embed …", and **Test connection** reports "Embeddings: 64 numbers per memory"
- `sqlite3 <DATA_DIR>/world.db 'select count(*) from memory_vectors'` grows
- in the 🧠 decision log, decide prompts still list memories

Then restart with the `EMBED_*` variables removed: the panel says "off" and decisions carry on.

- [ ] **Step 5: Commit**

```bash
git add tools/mock-llm.mjs server/crates/brain/src/bin/probe.rs README.md docs/deploy.md docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md
git commit -m "Document memory by meaning and let the mock and the probe try embeddings" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
