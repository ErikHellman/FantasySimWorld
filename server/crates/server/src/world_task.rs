//! The world task: the only place that advances the world.
//!
//! It owns the write side of the shared [`Core`] (sim, encoder, replay ring),
//! applies commands at tick boundaries, encodes one `tick` frame per tick and
//! publishes each wake's frames on a broadcast channel that the event streams
//! forward. It also keeps the world in the store: the chronicle and applied
//! commands as they happen, a snapshot every 30 s, after save points and on
//! shutdown, and it resumes from the newest snapshot on boot.
//!
//! The schedule: sleep until the next 50 ms deadline, run the ticks owed (at
//! most 12 per wake), and when more than 10 s are owed (a host pause, a
//! deploy) skip ahead instead of fast-forwarding.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use fsw_core::time::game_time;
use fsw_sim::state::{SAVE_VERSION, SimState};
use fsw_sim::{Command, Sim, SimEvent, create_state};
use parking_lot::{Mutex, RwLock};
use serde_json::json;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

use crate::api::wire::{LlmState, LlmStatus, Utterance, WireEvent, WorldReset};
use crate::encode::{Tracker, chronicle_entry};
use crate::store::{CommandRecord, SnapshotReason, SnapshotWrite, Store};

pub const TICK: Duration = Duration::from_millis(50);
/// Ticks run per wake at most; the rest wait for the next wake (which comes at once).
pub const MAX_TICKS_PER_WAKE: u32 = 12;
/// Owing more than this many ticks (10 s at 1x) skips ahead instead of catching up.
pub const MAX_BACKLOG_TICKS: u64 = 200;
/// Speeds an admin may choose.
pub const SPEEDS: [u8; 4] = [0, 1, 2, 4];

/// What the next wake should do.
#[derive(Debug, PartialEq, Eq)]
pub struct Due {
    pub ticks: u32,
    /// ticks given up because the loop fell too far behind
    pub skipped: u64,
}

/// Fixed-step schedule with bounded catch-up. Pure, so it is tested without a runtime.
#[derive(Debug)]
pub struct TickClock {
    speed: u8,
    /// when the next tick is due
    next: Instant,
}

impl TickClock {
    pub fn new(now: Instant, speed: u8) -> Self {
        let mut clock = Self { speed, next: now };
        clock.next = now + clock.period().unwrap_or(TICK);
        clock
    }

    pub fn speed(&self) -> u8 {
        self.speed
    }

    fn period(&self) -> Option<Duration> {
        (self.speed > 0).then(|| TICK / u32::from(self.speed))
    }

    /// When to wake next; far away while paused.
    pub fn deadline(&self) -> Instant {
        if self.speed == 0 { self.next + Duration::from_secs(3600) } else { self.next }
    }

    /// Changes the speed from now on; nothing is owed for the time before.
    pub fn set_speed(&mut self, speed: u8, now: Instant) {
        self.speed = speed;
        self.next = now + self.period().unwrap_or(TICK);
    }

    pub fn due(&mut self, now: Instant) -> Due {
        let Some(period) = self.period() else { return Due { ticks: 0, skipped: 0 } };
        if now < self.next {
            return Due { ticks: 0, skipped: 0 };
        }
        let owed = 1 + ((now - self.next).as_nanos() / period.as_nanos()) as u64;
        if owed > MAX_BACKLOG_TICKS {
            // Rebase: run one tick now and carry on from here.
            self.next = now + period;
            return Due { ticks: 1, skipped: owed - 1 };
        }
        let ticks = owed.min(u64::from(MAX_TICKS_PER_WAKE)) as u32;
        self.next += period * ticks;
        Due { ticks, skipped: 0 }
    }
}

/// The clock as everyone else sees it.
#[derive(Clone, Debug)]
pub struct WorldClock {
    pub tick: u64,
    pub speed: u8,
    /// when the last tick ran (the health check fails when this gets old)
    pub last_tick_at: Instant,
}

/// Ticks of replay kept for viewers who reconnect (5 s at 1x).
pub const RING_TICKS: usize = 100;
/// Spoken lines a snapshot carries.
pub const VOICES_KEPT: usize = 80;
/// The autosave interval.
pub const AUTOSAVE: Duration = Duration::from_secs(30);
/// A save point is written this long after the event, so a burst of them makes one snapshot.
pub const SAVE_POINT_DELAY: Duration = Duration::from_millis(1500);

/// Encoded `tick` events of the last [`RING_TICKS`] ticks.
#[derive(Default)]
pub struct TickRing {
    frames: VecDeque<(u64, Arc<str>)>,
}

impl TickRing {
    pub fn push(&mut self, tick: u64, json: Arc<str>) {
        self.frames.push_back((tick, json));
        while self.frames.len() > RING_TICKS {
            self.frames.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.frames.clear();
    }

    /// The frames after `last`, or `None` when the gap cannot be filled (further
    /// behind than the ring reaches, or ahead of the world).
    pub fn since(&self, last: u64, current: u64) -> Option<Vec<(u64, Arc<str>)>> {
        if last > current {
            return None;
        }
        if last == current {
            return Some(Vec::new());
        }
        let first = self.frames.front()?.0;
        if last + 1 < first {
            return None;
        }
        Some(self.frames.iter().filter(|(tick, _)| *tick > last).cloned().collect())
    }
}

/// Everything about the running world that readers need at one consistent tick.
pub struct Core {
    pub sim: Sim,
    pub tracker: Tracker,
    pub ring: TickRing,
    /// newest first
    pub voices: VecDeque<Utterance>,
    /// random per server start and per world reset; prefixes stream event ids
    pub boot_id: String,
    pub world_id: i64,
    pub speed: u8,
}

impl Core {
    pub fn tick(&self) -> u64 {
        self.sim.state.tick.max(0) as u64
    }
}

/// One viewer tab's choice of character to inspect.
#[derive(Clone, Debug)]
pub struct Watch {
    pub id: Option<String>,
    pub at: std::time::Instant,
}

/// What the streams forward. Built once, shared by every viewer.
#[derive(Clone, Debug)]
pub enum Outgoing {
    /// the `tick` events of one wake, in order
    Ticks(Arc<Vec<(u64, Arc<str>)>>),
    Status(Arc<str>),
    Roster(Arc<str>),
    /// the inspector for one character (sent to the viewers watching it)
    Detail {
        id: String,
        json: Arc<str>,
    },
    /// admins only
    BrainLog(Arc<str>),
    /// the world was replaced; viewers reload
    Reset(Arc<str>),
}

/// The sim events of one tick, for the brain.
#[derive(Debug)]
pub struct TickEvents {
    pub tick: u64,
    pub events: Vec<SimEvent>,
}

/// Shared between the world task, the digest task, the brain and the HTTP handlers.
pub struct World {
    /// written only by the world task (and never across an await)
    pub core: RwLock<Core>,
    pub store: Store,
    pub inputs: mpsc::Sender<WorldInput>,
    pub outgoing: broadcast::Sender<Outgoing>,
    pub events: broadcast::Sender<Arc<TickEvents>>,
    pub clock: watch::Receiver<WorldClock>,
    /// viewer id -> watched character
    pub watches: Mutex<HashMap<String, Watch>>,
    /// viewer id -> open streams (their watches never expire)
    pub connected: Mutex<HashMap<String, u32>>,
    /// the newest status and roster events, for viewers who connect
    pub status: Mutex<Option<Arc<str>>>,
    pub roster: Mutex<Option<Arc<str>>>,
    /// the newest detail per character
    pub details: Mutex<HashMap<String, Arc<str>>>,
    pub llm: Mutex<LlmStatus>,
    /// held at 1x because the model has a backlog
    pub throttled: AtomicBool,
    pub clock_skips: AtomicU32,
    /// how late each wake was (ms), newest last
    pub jitter: Mutex<VecDeque<f64>>,
}

/// Asks of the world task.
pub enum WorldInput {
    /// applied at the start of the next tick and recorded
    Command {
        command: Command,
        source: &'static str,
    },
    Speed(u8),
    /// a new world or an import; answered once the old world is gone
    Replace {
        state: Box<SimState>,
        reason: SnapshotReason,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

pub fn llm_off() -> LlmStatus {
    LlmStatus {
        state: LlmState::Off,
        in_flight: 0,
        queued: 0,
        requests: 0,
        failures: 0,
        tokens: 0,
        latency_ms: 0.0,
        protocol: String::new(),
        message: String::new(),
    }
}

pub fn new_boot_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn random_seed() -> u32 {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    u32::from_le_bytes(bytes) & 0x7fff_ffff
}

/// The speed an admin last chose, kept across restarts.
pub const SPEED_SETTING: &str = "sim";

/// Loads the active world from its newest snapshot, or makes a new one.
async fn boot(store: &Store) -> Result<(i64, SimState), String> {
    if let Some(loaded) = store.load_latest_snapshot().await.map_err(|e| e.to_string())? {
        match serde_json::from_slice::<SimState>(&loaded.state) {
            Ok(state) => {
                tracing::info!(world = loaded.world.id, tick = loaded.tick, reason = %loaded.reason, "resuming the world");
                return Ok((loaded.world.id, state));
            }
            Err(error) => {
                tracing::error!(%error, snapshot = loaded.id, "the newest snapshot does not load; starting a new world")
            }
        }
    }
    let seed = random_seed();
    let state = create_state(seed);
    let world_id = new_world(store, &state, SnapshotReason::NewWorld).await?;
    tracing::info!(world = world_id, seed, "created a new world");
    Ok((world_id, state))
}

/// Registers a world in the store (active), with its chronicle so far and a first snapshot.
async fn new_world(store: &Store, state: &SimState, reason: SnapshotReason) -> Result<i64, String> {
    let name = format!("Valley {}", state.seed);
    let world = store.create_world(&name, state.seed, true).await.map_err(|e| e.to_string())?;
    store
        .append_chronicle(world.id, state.chronicle.iter().map(chronicle_entry).collect())
        .await
        .map_err(|e| e.to_string())?;
    let body = serde_json::to_vec(state).map_err(|e| e.to_string())?;
    store
        .write_snapshot(SnapshotWrite {
            world_id: world.id,
            tick: state.tick.max(0) as u64,
            game_day: game_time(state.tick).day.max(0) as u32,
            reason,
            save_version: SAVE_VERSION,
            state: body,
            brain: Vec::new(),
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(world.id)
}

/// Writes a snapshot of `state` without holding anything up.
fn save(store: &Store, world_id: i64, state: SimState, reason: SnapshotReason) -> tokio::task::JoinHandle<()> {
    let store = store.clone();
    tokio::spawn(async move {
        let tick = state.tick.max(0) as u64;
        let game_day = game_time(state.tick).day.max(0) as u32;
        let body = match tokio::task::spawn_blocking(move || serde_json::to_vec(&state)).await {
            Ok(Ok(body)) => body,
            _ => {
                tracing::error!("cannot serialise the world for a snapshot");
                return;
            }
        };
        let write = SnapshotWrite {
            world_id,
            tick,
            game_day,
            reason,
            save_version: SAVE_VERSION,
            state: body,
            brain: Vec::new(),
        };
        match store.write_snapshot(write).await {
            Ok(saved) => tracing::debug!(tick, reason = reason.as_str(), bytes = saved.size_stored, "snapshot"),
            Err(error) => tracing::error!(%error, "snapshot failed"),
        }
    })
}

/// Starts the world task: resumes the active world (or makes one) and ticks it
/// until `shutdown`, then writes a last snapshot.
pub async fn start(
    store: Store,
    initial_speed: u8,
    shutdown: CancellationToken,
) -> Result<(Arc<World>, tokio::task::JoinHandle<()>), String> {
    let (world_id, state) = boot(&store).await?;
    let speed = match store.setting(SPEED_SETTING).await {
        Ok(Some(value)) => value["speed"].as_u64().map_or(initial_speed, |s| s as u8),
        _ => initial_speed,
    };
    let speed = if SPEEDS.contains(&speed) { speed } else { 1 };
    let now = Instant::now();
    let tick = state.tick.max(0) as u64;
    let (clock_tx, clock_rx) = watch::channel(WorldClock { tick, speed, last_tick_at: now });
    let (inputs, inputs_rx) = mpsc::channel(1024);
    // A slow viewer more than ~5 s behind is dropped and reconnects.
    let (outgoing, _) = broadcast::channel(512);
    let (events, _) = broadcast::channel(256);
    let tracker = Tracker::new(&state);
    let world = Arc::new(World {
        core: RwLock::new(Core {
            sim: Sim::new(state),
            tracker,
            ring: TickRing::default(),
            voices: VecDeque::new(),
            boot_id: new_boot_id(),
            world_id,
            speed,
        }),
        store: store.clone(),
        inputs,
        outgoing,
        events,
        clock: clock_rx,
        watches: Mutex::new(HashMap::new()),
        connected: Mutex::new(HashMap::new()),
        status: Mutex::new(None),
        roster: Mutex::new(None),
        details: Mutex::new(HashMap::new()),
        llm: Mutex::new(llm_off()),
        throttled: AtomicBool::new(false),
        clock_skips: AtomicU32::new(0),
        jitter: Mutex::new(VecDeque::new()),
    });
    let task = tokio::spawn(run(world.clone(), TickClock::new(now, speed), clock_tx, inputs_rx, shutdown));
    Ok((world, task))
}

/// The world as it was after one wake, for the store.
#[derive(Default)]
struct Wake {
    chronicle: Vec<crate::api::wire::ChronicleEntry>,
    save_point: bool,
}

async fn run(
    world: Arc<World>,
    mut clock: TickClock,
    clock_tx: watch::Sender<WorldClock>,
    mut inputs: mpsc::Receiver<WorldInput>,
    shutdown: CancellationToken,
) {
    let store = world.store.clone();
    let mut autosave = tokio::time::interval_at(Instant::now() + AUTOSAVE, AUTOSAVE);
    autosave.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut save_point_at: Option<Instant> = None;
    // Commands waiting for the next tick, with their source, for the record.
    let mut pending: Vec<(Command, &'static str)> = Vec::new();
    loop {
        let save_deadline = save_point_at.unwrap_or_else(|| Instant::now() + Duration::from_secs(3600));
        tokio::select! {
            () = shutdown.cancelled() => break,
            input = inputs.recv() => {
                let Some(input) = input else { break };
                match input {
                    WorldInput::Command { command, source } => {
                        world.core.write().sim.enqueue(command.clone());
                        pending.push((command, source));
                    }
                    WorldInput::Speed(speed) => {
                        clock.set_speed(speed, Instant::now());
                        world.core.write().speed = speed;
                        let tick = world.core.read().tick();
                        clock_tx.send_replace(WorldClock { tick, speed, last_tick_at: Instant::now() });
                        if let Err(error) = store.put_setting(SPEED_SETTING, &json!({ "speed": speed })).await {
                            tracing::error!(%error, "cannot keep the speed");
                        }
                    }
                    WorldInput::Replace { state, reason, reply } => {
                        let result = replace(&world, *state, reason).await;
                        pending.clear();
                        let _ = reply.send(result);
                    }
                }
                continue;
            }
            _ = autosave.tick() => {
                let (world_id, state) = { let core = world.core.read(); (core.world_id, core.sim.state.clone()) };
                save(&store, world_id, state, SnapshotReason::Interval);
                continue;
            }
            () = sleep_until(save_deadline), if save_point_at.is_some() => {
                save_point_at = None;
                let (world_id, state) = { let core = world.core.read(); (core.world_id, core.sim.state.clone()) };
                save(&store, world_id, state, SnapshotReason::SavePoint);
                continue;
            }
            () = sleep_until(clock.deadline()) => {}
        }
        let now = Instant::now();
        let late = now.saturating_duration_since(clock.deadline_for_jitter());
        let due = clock.due(now);
        if due.skipped > 0 {
            world.clock_skips.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(event = "clock_skip", skipped_ticks = due.skipped, "world fell behind; skipping ahead");
        }
        if due.ticks == 0 {
            continue;
        }
        {
            let mut jitter = world.jitter.lock();
            jitter.push_back(late.as_secs_f64() * 1000.0);
            while jitter.len() > 1200 {
                jitter.pop_front();
            }
        }

        let mut frames = Vec::with_capacity(due.ticks as usize);
        let mut wake = Wake::default();
        let mut records = Vec::new();
        let mut tick_events = Vec::new();
        let (world_id, tick) = {
            let mut core = world.core.write();
            for i in 0..due.ticks {
                let events = core.sim.step();
                let core = &mut *core;
                let tick = core.sim.state.tick.max(0) as u64;
                if i == 0 {
                    // Commands queued before this wake were applied in its first tick.
                    for (seq, (command, source)) in pending.drain(..).enumerate() {
                        let body = serde_json::to_string(&command).unwrap_or_default();
                        records.push(CommandRecord { tick, seq: seq as u32, source: source.to_owned(), body });
                    }
                }
                let frame = core.tracker.frame(&core.sim.state, &events);
                for event in &frame.ev {
                    match event {
                        WireEvent::Chronicle { entry } => wake.chronicle.push(entry.clone()),
                        WireEvent::Speech(utterance) => {
                            core.voices.push_front(utterance.clone());
                            core.voices.truncate(VOICES_KEPT);
                        }
                        _ => {}
                    }
                }
                wake.save_point |= events.iter().any(|e| matches!(e, SimEvent::SavePoint { .. }));
                let json: Arc<str> = Arc::from(serde_json::to_string(&frame).expect("tick frames always serialise"));
                core.ring.push(tick, json.clone());
                frames.push((tick, json));
                tick_events.push(Arc::new(TickEvents { tick, events }));
            }
            (core.world_id, core.tick())
        };
        clock_tx.send_replace(WorldClock { tick, speed: clock.speed(), last_tick_at: now });
        // No receivers is fine: nobody is watching.
        let _ = world.outgoing.send(Outgoing::Ticks(Arc::new(frames)));
        for events in tick_events {
            let _ = world.events.send(events);
        }
        if wake.save_point && save_point_at.is_none() {
            save_point_at = Some(now + SAVE_POINT_DELAY);
        }
        if !wake.chronicle.is_empty()
            && let Err(error) = store.append_chronicle(world_id, wake.chronicle).await
        {
            tracing::error!(%error, "cannot keep the chronicle");
        }
        if !records.is_empty()
            && let Err(error) = store.record_commands(world_id, records).await
        {
            tracing::error!(%error, "cannot record commands");
        }
    }

    let (world_id, state, tick) = {
        let core = world.core.read();
        (core.world_id, core.sim.state.clone(), core.tick())
    };
    let _ = save(&store, world_id, state, SnapshotReason::Shutdown).await;
    tracing::info!(tick, "world task stopped");
}

/// Replaces the running world (new world or import): a new store world, a
/// fresh encoder and boot id, and every viewer told to reload.
async fn replace(world: &World, state: SimState, reason: SnapshotReason) -> Result<(), String> {
    let world_id = new_world(&world.store, &state, reason).await?;
    let reset = {
        let mut core = world.core.write();
        core.tracker = Tracker::new(&state);
        core.sim = Sim::new(state);
        core.ring.clear();
        core.voices.clear();
        core.boot_id = new_boot_id();
        core.world_id = world_id;
        let label = if reason == SnapshotReason::Import { "import" } else { "new_world" };
        serde_json::to_string(&WorldReset { reason: label.into() }).expect("resets serialise")
    };
    world.details.lock().clear();
    *world.status.lock() = None;
    *world.roster.lock() = None;
    let _ = world.outgoing.send(Outgoing::Reset(Arc::from(reset)));
    tracing::info!(world = world_id, reason = reason.as_str(), "world replaced");
    Ok(())
}

impl TickClock {
    /// The deadline the current wake was for, to measure how late it came.
    fn deadline_for_jitter(&self) -> Instant {
        self.next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn ticks_on_each_deadline() {
        let start = Instant::now();
        let mut clock = TickClock::new(start, 1);
        assert_eq!(clock.due(start + ms(49)), Due { ticks: 0, skipped: 0 });
        assert_eq!(clock.due(start + ms(50)), Due { ticks: 1, skipped: 0 });
        assert_eq!(clock.due(start + ms(60)), Due { ticks: 0, skipped: 0 });
        assert_eq!(clock.deadline(), start + ms(100));
    }

    #[test]
    fn catches_up_twelve_at_a_time() {
        let start = Instant::now();
        let mut clock = TickClock::new(start, 1);
        // 2 s late: 40 ticks owed, paid back over four wakes.
        let late = start + ms(2000);
        let mut total = 0;
        let mut wakes = 0;
        loop {
            let due = clock.due(late);
            if due.ticks == 0 {
                break;
            }
            assert!(due.ticks <= MAX_TICKS_PER_WAKE);
            assert_eq!(due.skipped, 0);
            total += due.ticks;
            wakes += 1;
        }
        assert_eq!(total, 40);
        assert_eq!(wakes, 4);
    }

    #[test]
    fn skips_ahead_after_a_long_pause() {
        let start = Instant::now();
        let mut clock = TickClock::new(start, 1);
        let resumed = start + ms(60_000);
        let due = clock.due(resumed);
        assert_eq!(due.ticks, 1);
        assert_eq!(due.skipped, 1199);
        assert_eq!(clock.due(resumed + ms(10)), Due { ticks: 0, skipped: 0 });
        assert_eq!(clock.due(resumed + ms(50)), Due { ticks: 1, skipped: 0 });
    }

    #[test]
    fn speed_changes_the_period_and_pause_stops_it() {
        let start = Instant::now();
        let mut clock = TickClock::new(start, 4);
        assert_eq!(clock.due(start + ms(50)).ticks, 4);
        clock.set_speed(0, start + ms(50));
        assert_eq!(clock.due(start + ms(10_000)), Due { ticks: 0, skipped: 0 });
        // Unpausing owes nothing for the paused time.
        clock.set_speed(1, start + ms(10_000));
        assert_eq!(clock.due(start + ms(10_049)).ticks, 0);
        assert_eq!(clock.due(start + ms(10_050)).ticks, 1);
    }

    fn ticks_of(rx: &mut broadcast::Receiver<Outgoing>) -> Vec<u64> {
        let mut ticks = Vec::new();
        while let Ok(item) = rx.try_recv() {
            if let Outgoing::Ticks(frames) = item {
                ticks.extend(frames.iter().map(|(t, _)| *t));
            }
        }
        ticks
    }

    #[tokio::test]
    async fn world_task_ticks_at_20_hz_and_resumes_after_a_restart() {
        let (store, _dir) = crate::store::testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = start(store.clone(), 1, shutdown.clone()).await.unwrap();
        let mut rx = world.outgoing.subscribe();
        tokio::time::pause();
        // Just past the 20th deadline, so the world task has run it.
        tokio::time::sleep(ms(1025)).await;
        let ticks = ticks_of(&mut rx);
        assert_eq!(ticks, (1..=20).collect::<Vec<_>>());
        assert_eq!(world.clock.borrow().tick, 20);
        tokio::time::resume();
        shutdown.cancel();
        task.await.unwrap();
        let boot = world.core.read().boot_id.clone();

        // A restart resumes from the shutdown snapshot, at the same tick.
        let shutdown = CancellationToken::new();
        let (again, task) = start(store, 0, shutdown.clone()).await.unwrap();
        assert_eq!(again.core.read().tick(), 20);
        assert_ne!(again.core.read().boot_id, boot);
        shutdown.cancel();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn commands_are_applied_and_a_new_world_resets_viewers() {
        let (store, _dir) = crate::store::testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = start(store.clone(), 4, shutdown.clone()).await.unwrap();
        let mut rx = world.outgoing.subscribe();
        let id = world.core.read().sim.state.order[0].clone();
        let command = Command::Thought { id: id.clone(), text: "Something is off today.".into() };
        world.inputs.send(WorldInput::Command { command, source: "admin" }).await.unwrap();
        tokio::time::sleep(ms(200)).await;
        assert_eq!(world.core.read().sim.state.characters[&id].thought, "Something is off today.");

        let (reply, answer) = oneshot::channel();
        let state = Box::new(create_state(77));
        world.inputs.send(WorldInput::Replace { state, reason: SnapshotReason::NewWorld, reply }).await.unwrap();
        answer.await.unwrap().unwrap();
        assert_eq!(world.core.read().sim.state.seed, 77);
        let mut reset = false;
        while let Ok(item) = rx.try_recv() {
            reset |= matches!(item, Outgoing::Reset(_));
        }
        assert!(reset);
        assert_eq!(store.active_world().await.unwrap().unwrap().seed, 77);
        shutdown.cancel();
        task.await.unwrap();
    }

    #[test]
    fn the_ring_replays_a_short_gap_only() {
        let mut ring = TickRing::default();
        for tick in 1..=150 {
            ring.push(tick, Arc::from(format!("{tick}")));
        }
        assert_eq!(ring.since(140, 150).unwrap().len(), 10);
        assert_eq!(ring.since(150, 150).unwrap().len(), 0);
        assert_eq!(ring.since(50, 150).unwrap().len(), 100);
        assert!(ring.since(49, 150).is_none());
        assert!(ring.since(151, 150).is_none());
    }
}
