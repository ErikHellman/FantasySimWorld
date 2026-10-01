import { TICK_SECONDS } from '../core/time';
import type { SimEvent } from '../sim/events';
import type { Sim } from '../sim/sim';

export interface LoopHooks {
  /** after every sim tick */
  onTick(events: SimEvent[]): void;
  /** once per rendered frame; `alpha` is the fraction of a tick that has elapsed since the last one */
  onFrame(realDt: number, simDt: number, alpha: number): void;
}

const MAX_FRAME_SECONDS = 0.1;
const MAX_TICKS_PER_FRAME = 12;

/** Fixed-step simulation at 20 Hz, rendered as often as the display allows. */
export class GameLoop {
  /** 0 = paused */
  speed = 1;
  private accumulator = 0;
  private last = performance.now();

  constructor(private readonly sim: Sim, private readonly hooks: LoopHooks) {
    document.addEventListener('visibilitychange', () => {
      // Nothing is simulated while the tab is hidden; on return, do not try to catch up.
      this.last = performance.now();
      this.accumulator = 0;
    });
  }

  frame(): void {
    const now = performance.now();
    const realDt = Math.min(MAX_FRAME_SECONDS, (now - this.last) / 1000);
    this.last = now;
    const speed = document.hidden ? 0 : this.speed;
    this.accumulator += realDt * speed;
    let ticks = 0;
    while (this.accumulator >= TICK_SECONDS && ticks < MAX_TICKS_PER_FRAME) {
      this.accumulator -= TICK_SECONDS;
      this.hooks.onTick(this.sim.step());
      ticks++;
    }
    if (this.accumulator > TICK_SECONDS) this.accumulator = TICK_SECONDS; // fell behind: drop the backlog
    this.hooks.onFrame(realDt, realDt * speed, speed === 0 ? 1 : this.accumulator / TICK_SECONDS);
  }
}
