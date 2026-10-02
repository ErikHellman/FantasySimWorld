//! Runs the brain next to the world: feeds it each tick's events, keeps its
//! queue moving, and gives it the world through a [`WorldLink`]. Its decision
//! log goes to the store and to admins' streams; its status into `status`.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use fsw_brain::brain::{
    Brain, BrainState, BrainStatus, LogRecord, LogStatus as BrainLogStatus, RequestKind as Kind, WorldLink,
};
use fsw_brain::llm::client::LearnedCapabilities;
use fsw_brain::llm::queue::DailyUsage;
use fsw_brain::llm::transport::{LlmTransport, ReqwestTransport};
use fsw_brain::settings::{ApiKey, LlmSettings};
use fsw_sim::{Command, SimState};
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::api::wire::{BrainLogRecord, LlmState, LlmStatus, LogStatus, RequestKind};
use crate::store::settings::LLM_CAPABILITIES;
use crate::store::{BrainLogWrite, Store, now_ms, utc_day};
use crate::world_task::{Outgoing, World, WorldInput};

/// How often the brain's queue is pumped (it scans for decisions every 400 ms).
const UPDATE: Duration = Duration::from_millis(100);
/// How often learned endpoint quirks are saved.
const SAVE_LEARNED: Duration = Duration::from_secs(60);

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
