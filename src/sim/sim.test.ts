import { describe, expect, it } from 'vitest';
import { TICKS_PER_DAY } from '../core/time';
import { isWalkable, nearestWalkable } from '../world/navgrid';
import { createState } from './create';
import { Sim } from './sim';
import type { SimEvent } from './events';
import type { SimState } from './state';
import { living } from './util';

function run(state: SimState, ticks: number, onEvent?: (event: SimEvent) => void): Sim {
  const sim = new Sim(state);
  for (let i = 0; i < ticks; i++) {
    const events = sim.step();
    if (onEvent) events.forEach(onEvent);
  }
  return sim;
}

describe('simulation', () => {
  it('starts with the authored cast, all housed', () => {
    const state = createState(1);
    expect(state.order.length).toBe(12);
    for (const c of living(state)) {
      expect(c.homeId).toMatch(/^home_/);
      expect(isWalkable(c.pos.x, c.pos.z)).toBe(true);
    }
    expect(JSON.parse(JSON.stringify(state))).toEqual(state);
  });

  it('is deterministic, and survives a save and load in the middle', () => {
    const straight = run(createState(42), 4000).state;
    const first = run(createState(42), 2000).state;
    const reloaded = JSON.parse(JSON.stringify(first)) as SimState;
    const resumed = run(reloaded, 2000).state;
    expect(resumed).toEqual(straight);
  });

  it('gives different worlds for different seeds', () => {
    const a = run(createState(1), 3000).state;
    const b = run(createState(2), 3000).state;
    expect(a.rng).not.toBe(b.rng);
  });

  it('keeps everyone fed, rested and on walkable ground for three days', () => {
    const sim = new Sim(createState(5));
    let lowestHunger = 100;
    for (let i = 0; i < TICKS_PER_DAY * 3; i++) {
      sim.step();
      if (i % 200 !== 0) continue;
      for (const c of living(sim.state)) {
        lowestHunger = Math.min(lowestHunger, c.needs.hunger);
        expect(c.needs.energy).toBeGreaterThanOrEqual(0);
        expect(Number.isFinite(c.pos.x) && Number.isFinite(c.pos.z)).toBe(true);
        // Seats and work spots sit just off the nav grid, so "near walkable ground" is the invariant.
        expect(nearestWalkable(c.pos, 3), `${c.profile.name} at ${c.pos.x},${c.pos.z}`).not.toBeNull();
      }
    }
    expect(lowestHunger).toBeGreaterThan(2);
    for (const c of living(sim.state)) expect(c.memories.length).toBeLessThanOrEqual(140);
  });

  it('has people talking, sleeping and fighting off the dead', () => {
    const counts: Record<string, number> = {};
    const sim = run(createState(9), TICKS_PER_DAY * 3, (event) => {
      counts[event.type] = (counts[event.type] ?? 0) + 1;
    });
    expect(counts.conversation_ended ?? 0).toBeGreaterThan(20);
    expect(counts.woke ?? 0).toBeGreaterThan(10);
    expect(counts.woke ?? 0).toBeLessThan(200);
    expect(counts.monster_spawn ?? 0).toBeGreaterThan(0);
    expect(sim.state.chronicle.length).toBeGreaterThan(10);
  });
});

describe('long run', () => {
  it('keeps the village populated and killings rare over 60 days', () => {
    for (const seed of [11, 23]) {
      const sim = new Sim(createState(seed));
      let lowest = 12;
      for (let i = 0; i < TICKS_PER_DAY * 60; i++) {
        sim.step();
        if (i % 2000 === 0) lowest = Math.min(lowest, sim.state.order.length);
      }
      const dead = Object.values(sim.state.dead);
      const murders = dead.filter((record) => record.killerId !== null).length;
      expect(lowest, `seed ${seed} lowest population`).toBeGreaterThanOrEqual(7);
      expect(sim.state.order.length, `seed ${seed} final population`).toBeGreaterThanOrEqual(9);
      expect(dead.length, `seed ${seed} deaths`).toBeLessThan(30);
      expect(murders, `seed ${seed} murders`).toBeLessThanOrEqual(4);
      for (const c of living(sim.state)) expect(c.memories.length).toBeLessThanOrEqual(140);
    }
  }, 120_000);
});
