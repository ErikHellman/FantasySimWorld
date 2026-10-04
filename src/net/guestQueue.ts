import type { GuestAnswer } from '../generated/api';
import { HttpError } from './api';

/** How long to wait when the server cannot be reached, or does not say. */
const DEFAULT_WAIT_SECONDS = 60;

/** Where a guest stands in line, and when the next ask goes out (ms since the epoch). */
export interface GuestWait {
  position: number;
  seats: number;
  nextAt: number;
}

export interface GuestQueueOptions {
  /** `POST /auth/guest` with the ticket from the last answer */
  ask: (ticket: string | null) => Promise<GuestAnswer>;
  /** a ticket from before a reload, so the page keeps its place */
  ticket?: string | null;
  /** the ticket to keep for a reload; null once it is no longer needed */
  keep?: (ticket: string | null) => void;
  /** a seat: the guest cookie is set, and the world can be opened */
  admitted: () => void;
  waiting: (wait: GuestWait) => void;
  failed: (message: string) => void;
  /** runs `then` after `ms` and returns how to cancel it; `setTimeout` unless a test passes its own */
  after?: (then: () => void, ms: number) => () => void;
  now?: () => number;
}

/**
 * Asks the server for a guest seat and, while every seat is taken, asks again as often as the
 * server allows (once a minute), keeping the ticket that holds this page's place in line.
 */
export class GuestQueue {
  private ticket: string | null;
  private last: GuestWait | null = null;
  private cancelTimer: (() => void) | null = null;
  private stopped = false;

  constructor(private readonly options: GuestQueueOptions) {
    this.ticket = options.ticket ?? null;
  }

  start(): void {
    this.stopped = false;
    void this.ask();
  }

  cancel(): void {
    this.options.keep?.(null);
    this.stopped = true;
    this.cancelTimer?.();
    this.cancelTimer = null;
  }

  private async ask(): Promise<void> {
    this.cancelTimer = null;
    let answer: GuestAnswer;
    try {
      answer = await this.options.ask(this.ticket);
    } catch (error) {
      if (this.stopped) return;
      if (error instanceof HttpError && error.status === 404) {
        this.options.failed('Guests cannot watch on this server.');
      } else if (error instanceof HttpError && error.status === 429) {
        // Asked too soon (another timer, a clock that jumped): the place is kept, so just wait.
        this.wait(error.retryAfterSeconds ?? DEFAULT_WAIT_SECONDS, this.last);
      } else if (error instanceof HttpError) {
        this.options.failed(`${error.message}. Trying again in a minute.`);
        this.wait(DEFAULT_WAIT_SECONDS, this.last);
      } else {
        this.options.failed('The server is not answering. Trying again in a minute.');
        this.wait(DEFAULT_WAIT_SECONDS, this.last);
      }
      return;
    }
    if (this.stopped) return;
    if (answer.kind === 'admitted') {
      this.options.keep?.(null);
      this.options.admitted();
      return;
    }
    this.ticket = answer.ticket;
    this.options.keep?.(answer.ticket);
    this.wait(answer.retryAfterSeconds, { position: answer.position, seats: answer.seats, nextAt: 0 });
  }

  private wait(seconds: number, place: GuestWait | null): void {
    const now = this.options.now ?? Date.now;
    const after = this.options.after ?? ((then, ms) => {
      const timer = setTimeout(then, ms);
      return () => clearTimeout(timer);
    });
    if (place) {
      this.last = { ...place, nextAt: now() + seconds * 1000 };
      this.options.waiting(this.last);
    }
    this.cancelTimer = after(() => void this.ask(), seconds * 1000);
  }
}
