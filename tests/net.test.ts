import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import type { Entity, Snapshot, TickFrame } from '../src/generated/api';
import { Playback } from '../src/net/playback';
import { ViewWorld } from '../src/net/viewWorld';

const entity = (id: string, x: number): Entity => ({
  static: { id, kind: 'villager', name: id, look: { model: 'Knight', texture: null, hide: [], mainHand: 'sword', offHand: null }, maxHp: 100, hostile: false, voice: null },
  dyn: { p: [x * 100, 0], f: 0, a: ['idle', 0], hp: 100, v: true, mh: 'sword', oh: null, act: null, z: false },
});

const snapshot = (tick: number, entities: Entity[]): Snapshot => ({
  tick, speed: 1, entities, roster: [], chronicle: [], graves: [], alarm: false, voices: [], billboards: [], houses: [], ceremony: null, worldObjects: [], stockpiles: [],
  economy: { purse: 80, food: 0, potion: 0, gift: 0, trinket: 0, relic: 0 }, loot: [],
  weather: { kind: 'clear', intensity: 0, wind: 0, windDir: 0 },
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

  it('keeps what each billboard shows, from the snapshot and then from events', () => {
    const world = new ViewWorld();
    const board = { id: 'green_board', place: 'the village green', text: 'Warning: the river is wet.', tone: 'absurd' as const, custom: false };
    world.reset({ ...snapshot(1, []), billboards: [board] });
    const version = world.billboardsVersion;
    expect(world.billboards.get('green_board')?.text).toBe('Warning: the river is wet.');
    world.apply({ t: 2, ev: [{ type: 'billboard', billboard: { ...board, text: 'Fresh bread!', tone: 'pushy', custom: true } }] });
    expect(world.billboards.get('green_board')).toMatchObject({ text: 'Fresh bread!', custom: true });
    expect(world.billboards.size).toBe(1);
    expect(world.billboardsVersion).toBe(version + 1);
  });

  it('keeps the homes that are burning or in ruins, and forgets one that stands whole again', () => {
    const world = new ViewWorld();
    const house = { id: 'home_1', name: 'the red cottage', phase: 'burning' as const, fire: 20, built: 0 };
    world.reset({ ...snapshot(1, []), houses: [house] });
    const version = world.housesVersion;
    expect(world.houses.get('home_1')?.fire).toBe(20);
    world.apply({ t: 2, ev: [{ type: 'house', house: { ...house, fire: 45 } }] });
    world.apply({ t: 3, ev: [{ type: 'house', house: { ...house, phase: 'ruin', fire: 0 } }] });
    expect(world.houses.get('home_1')).toMatchObject({ phase: 'ruin', fire: 0 });
    world.apply({ t: 4, ev: [{ type: 'house', house: { ...house, phase: 'intact', fire: 0, built: 100 } }] });
    expect(world.houses.size).toBe(0);
    expect(world.housesVersion).toBe(version + 3);
  });

  it('keeps the ceremony the village has arranged, and forgets one that is over', () => {
    const world = new ViewWorld();
    const wedding = {
      id: 'y1', kind: 'wedding' as const, phase: 'announced' as const, placeId: 'green', placeName: 'the village green',
      ids: ['c1', 'c2'], names: ['Sylvi', 'Kael'], day: 5, time: '14:00',
    };
    world.reset({ ...snapshot(1, []), ceremony: wedding });
    const version = world.ceremonyVersion;
    expect(world.ceremony?.names).toEqual(['Sylvi', 'Kael']);
    world.apply({ t: 2, ev: [{ type: 'ceremony', ceremony: { ...wedding, phase: 'rite' } }] });
    expect(world.ceremony?.phase).toBe('rite');
    world.apply({ t: 3, ev: [{ type: 'ceremony', ceremony: { ...wedding, phase: 'over' } }] });
    expect(world.ceremony).toBeNull();
    expect(world.ceremonyVersion).toBe(version + 2);
  });

  it('keeps what differs from the map and what the stockpiles hold, forgetting what is whole or empty again', () => {
    const world = new ViewWorld();
    const tree = { id: 's:7', kind: 'tree' as const, name: 'a tree near the lumber camp', stage: 'standing', sinceTick: 40, progress: 20, fire: 0, placed: null };
    const camp = { place: 'lumber_camp', name: 'the lumber camp', wood: 4, stone: 0, ore: 0 };
    world.reset({ ...snapshot(1, []), worldObjects: [tree], stockpiles: [camp] });
    const objects = world.worldObjectsVersion;
    const piles = world.stockpilesVersion;
    expect(world.worldObjects.get('s:7')?.progress).toBe(20);
    world.apply({ t: 2, ev: [{ type: 'world_object', object: { ...tree, stage: 'falling', sinceTick: 900, progress: 0 } }] });
    world.apply({ t: 3, ev: [{ type: 'world_object', object: { id: 's:9', kind: 'rock', name: 'a rock near the old mine', stage: 'mined_out', sinceTick: 950, progress: 0, fire: 0, placed: null } }] });
    expect(world.worldObjects.get('s:7')).toMatchObject({ stage: 'falling', sinceTick: 900 });
    expect(world.worldObjects.size).toBe(2);
    world.apply({ t: 4, ev: [{ type: 'world_object', object: { ...tree, stage: 'intact' } }] });
    expect([...world.worldObjects.keys()]).toEqual(['s:9']);
    expect(world.worldObjectsVersion).toBe(objects + 3);
    world.apply({ t: 5, ev: [{ type: 'stockpile', stockpile: { ...camp, wood: 9 } }] });
    expect(world.stockpiles.get('lumber_camp')?.wood).toBe(9);
    world.apply({ t: 6, ev: [{ type: 'stockpile', stockpile: { ...camp, wood: 0 } }] });
    expect(world.stockpiles.size).toBe(0);
    expect(world.stockpilesVersion).toBe(piles + 2);
  });

  it('keeps what the dead left on the ground until it is picked up', () => {
    const world = new ViewWorld();
    const coins = { id: 'l:4', kind: 'coins' as const, count: 3, pos: { x: 1, z: 2 }, gone: false };
    world.reset({ ...snapshot(1, []), loot: [coins] });
    const version = world.lootVersion;
    expect(world.loot.get('l:4')?.count).toBe(3);
    world.apply({ t: 2, ev: [{ type: 'loot', loot: { id: 'l:5', kind: 'relic', count: 0, pos: { x: 4, z: 4 }, gone: false } }] });
    expect(world.loot.size).toBe(2);
    world.apply({ t: 3, ev: [{ type: 'loot', loot: { ...coins, gone: true } }] });
    expect([...world.loot.keys()]).toEqual(['l:5']);
    expect(world.lootVersion).toBe(version + 2);
  });

  it('keeps the chest and the stalls as coin and goods move', () => {
    const world = new ViewWorld();
    world.reset(snapshot(1, []));
    expect(world.economy.purse).toBe(80);
    const chest = world.economyVersion;
    world.apply({ t: 2, ev: [{ type: 'economy', economy: { purse: 74, food: 3, potion: 0, gift: 0, trinket: 0, relic: 0 } }] });
    expect(world.economy).toMatchObject({ purse: 74, food: 3 });
    expect(world.economyVersion).toBe(chest + 1);
  });

  it('follows the weather as it comes on and turns', () => {
    const world = new ViewWorld();
    world.reset(snapshot(1, []));
    expect(world.weather.kind).toBe('clear');
    const sky = world.weatherVersion;
    world.apply({ t: 2, ev: [{ type: 'weather', weather: { kind: 'rain', intensity: 35, wind: 20, windDir: 90 } }] });
    expect(world.weather).toEqual({ kind: 'rain', intensity: 35, wind: 20, windDir: 90 });
    expect(world.weatherVersion).toBe(sky + 1);
  });

  it('follows the server frame for frame', () => {
    // Written by the Rust encoder (`cargo run --bin export-frames` in server/); a server test keeps it current.
    const fixture = JSON.parse(readFileSync(join(__dirname, 'fixtures', 'server-frames.json'), 'utf8')) as {
      tick: number; entities: Entity[]; frames: TickFrame[]; expected: Entity[];
    };
    const world = new ViewWorld();
    world.reset(snapshot(fixture.tick, fixture.entities));
    for (const frame of fixture.frames) world.apply(frame);
    expect(world.tick).toBe(fixture.frames.at(-1)!.t);
    expect([...world.entities.keys()]).toEqual(fixture.expected.map((e) => e.static.id));
    for (const e of fixture.expected) {
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
