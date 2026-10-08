import { describe, expect, it } from 'vitest';
import { ambientLevels } from '../src/audio/ambient';
import type { AmbientCounts, Weather } from '../src/generated/api';
import { getPlace } from '../src/world/places';

const counts: AmbientCounts = { awake: 10, eating: 0, drinking: 0, smithing: 0, relaxing: 0 };
const green = getPlace('green').origin;
const listener = { x: green.x, z: green.z, distance: 60 };
const sky = (kind: Weather['kind'], intensity: number, wind = 0): Weather => ({ kind, intensity, wind, windDir: 0 });

describe('ambient levels in the weather', () => {
  it('is silent of rain and hail under a clear sky, as it always was', () => {
    const clear = ambientLevels(counts, 12, listener, [], sky('clear', 0));
    expect(clear.rain).toBe(0);
    expect(clear.hail).toBe(0);
    expect(clear.wind).toBe(ambientLevels(counts, 12, listener).wind);
  });

  it('brings the rain up and the birds and the village down as it pours', () => {
    const clear = ambientLevels(counts, 12, listener, [], sky('clear', 0));
    const pouring = ambientLevels(counts, 12, listener, [], sky('rain', 100, 30));
    expect(pouring.rain).toBeGreaterThan(0.6);
    expect(pouring.birds_day).toBeLessThan((clear.birds_day ?? 0) * 0.3);
    expect(pouring.village_day).toBeLessThan(clear.village_day ?? 0);
  });

  it('drums hail over a shower, and raises the wind with the wind', () => {
    const hail = ambientLevels(counts, 12, listener, [], sky('hail', 100, 60));
    expect(hail.hail).toBeGreaterThan(0.6);
    expect(hail.rain).toBeGreaterThan(0);
    const gale = ambientLevels(counts, 12, listener, [], sky('wind', 100, 100));
    expect(gale.wind).toBeGreaterThan(ambientLevels(counts, 12, listener).wind ?? 0);
    expect(gale.wind).toBeLessThanOrEqual(1);
  });
});
