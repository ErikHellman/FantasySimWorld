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
const attack = (ranged: 'arrow' | 'bolt' | 'magic' | 'food' | 'fire'): WireEvent =>
  ({ type: 'attack', attackerId: 'c1', targetId: 'c2', ranged, from: { x: 0, z: 0 }, to: { x: 2, z: 0 } }) as WireEvent;

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

  it('plays a rush of fire for the breath of the dragon, not the twang of a bow', () => {
    const { engine, played } = fakeEngine();
    playEffects(engine, [attack('fire')], () => true, listener);
    expect(played).toEqual(['magic_cast']);
  });

  it('rings the alarm when someone shouts for the dragon', () => {
    const { engine, played } = fakeEngine();
    const shout = { type: 'speech', speakerId: 'c1', text: 'A dragon! A dragon over the green! Take cover!', loud: true } as WireEvent;
    playEffects(engine, [shout], () => true, listener);
    expect(played).toEqual(['alarm']);
  });
});
