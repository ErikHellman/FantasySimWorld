import { describe, expect, it } from 'vitest';
import { twoFinger, type FingerPair } from './gestures';

const pair = (ax: number, ay: number, bx: number, by: number): FingerPair => [{ x: ax, y: ay }, { x: bx, y: by }];

describe('two-finger gestures', () => {
  it('reads fingers spreading apart as zooming in', () => {
    const move = twoFinger(pair(100, 100, 200, 100), pair(50, 100, 250, 100));
    expect(move.scale).toBeCloseTo(2);
    expect(move.twist).toBeCloseTo(0);
    expect(move.lift).toBe(0);
  });

  it('reads fingers pinching together as zooming out', () => {
    expect(twoFinger(pair(0, 0, 200, 0), pair(50, 0, 150, 0)).scale).toBeCloseTo(0.5);
  });

  it('reads a turn of the pair as a twist, clockwise on screen being positive', () => {
    // From pointing right to pointing down is a quarter turn clockwise, since screen y grows downwards.
    const move = twoFinger(pair(0, 0, 100, 0), pair(0, 0, 0, 100));
    expect(move.twist).toBeCloseTo(Math.PI / 2);
    expect(move.scale).toBeCloseTo(1);
  });

  it('takes the short way round when the angle crosses the half turn', () => {
    // Just above pointing left to just below it: a small anticlockwise turn, not nearly a whole one.
    const move = twoFinger(pair(0, 0, -100, -1), pair(0, 0, -100, 1));
    expect(move.twist).toBeCloseTo(-0.02, 3);
  });

  it('reads both fingers moving up together as a lift, with no zoom or twist', () => {
    const move = twoFinger(pair(100, 300, 200, 300), pair(100, 260, 200, 260));
    expect(move.lift).toBeCloseTo(-40);
    expect(move.scale).toBeCloseTo(1);
    expect(move.twist).toBeCloseTo(0);
  });

  it('does not lift when the fingers move vertically in opposite directions', () => {
    expect(twoFinger(pair(100, 100, 100, 200), pair(100, 90, 100, 230)).lift).toBe(0);
  });

  it('does nothing for fingers on the same spot', () => {
    const move = twoFinger(pair(10, 10, 10, 10), pair(10, 10, 20, 10));
    expect(move.scale).toBe(1);
    expect(move.twist).toBe(0);
  });
});
