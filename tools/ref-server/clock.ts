// The tick schedule, the same policy as the Rust server's `TickClock`: wake on
// 50 ms deadlines, run the ticks owed (speed × elapsed / 50 ms), at most 12 per
// wake, and when more than 200 are owed (a host pause, a debugger, a laptop
// lid) start again from now instead of fast-forwarding.
export const TICK_MS = 50;
export const MAX_TICKS_PER_WAKE = 12;
export const SKIP_AFTER = 200;

export class TickClock {
  private epoch: number;
  private done = 0;
  private speed = 1;
  /** times the clock fell too far behind and skipped ahead */
  skips = 0;

  constructor(now: number) {
    this.epoch = now;
  }

  private rebase(now: number, speed: number): void {
    this.epoch = now;
    this.done = 0;
    this.speed = speed;
  }

  /** How many ticks to run now. */
  due(now: number, speed: number): number {
    if (speed !== this.speed) this.rebase(now, speed);
    if (speed <= 0) return 0;
    const owed = Math.floor(((now - this.epoch) * speed) / TICK_MS) - this.done;
    if (owed > SKIP_AFTER) {
      this.rebase(now, speed);
      this.skips++;
      return 0;
    }
    const run = Math.max(0, Math.min(MAX_TICKS_PER_WAKE, owed));
    this.done += run;
    return run;
  }

  /** The next 50 ms deadline after `now`, whatever the speed. */
  nextWake(now: number): number {
    const elapsed = Math.max(0, now - this.epoch);
    return this.epoch + (Math.floor(elapsed / TICK_MS) + 1) * TICK_MS;
  }
}
