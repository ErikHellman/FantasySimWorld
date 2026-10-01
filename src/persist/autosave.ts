import type { SimEvent } from '../sim/events';
import type { Sim } from '../sim/sim';
import { writeSave } from './db';

const INTERVAL_MS = 30_000;
const SAVE_POINT_DELAY_MS = 1_500;

/**
 * Saves on a timer, shortly after notable events (a death, a new day), and
 * when the tab is hidden. Only the tab holding the lock saves, so two tabs
 * cannot overwrite each other.
 */
export class Autosaver {
  private writing = false;
  private pending: number | null = null;
  private owner = false;
  lastSavedAt = 0;
  onSaved: (() => void) | null = null;

  constructor(private readonly sim: Sim) {}

  /** Resolves true if this tab may save; false if another tab already holds the world. */
  async start(): Promise<boolean> {
    this.owner = await acquireTabLock();
    if (!this.owner) return false;
    void navigator.storage?.persist?.();
    window.setInterval(() => void this.save(), INTERVAL_MS);
    document.addEventListener('visibilitychange', () => {
      if (document.hidden) void this.save();
    });
    return true;
  }

  handle(events: readonly SimEvent[]): void {
    if (this.pending !== null || !events.some((event) => event.type === 'save_point')) return;
    this.pending = window.setTimeout(() => {
      this.pending = null;
      void this.save();
    }, SAVE_POINT_DELAY_MS);
  }

  async save(): Promise<void> {
    if (!this.owner || this.writing) return;
    this.writing = true;
    try {
      await writeSave(this.sim.state);
      this.lastSavedAt = Date.now();
      this.onSaved?.();
    } catch (error) {
      console.warn('Autosave failed', error);
    } finally {
      this.writing = false;
    }
  }
}

function acquireTabLock(): Promise<boolean> {
  if (!navigator.locks) return Promise.resolve(true);
  return new Promise((resolve) => {
    void navigator.locks.request('fantasy-sim-world-save', { ifAvailable: true }, (lock) => {
      resolve(lock !== null);
      // Hold the lock for the life of the tab.
      return lock ? new Promise<void>(() => {}) : undefined;
    });
  });
}
