/**
 * Keeps demand on the language model below what it can serve. A token bucket
 * refills at 70% of the endpoint's measured capacity; urgent requests always
 * go, ordinary ones need a token, and idle musings need a comfortable reserve.
 * A fast cloud endpoint therefore gets richer behaviour and a slow local model
 * sparser behaviour, with no setting to tune.
 */
export class Governor {
  private tokens = 3;
  private max = 3;
  private lastRefill = 0;
  private lastTune = 0;

  /** `initial` is the starting budget; tests pass a large one so nothing is held back. */
  reset(initial = 3): void {
    this.tokens = initial;
    this.max = initial;
    this.lastRefill = 0;
    this.lastTune = 0;
  }

  /** `capacity` is requests per second the endpoint can absorb. */
  refill(capacity: number, now: number): void {
    const dt = this.lastRefill === 0 ? 0 : Math.min(5, (now - this.lastRefill) / 1000);
    this.lastRefill = now;
    this.max = Math.max(this.max > 40 ? this.max : 0, Math.min(40, Math.max(3, capacity * 20)));
    this.tokens = Math.min(this.max, this.tokens + dt * capacity * 0.7);
  }

  spend(): void {
    this.tokens = Math.max(0, this.tokens - 1);
  }

  /** tier 0: life or death; tier 1: someone is waiting on it; tier 2: nice to have. */
  allow(tier: 0 | 1 | 2): boolean {
    if (tier === 0) return true;
    if (tier === 1) return this.tokens >= 1;
    return this.tokens >= Math.max(2, this.max * 0.45);
  }

  private get fill(): number {
    return this.max > 0 ? this.tokens / this.max : 0;
  }

  /** How eagerly villagers should strike up conversations (each one costs requests). */
  chatter(): number {
    return 0.3 + 0.45 * this.fill;
  }

  /** Multiplier on how long a villager waits between unprompted decisions. */
  patience(): number {
    return 1 + (1 - this.fill) * 2;
  }

  shouldRetune(now: number): boolean {
    if (now - this.lastTune < 5000) return false;
    this.lastTune = now;
    return true;
  }
}
