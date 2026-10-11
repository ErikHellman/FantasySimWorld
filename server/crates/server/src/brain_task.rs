//! Runs the brain next to the world: feeds it each tick's events, keeps its
//! queue moving, and gives it the world through a [`WorldLink`]. Its decision
//! log goes to the store and to admins' streams; its status into `status`.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use fsw_brain::brain::{
    Brain, BrainState, BrainStatus, LogRecord, LogStatus as BrainLogStatus, RequestKind as Kind, WorldLink,
};
use fsw_brain::llm::client::LearnedCapabilities;
use fsw_brain::llm::embed::Embedder;
use fsw_brain::llm::queue::DailyUsage;
use fsw_brain::llm::transport::{LlmTransport, ReqwestTransport};
use fsw_brain::recall::MemoryIndex;
use fsw_brain::settings::{ApiKey, EmbedSettings, LlmSettings};
use fsw_sim::util::living;
use fsw_sim::{Command, SimState};
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::api::wire::{BrainLogRecord, LlmState, LlmStatus, LogStatus, RequestKind};
use crate::store::settings::LLM_CAPABILITIES;
use crate::store::{BrainLogWrite, EMBED_USAGE, Store, now_ms, utc_day};
use crate::world_task::{Outgoing, World, WorldInput};

/// How often the brain's queue is pumped (it scans for decisions every 400 ms).
const UPDATE: Duration = Duration::from_millis(100);
/// How often learned endpoint quirks are saved.
const SAVE_LEARNED: Duration = Duration::from_secs(60);
/// How often the indexer looks for memories without a vector.
const INDEX_EVERY: Duration = Duration::from_secs(5);
/// Requests per pass, so a fresh index fills over a few minutes instead of in one burst.
const INDEX_BATCHES: usize = 4;
/// How often kept vectors are marked as in use and the cache is trimmed to the memories of the living.
const TOUCH_EVERY: Duration = Duration::from_secs(3600);
/// How long after start the first touch runs. A server restarted more often than hourly would otherwise never
/// refresh `used_ms`, and vectors still in use would be swept as unused after a week.
const FIRST_TOUCH: Duration = Duration::from_secs(60);

struct Link {
    world: Arc<World>,
    store: Store,
}

fn kind(kind: Kind) -> RequestKind {
    match kind {
        Kind::Mercy => RequestKind::Mercy,
        Kind::React => RequestKind::React,
        Kind::Reply => RequestKind::Reply,
        Kind::Converse => RequestKind::Converse,
        Kind::Decide => RequestKind::Decide,
        Kind::PlanDay => RequestKind::PlanDay,
        Kind::Billboard => RequestKind::Billboard,
        Kind::Slogan => RequestKind::Slogan,
        Kind::Herald => RequestKind::Herald,
        Kind::Ceremony => RequestKind::Ceremony,
        Kind::Name => RequestKind::Name,
        Kind::Reflect => RequestKind::Reflect,
    }
}

fn status(status: BrainLogStatus) -> LogStatus {
    match status {
        BrainLogStatus::Ok => LogStatus::Ok,
        BrainLogStatus::Fallback => LogStatus::Fallback,
        BrainLogStatus::Error => LogStatus::Error,
        BrainLogStatus::Stale => LogStatus::Stale,
    }
}

pub fn llm_status(status: &BrainStatus) -> LlmStatus {
    LlmStatus {
        state: match status.state {
            BrainState::Off => LlmState::Off,
            BrainState::Ready => LlmState::Ready,
            BrainState::Busy => LlmState::Busy,
            BrainState::Error => LlmState::Error,
        },
        in_flight: status.in_flight,
        queued: status.queued,
        requests: status.requests,
        failures: status.failures,
        tokens: status.tokens,
        latency_ms: status.latency_ms,
        protocol: status.protocol.clone(),
        message: status.message.clone(),
    }
}

impl WorldLink for Link {
    fn with_state(&self, f: &mut dyn FnMut(&SimState)) {
        f(&self.world.core.read().sim.state);
    }

    fn send(&self, command: Command) {
        // A full queue means the world task is gone or wedged; a decision is not worth waiting for.
        if self.world.inputs.try_send(WorldInput::Command { command, source: "brain" }).is_err() {
            tracing::warn!("the world did not take a brain command");
        }
    }

    fn speed(&self) -> u8 {
        let speed = self.world.core.read().speed;
        if self.world.throttled.load(Ordering::Relaxed) { speed.min(1) } else { speed }
    }

    fn log(&self, record: LogRecord) {
        let world_id = self.world.core.read().world_id;
        let failed = record.status == BrainLogStatus::Error;
        let tokens = record.prompt_tokens + record.completion_tokens;
        let write = BrainLogWrite {
            world_id,
            tick: record.tick,
            character: record.character.clone(),
            kind: kind(record.kind),
            status: status(record.status),
            latency_ms: record.latency_ms,
            prompt_tokens: record.prompt_tokens as u32,
            completion_tokens: record.completion_tokens as u32,
            summary: record.summary.clone(),
            prompt: record.prompt.clone(),
            response: record.response.clone(),
            commands: record.commands.clone(),
        };
        let (store, world) = (self.store.clone(), self.world.clone());
        tokio::spawn(async move {
            let request_kind = write.kind;
            let mut wire = BrainLogRecord {
                id: 0,
                tick: write.tick,
                at: now_ms().max(0) as u64,
                character: write.character.clone(),
                kind: write.kind,
                status: write.status,
                latency_ms: write.latency_ms,
                prompt_tokens: write.prompt_tokens,
                completion_tokens: write.completion_tokens,
                summary: write.summary.clone(),
                prompt: write.prompt.clone(),
                response: write.response.clone(),
                commands: write.commands.clone(),
            };
            match store.insert_brain_log(write).await {
                Ok(id) => wire.id = id,
                Err(error) => tracing::error!(%error, "cannot keep the brain log"),
            }
            let label = serde_json::to_value(request_kind).ok().and_then(|v| v.as_str().map(str::to_owned));
            if let Err(error) = store.add_usage(&label.unwrap_or_default(), 1, u64::from(failed), tokens).await {
                tracing::error!(%error, "cannot count brain usage");
            }
            if let Ok(json) = serde_json::to_string(&wire) {
                let _ = world.outgoing.send(Outgoing::BrainLog(Arc::from(json)));
            }
        });
    }

    fn status(&self, status: BrainStatus) {
        *self.world.llm.lock() = llm_status(&status);
    }
}

async fn load_learned(store: &Store) -> Option<LearnedCapabilities> {
    serde_json::from_value(store.setting(LLM_CAPABILITIES).await.ok()??).ok()
}

/// Today's usage across request kinds, so a restart keeps counting towards the daily caps.
async fn load_usage(store: &Store) -> Option<DailyUsage> {
    let usage = store.usage_today().await.ok()?;
    Some(DailyUsage {
        day: utc_day(now_ms()).max(0) as u64,
        requests: u64::from(usage.requests_24h),
        failures: u64::from(usage.failures_24h),
        tokens: usage.tokens_24h,
    })
}

pub fn transport() -> Arc<dyn LlmTransport> {
    match ReqwestTransport::new() {
        Ok(transport) => Arc::new(transport),
        Err(error) => {
            tracing::error!(%error, "no HTTP client for the model; the brain stays off");
            fsw_brain::llm::transport::FakeTransport::new(|_| async { Err("no HTTP client".to_owned()) })
        }
    }
}

/// Builds the brain over the running world with the model from the environment,
/// plus what the store remembers: learned capabilities and today's usage.
pub async fn start(world: Arc<World>, store: Store, settings: LlmSettings, key: ApiKey) -> Brain {
    let brain = Brain::new(Arc::new(Link { world, store: store.clone() }), transport());
    brain.configure(settings, key, load_learned(&store).await, load_usage(&store).await);
    brain
}

/// The embedding model with the vectors kept for the memories of the living, or `None` when `EMBED_*` is not set.
pub async fn start_memory(
    world: &World,
    store: &Store,
    settings: EmbedSettings,
    key: ApiKey,
) -> Option<Arc<MemoryIndex>> {
    if !settings.enabled() {
        return None;
    }
    let index = Arc::new(MemoryIndex::new(Embedder::new(settings, key, transport())));
    let wanted = memory_texts(world).iter().map(|text| index.cache().key(text)).collect();
    match store.load_vectors(index.cache().model(), wanted).await {
        Ok(rows) => {
            for (key, vector) in rows {
                index.cache().insert(key, vector);
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

/// One indexer pass: embeds what has no vector yet, keeps it and counts the usage, the cues' since the last pass
/// included. Returns how many were added. `failing` is whether the last request the indexer made failed; the
/// pass logs only when that changes, so an endpoint that stays down is reported once, not every few seconds.
pub async fn index_once(index: &MemoryIndex, world: &World, store: &Store, failing: &mut bool) -> usize {
    let texts = memory_texts(world);
    let indexed = index.index(&texts, INDEX_BATCHES).await;
    if let Some(error) = &indexed.error {
        if !*failing {
            tracing::warn!(%error, "embedding memories failed; recall falls back to shared words meanwhile");
        }
        *failing = true;
    } else if *failing && indexed.usage.requests > 0 {
        tracing::info!("embedding memories works again");
        *failing = false;
    }
    let added = indexed.added.len();
    if let Err(error) = store.put_vectors(index.cache().model(), indexed.added).await {
        tracing::error!(%error, "cannot keep memory vectors");
    }
    let mut usage = indexed.usage;
    usage.add(index.take_cue_usage());
    if !usage.is_empty()
        && let Err(error) = store.add_usage(EMBED_USAGE, usage.requests, usage.failures, usage.tokens).await
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
        let mut touching = tokio::time::interval_at(tokio::time::Instant::now() + FIRST_TOUCH, TOUCH_EVERY);
        let mut failing = false;
        // A pass is raced against shutdown because a hung endpoint can hold it for the embed timeout, and the
        // store must still be closed in time. A pass dropped here only loses vectors not yet stored; they are
        // embedded again at the next start.
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                _ = every.tick() => tokio::select! {
                    () = shutdown.cancelled() => break,
                    _ = index_once(&index, &world, &store, &mut failing) => {}
                },
                _ = touching.tick() => tokio::select! {
                    () = shutdown.cancelled() => break,
                    () = touch(&index, &world, &store) => {}
                },
            }
        }
    })
}

/// Feeds the brain until shutdown: each tick's events, a queue pump every 100 ms,
/// and the 1x hold while the model has a backlog.
pub fn spawn(
    brain: Brain,
    world: Arc<World>,
    store: Store,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut events = world.events.subscribe();
        let mut update = tokio::time::interval(UPDATE);
        update.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut save = tokio::time::interval_at(tokio::time::Instant::now() + SAVE_LEARNED, SAVE_LEARNED);
        let mut saved: Option<LearnedCapabilities> = None;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                tick = events.recv() => match tick {
                    Ok(tick) => brain.handle(&tick.events),
                    Err(RecvError::Lagged(missed)) => tracing::warn!(missed, "the brain missed ticks"),
                    Err(RecvError::Closed) => break,
                },
                _ = update.tick() => {
                    brain.update(false);
                    let speed = world.core.read().speed;
                    world.throttled.store(speed > 1 && brain.backlogged(), Ordering::Relaxed);
                }
                _ = save.tick() => {
                    let learned = brain.learned();
                    if learned.is_some() && learned != saved {
                        if let Ok(value) = serde_json::to_value(&learned)
                            && let Err(error) = store.put_setting(LLM_CAPABILITIES, &value).await
                        {
                            tracing::error!(%error, "cannot keep what the client learned");
                        }
                        saved = learned;
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use fsw_brain::llm::transport::{FakeTransport, HttpRequest, HttpResponse};
    use fsw_brain::recall::VectorLookup;
    use fsw_sim::state::{Memory, MemoryKind};
    use serde_json::json;

    use super::*;
    use crate::store::testing;

    /// A paused world with the test cast in it (a new world is empty until it is founded).
    async fn founded(store: Store, shutdown: CancellationToken) -> (Arc<World>, tokio::task::JoinHandle<()>) {
        let (world, task) = crate::world_task::start(store, 0, shutdown).await.unwrap();
        let (reply, answer) = tokio::sync::oneshot::channel();
        let state = Box::new(fsw_testing::create_test_state(6));
        let reason = crate::store::SnapshotReason::NewWorld;
        world.inputs.send(WorldInput::Replace { state, reason, reply }).await.unwrap();
        answer.await.unwrap().unwrap();
        (world, task)
    }

    fn remember_bonfire(world: &World) {
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

    #[tokio::test]
    async fn indexes_the_memories_of_the_living_once_and_keeps_them() {
        let (store, _dir) = testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = founded(store.clone(), shutdown.clone()).await;
        remember_bonfire(&world);
        let endpoint = FakeTransport::new(|request: HttpRequest| {
            let count = request.json()["input"].as_array().map_or(0, Vec::len);
            let data: Vec<_> = (0..count).map(|i| json!({ "index": i, "embedding": [1.0, i as f64 + 1.0] })).collect();
            let response = HttpResponse::json(200, &json!({ "data": data, "usage": { "total_tokens": 7 } }));
            async move { Ok(response) }
        });
        let settings = EmbedSettings { base_url: "http://embed.test/v1".into(), model: "e1".into() };
        let index = MemoryIndex::new(Embedder::new(settings, ApiKey::default(), endpoint.clone()));

        let (mut passes, mut failing) = (0, false);
        while index_once(&index, &world, &store, &mut failing).await > 0 {
            passes += 1;
            assert!(passes < 100, "the index never fills");
        }
        let requests = endpoint.requests().len();
        assert!(requests > 0);
        assert_eq!(index_once(&index, &world, &store, &mut failing).await, 0);
        assert_eq!(endpoint.requests().len(), requests, "nothing is embedded twice");
        assert!(index.cache().vector("Watched the bonfire on the green.").is_some());
        let living: HashSet<i64> = memory_texts(&world).iter().map(|text| index.cache().key(text)).collect();
        assert_eq!(store.load_vectors("e1", living).await.unwrap().len(), index.cache().len());

        let today = utc_day(now_ms());
        let usage = store.usage(today, today).await.unwrap();
        assert!(usage.iter().any(|row| row.kind == EMBED_USAGE && row.requests == requests as u64));
        assert_eq!(store.usage_today().await.unwrap().requests_24h, 0, "embeddings do not count against the model");

        shutdown.cancel();
        let _ = task.await;
    }

    #[tokio::test]
    async fn a_failing_endpoint_stores_nothing_and_counts_one_failure() {
        let (store, _dir) = testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = founded(store.clone(), shutdown.clone()).await;
        remember_bonfire(&world);
        let (endpoint, index) = failing_index();

        let mut failing = false;
        assert_eq!(index_once(&index, &world, &store, &mut failing).await, 0);
        assert!(failing);
        assert_eq!(endpoint.requests().len(), 1, "a failed request ends the pass");
        let living: HashSet<i64> = memory_texts(&world).iter().map(|text| index.cache().key(text)).collect();
        assert!(store.load_vectors("e1", living).await.unwrap().is_empty());
        assert_eq!(index.cache().len(), 0);
        let row = embed_usage(&store).await.expect("a usage row for the failure");
        assert_eq!((row.requests, row.failures), (1, 1));

        shutdown.cancel();
        let _ = task.await;
    }

    fn failing_index() -> (Arc<FakeTransport>, MemoryIndex) {
        let endpoint = FakeTransport::new(|_: HttpRequest| async { Ok(HttpResponse::json(500, &json!({}))) });
        let settings = EmbedSettings { base_url: "http://embed.test/v1".into(), model: "e1".into() };
        (endpoint.clone(), MemoryIndex::new(Embedder::new(settings, ApiKey::default(), endpoint)))
    }

    async fn embed_usage(store: &Store) -> Option<crate::store::UsageRow> {
        let today = utc_day(now_ms());
        store.usage(today, today).await.unwrap().into_iter().find(|row| row.kind == EMBED_USAGE)
    }

    #[tokio::test]
    async fn passes_while_the_embedder_rests_count_no_more_failures() {
        let (store, _dir) = testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = founded(store.clone(), shutdown.clone()).await;
        remember_bonfire(&world);
        let (endpoint, index) = failing_index();

        // Four failures in a row put the embedder to rest; the passes after that send nothing.
        let mut failing = false;
        for _ in 0..7 {
            assert_eq!(index_once(&index, &world, &store, &mut failing).await, 0);
        }
        assert!(failing);
        assert_eq!(endpoint.requests().len(), 4);
        let row = embed_usage(&store).await.expect("a usage row for the failures");
        assert_eq!((row.requests, row.failures), (4, 4));

        shutdown.cancel();
        let _ = task.await;
    }

    #[tokio::test]
    async fn counts_the_cues_with_the_indexers_own_requests() {
        let (store, _dir) = testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = founded(store.clone(), shutdown.clone()).await;
        remember_bonfire(&world);
        let endpoint = FakeTransport::new(|request: HttpRequest| {
            let count = request.json()["input"].as_array().map_or(0, Vec::len);
            let data: Vec<_> = (0..count).map(|i| json!({ "index": i, "embedding": [1.0, 0.0] })).collect();
            let response = HttpResponse::json(200, &json!({ "data": data, "usage": { "total_tokens": 5 } }));
            async move { Ok(response) }
        });
        let settings = EmbedSettings { base_url: "http://embed.test/v1".into(), model: "e1".into() };
        let index = MemoryIndex::new(Embedder::new(settings, ApiKey::default(), endpoint.clone()));

        for cue in ["smoke", "bread", "a wolf"] {
            assert!(index.cue(cue, Duration::from_secs(2)).await.is_some());
        }
        let mut failing = false;
        index_once(&index, &world, &store, &mut failing).await;
        let requests = endpoint.requests().len() as u64;
        assert!(requests > 3, "the pass embedded memories too");
        let row = embed_usage(&store).await.expect("a usage row");
        assert_eq!((row.requests, row.failures, row.tokens), (requests, 0, requests * 5));

        // The cues were counted once; the next pass adds only its own.
        index_once(&index, &world, &store, &mut failing).await;
        let row = embed_usage(&store).await.unwrap();
        assert_eq!(row.requests, endpoint.requests().len() as u64);

        shutdown.cancel();
        let _ = task.await;
    }
}
