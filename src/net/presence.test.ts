import { describe, expect, it } from 'vitest';
import { Heartbeat } from './presence';

function harness(watched: { now: boolean }) {
  let tick = (): void => {};
  let stopped = false;
  let reports = 0;
  let period = 0;
  const balances: number[] = [];
  const heartbeat = new Heartbeat({
    seconds: 30,
    watched: () => watched.now,
    report: () => Promise.resolve(++reports * 2),
    balance: (balance) => balances.push(balance),
    every: (run, ms) => {
      tick = run;
      period = ms;
      return () => (stopped = true);
    },
  });
  return { heartbeat, beat: () => tick(), reports: () => reports, balances, period: () => period, stopped: () => stopped };
}

describe('the heartbeat that earns credits', () => {
  it('reports at once and then every beat, passing on the balance', async () => {
    const h = harness({ now: true });
    h.heartbeat.start();
    expect(h.period()).toBe(30_000);
    expect(h.reports()).toBe(1);
    h.beat();
    h.beat();
    await Promise.resolve();
    expect(h.reports()).toBe(3);
    expect(h.balances).toEqual([2, 4, 6]);
  });

  it('says nothing while the tab is hidden or out of focus', () => {
    const watched = { now: false };
    const h = harness(watched);
    h.heartbeat.start();
    h.beat();
    expect(h.reports()).toBe(0);
    watched.now = true;
    h.beat();
    expect(h.reports()).toBe(1);
    watched.now = false;
    h.beat();
    expect(h.reports()).toBe(1);
  });

  it('starts once and stops when ended', () => {
    const h = harness({ now: true });
    h.heartbeat.start();
    h.heartbeat.start();
    expect(h.reports()).toBe(1);
    h.heartbeat.end();
    expect(h.stopped()).toBe(true);
  });

  it('carries on after a report fails', async () => {
    let calls = 0;
    let tick = (): void => {};
    const balances: number[] = [];
    const heartbeat = new Heartbeat({
      seconds: 30,
      watched: () => true,
      report: () => (++calls === 1 ? Promise.reject(new Error('offline')) : Promise.resolve(7)),
      balance: (balance) => balances.push(balance),
      every: (run) => {
        tick = run;
        return () => {};
      },
    });
    heartbeat.start();
    tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(balances).toEqual([7]);
  });
});
