import { describe, expect, it } from 'vitest';
import type { GuestAnswer } from '../generated/api';
import { HttpError } from './api';
import { GuestQueue, type GuestWait } from './guestQueue';

const queued = (position: number, ticket = 't1'): GuestAnswer => ({ kind: 'queued', ticket, position, seats: 25, retryAfterSeconds: 60 });

/** A line whose answers are scripted, with a timer the test fires by hand. */
function harness(answers: (GuestAnswer | Error)[]) {
  const asked: (string | null)[] = [];
  const waits: GuestWait[] = [];
  const problems: string[] = [];
  let admitted = 0;
  let pending: { run: () => void; ms: number } | null = null;
  const queue = new GuestQueue({
    ask: (ticket) => {
      asked.push(ticket);
      const next = answers.shift();
      if (!next) return new Promise(() => {});
      return next instanceof Error ? Promise.reject(next) : Promise.resolve(next);
    },
    admitted: () => admitted++,
    waiting: (wait) => waits.push(wait),
    failed: (message) => problems.push(message),
    after: (run, ms) => {
      pending = { run, ms };
      return () => (pending = null);
    },
    now: () => 1000,
  });
  const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
  return {
    queue, asked, waits, problems, settle,
    admitted: () => admitted,
    pending: () => pending,
    fire: async () => {
      const run = pending?.run;
      pending = null;
      run?.();
      await settle();
    },
  };
}

describe('the line for a guest seat', () => {
  it('goes straight in when a seat is free', async () => {
    const h = harness([{ kind: 'admitted', secondsLeft: 900 }]);
    h.queue.start();
    await h.settle();
    expect(h.asked).toEqual([null]);
    expect(h.admitted()).toBe(1);
    expect(h.pending()).toBeNull();
  });

  it('asks again with its ticket once a minute until a seat comes free', async () => {
    const h = harness([queued(3), queued(1), { kind: 'admitted', secondsLeft: 900 }]);
    h.queue.start();
    await h.settle();
    expect(h.waits).toEqual([{ position: 3, seats: 25, nextAt: 61_000 }]);
    expect(h.pending()?.ms).toBe(60_000);
    await h.fire();
    expect(h.waits.at(-1)?.position).toBe(1);
    await h.fire();
    expect(h.asked).toEqual([null, 't1', 't1']);
    expect(h.admitted()).toBe(1);
  });

  it('waits as long as the server says when it asked too soon', async () => {
    const h = harness([queued(2), new HttpError(429, 'ask again in a minute', 42), queued(1)]);
    h.queue.start();
    await h.settle();
    await h.fire();
    expect(h.pending()?.ms).toBe(42_000);
    expect(h.waits.at(-1)).toEqual({ position: 2, seats: 25, nextAt: 43_000 });
    await h.fire();
    expect(h.waits.at(-1)?.position).toBe(1);
  });

  it('gives up when guests are off, and keeps trying when the server is away', async () => {
    const off = harness([new HttpError(404, 'guest viewing is off')]);
    off.queue.start();
    await off.settle();
    expect(off.problems).toEqual(['Guests cannot watch on this server.']);
    expect(off.pending()).toBeNull();

    const away = harness([new TypeError('Failed to fetch'), queued(1)]);
    away.queue.start();
    await away.settle();
    expect(away.problems).toEqual(['The server is not answering. Trying again in a minute.']);
    expect(away.pending()?.ms).toBe(60_000);
    await away.fire();
    expect(away.waits.at(-1)?.position).toBe(1);
  });

  it('keeps its ticket for a reload, and starts from one', async () => {
    const kept: (string | null)[] = [];
    const queue = new GuestQueue({
      ask: (ticket) => Promise.resolve(ticket === 'old' ? queued(4, 'old') : { kind: 'admitted', secondsLeft: 900 }),
      ticket: 'old',
      keep: (ticket) => kept.push(ticket),
      admitted: () => {},
      waiting: () => {},
      failed: () => {},
      after: () => () => {},
    });
    queue.start();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(kept).toEqual(['old']);
    queue.cancel();
    expect(kept).toEqual(['old', null]);
  });

  it('stops asking once cancelled', async () => {
    const h = harness([queued(2), queued(1)]);
    h.queue.start();
    await h.settle();
    h.queue.cancel();
    expect(h.pending()).toBeNull();
    expect(h.asked).toEqual([null]);
  });
});
