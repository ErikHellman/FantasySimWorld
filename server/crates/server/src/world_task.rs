//! The world task: the only place that advances the world.
//!
//! Phase 0 ticks an empty world, so the stream, health check and the hosting
//! platform's proxy can be proven before the sim exists. The schedule is the
//! final one: sleep until the next 50 ms deadline, run the ticks owed (at most
//! 12 per wake), and when more than 10 s are owed (a host pause, a deploy)
//! skip ahead instead of fast-forwarding, like the browser's hidden-tab rule.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, watch};
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

use crate::api::wire::TickFrame;

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

/// The frames of one wake, already encoded, in tick order.
#[derive(Debug)]
pub struct FrameBatch {
    pub frames: Vec<(u64, Arc<str>)>,
}

pub struct WorldHandles {
    pub clock: watch::Receiver<WorldClock>,
    pub frames: broadcast::Sender<Arc<FrameBatch>>,
}

/// Starts the world task. It stops when `shutdown` is cancelled.
pub fn spawn(speed: u8, shutdown: CancellationToken) -> (WorldHandles, tokio::task::JoinHandle<()>) {
    let now = Instant::now();
    let (clock_tx, clock_rx) = watch::channel(WorldClock { tick: 0, speed, last_tick_at: now });
    // A slow viewer more than ~5 s behind is dropped and reconnects.
    let (frames_tx, _) = broadcast::channel(256);
    let handles = WorldHandles { clock: clock_rx, frames: frames_tx.clone() };
    let task = tokio::spawn(run(TickClock::new(now, speed), clock_tx, frames_tx, shutdown));
    (handles, task)
}

async fn run(
    mut clock: TickClock,
    clock_tx: watch::Sender<WorldClock>,
    frames_tx: broadcast::Sender<Arc<FrameBatch>>,
    shutdown: CancellationToken,
) {
    let mut tick = 0u64;
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            () = sleep_until(clock.deadline()) => {}
        }
        let now = Instant::now();
        let due = clock.due(now);
        if due.skipped > 0 {
            tracing::warn!(
                event = "clock_skip",
                skipped_ticks = due.skipped,
                tick,
                "world fell behind; skipping ahead"
            );
        }
        if due.ticks == 0 {
            continue;
        }
        let mut frames = Vec::with_capacity(due.ticks as usize);
        for _ in 0..due.ticks {
            tick += 1;
            let frame = TickFrame { t: tick, ..Default::default() };
            let json = serde_json::to_string(&frame).expect("tick frames always serialise");
            frames.push((tick, Arc::from(json)));
        }
        clock_tx.send_replace(WorldClock { tick, speed: clock.speed(), last_tick_at: now });
        // No receivers is fine: nobody is watching.
        let _ = frames_tx.send(Arc::new(FrameBatch { frames }));
    }
    tracing::info!(tick, "world task stopped");
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

    #[tokio::test(start_paused = true)]
    async fn world_task_ticks_at_20_hz() {
        let shutdown = CancellationToken::new();
        let (handles, task) = spawn(1, shutdown.clone());
        let mut frames = handles.frames.subscribe();
        // Just past the 20th deadline, so the world task has run it.
        tokio::time::sleep(ms(1025)).await;
        let mut ticks = Vec::new();
        while let Ok(batch) = frames.try_recv() {
            ticks.extend(batch.frames.iter().map(|(t, _)| *t));
        }
        assert_eq!(ticks, (1..=20).collect::<Vec<_>>());
        assert_eq!(handles.clock.borrow().tick, 20);
        shutdown.cancel();
        task.await.unwrap();
    }
}
