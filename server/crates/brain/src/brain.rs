//! Connects the village to a language model (port of `src/brain/brain.ts`).
//!
//! It watches for moments that deserve a decision, asks the model through the
//! budgeted [`JobQueue`], checks each answer against the world as it is by
//! then, and sends the result to the sim as commands. When the model is slow,
//! wrong or absent nothing here blocks: the sim's routines and local lines
//! carry on. The world is reached through a [`WorldLink`], so this crate does
//! not depend on the server; state is only read, in short sections, never
//! across an await.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use fsw_core::jsmath::{js_round, utf16_truncate};
use fsw_core::time::{format_clock, game_time, is_night};
use fsw_sim::Command;
use fsw_sim::commands::{RelationshipNote, TuningPatch};
use fsw_sim::events::SimEvent;
use fsw_sim::state::{
    BoardReaction, ConversationLine, FightMode, Id, Intent, IntentAction, IntentSource, Map, PlanSource, SimState,
    StimulusKind,
};
use fsw_sim::systems::billboards::{has_read, reading, shown};
use fsw_sim::util::{first_name, is_fighting, living, name_of};
use fsw_world::places::{billboards, get_place};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::time::Instant;

use crate::governor::{Governor, Tier};
use crate::llm::client::{LearnedCapabilities, LlmClient};
use crate::llm::queue::{DailyUsage, Job, JobQueue};
use crate::llm::transport::LlmTransport;
use crate::llm::types::{LlmError, LlmErrorKind, LlmRequest, LlmResponse, Protocol};
use crate::prompts::{
    Prompt, billboard_prompt, conversation_prompt, decide_prompt, mercy_prompt, plan_prompt, prompt_text, react_prompt,
    reflect_prompt, reply_prompt,
};
use crate::settings::{ApiKey, ConversationMode, LlmSettings};
use crate::validate::{
    feeling, interpret_conversation, interpret_decision, interpret_plan, reflection_relationships, spoken,
};

const RETRY_AFTER: Duration = Duration::from_secs(60);
const SCAN_INTERVAL: Duration = Duration::from_millis(400);
const MAX_TURN_LINES: usize = 6;
/// Starting request budget; suits a model of unknown speed.
const INITIAL_BUDGET: f64 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind {
    Mercy,
    React,
    Reply,
    Converse,
    Decide,
    PlanDay,
    Billboard,
    Reflect,
}

impl RequestKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mercy => "mercy",
            Self::React => "react",
            Self::Reply => "reply",
            Self::Converse => "converse",
            Self::Decide => "decide",
            Self::PlanDay => "plan_day",
            Self::Billboard => "billboard",
            Self::Reflect => "reflect",
        }
    }

    fn priority(self) -> i32 {
        match self {
            Self::Mercy => 0,
            Self::React => 1,
            Self::Reply | Self::Converse => 2,
            Self::Decide => 4,
            Self::PlanDay => 5,
            Self::Billboard => 6,
            Self::Reflect => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogStatus {
    Ok,
    Fallback,
    Error,
    Stale,
}

/// One request in the decision log.
#[derive(Clone, Debug)]
pub struct LogRecord {
    pub tick: u64,
    pub character: String,
    pub kind: RequestKind,
    pub status: LogStatus,
    pub latency_ms: u32,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub summary: String,
    pub prompt: String,
    pub response: String,
    /// one line per command the answer produced
    pub commands: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrainState {
    Off,
    Ready,
    Busy,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrainStatus {
    pub state: BrainState,
    pub in_flight: u32,
    pub queued: u32,
    pub requests: u32,
    pub failures: u32,
    pub tokens: u64,
    pub latency_ms: f64,
    pub protocol: String,
    pub message: String,
}

/// How the brain reaches the world it thinks about.
pub trait WorldLink: Send + Sync + 'static {
    /// Calls `f` with the current state (under a short read lock).
    fn with_state(&self, f: &mut dyn FnMut(&SimState));
    /// Queues a command for the next tick.
    fn send(&self, command: Command);
    /// Current simulation speed (0 = paused).
    fn speed(&self) -> u8;
    fn log(&self, record: LogRecord);
    fn status(&self, status: BrainStatus);
}

fn read<R>(link: &dyn WorldLink, f: impl FnOnce(&SimState) -> R) -> R {
    let mut f = Some(f);
    let mut out = None;
    link.with_state(&mut |state| out = Some((f.take().expect("called once"))(state)));
    out.expect("with_state calls back")
}

#[derive(Clone, Copy, Debug, Default)]
struct Mind {
    last_decide_at: Option<Instant>,
    last_react_at: Option<Instant>,
    planned_day: i64,
    reflected_day: i64,
    /// after a failed plan or reflection, do not ask again before this time
    retry_at: Option<Instant>,
}

fn since(at: Option<Instant>, now: Instant) -> Duration {
    at.map_or(Duration::MAX, |at| now.saturating_duration_since(at))
}

struct Inner {
    link: Arc<dyn WorldLink>,
    transport: Arc<dyn LlmTransport>,
    state: Mutex<BrainInner>,
}

struct BrainInner {
    client: Option<Arc<LlmClient>>,
    queue: Option<Arc<JobQueue>>,
    settings: Option<LlmSettings>,
    governor: Governor,
    minds: HashMap<Id, Mind>,
    /// conversation id -> transcript as the brain knows it (turn-by-turn mode)
    turns: HashMap<Id, Vec<ConversationLine>>,
    last_scan: Option<Instant>,
    tokens: u64,
    last_error: String,
}

/// What an answer amounted to.
struct Applied {
    status: LogStatus,
    summary: String,
    commands: Vec<Command>,
}

impl Applied {
    fn ok(summary: impl Into<String>, commands: Vec<Command>) -> Self {
        Self { status: LogStatus::Ok, summary: summary.into(), commands }
    }

    fn fallback(summary: impl Into<String>) -> Self {
        Self { status: LogStatus::Fallback, summary: summary.into(), commands: Vec::new() }
    }

    fn stale(summary: impl Into<String>) -> Self {
        Self { status: LogStatus::Stale, summary: summary.into(), commands: Vec::new() }
    }
}

/// A prompt ready to send, the commands that go with sending it, and whatever `apply` needs to know.
struct Built<T> {
    prompt: Prompt,
    commands: Vec<Command>,
    context: T,
}

fn built<T>(prompt: Prompt, context: T) -> Option<Built<T>> {
    Some(Built { prompt, commands: Vec::new(), context })
}

type OnFail = Arc<dyn Fn() + Send + Sync>;
/// Runs after a successful answer has been applied.
type After = Box<dyn FnOnce(&Brain) + Send>;

/// One line per command, for the decision log.
pub fn describe_command(state: &SimState, command: &Command) -> String {
    match command {
        Command::SetIntents { intents, .. } => {
            intents.iter().map(|intent| describe_intent(state, intent)).collect::<Vec<_>>().join(" → ")
        }
        other => serde_json::to_value(other)
            .ok()
            .and_then(|v| v.get("type").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_default(),
    }
}

fn describe_intent(state: &SimState, intent: &Intent) -> String {
    match &intent.action {
        IntentAction::GoTo { place_id } => format!("go to {place_id}"),
        IntentAction::Activity { activity, .. } => activity.as_str().into(),
        IntentAction::Talk { target_id, opener, tone } => {
            format!("talk to {} ({}): “{opener}”", name_of(state, target_id), tone.as_str())
        }
        IntentAction::Give { target_id, item } => format!("give {} to {}", item.as_str(), name_of(state, target_id)),
        IntentAction::Fight { target_id, mode } => match mode {
            FightMode::Monster => "fight".into(),
            FightMode::Brawl => format!("brawl {}", name_of(state, target_id)),
            FightMode::Lethal => format!("lethal {}", name_of(state, target_id)),
        },
        IntentAction::Flee => "flee".into(),
        IntentAction::Wait { .. } => "wait".into(),
        IntentAction::Nap { .. } => "nap".into(),
        IntentAction::Stalk { target_id } => format!("stalk {}", name_of(state, target_id)),
    }
}

/// The brain. Cheap to clone; clones share everything.
#[derive(Clone)]
pub struct Brain {
    inner: Arc<Inner>,
}

impl Brain {
    pub fn new(link: Arc<dyn WorldLink>, transport: Arc<dyn LlmTransport>) -> Self {
        let brain = Self {
            inner: Arc::new(Inner {
                link,
                transport,
                state: Mutex::new(BrainInner {
                    client: None,
                    queue: None,
                    settings: None,
                    governor: Governor::new(INITIAL_BUDGET),
                    minds: HashMap::new(),
                    turns: HashMap::new(),
                    last_scan: None,
                    tokens: 0,
                    last_error: String::new(),
                }),
            }),
        };
        brain.publish();
        brain
    }

    pub fn enabled(&self) -> bool {
        self.inner.state.lock().client.is_some()
    }

    /// True when the model has more waiting than it can clear soon; the world is held at 1x meanwhile.
    pub fn backlogged(&self) -> bool {
        let queue = self.inner.state.lock().queue.clone();
        queue.is_some_and(|q| q.waiting() > q.concurrency() as usize * 2)
    }

    /// Capabilities learned from the endpoint, to persist across restarts.
    pub fn learned(&self) -> Option<LearnedCapabilities> {
        self.inner.state.lock().client.as_ref().map(|c| c.learned())
    }

    /// Today's request and token counts, to persist across restarts.
    pub fn usage(&self) -> Option<DailyUsage> {
        self.inner.state.lock().queue.as_ref().map(|q| q.usage())
    }

    /// Applies new settings; a disabled or incomplete configuration falls back to local minds.
    pub fn configure(
        &self,
        settings: LlmSettings,
        api_key: ApiKey,
        learned: Option<LearnedCapabilities>,
        usage: Option<DailyUsage>,
    ) {
        let usable = settings.enabled && !settings.base_url.trim().is_empty() && !settings.model.trim().is_empty();
        {
            let mut s = self.inner.state.lock();
            if let Some(queue) = s.queue.take() {
                queue.clear();
            }
            s.turns.clear();
            s.last_error.clear();
            s.client = None;
            s.settings = None;
            if usable {
                let client = Arc::new(LlmClient::new(settings.clone(), api_key, self.inner.transport.clone()));
                if let Some(learned) = &learned {
                    client.restore(learned);
                }
                let queue = Arc::new(JobQueue::new(settings.concurrency.max(1), settings.requests_per_minute));
                queue.set_daily_caps(settings.max_requests_per_day, settings.max_tokens_per_day);
                if let Some(usage) = usage {
                    queue.restore_usage(usage);
                }
                s.client = Some(client);
                s.queue = Some(queue);
                s.settings = Some(settings);
                s.governor.reset(INITIAL_BUDGET);
            }
        }
        if usable {
            self.apply_tuning();
        } else {
            self.inner.link.send(Command::Tuning {
                tuning: TuningPatch { chatter: Some(0.5), script_wait_ticks: Some(0), mercy_wait_ticks: Some(40) },
            });
        }
        self.publish();
    }

    /// Starts the governor with a large budget so tests are not held back.
    pub fn set_initial_budget_for_tests(&self, budget: f64) {
        self.inner.state.lock().governor.reset(budget);
    }

    /// Prompt size in tokens: smaller when requests are served one at a time by a local model.
    fn prompt_budget(&self) -> usize {
        let concurrency = self.inner.state.lock().settings.as_ref().map_or(1, |s| s.concurrency);
        if concurrency <= 1 { 900 } else { 1500 }
    }

    pub fn status(&self) -> BrainStatus {
        let s = self.inner.state.lock();
        let (Some(queue), Some(client)) = (&s.queue, &s.client) else {
            return BrainStatus {
                state: BrainState::Off,
                in_flight: 0,
                queued: 0,
                requests: 0,
                failures: 0,
                tokens: s.tokens,
                latency_ms: 0.0,
                protocol: String::new(),
                message: String::new(),
            };
        };
        let stats = queue.stats();
        let busy = stats.in_flight + stats.waiting > 0;
        BrainStatus {
            state: if stats.circuit_open {
                BrainState::Error
            } else if busy {
                BrainState::Busy
            } else {
                BrainState::Ready
            },
            in_flight: stats.in_flight as u32,
            queued: stats.waiting as u32,
            requests: (stats.completed + stats.failed) as u32,
            failures: stats.failed as u32,
            tokens: s.tokens,
            latency_ms: stats.latency_ms,
            protocol: match client.protocol() {
                Protocol::Tools => "native tool calls".into(),
                Protocol::Json => "JSON output".into(),
            },
            message: if stats.circuit_open {
                format!("Paused after repeated failures; villagers are on their routines. {}", s.last_error)
            } else {
                s.last_error.clone()
            },
        }
    }

    fn publish(&self) {
        self.inner.link.status(self.status());
    }

    fn apply_tuning(&self) {
        let (latency_ms, chatter) = {
            let s = self.inner.state.lock();
            let Some(queue) = &s.queue else { return };
            (queue.latency_ms(), s.governor.chatter())
        };
        let speed = f64::from(self.inner.link.speed().max(1));
        let latency = if latency_ms > 0.0 { latency_ms } else { 5000.0 } / 1000.0;
        // How long a conversation or a yielded opponent waits for the model, in real seconds, as ticks.
        let wait_seconds = (latency * 2.0 + 4.0).clamp(7.0, 28.0);
        let ticks = js_round(wait_seconds * 20.0 * speed) as i64;
        self.inner.link.send(Command::Tuning {
            tuning: TuningPatch {
                chatter: Some(chatter),
                script_wait_ticks: Some(ticks),
                mercy_wait_ticks: Some(ticks),
            },
        });
    }

    fn mind(s: &mut BrainInner, id: &str) -> Mind {
        *s.minds.entry(id.to_owned()).or_default()
    }

    fn update_mind(&self, id: &str, f: impl FnOnce(&mut Mind)) {
        f(self.inner.state.lock().minds.entry(id.to_owned()).or_default());
    }

    fn record(
        &self,
        kind: RequestKind,
        character: &str,
        applied: (LogStatus, String, Vec<String>),
        prompt: Option<&Prompt>,
        response: Option<&LlmResponse>,
        error: Option<&LlmError>,
    ) {
        let (status, summary, commands) = applied;
        {
            let mut s = self.inner.state.lock();
            if let Some(r) = response {
                s.tokens += r.prompt_tokens + r.completion_tokens;
            }
            if let Some(e) = error {
                s.last_error = e.message.clone();
            } else if status == LogStatus::Ok {
                s.last_error.clear();
            }
        }
        let tick = read(&*self.inner.link, |state| state.tick.max(0) as u64);
        let response_text = match (response, error) {
            (Some(r), _) => r.raw.clone(),
            (None, Some(e)) => match &e.raw {
                Some(raw) => format!("{}\n\n{raw}", e.message),
                None => e.message.clone(),
            },
            _ => String::new(),
        };
        self.inner.link.log(LogRecord {
            tick,
            character: character.to_owned(),
            kind,
            status,
            latency_ms: response.map_or(0, |r| r.latency.as_millis() as u32),
            prompt_tokens: response.map_or(0, |r| r.prompt_tokens),
            completion_tokens: response.map_or(0, |r| r.completion_tokens),
            summary,
            prompt: prompt.map(prompt_text).unwrap_or_default(),
            response: response_text,
            commands,
        });
    }

    /// Runs one request through the queue. `build` is called when the request
    /// is sent, so the prompt describes the world at that moment (`None` skips
    /// it); `apply` gets the reply and says what came of it.
    #[allow(clippy::too_many_arguments)]
    fn request<T: Send + 'static>(
        &self,
        kind: RequestKind,
        key: String,
        character: Option<Id>,
        ttl: Duration,
        build: impl FnOnce(&SimState) -> Option<Built<T>> + Send + 'static,
        apply: impl FnOnce(&SimState, &LlmResponse, T) -> Applied + Send + 'static,
        on_fail: Option<OnFail>,
        after: Option<After>,
    ) {
        let (queue, client) = {
            let mut s = self.inner.state.lock();
            match (s.queue.clone(), s.client.clone()) {
                (Some(queue), Some(client)) => {
                    s.governor.spend();
                    (queue, client)
                }
                _ => {
                    drop(s);
                    if let Some(on_fail) = on_fail {
                        on_fail();
                    }
                    return;
                }
            }
        };
        let fail = move || {
            if let Some(on_fail) = &on_fail {
                on_fail();
            }
        };
        let fail = Arc::new(fail);
        let drop_fail = fail.clone();
        let brain = self.clone();
        let job = Job::new(key, kind.priority(), Instant::now() + ttl, move |cancel| async move {
            let link = brain.inner.link.clone();
            let Some(built) = read(&*link, build) else {
                fail();
                return Ok(0);
            };
            for command in built.commands {
                link.send(command);
            }
            let name = character
                .as_deref()
                .map(|id| {
                    read(&*link, |state| {
                        state.characters.get(id).map_or("—".to_owned(), |c| first_name(c).to_owned())
                    })
                })
                .unwrap_or_else(|| "—".to_owned());
            let request = LlmRequest {
                messages: built.prompt.messages.clone(),
                tools: built.prompt.tools.clone(),
                max_calls: built.prompt.max_calls,
                cancel: Some(cancel),
                ..Default::default()
            };
            match client.complete(&request).await {
                Ok(response) => {
                    let (applied, lines) = read(&*link, |state| {
                        let applied = apply(state, &response, built.context);
                        let lines = applied.commands.iter().map(|c| describe_command(state, c)).collect::<Vec<_>>();
                        (applied, lines)
                    });
                    for command in applied.commands {
                        link.send(command);
                    }
                    let status = applied.status;
                    brain.record(
                        kind,
                        &name,
                        (status, applied.summary, lines),
                        Some(&built.prompt),
                        Some(&response),
                        None,
                    );
                    if status != LogStatus::Ok {
                        fail();
                    } else if let Some(after) = after {
                        after(&brain);
                    }
                    brain.publish();
                    Ok(response.tokens_used)
                }
                Err(error) => {
                    fail();
                    if error.kind != LlmErrorKind::Aborted {
                        let summary = error.message.clone();
                        brain.record(
                            kind,
                            &name,
                            (LogStatus::Error, summary, Vec::new()),
                            Some(&built.prompt),
                            None,
                            Some(&error),
                        );
                    }
                    brain.publish();
                    Err(error)
                }
            }
        })
        .on_drop(move |_| drop_fail());
        queue.enqueue(job);
    }

    /// Per sim tick: react to events that need the model at once.
    pub fn handle(&self, events: &[SimEvent]) {
        for event in events {
            match event {
                SimEvent::ConversationStarted { convo_id, .. } => self.on_conversation(convo_id),
                SimEvent::ConversationEnded { convo_id, .. } => {
                    let queue = {
                        let mut s = self.inner.state.lock();
                        s.turns.remove(convo_id);
                        s.queue.clone()
                    };
                    if let Some(queue) = queue {
                        queue.cancel(&format!("v:{convo_id}"));
                    }
                }
                SimEvent::MercyRequest { attacker_id, victim_id } => self.on_mercy(attacker_id, victim_id),
                SimEvent::BillboardRead { id, board_id } => self.on_billboard(id, board_id),
                SimEvent::Death { id, monster: false, .. } => {
                    let queue = {
                        let mut s = self.inner.state.lock();
                        s.minds.remove(id);
                        s.queue.clone()
                    };
                    if let Some(queue) = queue {
                        queue.cancel(&format!("c:{id}"));
                    }
                }
                _ => {}
            }
        }
    }

    /// Call often (every 100 ms or so): keeps the queue moving and, a few times
    /// a second, looks for characters with something to decide.
    pub fn update(&self, force: bool) {
        let Some(queue) = self.inner.state.lock().queue.clone() else { return };
        let paused = self.inner.link.speed() == 0;
        queue.set_paused(paused);
        queue.pump();
        let now = Instant::now();
        let retune = {
            let mut s = self.inner.state.lock();
            if paused || (!force && since(s.last_scan, now) < SCAN_INTERVAL) {
                return;
            }
            s.last_scan = Some(now);
            let capacity = queue.capacity();
            s.governor.refill(capacity, now);
            s.governor.should_retune(now)
        };
        if retune {
            self.apply_tuning();
        }
        self.scan(now, &queue);
        self.publish();
    }

    fn scan(&self, now: Instant, queue: &JobQueue) {
        enum Ask {
            React(Id),
            Reflect(Id, i64),
            Plan(Id),
        }
        let mut asks = Vec::new();
        let mut deciders: Vec<(Id, i64, Option<Instant>)> = Vec::new();
        {
            let mut s = self.inner.state.lock();
            let s = &mut *s;
            read(&*self.inner.link, |state| {
                let time = game_time(state.tick);
                for c in living(state) {
                    if queue.has(&format!("c:{}", c.id))
                        || c.convo_id.is_some()
                        || c.combat.yielded
                        || c.combat.mercy_until_tick > 0
                    {
                        continue;
                    }
                    let mind = Self::mind(s, &c.id);
                    let urgency = c.stimuli.iter().map(|st| st.urgency).max().unwrap_or(-1);

                    // Danger: one quick fight-or-flee call, while instinct has already acted.
                    if is_fighting(c) {
                        let fresh = c.stimuli.iter().any(|st| {
                            matches!(st.kind, StimulusKind::Threat | StimulusKind::Attacked | StimulusKind::Alarm)
                                && state.tick - st.tick < 200
                        });
                        let reflex = c.intent.as_ref().is_some_and(|i| i.source == IntentSource::Reflex);
                        if fresh
                            && reflex
                            && since(mind.last_react_at, now) > Duration::from_secs(30)
                            && s.governor.allow(Tier::Urgent)
                        {
                            s.minds.entry(c.id.clone()).or_default().last_react_at = Some(now);
                            asks.push(Ask::React(c.id.clone()));
                        }
                        continue;
                    }

                    let retry_ok = mind.retry_at.is_none_or(|at| now >= at);
                    if c.asleep {
                        let night = if time.hour < 12.0 { time.day - 1 } else { time.day };
                        if is_night(time.hour)
                            && mind.reflected_day != night
                            && c.last_reflection_day != night
                            && c.memories.len() > 4
                            && retry_ok
                            && s.governor.allow(Tier::Idle)
                        {
                            s.minds.entry(c.id.clone()).or_default().reflected_day = night;
                            asks.push(Ask::Reflect(c.id.clone(), night));
                        }
                        continue;
                    }

                    // Each morning, once: a plan for the day in the character's own words. A plan
                    // the model already wrote today (before a restart) counts.
                    let planned_already = c.plan_source == PlanSource::Llm && c.plan_day == time.day;
                    if mind.planned_day != time.day
                        && !planned_already
                        && time.hour >= 4.5
                        && time.hour < 11.0
                        && c.plan_day == time.day
                        && retry_ok
                        && s.governor.allow(Tier::Awaited)
                    {
                        s.minds.entry(c.id.clone()).or_default().planned_day = time.day;
                        asks.push(Ask::Plan(c.id.clone()));
                        continue;
                    }

                    if urgency < 0 {
                        continue;
                    }
                    let cooldown = if urgency >= 2 {
                        Duration::from_secs(6)
                    } else if urgency >= 1 {
                        Duration::from_secs(18)
                    } else {
                        Duration::from_secs_f64(40.0 * s.governor.patience())
                    };
                    if since(mind.last_decide_at, now) >= cooldown {
                        deciders.push((c.id.clone(), urgency, mind.last_decide_at));
                    }
                }
            });
        }
        for ask in asks {
            match ask {
                Ask::React(id) => self.request_react(id),
                Ask::Reflect(id, night) => self.request_reflect(id, night),
                Ask::Plan(id) => self.request_plan(id),
            }
        }

        // Whoever has waited longest for a decision goes first, so a slow model is shared fairly.
        deciders.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
        for (id, urgency, _) in deciders {
            {
                let mut s = self.inner.state.lock();
                let tier = if urgency >= 1 { Tier::Awaited } else { Tier::Idle };
                if !s.governor.allow(tier) {
                    break;
                }
                s.minds.entry(id.clone()).or_default().last_decide_at = Some(now);
            }
            self.request_decide(id);
        }
    }

    fn request_decide(&self, id: Id) {
        let budget = self.prompt_budget();
        let build_id = id.clone();
        let apply_id = id.clone();
        self.request(
            RequestKind::Decide,
            format!("c:{id}"),
            Some(id),
            Duration::from_secs(30),
            move |state| {
                let me = state.characters.get(&build_id)?;
                if me.convo_id.is_some() || me.asleep || is_fighting(me) || me.combat.yielded {
                    return None;
                }
                let prompt = decide_prompt(state, me, budget);
                Some(Built {
                    prompt,
                    commands: vec![Command::ClearStimuli { id: build_id.clone(), through_tick: state.tick }],
                    context: me.epoch,
                })
            },
            move |state, response, epoch| {
                let Some(me) = state.characters.get(&apply_id).filter(|c| c.epoch == epoch) else {
                    return Applied::stale("the situation changed before the answer came");
                };
                let decision = interpret_decision(state, me, &response.calls);
                let steps = decision.intents.iter().map(|i| describe_intent(state, i)).collect::<Vec<_>>().join(" → ");
                let problems = if decision.problems.is_empty() {
                    String::new()
                } else {
                    format!(" (dropped: {})", decision.problems.join("; "))
                };
                if !decision.intents.is_empty() {
                    let command = Command::SetIntents { id: apply_id.clone(), epoch, intents: decision.intents };
                    return Applied::ok(format!("{steps}{problems}"), vec![command]);
                }
                let mut commands = Vec::new();
                if let Some(thought) = &decision.thought {
                    commands.push(Command::Thought { id: apply_id.clone(), text: thought.clone() });
                }
                if decision.keep {
                    let note = decision.thought.as_ref().map_or(String::new(), |t| format!(": {t}"));
                    return Applied::ok(format!("carries on{note}"), commands);
                }
                Applied { status: LogStatus::Fallback, summary: format!("nothing usable{problems}"), commands }
            },
            None,
            None,
        );
    }

    fn request_react(&self, id: Id) {
        let build_id = id.clone();
        let apply_id = id.clone();
        self.request(
            RequestKind::React,
            format!("c:{id}"),
            Some(id),
            Duration::from_secs(9),
            move |state| {
                let me = state.characters.get(&build_id).filter(|c| is_fighting(c) && !c.combat.yielded)?;
                Some(Built {
                    prompt: react_prompt(state, me),
                    commands: vec![Command::ClearStimuli { id: build_id.clone(), through_tick: state.tick }],
                    context: me.epoch,
                })
            },
            move |state, response, epoch| {
                let Some(me) = state.characters.get(&apply_id).filter(|c| c.epoch == epoch) else {
                    return Applied::stale("the fight moved on before the answer came");
                };
                let decision = interpret_decision(state, me, &response.calls);
                let Some(intent) = decision.intents.into_iter().next() else {
                    return Applied::fallback(format!("nothing usable ({})", decision.problems.join("; ")));
                };
                let said = intent.say.as_ref().map_or(String::new(), |s| format!(": “{s}”"));
                let summary = format!("{}{said}", intent.action.kind());
                Applied::ok(summary, vec![Command::SetIntents { id: apply_id.clone(), epoch, intents: vec![intent] }])
            },
            None,
            None,
        );
    }

    fn request_plan(&self, id: Id) {
        let budget = self.prompt_budget();
        let (build_id, apply_id, fail_id) = (id.clone(), id.clone(), id.clone());
        let brain = self.clone();
        let on_fail: OnFail = Arc::new(move || {
            // Try again later this morning, but not at once.
            brain.update_mind(&fail_id, |mind| {
                mind.planned_day = 0;
                mind.retry_at = Some(Instant::now() + RETRY_AFTER);
            });
        });
        self.request(
            RequestKind::PlanDay,
            format!("c:{id}"),
            Some(id),
            Duration::from_secs(90),
            move |state| {
                let me = state.characters.get(&build_id).filter(|c| !c.asleep && !is_fighting(c))?;
                built(plan_prompt(state, me, budget), ())
            },
            move |_, response, ()| {
                let Some(plan) = interpret_plan(&response.calls) else {
                    return Applied::fallback("the plan could not be read");
                };
                let summary = plan
                    .blocks
                    .iter()
                    .map(|b| format!("{} {}", format_clock(b.hour * 60.0), b.activity.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ");
                Applied::ok(
                    summary,
                    vec![Command::SetPlan { id: apply_id.clone(), blocks: plan.blocks, thought: plan.thought }],
                )
            },
            Some(on_fail),
            None,
        );
    }

    fn request_reflect(&self, id: Id, night: i64) {
        let budget = self.prompt_budget();
        let (build_id, apply_id, fail_id) = (id.clone(), id.clone(), id.clone());
        let brain = self.clone();
        let on_fail: OnFail = Arc::new(move || {
            // Let it be tried again later the same night, but not at once.
            brain.update_mind(&fail_id, |mind| {
                mind.reflected_day = 0;
                mind.retry_at = Some(Instant::now() + RETRY_AFTER);
            });
        });
        let done_id = id.clone();
        self.request(
            RequestKind::Reflect,
            format!("c:{id}"),
            Some(id),
            Duration::from_secs(240),
            move |state| {
                let me = state.characters.get(&build_id).filter(|c| c.asleep)?;
                built(reflect_prompt(state, me, budget), ())
            },
            move |state, response, ()| {
                let call = response.calls.iter().find(|c| c.name == "reflection");
                let summary = call.and_then(|c| c.args.get("summary")).and_then(Value::as_str).unwrap_or_default();
                let Some(call) = call.filter(|_| !summary.trim().is_empty()) else {
                    return Applied::fallback("the reflection could not be read");
                };
                let insights = match call.args.get("insights") {
                    Some(Value::Array(items)) => items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
                    _ => Vec::new(),
                };
                let relationships = reflection_relationships(state, &call.args, &apply_id)
                    .into_iter()
                    .map(|(id, summary)| RelationshipNote { id, summary })
                    .collect();
                let command =
                    Command::Reflection { id: apply_id.clone(), summary: summary.to_owned(), insights, relationships };
                Applied::ok(utf16_truncate(summary, 140), vec![command])
            },
            Some(on_fail),
            Some(Box::new(move |brain: &Brain| brain.update_mind(&done_id, |mind| mind.reflected_day = night))),
        );
    }

    fn conversation_mode(&self) -> ConversationMode {
        let s = self.inner.state.lock();
        let preference = s.settings.as_ref().map_or(ConversationMode::Auto, |s| s.conversations);
        if preference != ConversationMode::Auto {
            return preference;
        }
        if s.queue.as_ref().is_some_and(|q| q.capacity() >= 1.2) {
            ConversationMode::Turns
        } else {
            ConversationMode::Batched
        }
    }

    fn on_conversation(&self, convo_id: &str) {
        let link = self.inner.link.clone();
        let (enabled, allowed) = {
            let s = self.inner.state.lock();
            (s.queue.is_some(), s.governor.allow(Tier::Awaited))
        };
        if !enabled || !allowed {
            if enabled {
                link.send(Command::ConversationLocal { convo_id: convo_id.to_owned() });
            }
            return;
        }
        if self.conversation_mode() == ConversationMode::Turns {
            let Some((lines, first)) = read(&*link, |state| {
                state.conversations.get(convo_id).map(|c| {
                    let first = if c.lines.is_empty() { c.a.clone() } else { c.b.clone() };
                    (c.lines.clone(), first)
                })
            }) else {
                return;
            };
            self.inner.state.lock().turns.insert(convo_id.to_owned(), lines);
            // Whoever has not spoken yet goes first: the initiator if there is no opening line.
            self.request_reply(convo_id.to_owned(), first);
            return;
        }
        let local_id = convo_id.to_owned();
        let use_local: OnFail = Arc::new({
            let link = link.clone();
            move || link.send(Command::ConversationLocal { convo_id: local_id.clone() })
        });
        let budget = self.prompt_budget();
        let (build_id, apply_id) = (convo_id.to_owned(), convo_id.to_owned());
        let initiator = read(&*link, |state| state.conversations.get(convo_id).map(|c| c.a.clone()));
        self.request(
            RequestKind::Converse,
            format!("v:{convo_id}"),
            initiator,
            Duration::from_secs(20),
            move |state| {
                let convo = state.conversations.get(&build_id).filter(|c| !c.closed)?;
                let (a, b) = (state.characters.get(&convo.a)?, state.characters.get(&convo.b)?);
                built(conversation_prompt(state, convo, a, b, budget), ())
            },
            move |state, response, ()| {
                let parties = state
                    .conversations
                    .get(&apply_id)
                    .filter(|c| !c.closed)
                    .and_then(|c| Some((c, state.characters.get(&c.a)?, state.characters.get(&c.b)?)));
                let Some((convo, a, b)) = parties else { return Applied::stale("the conversation was already over") };
                let Some(script) = interpret_conversation(&response.calls, a, b, convo.lines.is_empty()) else {
                    return Applied::fallback("the lines could not be read");
                };
                let topic = if script.topic.is_empty() { "this and that" } else { &script.topic };
                let summary = format!(
                    "{} & {} on {topic} ({} lines)",
                    first_name(a),
                    first_name(b),
                    script.lines.len() + convo.lines.len()
                );
                let command = Command::ConversationLines {
                    convo_id: apply_id.clone(),
                    lines: script.lines,
                    topic: Some(script.topic).filter(|t| !t.is_empty()),
                    feelings: Some(script.feelings),
                    closed: true,
                };
                Applied::ok(summary, vec![command])
            },
            Some(use_local),
            None,
        );
    }

    /// Turn-by-turn: one speaker's next line, then the other's, until someone ends it.
    fn request_reply(&self, convo_id: Id, speaker: Id) {
        let link = self.inner.link.clone();
        let brain = self.clone();
        let local_id = convo_id.clone();
        let use_local: OnFail = Arc::new(move || {
            brain.inner.state.lock().turns.remove(&local_id);
            link.send(Command::ConversationLocal { convo_id: local_id.clone() });
        });
        let budget = self.prompt_budget();
        let (build_id, build_speaker) = (convo_id.clone(), speaker.clone());
        let (apply_id, apply_speaker) = (convo_id.clone(), speaker.clone());
        let turns_for_build = self.clone();
        let turns_for_apply = self.clone();
        let next: Arc<Mutex<Option<Id>>> = Arc::new(Mutex::new(None));
        let next_for_apply = next.clone();
        let after_id = convo_id.clone();
        self.request(
            RequestKind::Reply,
            format!("v:{convo_id}"),
            Some(speaker),
            Duration::from_secs(20),
            move |state| {
                let convo = state.conversations.get(&build_id).filter(|c| !c.closed)?;
                let lines = turns_for_build.inner.state.lock().turns.get(&build_id).cloned()?;
                let me = state.characters.get(&build_speaker)?;
                let other_id = if convo.a == build_speaker { &convo.b } else { &convo.a };
                let other = state.characters.get(other_id)?;
                built(reply_prompt(state, convo, &lines, me, other, budget), ())
            },
            move |state, response, ()| {
                let Some(convo) = state.conversations.get(&apply_id).filter(|c| !c.closed) else {
                    return Applied::stale("the conversation was already over");
                };
                let mut s = turns_for_apply.inner.state.lock();
                let Some(lines) = s.turns.get_mut(&apply_id) else {
                    return Applied::stale("the conversation was already over");
                };
                let call = response.calls.iter().find(|c| c.name == "reply");
                let Some((call, text)) = call.and_then(|c| spoken(c.args.get("text")).map(|t| (c, t))) else {
                    return Applied::fallback("the reply could not be read");
                };
                lines.push(ConversationLine { speaker_id: apply_speaker.clone(), text: text.clone() });
                let closed = call.args.get("end") == Some(&Value::Bool(true)) || lines.len() >= MAX_TURN_LINES;
                let feelings = feeling(call.args.get("feeling")).map(|f| {
                    let mut map = Map::new();
                    map.insert(apply_speaker.clone(), f);
                    map
                });
                if closed {
                    s.turns.remove(&apply_id);
                } else {
                    *next_for_apply.lock() =
                        Some(if convo.a == apply_speaker { convo.b.clone() } else { convo.a.clone() });
                }
                let command = Command::ConversationLines {
                    convo_id: apply_id.clone(),
                    lines: vec![ConversationLine { speaker_id: apply_speaker.clone(), text: text.clone() }],
                    topic: Some(convo.topic.clone()).filter(|t| !t.is_empty()),
                    feelings,
                    closed,
                };
                Applied::ok(format!("“{text}”{}", if closed { " (ends)" } else { "" }), vec![command])
            },
            Some(use_local),
            Some(Box::new(move |brain: &Brain| {
                // Ask for the answer straight away, once this request has left the queue,
                // so it is ready by the time this line has been spoken.
                if let Some(next) = next.lock().take() {
                    let brain = brain.clone();
                    tokio::spawn(async move {
                        tokio::task::yield_now().await;
                        brain.request_reply(after_id, next);
                    });
                }
            })),
        );
    }

    fn on_mercy(&self, attacker: &str, victim: &str) {
        let (build_attacker, build_victim, apply_attacker) =
            (attacker.to_owned(), victim.to_owned(), attacker.to_owned());
        self.request(
            RequestKind::Mercy,
            format!("c:{attacker}"),
            Some(attacker.to_owned()),
            Duration::from_secs(15),
            move |state| {
                let a = state.characters.get(&build_attacker).filter(|a| a.combat.mercy_until_tick > 0)?;
                let v = state.characters.get(&build_victim)?;
                built(mercy_prompt(state, a, v), ())
            },
            move |_, response, ()| {
                let Some(call) = response.calls.iter().find(|c| c.name == "spare" || c.name == "finish") else {
                    return Applied::fallback("no choice was made; mercy by default");
                };
                let say = spoken(call.args.get("say"));
                let mut commands = Vec::new();
                if let Some(Value::String(thought)) = call.args.get("thought") {
                    commands.push(Command::Thought { id: apply_attacker.clone(), text: thought.clone() });
                }
                let spare = call.name == "spare";
                let summary = format!("{}{}", call.name, say.as_ref().map_or(String::new(), |s| format!(": “{s}”")));
                commands.push(Command::Mercy { attacker_id: apply_attacker.clone(), spare, say });
                Applied::ok(summary, commands)
            },
            None,
            None,
        );
    }

    /// Someone is reading a billboard: ask how they take it. Only when the model has room to spare; the sim
    /// gives them a stock reaction when no answer comes.
    fn on_billboard(&self, id: &str, board_id: &str) {
        let allowed = {
            let s = self.inner.state.lock();
            s.queue.is_some() && s.governor.allow(Tier::Idle)
        };
        if !allowed || self.backlogged() {
            return;
        }
        let (build_id, apply_id, board) = (id.to_owned(), id.to_owned(), board_id.to_owned());
        self.request(
            RequestKind::Billboard,
            format!("c:{id}:board"),
            Some(id.to_owned()),
            Duration::from_secs(10),
            move |state| {
                let c = state.characters.get(&build_id)?;
                let index = reading(c).filter(|&i| billboards()[i].id == board && !has_read(state, c, i))?;
                let place = get_place(&billboards()[index].place_id).name;
                let text = shown(state, index).text;
                built(billboard_prompt(state, c, place, &text), billboards()[index].id.clone())
            },
            move |_, response, board_id| {
                let Some(call) = response.calls.iter().find(|c| c.name == "react_to_billboard") else {
                    return Applied::fallback("no reaction; the stock one will do");
                };
                let Some(reaction) = call.args.get("reaction").and_then(Value::as_str).and_then(BoardReaction::parse)
                else {
                    return Applied::fallback("an unknown reaction; the stock one will do");
                };
                let say = spoken(call.args.get("say"));
                let thought = match call.args.get("thought") {
                    Some(Value::String(thought)) if !thought.trim().is_empty() => Some(thought.clone()),
                    _ => None,
                };
                let summary =
                    format!("{}{}", reaction.as_str(), say.as_ref().map_or(String::new(), |s| format!(": “{s}”")));
                Applied::ok(
                    summary,
                    vec![Command::BillboardReaction { id: apply_id.clone(), board_id, reaction, say, thought }],
                )
            },
            None,
            None,
        );
    }
}
