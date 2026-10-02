//! Keeps demand on the language model below what it can serve (port of `src/brain/governor.ts`).
//!
//! A token bucket refills at 70% of the endpoint's measured capacity; urgent
//! requests always go, ordinary ones need a token, and idle musings need a
//! comfortable reserve. A fast cloud endpoint therefore gets richer behaviour
//! and a slow local model sparser behaviour, with no setting to tune. Pure:
//! the caller passes the time.

use std::time::Duration;

use tokio::time::Instant;

/// How much a request matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// life or death; always allowed
    Urgent,
    /// someone is waiting on it
    Awaited,
    /// nice to have
    Idle,
}

/// The longest gap a single refill credits.
const MAX_REFILL_GAP: f64 = 5.0;
/// How often the brain re-sends its tuning to the sim.
const RETUNE_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct Governor {
    tokens: f64,
    max: f64,
    last_refill: Option<Instant>,
    last_tune: Option<Instant>,
}

impl Default for Governor {
    fn default() -> Self {
        Self::new(3.0)
    }
}

impl Governor {
    /// `initial` is the starting budget; tests pass a large one so nothing is held back.
    pub fn new(initial: f64) -> Self {
        Self { tokens: initial, max: initial, last_refill: None, last_tune: None }
    }

    pub fn reset(&mut self, initial: f64) {
        *self = Self::new(initial);
    }

    pub fn tokens(&self) -> f64 {
        self.tokens
    }

    pub fn max(&self) -> f64 {
        self.max
    }

    /// `capacity` is requests per second the endpoint can absorb.
    pub fn refill(&mut self, capacity: f64, now: Instant) {
        let dt = self
            .last_refill
            .map_or(0.0, |last| (now.saturating_duration_since(last).as_secs_f64()).min(MAX_REFILL_GAP));
        self.last_refill = Some(now);
        let floor = if self.max > 40.0 { self.max } else { 0.0 };
        self.max = floor.max((capacity * 20.0).clamp(3.0, 40.0));
        self.tokens = self.max.min(self.tokens + dt * capacity * 0.7);
    }

    pub fn spend(&mut self) {
        self.tokens = (self.tokens - 1.0).max(0.0);
    }

    pub fn allow(&self, tier: Tier) -> bool {
        match tier {
            Tier::Urgent => true,
            Tier::Awaited => self.tokens >= 1.0,
            Tier::Idle => self.tokens >= (self.max * 0.45).max(2.0),
        }
    }

    fn fill(&self) -> f64 {
        if self.max > 0.0 { self.tokens / self.max } else { 0.0 }
    }

    /// How eagerly villagers should strike up conversations (each one costs requests).
    pub fn chatter(&self) -> f64 {
        0.3 + 0.45 * self.fill()
    }

    /// Multiplier on how long a villager waits between unprompted decisions.
    pub fn patience(&self) -> f64 {
        1.0 + (1.0 - self.fill()) * 2.0
    }

    pub fn should_retune(&mut self, now: Instant) -> bool {
        if self.last_tune.is_some_and(|last| now.saturating_duration_since(last) < RETUNE_EVERY) {
            return false;
        }
        self.last_tune = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(start: Instant, s: f64) -> Instant {
        start + Duration::from_secs_f64(s)
    }

    #[test]
    fn starts_with_a_small_budget_and_spends_it() {
        let mut governor = Governor::default();
        governor.spend();
        assert!(governor.allow(Tier::Idle), "idle needs two tokens");
        governor.spend();
        assert!(!governor.allow(Tier::Idle));
        assert!(governor.allow(Tier::Awaited));
        governor.spend();
        governor.spend();
        assert_eq!(governor.tokens(), 0.0);
        assert!(!governor.allow(Tier::Awaited));
        assert!(governor.allow(Tier::Urgent));
        assert_eq!(governor.chatter(), 0.3);
        assert_eq!(governor.patience(), 3.0);
    }

    #[test]
    fn refills_at_seventy_percent_of_capacity_with_a_five_second_cap() {
        let start = Instant::now();
        let mut governor = Governor::new(0.0);
        governor.refill(1.0, start);
        assert_eq!(governor.tokens(), 0.0, "the first refill only sets the clock");
        assert_eq!(governor.max(), 20.0);
        governor.refill(1.0, secs(start, 2.0));
        assert!((governor.tokens() - 1.4).abs() < 1e-9);
        governor.refill(1.0, secs(start, 60.0));
        assert!((governor.tokens() - 4.9).abs() < 1e-9, "a long gap counts as 5 s");
        assert!(governor.allow(Tier::Awaited));
        assert!(!governor.allow(Tier::Idle), "idle needs 45% of 20");
    }

    #[test]
    fn sizes_the_bucket_from_capacity() {
        let start = Instant::now();
        let mut governor = Governor::new(3.0);
        governor.refill(0.05, start);
        assert_eq!(governor.max(), 3.0);
        governor.refill(10.0, secs(start, 1.0));
        assert_eq!(governor.max(), 40.0);
        governor.refill(0.05, secs(start, 2.0));
        assert_eq!(governor.max(), 3.0, "the bucket shrinks back when the endpoint slows");
        let mut generous = Governor::new(1000.0);
        generous.refill(0.05, start);
        assert_eq!(generous.max(), 1000.0, "a test budget above 40 is kept");
        assert_eq!(generous.tokens(), 1000.0);
    }

    #[test]
    fn retunes_at_most_every_five_seconds() {
        let start = Instant::now();
        let mut governor = Governor::default();
        assert!(governor.should_retune(start));
        assert!(!governor.should_retune(secs(start, 4.9)));
        assert!(governor.should_retune(secs(start, 5.0)));
        governor.reset(3.0);
        assert!(governor.should_retune(secs(start, 5.1)));
    }

    #[test]
    fn chatter_and_patience_follow_the_fill() {
        let start = Instant::now();
        let mut governor = Governor::new(0.0);
        governor.refill(1.0, start);
        governor.refill(1.0, secs(start, 5.0));
        governor.refill(1.0, secs(start, 10.0));
        governor.refill(1.0, secs(start, 15.0));
        governor.refill(1.0, secs(start, 20.0));
        assert_eq!(governor.tokens(), 14.0);
        assert!((governor.chatter() - (0.3 + 0.45 * 0.7)).abs() < 1e-9);
        assert!((governor.patience() - 1.6).abs() < 1e-9);
    }
}
