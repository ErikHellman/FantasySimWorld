import { describe, expect, it } from 'vitest';
import type { Entity, Snapshot, TickFrame } from '../src/generated/api';
import { Playback } from '../src/net/playback';
import { ViewWorld } from '../src/net/viewWorld';
import { createState } from '../src/sim/create';
import { Sim } from '../src/sim/sim';
import { DeltaTracker, entitiesOf } from '../tools/ref-server/encode';

const entity = (id: string, x: number): Entity => ({
  static: { id, kind: 'villager', name: id, look: { model: 'Knight', texture: null, hide: [], mainHand: 'sword', offHand: null }, maxHp: 100, hostile: false, voice: null },
  dyn: { p: [x * 100, 0], f: 0, a: ['idle', 0], hp: 100, v: true, mh: 'sword', oh: null, act: null, z: false },
});

const snapshot = (tick: number, entities: Entity[]): Snapshot => ({
  tick, speed: 1, entities, roster: [], chronicle: [], graves: [], alarm: false, voices: [],
  llm: { state: 'off', inFlight: 0, queued: 0, requests: 0, failures: 0, tokens: 0, latencyMs: 0, protocol: '', message: '' },
});

describe('view world', () => {
  it('decodes entities and applies deltas', () => {
    const world = new ViewWorld();
    world.reset(snapshot(10, [entity('c1', 1.5)]));
    expect(world.entities.get('c1')).toMatchObject({ x: 1.5, z: 0, mainHand: 'sword', anim: 'idle' });
    world.apply({ t: 11, d: [{ id: 'c1', p: [250, -100], f: 157, mh: null, act: 'smith' }], add: [entity('c2', 0)] });
    expect(world.tick).toBe(11);
    expect(world.entities.get('c1')).toMatchObject({ x: 2.5, z: -1, facing: 1.57, mainHand: null, act: 'smith', offHand: null });
    world.apply({ t: 12, del: ['c2'] });
    expect([...world.entities.keys()]).toEqual(['c1']);
  });

  it('keys graves by plot and character, a reused plot replacing its old grave', () => {
    const world = new ViewWorld();
    world.reset({ ...snapshot(1, []), graves: [{ plot: 0, characterId: 'c1', name: 'A', day: 1, cause: 'x' }] });
    const version = world.gravesVersion;
    world.apply({ t: 2, ev: [{ type: 'grave', grave: { plot: 1, characterId: 'c2', name: 'B', day: 2, cause: 'y' } }] });
    world.apply({ t: 3, ev: [{ type: 'grave', grave: { plot: 0, characterId: 'c3', name: 'C', day: 3, cause: 'z' } }] });
    expect([...world.graves.keys()].sort()).toEqual(['0:c3', '1:c2']);
    expect(world.gravesVersion).toBe(version + 2);
  });

  it('follows the reference server frame for frame', () => {
    const sim = new Sim(createState(99));
    const tracker = new DeltaTracker(sim.state);
    const world = new ViewWorld();
    world.reset(snapshot(sim.state.tick, tracker.entities()));
    for (let i = 0; i < 600; i++) world.apply(tracker.frame(sim.state, sim.step()));
    const expected = entitiesOf(sim.state);
    expect([...world.entities.keys()]).toEqual(expected.map((e) => e.static.id));
    for (const e of expected) {
      const view = world.entities.get(e.static.id)!;
      expect([Math.round(view.x * 100), Math.round(view.z * 100)]).toEqual(e.dyn.p);
      expect([view.anim, view.animSeq, view.mainHand, view.offHand]).toEqual([e.dyn.a[0], e.dyn.a[1], e.dyn.mh, e.dyn.oh]);
    }
  });
});

describe('playback', () => {
  function setup(speed = 1) {
    const applied: Array<{ t: number; fast: boolean }> = [];
    const playback = new Playback({ onTick: (frame, fast) => applied.push({ t: frame.t, fast }) });
    playback.speed = speed;
    playback.reset(100);
    return { playback, applied };
  }
  const frame = (t: number): TickFrame => ({ t });

  it('waits for a cushion of two ticks, then plays one tick per 50 ms', () => {
    const { playback, applied } = setup();
    playback.push(frame(101));
    expect(playback.advance(0.05).alpha).toBe(1);
    expect(applied).toEqual([]);
    playback.push(frame(102));
    playback.advance(0.05);
    expect(applied.map((a) => a.t)).toEqual([101]);
    playback.push(frame(103));
    playback.advance(0.025);
    playback.push(frame(104));
    const { alpha } = playback.advance(0.025);
    expect(applied.map((a) => a.t)).toEqual([101, 102]);
    expect(alpha).toBeGreaterThanOrEqual(0);
    expect(alpha).toBeLessThan(1);
  });

  it('holds alpha at 1 when it runs out of frames', () => {
    const { playback, applied } = setup();
    playback.push(frame(101));
    playback.push(frame(102));
    for (let i = 0; i < 10; i++) playback.advance(0.05);
    expect(applied.map((a) => a.t)).toEqual([101, 102]);
    expect(playback.advance(0.05).alpha).toBe(1);
    playback.push(frame(103));
    playback.advance(0.05);
    expect(applied.map((a) => a.t)).toEqual([101, 102, 103]);
  });

  it('applies a backlog at once, keeping the cushion, and marks it fast', () => {
    const { playback, applied } = setup();
    for (let t = 101; t <= 110; t++) playback.push(frame(t));
    playback.advance(0.001);
    expect(applied.filter((a) => a.fast).map((a) => a.t)).toEqual([101, 102, 103, 104, 105, 106, 107, 108]);
    expect(playback.buffered).toBe(2);
  });

  it('keeps up with four ticks per wake at 4x without calling it a backlog', () => {
    const { playback, applied } = setup(4);
    let t = 100;
    for (let wake = 0; wake < 40; wake++) {
      for (let i = 0; i < 4; i++) playback.push(frame(++t));
      for (let f = 0; f < 3; f++) playback.advance(0.05 / 3);
    }
    expect(applied.some((a) => a.fast)).toBe(false);
    expect(applied.length).toBeGreaterThan(140);
    expect(applied.map((a) => a.t)).toEqual(applied.map((_, i) => 101 + i));
  });

  it('ignores frames it already has', () => {
    const { playback } = setup();
    playback.push(frame(101));
    playback.push(frame(101));
    playback.push(frame(100));
    expect(playback.buffered).toBe(1);
  });
});
