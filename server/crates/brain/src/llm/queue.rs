//! The request queue (port of `src/brain/llm/queue.ts`).
//!
//! Runs LLM jobs with a concurrency limit, strict priority order, an optional
//! requests-per-minute cap, daily request and token caps, and a circuit
//! breaker, so a slow or broken endpoint never piles up work: late jobs are
//! dropped and their callers fall back.
//!
//! Each job runs in its own task with a `CancellationToken`. Like the
//! browser's queue, starting work is driven by [`JobQueue::pump`]: it runs on
//! every enqueue and every finished job, and the brain calls it on each scan
//! because expiry and the rate limits depend on time.
//!
//! New on the server: the daily caps (`max_requests_per_day`,
//! `max_tokens_per_day`). The budget at any moment is the cap times the share
//! of the UTC day gone by plus half an hour's worth, so a busy morning cannot
//! leave the village mute in the evening. Requests are counted per job
//! started; tokens as each job reports them. A job that panics frees its slot
//! and counts as failed, without tripping the breaker.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::transport::BoxFuture;
use super::types::{LlmError, LlmErrorKind};

/// Consecutive failures that open the circuit.
pub const CIRCUIT_FAILURES: u32 = 4;
/// How long the circuit stays open.
pub const CIRCUIT_OPEN: Duration = Duration::from_secs(30);
/// The requests-per-minute window.
pub const RATE_WINDOW: Duration = Duration::from_secs(60);
/// The longest a Retry-After pauses the queue.
pub const MAX_RETRY_PAUSE: Duration = Duration::from_secs(60);
pub const DAY_SECS: u64 = 86_400;
/// Share of a daily cap that may be spent ahead of schedule: half an hour's worth.
const BURST_FRACTION: f64 = 1.0 / 48.0;

/// Why a job never ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DropReason {
    Expired,
    Replaced,
    Circuit,
    Cancelled,
}

/// What a job's run returns: the tokens it spent, or why it failed.
pub type JobResult = Result<u64, LlmError>;
type RunFn = Box<dyn FnOnce(CancellationToken) -> BoxFuture<'static, JobResult> + Send>;
type DropFn = Box<dyn FnOnce(DropReason) + Send>;

pub struct Job {
    /// jobs with the same key replace each other while queued
    pub key: String,
    /// lower runs first
    pub priority: i32,
    /// dropped (on_drop is called) if not started by this time
    pub expires_at: Instant,
    run: RunFn,
    on_drop: Option<DropFn>,
}

impl Job {
    /// `run` is called when the job starts, in its own task; the token is
    /// cancelled when the job is cancelled. It returns the tokens it spent.
    pub fn new<F, Fut>(key: impl Into<String>, priority: i32, expires_at: Instant, run: F) -> Self
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = JobResult> + Send + 'static,
    {
        Self { key: key.into(), priority, expires_at, run: Box::new(move |token| Box::pin(run(token))), on_drop: None }
    }

    /// Called instead of running when the job is dropped.
    pub fn on_drop(mut self, on_drop: impl FnOnce(DropReason) + Send + 'static) -> Self {
        self.on_drop = Some(Box::new(on_drop));
        self
    }

    fn dropped(self, reason: DropReason) {
        if let Some(on_drop) = self.on_drop {
            on_drop(reason);
        }
    }
}

impl fmt::Debug for Job {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Job").field("key", &self.key).field("priority", &self.priority).finish_non_exhaustive()
    }
}

/// Requests, failures and tokens of one UTC day, for the daily caps and the admin panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsage {
    /// days since 1970-01-01, UTC
    pub day: u64,
    pub requests: u64,
    pub failures: u64,
    pub tokens: u64,
}

/// Daily request and token caps spread through the day. Pure: every call takes the wall time.
#[derive(Debug, Clone, Default)]
pub struct DailyBudget {
    /// 0 = no cap
    pub max_requests: u64,
    /// 0 = no cap
    pub max_tokens: u64,
    usage: DailyUsage,
}

impl DailyBudget {
    pub fn new(max_requests: u64, max_tokens: u64) -> Self {
        Self { max_requests, max_tokens, usage: DailyUsage::default() }
    }

    /// The UTC day and the share of it gone by.
    pub fn day_and_fraction(now: SystemTime) -> (u64, f64) {
        let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
        let day = (secs / DAY_SECS as f64).floor();
        (day as u64, (secs - day * DAY_SECS as f64) / DAY_SECS as f64)
    }

    /// What may have been spent by this point of the day: the cap's share plus a small burst.
    pub fn budget(cap: u64, fraction: f64) -> f64 {
        let cap = cap as f64;
        (cap * fraction + (cap * BURST_FRACTION).max(1.0)).min(cap)
    }

    fn roll(&mut self, now: SystemTime) -> f64 {
        let (day, fraction) = Self::day_and_fraction(now);
        if self.usage.day != day {
            self.usage = DailyUsage { day, ..DailyUsage::default() };
        }
        fraction
    }

    /// Whether another request fits today's budget so far.
    pub fn allows(&mut self, now: SystemTime) -> bool {
        let fraction = self.roll(now);
        let fits = |used: u64, cap: u64| cap == 0 || (used as f64) < Self::budget(cap, fraction);
        fits(self.usage.requests, self.max_requests) && fits(self.usage.tokens, self.max_tokens)
    }

    pub fn record_request(&mut self, now: SystemTime) {
        self.roll(now);
        self.usage.requests += 1;
    }

    pub fn record_failure(&mut self, now: SystemTime) {
        self.roll(now);
        self.usage.failures += 1;
    }

    pub fn record_tokens(&mut self, now: SystemTime, tokens: u64) {
        self.roll(now);
        self.usage.tokens += tokens;
    }

    /// Today's usage (zero if the stored day has passed).
    pub fn usage(&mut self, now: SystemTime) -> DailyUsage {
        self.roll(now);
        self.usage
    }

    /// Carries usage over a restart; ignored on the next call if it is from another day.
    pub fn restore(&mut self, usage: DailyUsage) {
        self.usage = usage;
    }
}

/// A snapshot for the status line and the admin panel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueStats {
    pub in_flight: usize,
    pub waiting: usize,
    pub completed: u64,
    pub failed: u64,
    /// moving average of how long a job takes
    pub latency_ms: f64,
    pub circuit_open: bool,
    /// requests per second the endpoint can absorb
    pub capacity: f64,
    pub today: DailyUsage,
}

/// The wall clock used for the daily caps; tests inject their own.
pub type WallClock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Slot {
    priority: i32,
    expires_at: Instant,
    seq: u64,
    key: String,
}

struct Queued {
    job: Job,
    seq: u64,
}

struct State {
    /// live queued jobs by key; the heap may also hold stale slots, skipped by `seq`
    queued: HashMap<String, Queued>,
    heap: BinaryHeap<Reverse<Slot>>,
    running: HashMap<String, CancellationToken>,
    start_times: VecDeque<Instant>,
    failures: u32,
    circuit_open_until: Option<Instant>,
    paused_until: Option<Instant>,
    paused: bool,
    latency_ms: f64,
    /// moving average of tokens per job, for the token cap's share of capacity
    tokens_per_job: f64,
    completed: u64,
    failed: u64,
    concurrency: u32,
    /// permits to retire as running jobs finish, after the concurrency was lowered
    permits_to_forget: u32,
    requests_per_minute: u32,
    daily: DailyBudget,
    seq: u64,
}

impl State {
    fn circuit_open(&self, now: Instant) -> bool {
        self.circuit_open_until.is_some_and(|until| now < until)
    }

    fn under_rate_limit(&mut self, now: Instant) -> bool {
        if self.requests_per_minute == 0 {
            return true;
        }
        while self.start_times.front().is_some_and(|start| now - *start > RATE_WINDOW) {
            self.start_times.pop_front();
        }
        self.start_times.len() < self.requests_per_minute as usize
    }

    fn pop(&mut self) -> Option<Job> {
        while let Some(Reverse(slot)) = self.heap.pop() {
            if self.queued.get(&slot.key).is_some_and(|queued| queued.seq == slot.seq) {
                return self.queued.remove(&slot.key).map(|queued| queued.job);
            }
        }
        None
    }

    /// Removes the queued jobs matching `filter`, in queue order.
    fn take(&mut self, filter: impl Fn(&Job) -> bool) -> Vec<Job> {
        let mut keys: Vec<(i32, Instant, u64, String)> = self
            .queued
            .iter()
            .filter(|(_, queued)| filter(&queued.job))
            .map(|(key, queued)| (queued.job.priority, queued.job.expires_at, queued.seq, key.clone()))
            .collect();
        keys.sort();
        let taken =
            keys.into_iter().filter_map(|(.., key)| self.queued.remove(&key)).map(|queued| queued.job).collect();
        self.compact();
        taken
    }

    /// Drops stale heap slots once they outnumber the live ones.
    fn compact(&mut self) {
        if self.heap.len() > 2 * self.queued.len() + 32 {
            let live = self.queued.iter().map(|(key, queued)| {
                Reverse(Slot {
                    priority: queued.job.priority,
                    expires_at: queued.job.expires_at,
                    seq: queued.seq,
                    key: key.clone(),
                })
            });
            self.heap = live.collect();
        }
    }

    fn capacity(&self) -> f64 {
        let latency = if self.latency_ms == 0.0 { 4000.0 } else { self.latency_ms };
        let mut capacity = f64::from(self.concurrency) / (latency / 1000.0).max(0.3);
        let day = DAY_SECS as f64;
        if self.daily.max_requests > 0 {
            capacity = capacity.min(self.daily.max_requests as f64 / day);
        }
        if self.daily.max_tokens > 0 && self.tokens_per_job > 0.0 {
            capacity = capacity.min(self.daily.max_tokens as f64 / day / self.tokens_per_job);
        }
        capacity
    }
}

struct Shared {
    state: Mutex<State>,
    semaphore: Arc<Semaphore>,
    clock: WallClock,
}

/// The job queue. Cheap to clone; every clone is the same queue. Needs a Tokio runtime.
#[derive(Clone)]
pub struct JobQueue {
    shared: Arc<Shared>,
}

impl fmt::Debug for JobQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobQueue").field("stats", &self.stats()).finish()
    }
}

impl JobQueue {
    /// `requests_per_minute` 0 means no cap.
    pub fn new(concurrency: u32, requests_per_minute: u32) -> Self {
        Self::with_clock(concurrency, requests_per_minute, Arc::new(SystemTime::now))
    }

    pub fn with_clock(concurrency: u32, requests_per_minute: u32, clock: WallClock) -> Self {
        let state = State {
            queued: HashMap::new(),
            heap: BinaryHeap::new(),
            running: HashMap::new(),
            start_times: VecDeque::new(),
            failures: 0,
            circuit_open_until: None,
            paused_until: None,
            paused: false,
            latency_ms: 0.0,
            tokens_per_job: 0.0,
            completed: 0,
            failed: 0,
            concurrency,
            permits_to_forget: 0,
            requests_per_minute,
            daily: DailyBudget::default(),
            seq: 0,
        };
        let semaphore = Arc::new(Semaphore::new(concurrency as usize));
        Self { shared: Arc::new(Shared { state: Mutex::new(state), semaphore, clock }) }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wall(&self) -> SystemTime {
        (self.shared.clock)()
    }

    pub fn in_flight(&self) -> usize {
        self.state().running.len()
    }

    pub fn waiting(&self) -> usize {
        self.state().queued.len()
    }

    pub fn completed(&self) -> u64 {
        self.state().completed
    }

    pub fn failed(&self) -> u64 {
        self.state().failed
    }

    /// Moving average of how long a job takes, in ms (0 until one finished).
    pub fn latency_ms(&self) -> f64 {
        self.state().latency_ms
    }

    pub fn concurrency(&self) -> u32 {
        self.state().concurrency
    }

    pub fn circuit_open(&self) -> bool {
        self.state().circuit_open(Instant::now())
    }

    /// Requests per second this endpoint can absorb at the current latency, within the daily caps.
    pub fn capacity(&self) -> f64 {
        self.state().capacity()
    }

    pub fn has(&self, key: &str) -> bool {
        let state = self.state();
        state.running.contains_key(key) || state.queued.contains_key(key)
    }

    pub fn stats(&self) -> QueueStats {
        let wall = self.wall();
        let mut state = self.state();
        QueueStats {
            in_flight: state.running.len(),
            waiting: state.queued.len(),
            completed: state.completed,
            failed: state.failed,
            latency_ms: state.latency_ms,
            circuit_open: state.circuit_open(Instant::now()),
            capacity: state.capacity(),
            today: state.daily.usage(wall),
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.state().paused = paused;
        if !paused {
            self.pump();
        }
    }

    pub fn set_requests_per_minute(&self, requests_per_minute: u32) {
        self.state().requests_per_minute = requests_per_minute;
    }

    /// 0 = no cap.
    pub fn set_daily_caps(&self, max_requests: u64, max_tokens: u64) {
        let mut state = self.state();
        state.daily.max_requests = max_requests;
        state.daily.max_tokens = max_tokens;
    }

    /// Today's usage, to persist.
    pub fn usage(&self) -> DailyUsage {
        let wall = self.wall();
        self.state().daily.usage(wall)
    }

    /// Usage persisted before a restart.
    pub fn restore_usage(&self, usage: DailyUsage) {
        self.state().daily.restore(usage);
    }

    /// Changes how many jobs run at once; running jobs above a lowered limit finish first.
    pub fn set_concurrency(&self, concurrency: u32) {
        {
            let mut state = self.state();
            let semaphore = &self.shared.semaphore;
            if concurrency > state.concurrency {
                let mut add = concurrency - state.concurrency;
                let repaid = add.min(state.permits_to_forget);
                state.permits_to_forget -= repaid;
                add -= repaid;
                semaphore.add_permits(add as usize);
            } else {
                let remove = state.concurrency - concurrency;
                let forgotten = semaphore.forget_permits(remove as usize) as u32;
                state.permits_to_forget += remove - forgotten;
            }
            state.concurrency = concurrency;
        }
        self.pump();
    }

    pub fn enqueue(&self, job: Job) {
        let mut dropped = Vec::new();
        {
            let mut state = self.state();
            if state.circuit_open(Instant::now()) {
                drop(state);
                job.dropped(DropReason::Circuit);
                return;
            }
            if let Some(existing) = state.queued.remove(&job.key) {
                dropped.push(existing.job);
            }
            state.seq += 1;
            let seq = state.seq;
            let slot = Slot { priority: job.priority, expires_at: job.expires_at, seq, key: job.key.clone() };
            state.heap.push(Reverse(slot));
            state.queued.insert(job.key.clone(), Queued { job, seq });
            state.compact();
        }
        for job in dropped {
            job.dropped(DropReason::Replaced);
        }
        self.pump();
    }

    /// Cancels queued and running jobs whose key starts with `prefix`.
    pub fn cancel(&self, prefix: &str) {
        let cancelled = {
            let mut state = self.state();
            for (key, token) in &state.running {
                if key.starts_with(prefix) {
                    token.cancel();
                }
            }
            state.take(|job| job.key.starts_with(prefix))
        };
        for job in cancelled.into_iter().rev() {
            job.dropped(DropReason::Cancelled);
        }
    }

    pub fn clear(&self) {
        self.cancel("");
    }

    /// Starts as many queued jobs as the limits allow. Call regularly: expiry and rate limits are time-based.
    pub fn pump(&self) {
        let mut dropped = Vec::new();
        {
            let wall = self.wall();
            let mut state = self.state();
            let now = Instant::now();
            let expired = state.take(|job| job.expires_at <= now);
            dropped.extend(expired.into_iter().rev().map(|job| (job, DropReason::Expired)));
            let paused = state.paused || state.paused_until.is_some_and(|until| now < until);
            while !paused && !state.queued.is_empty() && state.under_rate_limit(now) && state.daily.allows(wall) {
                let Ok(permit) = self.shared.semaphore.clone().try_acquire_owned() else { break };
                let Some(job) = state.pop() else { break };
                if state.running.contains_key(&job.key) {
                    dropped.push((job, DropReason::Replaced));
                    continue;
                }
                self.start(&mut state, job, permit, now, wall);
            }
        }
        for (job, reason) in dropped {
            job.dropped(reason);
        }
    }

    fn start(&self, state: &mut State, job: Job, permit: OwnedSemaphorePermit, now: Instant, wall: SystemTime) {
        let token = CancellationToken::new();
        state.running.insert(job.key.clone(), token.clone());
        state.start_times.push_back(now);
        state.daily.record_request(wall);
        let guard = Finish { queue: self.clone(), key: job.key, started: now, permit: Some(permit), done: false };
        let run = job.run;
        tokio::spawn(async move {
            let result = run(token).await;
            guard.finish(result);
        });
    }

    /// Books a finished job. `None` means it never returned (it panicked).
    fn settle(
        &self,
        key: &str,
        started: Instant,
        result: Option<JobResult>,
        permit: Option<OwnedSemaphorePermit>,
    ) -> Vec<Job> {
        let wall = self.wall();
        let mut state = self.state();
        let now = Instant::now();
        let mut dropped = Vec::new();
        match result {
            Some(Ok(tokens)) => {
                state.failures = 0;
                state.completed += 1;
                let elapsed = (now - started).as_secs_f64() * 1000.0;
                state.latency_ms =
                    if state.latency_ms == 0.0 { elapsed } else { state.latency_ms * 0.75 + elapsed * 0.25 };
                state.daily.record_tokens(wall, tokens);
                if tokens > 0 {
                    let tokens = tokens as f64;
                    state.tokens_per_job =
                        if state.tokens_per_job == 0.0 { tokens } else { state.tokens_per_job * 0.9 + tokens * 0.1 };
                }
            }
            Some(Err(error)) => {
                state.failed += 1;
                state.daily.record_failure(wall);
                state.daily.record_tokens(wall, error.tokens_used);
                if error.kind != LlmErrorKind::Aborted {
                    if let Some(retry_after) = error.retry_after {
                        state.paused_until = Some(now + retry_after.min(MAX_RETRY_PAUSE));
                    }
                    // A model that answered but called no tool is a prompt problem, not an outage.
                    if !matches!(error.kind, LlmErrorKind::NoCall | LlmErrorKind::Truncated) {
                        state.failures += 1;
                        if state.failures >= CIRCUIT_FAILURES {
                            state.circuit_open_until = Some(now + CIRCUIT_OPEN);
                            state.failures = 0;
                            dropped = state.take(|_| true);
                        }
                    }
                }
            }
            None => {
                state.failed += 1;
                state.daily.record_failure(wall);
            }
        }
        state.running.remove(key);
        if let Some(permit) = permit {
            if state.permits_to_forget > 0 {
                state.permits_to_forget -= 1;
                permit.forget();
            } else {
                drop(permit);
            }
        }
        dropped
    }
}

/// Books a running job when it ends, also when its task panics or is torn down.
struct Finish {
    queue: JobQueue,
    key: String,
    started: Instant,
    permit: Option<OwnedSemaphorePermit>,
    done: bool,
}

impl Finish {
    fn finish(mut self, result: JobResult) {
        self.done = true;
        let dropped = self.queue.settle(&self.key, self.started, Some(result), self.permit.take());
        for job in dropped {
            job.dropped(DropReason::Circuit);
        }
        self.queue.pump();
    }
}

impl Drop for Finish {
    fn drop(&mut self) {
        if !self.done {
            self.queue.settle(&self.key, self.started, None, self.permit.take());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use tokio::time::sleep;

    use super::*;

    type Log = Arc<Mutex<Vec<String>>>;

    fn log() -> Log {
        Arc::new(Mutex::new(Vec::new()))
    }

    fn entries(log: &Log) -> Vec<String> {
        log.lock().unwrap().clone()
    }

    fn later(ms: u64) -> Instant {
        Instant::now() + Duration::from_millis(ms)
    }

    /// A job that logs its key when it starts and its drop reason if dropped, then works for `ms`.
    fn job(key: &str, priority: i32, ms: u64, order: &Log, dropped: &Log) -> Job {
        let (name, order, dropped) = (key.to_string(), order.clone(), dropped.clone());
        Job::new(key, priority, later(5000), move |_| async move {
            order.lock().unwrap().push(name);
            sleep(Duration::from_millis(ms)).await;
            Ok(0)
        })
        .on_drop({
            let key = key.to_string();
            move |reason| dropped.lock().unwrap().push(format!("{key}:{reason:?}"))
        })
    }

    fn failing(key: &str, kind: LlmErrorKind) -> Job {
        Job::new(key, 1, later(5000), move |_| async move { Err(LlmError::new(kind, "down")) })
    }

    #[tokio::test(start_paused = true)]
    async fn respects_concurrency_and_priority_and_replaces_queued_jobs_with_the_same_key() {
        let queue = JobQueue::new(1, 0);
        let (order, dropped) = (log(), log());
        queue.enqueue(job("first", 5, 5, &order, &dropped));
        queue.enqueue(job("low", 9, 5, &order, &dropped));
        queue.enqueue(job("high", 1, 5, &order, &dropped));
        queue.enqueue(job("low", 9, 5, &order, &dropped));
        assert_eq!(queue.in_flight(), 1);
        assert!(queue.has("low") && queue.has("first"));
        sleep(Duration::from_millis(40)).await;
        assert_eq!(entries(&order), ["first", "high", "low"]);
        assert_eq!(entries(&dropped), ["low:Replaced"]);
        assert_eq!(queue.completed(), 3);
        assert!((queue.latency_ms() - 5.0).abs() < 1e-6);
    }

    #[tokio::test(start_paused = true)]
    async fn drops_expired_jobs_and_opens_the_circuit_after_repeated_failures() {
        let queue = JobQueue::new(1, 0);
        let (order, dropped) = (log(), log());
        queue.enqueue(job("slow", 1, 30, &order, &dropped));
        let late = Job::new("late", 1, later(5), |_| async { Ok(0) });
        let sink = dropped.clone();
        queue.enqueue(late.on_drop(move |reason| sink.lock().unwrap().push(format!("{reason:?}"))));
        sleep(Duration::from_millis(50)).await;
        assert_eq!(entries(&dropped), ["Expired"]);

        for i in 0..4 {
            queue.enqueue(failing(&format!("bad{i}"), LlmErrorKind::Network));
            sleep(Duration::from_millis(5)).await;
        }
        assert!(queue.circuit_open());
        let sink = dropped.clone();
        queue.enqueue(
            Job::new("after", 1, later(5000), |_| async { Ok(0) })
                .on_drop(move |reason| sink.lock().unwrap().push(format!("{reason:?}"))),
        );
        assert_eq!(entries(&dropped), ["Expired", "Circuit"]);
        assert_eq!(queue.failed(), 4);

        // The breaker closes again after its cool-down.
        sleep(CIRCUIT_OPEN).await;
        assert!(!queue.circuit_open());
    }

    #[tokio::test(start_paused = true)]
    async fn opening_the_circuit_drops_everything_waiting() {
        let queue = JobQueue::new(1, 0);
        let (order, dropped) = (log(), log());
        for i in 0..3 {
            queue.enqueue(failing(&format!("bad{i}"), LlmErrorKind::Http));
            sleep(Duration::from_millis(1)).await;
        }
        queue.enqueue(failing("bad3", LlmErrorKind::Timeout));
        queue.enqueue(job("waiting", 3, 1, &order, &dropped));
        sleep(Duration::from_millis(5)).await;
        assert_eq!(entries(&dropped), ["waiting:Circuit"]);
        assert!(entries(&order).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn prompt_problems_and_cancellations_do_not_trip_the_breaker() {
        let queue = JobQueue::new(1, 0);
        for i in 0..6 {
            let kind = [LlmErrorKind::NoCall, LlmErrorKind::Truncated, LlmErrorKind::Aborted][i % 3];
            queue.enqueue(failing(&format!("j{i}"), kind));
            sleep(Duration::from_millis(1)).await;
        }
        assert!(!queue.circuit_open());
        assert_eq!(queue.failed(), 6);
    }

    #[tokio::test(start_paused = true)]
    async fn holds_to_the_requests_per_minute_window() {
        let queue = JobQueue::new(4, 2);
        let (order, dropped) = (log(), log());
        for key in ["a", "b", "c"] {
            queue.enqueue(Job { expires_at: later(120_000), ..job(key, 1, 1, &order, &dropped) });
        }
        sleep(Duration::from_secs(30)).await;
        queue.pump();
        assert_eq!(entries(&order), ["a", "b"]);
        sleep(Duration::from_secs(31)).await;
        queue.pump();
        sleep(Duration::from_millis(5)).await;
        assert_eq!(entries(&order), ["a", "b", "c"]);
    }

    #[tokio::test(start_paused = true)]
    async fn pauses_for_retry_after_up_to_a_minute() {
        let queue = JobQueue::new(1, 0);
        let (order, dropped) = (log(), log());
        queue.enqueue(Job::new("limited", 1, later(5000), |_| async {
            let mut error = LlmError::new(LlmErrorKind::Http, "429");
            error.retry_after = Some(Duration::from_secs(600));
            Err(error)
        }));
        sleep(Duration::from_millis(1)).await;
        queue.enqueue(Job { expires_at: later(120_000), ..job("next", 1, 1, &order, &dropped) });
        sleep(Duration::from_secs(59)).await;
        queue.pump();
        assert!(entries(&order).is_empty());
        sleep(Duration::from_secs(2)).await;
        queue.pump();
        sleep(Duration::from_millis(5)).await;
        assert_eq!(entries(&order), ["next"]);
    }

    #[tokio::test(start_paused = true)]
    async fn cancels_queued_and_running_jobs_by_key_prefix() {
        let queue = JobQueue::new(1, 0);
        let (order, dropped) = (log(), log());
        let outcome = log();
        let seen = outcome.clone();
        queue.enqueue(Job::new("c:1", 1, later(5000), move |token| async move {
            token.cancelled().await;
            seen.lock().unwrap().push("aborted".into());
            Err(LlmError::new(LlmErrorKind::Aborted, "cancelled"))
        }));
        queue.enqueue(job("c:1:plan", 2, 1, &order, &dropped));
        queue.enqueue(job("c:2", 3, 1, &order, &dropped));
        sleep(Duration::from_millis(1)).await;
        queue.cancel("c:1");
        assert_eq!(entries(&dropped), ["c:1:plan:Cancelled"]);
        sleep(Duration::from_millis(5)).await;
        assert_eq!(entries(&outcome), ["aborted"]);
        assert_eq!(entries(&order), ["c:2"]);
        assert!(!queue.circuit_open());
    }

    #[tokio::test(start_paused = true)]
    async fn a_queued_job_whose_key_is_running_is_dropped_when_it_would_start() {
        let queue = JobQueue::new(2, 0);
        let (order, dropped) = (log(), log());
        queue.enqueue(job("k", 1, 10, &order, &dropped));
        queue.enqueue(job("k", 1, 10, &order, &dropped));
        sleep(Duration::from_millis(20)).await;
        assert_eq!(entries(&order), ["k"]);
        assert_eq!(entries(&dropped), ["k:Replaced"]);
    }

    #[tokio::test(start_paused = true)]
    async fn holds_back_while_paused() {
        let queue = JobQueue::new(1, 0);
        let (order, dropped) = (log(), log());
        queue.set_paused(true);
        queue.enqueue(job("a", 1, 1, &order, &dropped));
        sleep(Duration::from_millis(5)).await;
        assert_eq!(queue.waiting(), 1);
        queue.set_paused(false);
        sleep(Duration::from_millis(5)).await;
        assert_eq!(entries(&order), ["a"]);
    }

    #[tokio::test(start_paused = true)]
    async fn changes_concurrency_at_run_time() {
        let queue = JobQueue::new(2, 0);
        let (order, dropped) = (log(), log());
        for key in ["a", "b", "c", "d"] {
            queue.enqueue(job(key, 1, 10, &order, &dropped));
        }
        assert_eq!(queue.in_flight(), 2);
        queue.set_concurrency(1);
        sleep(Duration::from_millis(11)).await;
        assert_eq!(queue.in_flight(), 1);
        queue.set_concurrency(3);
        assert_eq!(queue.in_flight(), 2);
        sleep(Duration::from_millis(30)).await;
        assert_eq!(queue.completed(), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn a_panicking_job_frees_its_slot() {
        let queue = JobQueue::new(1, 0);
        let (order, dropped) = (log(), log());
        queue.enqueue(Job::new("boom", 1, later(5000), |_| async { panic!("bug in a job") }));
        sleep(Duration::from_millis(1)).await;
        assert_eq!(queue.in_flight(), 0);
        assert_eq!(queue.failed(), 1);
        queue.enqueue(job("next", 1, 1, &order, &dropped));
        sleep(Duration::from_millis(5)).await;
        assert_eq!(entries(&order), ["next"]);
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn spreads_the_daily_budget_through_the_day() {
        let day = 20_000 * DAY_SECS;
        let mut budget = DailyBudget::new(4800, 0);
        // At midnight only the burst (half an hour's worth: 100) is available.
        for _ in 0..100 {
            assert!(budget.allows(at(day)));
            budget.record_request(at(day));
        }
        assert!(!budget.allows(at(day)));
        // By 06:00 a quarter of the day's requests plus the burst.
        assert!(budget.allows(at(day + 6 * 3600)));
        assert_eq!(DailyBudget::budget(4800, 0.25), 1300.0);
        assert_eq!(DailyBudget::budget(4800, 0.999), 4800.0);
        assert_eq!(DailyBudget::budget(10, 0.0), 1.0);
        // A new day starts from zero.
        let usage = budget.usage(at(day + DAY_SECS));
        assert_eq!(usage, DailyUsage { day: 20_001, ..DailyUsage::default() });
    }

    #[test]
    fn caps_tokens_and_carries_usage_over_a_restart() {
        let noon = 20_000 * DAY_SECS + DAY_SECS / 2;
        let mut budget = DailyBudget::new(0, 1_000_000);
        budget.record_tokens(at(noon), 520_000);
        assert!(budget.allows(at(noon)));
        budget.record_tokens(at(noon), 10_000);
        assert!(!budget.allows(at(noon)));
        let stored = serde_json::to_string(&budget.usage(at(noon))).unwrap();
        assert!(stored.contains("\"tokens\":530000"), "{stored}");

        let mut restarted = DailyBudget::new(0, 1_000_000);
        restarted.restore(serde_json::from_str(&stored).unwrap());
        assert!(!restarted.allows(at(noon)));
        assert!(restarted.allows(at(noon + DAY_SECS)));
    }

    #[tokio::test(start_paused = true)]
    async fn the_queue_waits_for_daily_budget() {
        let wall = Arc::new(AtomicU64::new(20_000 * DAY_SECS));
        let clock = wall.clone();
        let queue = JobQueue::with_clock(4, 0, Arc::new(move || at(clock.load(Ordering::Relaxed))));
        queue.set_daily_caps(96, 0); // burst of 2 at midnight, then one every 15 minutes
        let (order, dropped) = (log(), log());
        for key in ["a", "b", "c"] {
            queue.enqueue(Job { expires_at: later(3_600_000), ..job(key, 1, 1, &order, &dropped) });
        }
        sleep(Duration::from_millis(5)).await;
        assert_eq!(entries(&order), ["a", "b"]);
        assert_eq!(queue.usage().requests, 2);
        wall.fetch_add(15 * 60, Ordering::Relaxed);
        queue.pump();
        sleep(Duration::from_millis(5)).await;
        assert_eq!(entries(&order), ["a", "b", "c"]);
        // The daily cap also bounds the capacity the governor sees.
        assert!((queue.capacity() - 96.0 / 86_400.0).abs() < 1e-12);
    }

    #[tokio::test(start_paused = true)]
    async fn counts_tokens_reported_by_jobs() {
        let queue = JobQueue::new(1, 0);
        queue.enqueue(Job::new("ok", 1, later(5000), |_| async { Ok(120) }));
        sleep(Duration::from_millis(1)).await;
        queue.enqueue(Job::new("bad", 1, later(5000), |_| async {
            let mut error = LlmError::new(LlmErrorKind::NoCall, "prose");
            error.tokens_used = 30;
            Err(error)
        }));
        sleep(Duration::from_millis(1)).await;
        let stats = queue.stats();
        assert_eq!(stats.today.tokens, 150);
        assert_eq!(stats.today.requests, 2);
        assert_eq!(stats.today.failures, 1);
    }
}
