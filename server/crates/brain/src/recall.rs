//! Memories found by meaning: the vectors the brain has for memory texts, and the
//! cue a prompt compares them with. None of this enters the sim's state; it only
//! changes which memories a prompt recalls, so saves and replay never see it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::time::Instant;

use crate::llm::embed::{BATCH, EmbedError, Embedder};

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

/// Embedding requests to count in the usage table. `requests` counts every request sent, failed ones too,
/// as the language model's rows do; a refusal that sent nothing is not a request.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EmbedUsage {
    pub requests: u64,
    pub failures: u64,
    pub tokens: u64,
}

impl EmbedUsage {
    /// Whether there is nothing to count.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Adds another count to this one.
    pub fn add(&mut self, other: EmbedUsage) {
        self.requests += other.requests;
        self.failures += other.failures;
        self.tokens += other.tokens;
    }
}

/// What one indexing pass did: the new vectors to keep, and the usage to count.
#[derive(Debug, Default)]
pub struct Indexed {
    pub added: Vec<(i64, Arc<[f32]>)>,
    pub usage: EmbedUsage,
    /// Why a request failed, if one did.
    pub error: Option<String>,
    /// Whether the embedder refused to send (it is resting or over its limit), which ended the pass early.
    pub refused: bool,
}

/// How long cues are skipped after one fails or times out. A cue that gives up at its wait never lets
/// the embedder's own breaker count a failure, so without a rest every request would wait out a dead endpoint.
const CUE_REST: Duration = Duration::from_secs(60);

/// The embedder and the vectors it has made: what the brain recalls with.
pub struct MemoryIndex {
    embedder: Embedder,
    cache: VectorCache,
    /// until when cues are skipped because the last one failed
    cue_resting_until: Mutex<Option<Instant>>,
    /// the cues' requests since the indexer last took the count; it records them with its own
    cue_usage: Mutex<EmbedUsage>,
}

impl MemoryIndex {
    pub fn new(embedder: Embedder) -> Self {
        let cache = VectorCache::new(embedder.model());
        Self { embedder, cache, cue_resting_until: Mutex::new(None), cue_usage: Mutex::new(EmbedUsage::default()) }
    }

    pub fn cache(&self) -> &VectorCache {
        &self.cache
    }

    /// The vector for a prompt's cue. `None` when the cue is blank, or the endpoint fails
    /// or takes longer than `wait`, and for [`CUE_REST`] after such a failure; the prompt then
    /// recalls by shared words.
    pub async fn cue(&self, text: &str, wait: Duration) -> Option<Arc<[f32]>> {
        if text.trim().is_empty() {
            return None;
        }
        let resting = self.cue_resting_until.lock().is_some_and(|until| Instant::now() < until);
        if resting {
            return None;
        }
        let embedded = tokio::time::timeout(wait, self.embedder.embed(&[text.to_owned()])).await;
        // A cue given up at its wait counts as a failed request even if it was still queued behind the indexer's:
        // from the prompt's side the endpoint did not answer.
        let (vector, usage) = match embedded {
            Ok(Ok(embedded)) => {
                let usage = EmbedUsage { requests: 1, failures: 0, tokens: embedded.tokens };
                (embedded.vectors.into_iter().next(), usage)
            }
            Ok(Err(EmbedError::Refused(_))) => (None, EmbedUsage::default()),
            Ok(Err(EmbedError::Failed(_))) | Err(_) => (None, EmbedUsage { requests: 1, failures: 1, tokens: 0 }),
        };
        self.cue_usage.lock().add(usage);
        *self.cue_resting_until.lock() = if vector.is_some() { None } else { Some(Instant::now() + CUE_REST) };
        vector.map(Arc::from)
    }

    /// The cues' usage since the last call, which the indexer adds to the usage table with its own.
    pub fn take_cue_usage(&self) -> EmbedUsage {
        std::mem::take(&mut *self.cue_usage.lock())
    }

    /// Embeds the texts that have no vector yet, [`BATCH`] at a time and at most
    /// `max_batches` requests, and adds them to the cache. Stops at the first failure, and at a
    /// refusal, which counts as nothing because nothing was sent.
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
                    indexed.usage.add(EmbedUsage { requests: 1, failures: 0, tokens: embedded.tokens });
                    for (text, vector) in chunk.iter().zip(embedded.vectors) {
                        let (key, vector) = (self.cache.key(text), Arc::<[f32]>::from(vector));
                        self.cache.insert(key, vector.clone());
                        indexed.added.push((key, vector));
                    }
                }
                Err(EmbedError::Refused(_)) => {
                    indexed.refused = true;
                    break;
                }
                Err(EmbedError::Failed(error)) => {
                    indexed.usage.add(EmbedUsage { requests: 1, failures: 1, tokens: 0 });
                    indexed.error = Some(error);
                    break;
                }
            }
        }
        indexed
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::json;

    use super::{EmbedUsage, MemoryIndex, Recall, VectorCache, VectorLookup, text_key};
    use crate::llm::embed::{BATCH, Embedder};
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
        assert_eq!((indexed.added.len(), indexed.usage.requests, indexed.usage.tokens), (2, 1, 3));
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
        assert!(indexed.added.is_empty() && indexed.error.is_some() && !indexed.refused);
        assert_eq!((indexed.usage.requests, indexed.usage.failures), (1, 1));
        assert!(failing.cache().is_empty());
    }

    #[tokio::test]
    async fn a_pass_the_resting_embedder_refuses_counts_nothing_and_sends_nothing() {
        let failing = endpoint(true);
        let index = index(failing.clone());
        let texts = vec!["smoke".to_owned()];
        // Four failures in a row put the embedder to rest.
        for _ in 0..4 {
            assert_eq!(index.index(&texts, 4).await.usage.failures, 1);
        }
        let refused = index.index(&texts, 4).await;
        assert!(refused.refused && refused.error.is_none());
        assert_eq!(refused.usage, EmbedUsage::default());
        assert_eq!(failing.requests().len(), 4);
    }

    #[tokio::test]
    async fn cues_count_their_requests_until_the_count_is_taken() {
        let index = index(endpoint(false));
        let wait = Duration::from_secs(2);
        index.cue("smoke", wait).await.unwrap();
        index.cue("fire", wait).await.unwrap();
        assert_eq!(index.take_cue_usage(), EmbedUsage { requests: 2, failures: 0, tokens: 6 });
        assert_eq!(index.take_cue_usage(), EmbedUsage::default());

        let failing = self::index(endpoint(true));
        assert_eq!(failing.cue("smoke", wait).await, None);
        // The next cue rests and sends nothing, so it is not a request.
        assert_eq!(failing.cue("smoke", wait).await, None);
        assert_eq!(failing.take_cue_usage(), EmbedUsage { requests: 1, failures: 1, tokens: 0 });
    }

    #[tokio::test(start_paused = true)]
    async fn a_cue_that_times_out_counts_as_a_failed_request() {
        let hung = FakeTransport::new(|_| std::future::pending::<Result<HttpResponse, String>>());
        let index = self::index(hung);
        assert_eq!(index.cue("smoke", Duration::from_secs(2)).await, None);
        assert_eq!(index.take_cue_usage(), EmbedUsage { requests: 1, failures: 1, tokens: 0 });
    }

    #[tokio::test]
    async fn a_cue_is_embedded_or_given_up_on() {
        let index = index(endpoint(false));
        assert_eq!(index.cue("smoke", Duration::from_secs(2)).await.as_deref(), Some(&[1.0f32, 0.0][..]));
        assert_eq!(index.cue("   ", Duration::from_secs(2)).await, None);
        let failing = self::index(endpoint(true));
        assert_eq!(failing.cue("smoke", Duration::from_secs(2)).await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_hung_endpoint_is_asked_once_a_minute() {
        let wait = Duration::from_secs(2);
        let hung = FakeTransport::new(|_| std::future::pending::<Result<HttpResponse, String>>());
        let index = self::index(hung.clone());
        assert_eq!(index.cue("smoke", wait).await, None);
        assert_eq!(hung.requests().len(), 1);

        let before = tokio::time::Instant::now();
        assert_eq!(index.cue("smoke", wait).await, None);
        assert!(before.elapsed() < wait);
        assert_eq!(hung.requests().len(), 1);

        tokio::time::advance(Duration::from_secs(61)).await;
        assert_eq!(index.cue("smoke", wait).await, None);
        assert_eq!(hung.requests().len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_cue_rests_the_next_one() {
        let failing = endpoint(true);
        let index = self::index(failing.clone());
        let wait = Duration::from_secs(2);
        assert_eq!(index.cue("smoke", wait).await, None);
        let asked = failing.requests().len();
        assert_eq!(index.cue("smoke", wait).await, None);
        assert_eq!(failing.requests().len(), asked);
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
