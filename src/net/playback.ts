import type { TickFrame } from '../generated/api';

const TICKS_PER_SECOND = 20;
/** ticks of cushion kept between the newest frame and what is shown, at 1x */
const CUSHION = 2;
/** more frames than this waiting (at 1x) and all but the cushion are applied at once */
const BACKLOG = 4;
const NUDGE = 0.05;
const NUDGE_SECONDS = 1;

export interface PlaybackHooks {
  /**
   * One whole tick, in order. `fast` is set for the frames of a backlog that
   * are applied at once: speech and the chronicle still matter, effects do not.
   */
  onTick(frame: TickFrame, fast: boolean): void;
}

/**
 * The jitter buffer. Frames arrive in bursts (one server wake carries up to a
 * dozen); playback runs smoothly a couple of ticks behind the newest frame,
 * applies each frame when its tick comes up, and gives the renderer the
 * fraction of a tick between the last two applied (`alpha`).
 *
 * The cushion and backlog scale with the speed, because at 4x one 50 ms
 * server wake carries four ticks.
 */
export class Playback {
  private readonly frames: TickFrame[] = [];
  /** the fractional tick being shown */
  private playTick = 0;
  /** the last whole tick applied */
  applied = 0;
  private newest = 0;
  /** world speed as the server reports it (0 = paused) */
  speed = 1;
  private rate = 1;
  private nudgeLeft = 0;
  /** waiting for the first cushion after a snapshot */
  private priming = true;

  constructor(private readonly hooks: PlaybackHooks) {}

  /** Starts over from a snapshot at `tick`. */
  reset(tick: number): void {
    this.frames.length = 0;
    this.playTick = tick;
    this.applied = tick;
    this.newest = tick;
    this.rate = 1;
    this.nudgeLeft = 0;
    this.priming = true;
  }

  /** Frames waiting to be applied. */
  get buffered(): number {
    return this.frames.length;
  }

  push(frame: TickFrame): void {
    if (frame.t <= this.newest) return; // already have it (a replay overlapping what arrived)
    this.frames.push(frame);
    this.newest = frame.t;
  }

  private applyNext(fast: boolean): void {
    const frame = this.frames.shift();
    if (!frame) return;
    this.applied = frame.t;
    this.hooks.onTick(frame, fast);
  }

  /**
   * Advances by `dt` real seconds and applies the ticks that come up.
   * Returns `alpha` (0..1 between the last two applied ticks) and `simDt`,
   * the simulated seconds that passed, for animations and effects.
   */
  advance(dt: number): { alpha: number; simDt: number } {
    const scale = Math.max(1, this.speed);
    const cushion = CUSHION * scale;
    // A paused world sends nothing; play out what is already here at 1x.
    const speed = this.speed > 0 ? this.speed : this.frames.length > 0 ? 1 : 0;

    if (this.frames.length > BACKLOG * scale) {
      // Fell behind (a stall, a hidden tab): catch up at once, keeping the cushion.
      while (this.frames.length > cushion) this.applyNext(true);
      this.playTick = this.applied;
      this.priming = false;
    }

    const depth = this.newest - this.playTick;
    if (this.priming) {
      if (depth < cushion) return { alpha: 1, simDt: 0 };
      this.priming = false;
    }
    if (this.nudgeLeft <= 0) {
      // Drifting from the cushion: play a little faster or slower for a second.
      this.rate = depth > cushion + scale ? 1 + NUDGE : depth < cushion - scale ? 1 - NUDGE : 1;
      this.nudgeLeft = this.rate === 1 ? 0 : NUDGE_SECONDS;
    } else {
      this.nudgeLeft -= dt;
    }

    const step = dt * TICKS_PER_SECOND * speed * this.rate;
    this.playTick += step;
    while (this.frames.length > 0 && (this.frames[0] as TickFrame).t <= Math.floor(this.playTick)) this.applyNext(false);
    // Out of frames: hold on the newest state rather than run ahead of it.
    if (this.playTick > this.applied + 1) this.playTick = this.applied + 1;
    const alpha = Math.max(0, Math.min(1, this.playTick - this.applied));
    return { alpha, simDt: step / TICKS_PER_SECOND };
  }
}
