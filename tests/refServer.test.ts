import { describe, expect, it } from 'vitest';
import type { Dyn, Entity, TickFrame } from '../src/generated/api';
import { createState } from '../src/sim/create';
import { Sim } from '../src/sim/sim';
import type { SimState } from '../src/sim/state';
import { killCharacter } from '../src/sim/systems/death';
import { makeCtx } from '../src/sim/util';
import { MAX_TICKS_PER_WAKE, TickClock } from '../tools/ref-server/clock';
import { ambientOf, chroniclePage, detailOf, rosterOf } from '../tools/ref-server/digest';
import { DeltaTracker, applyDelta, characterDyn, corpseId, diffDyn, entitiesOf, wireEvents } from '../tools/ref-server/encode';
import { TickRing, parseEventId, sseEvent } from '../tools/ref-server/sse';

/** Plays frames onto a snapshot the way a client does. */
function replay(start: Entity[], frames: TickFrame[]): Map<string, Entity> {
  const world = new Map(start.map((entity) => [entity.static.id, structuredClone(entity)]));
  for (const frame of frames) {
    for (const entity of frame.add ?? []) world.set(entity.static.id, structuredClone(entity));
    for (const id of frame.del ?? []) world.delete(id);
    for (const delta of frame.d ?? []) {
      const entity = world.get(delta.id);
      if (!entity) throw new Error(`delta for unknown ${delta.id} at ${frame.t}`);
      entity.dyn = applyDelta(entity.dyn, delta);
    }
  }
  return world;
}

function run(state: SimState, ticks: number): { sim: Sim; tracker: DeltaTracker; frames: TickFrame[]; start: Entity[] } {
  const sim = new Sim(state);
  const tracker = new DeltaTracker(sim.state);
  const start = tracker.entities();
  const frames: TickFrame[] = [];
  for (let i = 0; i < ticks; i++) frames.push(tracker.frame(sim.state, sim.step()));
  return { sim, tracker, frames, start };
}

describe('reference server encoder', () => {
  it('rebuilds the exact entity list from a snapshot and the frames that follow', () => {
    const { sim, frames, start, tracker } = run(createState(1337), 1200);
    expect(frames.some((frame) => frame.d)).toBe(true);
    const rebuilt = replay(start, frames);
    expect([...rebuilt.values()]).toEqual(tracker.entities());
    expect(tracker.entities()).toEqual(entitiesOf(sim.state));
  });

  it('keeps a late snapshot consistent with the next frames', () => {
    const sim = new Sim(createState(7));
    const tracker = new DeltaTracker(sim.state);
    for (let i = 0; i < 500; i++) tracker.frame(sim.state, sim.step());
    const snapshot = tracker.entities();
    expect(snapshot).toEqual(entitiesOf(sim.state));
    const frames: TickFrame[] = [];
    for (let i = 0; i < 300; i++) frames.push(tracker.frame(sim.state, sim.step()));
    expect([...replay(snapshot, frames).values()]).toEqual(entitiesOf(sim.state));
  });

  it('sends nothing for an entity that has not changed', () => {
    const dyn: Dyn = { p: [100, -250], f: 157, a: ['idle', 3], hp: 90, v: true, mh: 'sword', oh: null, act: null, z: false };
    expect(diffDyn('c1', dyn, { ...dyn })).toBeNull();
    expect(JSON.stringify({ t: 5 })).toBe('{"t":5}');
  });

  it('tells a cleared hand from an unchanged one', () => {
    const before: Dyn = { p: [0, 0], f: 0, a: ['idle', 0], hp: 90, v: true, mh: 'sword', oh: 'shield', act: null, z: false };
    const delta = diffDyn('c1', before, { ...before, mh: null, act: 'smith' });
    expect(JSON.stringify(delta)).toBe('{"id":"c1","mh":null,"act":"smith"}');
  });

  it('quantises positions, facing and hit points', () => {
    const state = createState(3);
    const c = state.characters[state.order[0] as string]!;
    c.pos = { x: 12.345, z: -0.005 };
    c.facing = 1.5708;
    c.hp = 88.5;
    const dyn = characterDyn(c);
    expect(dyn.p).toEqual([1235, -0]);
    expect(dyn.f).toBe(157);
    expect(dyn.hp).toBe(89);
  });

  it('puts weapons away at ease and swaps them for a tool at work', () => {
    const state = createState(3);
    const c = state.characters[state.order[0] as string]!;
    c.look.mainHand = 'sword_1handed';
    c.look.offHand = 'shield_round';
    c.anim = 'sit_chair';
    c.tool = null;
    expect([characterDyn(c).mh, characterDyn(c).oh]).toEqual([null, null]);
    c.anim = 'hammer';
    c.tool = 'tools/hammer';
    expect([characterDyn(c).mh, characterDyn(c).oh]).toEqual(['tools/hammer', null]);
    c.anim = 'idle';
    c.tool = null;
    expect([characterDyn(c).mh, characterDyn(c).oh]).toEqual(['sword_1handed', 'shield_round']);
  });

  it('turns a death into a corpse with its own id and a grave event', () => {
    const sim = new Sim(createState(11));
    const tracker = new DeltaTracker(sim.state);
    const victim = sim.state.characters[sim.state.order[2] as string]!;
    // Kill through the death system, as a fatal hit would, inside one tick's events.
    const events = sim.step();
    killCharacter(makeCtx(sim.state, events), victim, 'a test', null);
    const frame = tracker.frame(sim.state, events);
    expect(frame.del).toContain(victim.id);
    expect(frame.add?.map((entity) => entity.static.id)).toContain(corpseId(victim.id));
    const corpse = frame.add?.find((entity) => entity.static.kind === 'corpse');
    expect(corpse?.static.name).toBe('');
    expect(frame.ev?.some((event) => event.type === 'grave' && event.grave.characterId === victim.id)).toBe(true);
    expect(frame.ev?.some((event) => event.type === 'chronicle')).toBe(true);
  });

  it('maps speech to a full utterance and drops the server-only events', () => {
    const state = createState(5);
    const [a, b] = state.order as [string, string];
    const events = wireEvents(state, [
      { type: 'speech', speakerId: a, toId: b, text: 'Good morning.', durationTicks: 40, loud: false },
      { type: 'conversation_started', convoId: 'v1', a, b, tone: 'friendly' },
      { type: 'save_point', reason: 'test' },
    ]);
    expect(events).toHaveLength(1);
    const speech = events[0]!;
    expect(speech.type).toBe('speech');
    if (speech.type !== 'speech') return;
    expect(speech.speakerName).toBe(state.characters[a]!.profile.name.split(' ')[0]);
    expect(speech.toId).toBe(b);
    expect(speech.durationMs).toBe(2000);
    expect(speech.voice).toEqual(state.characters[a]!.profile.voice);
  });

  it('produces near-empty frames while the village sleeps', () => {
    const sim = new Sim(createState(1337));
    // Run into the small hours of the first night.
    for (let i = 0; i < 20 * 60 * 20; i++) sim.step();
    const tracker = new DeltaTracker(sim.state);
    let bytes = 0;
    for (let i = 0; i < 200; i++) bytes += sseEvent('tick', tracker.frame(sim.state, sim.step()), `abcdef:${sim.state.tick}`).length;
    // Under 1 KB/s while everyone sleeps (allowing for the odd monster).
    expect(bytes / 10).toBeLessThan(2500);
  });
});

describe('reference server digest', () => {
  const sim = new Sim(createState(1337));
  for (let i = 0; i < 3000; i++) sim.step();
  const state = sim.state;

  it('renders the roster and the inspector with labels', () => {
    const roster = rosterOf(state);
    expect(roster).toHaveLength(state.order.length);
    for (const row of roster) {
      expect(row.status.length).toBeGreaterThan(0);
      expect(row.hp).toBeGreaterThan(0);
    }
    const detail = detailOf(state, state.order[0] as string)!;
    expect(detail.className).not.toBe('');
    expect(detail.plan.filter((row) => row.current)).toHaveLength(1);
    expect(detail.plan[0]!.time).toMatch(/^\d\d:\d\d$/);
    for (const row of detail.relations) expect(row.rung).not.toContain('_');
    expect(detail.memories.length).toBeLessThanOrEqual(40);
    expect(detailOf(state, 'nobody')).toBeNull();
  });

  it('counts what the ambient mixer needs', () => {
    const counts = ambientOf(state);
    expect(counts.awake).toBeLessThanOrEqual(state.order.length);
    expect(counts.eating + counts.drinking + counts.smithing + counts.relaxing).toBeLessThanOrEqual(state.order.length);
  });

  it('pages the chronicle backwards, oldest first', () => {
    const entries = [1, 2, 2, 5, 9, 12].map((tick) => ({ tick, kind: 'world' as const, text: `t${tick}`, ids: [] }));
    expect(chroniclePage(entries, 100, 2)).toEqual({ entries: entries.slice(4), more: true });
    expect(chroniclePage(entries, 9, 3)).toEqual({ entries: entries.slice(1, 4), more: true });
    expect(chroniclePage(entries, 5, 10)).toEqual({ entries: entries.slice(0, 3), more: false });
  });
});

describe('reference server clock and stream', () => {
  it('runs one tick per 50 ms at 1x, more at higher speeds, and caps a wake at 12', () => {
    const clock = new TickClock(0);
    expect(clock.due(50, 1)).toBe(1);
    expect(clock.due(100, 1)).toBe(1);
    expect(clock.due(150.5, 1)).toBe(1);
    const fast = new TickClock(0);
    fast.due(0, 4);
    expect(fast.due(50, 4)).toBe(4);
    const behind = new TickClock(0);
    behind.due(0, 1);
    expect(behind.due(1000, 1)).toBe(MAX_TICKS_PER_WAKE);
    expect(behind.due(1001, 1)).toBe(20 - MAX_TICKS_PER_WAKE);
  });

  it('skips ahead instead of fast-forwarding past 200 owed ticks', () => {
    const clock = new TickClock(0);
    clock.due(0, 1);
    expect(clock.due(60_000, 1)).toBe(0);
    expect(clock.skips).toBe(1);
    expect(clock.due(60_050, 1)).toBe(1);
  });

  it('stops while paused and wakes on 50 ms deadlines', () => {
    const clock = new TickClock(0);
    expect(clock.due(500, 0)).toBe(0);
    expect(clock.nextWake(510)).toBe(550);
  });

  it('replays the gap from the ring, or asks for a snapshot', () => {
    const ring = new TickRing(100);
    for (let t = 1; t <= 150; t++) ring.push(t, `tick ${t}`);
    expect(ring.since(140, 150)).toEqual(['tick 141', 'tick 142', 'tick 143', 'tick 144', 'tick 145', 'tick 146', 'tick 147', 'tick 148', 'tick 149', 'tick 150']);
    expect(ring.since(150, 150)).toEqual([]);
    expect(ring.since(50, 150)).toHaveLength(100);
    expect(ring.since(20, 150)).toBeNull();
    expect(ring.since(151, 150)).toBeNull();
    expect(parseEventId('abc123:42')).toEqual({ bootId: 'abc123', tick: 42 });
    expect(parseEventId('nonsense')).toBeNull();
  });

  it('frames events one data line each', () => {
    expect(sseEvent('tick', { t: 1 }, 'b:1')).toBe('id: b:1\nevent: tick\ndata: {"t":1}\n\n');
    expect(sseEvent('status', { text: 'two\nlines' })).toBe('event: status\ndata: {"text":"two\\nlines"}\n\n');
  });
});
