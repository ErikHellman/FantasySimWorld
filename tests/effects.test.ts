import { describe, expect, it } from 'vitest';
import { playEffects } from '../src/audio/ambient';
import type { AudioEngine } from '../src/audio/engine';
import type { WireEvent } from '../src/generated/api';

/** Records what the engine was asked to play. */
function fakeEngine(): { engine: AudioEngine; played: string[] } {
  const played: string[] = [];
  const engine = { play: (id: string, volume: number): void => { if (volume > 0) played.push(id); } } as unknown as AudioEngine;
  return { engine, played };
}

const listener = { x: 0, z: 0, distance: 10 };
const attack = (ranged: 'arrow' | 'bolt' | 'magic' | 'food'): WireEvent =>
  ({ type: 'attack', attackerId: 'c1', targetId: 'c2', ranged, from: { x: 0, z: 0 }, to: { x: 2, z: 0 } }) as WireEvent;

const hit = (outcome?: 'blocked' | 'dodged'): WireEvent =>
  ({ type: 'hit', attackerId: 'm1', targetId: 'c2', damage: 3, pos: { x: 1, z: 0 }, fatal: false, ...(outcome ? { outcome } : {}) }) as WireEvent;

describe('effects', () => {
  it('plays a shot for an arrow and a bolt, and a spell for magic', () => {
    const { engine, played } = fakeEngine();
    playEffects(engine, [attack('arrow'), attack('bolt'), attack('magic')], () => true, listener);
    expect(played).toEqual(['arrow_shot', 'arrow_shot', 'magic_cast']);
  });

  it('makes no sound for an apple a child throws', () => {
    const { engine, played } = fakeEngine();
    playEffects(engine, [attack('food')], () => true, listener);
    expect(played).toEqual([]);
  });

  it('cries out at a blow that lands, rings at a block and whooshes at a dodge', () => {
    const { engine, played } = fakeEngine();
    playEffects(engine, [hit(), hit('blocked'), hit('dodged')], () => true, listener);
    expect(played).toEqual(['sword_hit', 'hurt', 'block_clang', 'dodge_whoosh']);
  });

  it('sounds the war horn when a band comes up the road, and a fallen raider falls like a man', () => {
    const { engine, played } = fakeEngine();
    const death = (id: string): WireEvent => ({ type: 'death', id, name: 'x', pos: { x: 0, z: 0 }, monster: true }) as WireEvent;
    const raid = (phase: 'coming' | 'leaving' | 'gone'): WireEvent => ({ type: 'raid', phase }) as WireEvent;
    playEffects(engine, [raid('coming'), raid('leaving'), death('m1'), death('m2')], () => false, listener, (id) => id === 'm2');
    expect(played).toEqual(['war_horn', 'monster_death', 'death']);
  });
});
