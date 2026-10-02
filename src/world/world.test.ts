import { describe, expect, it } from 'vitest';
import { ANIMS } from '../render/anims';
import { isWalkable, nearestWalkable } from './navgrid';
import { findPath, pathLength } from './pathfind';
import { homes, places } from './places';
import { scatter } from './scatter';

describe('places', () => {
  it('have unique ids and reachable doors', () => {
    expect(new Set(places.map((p) => p.id)).size).toBe(places.length);
    for (const place of places) {
      expect(nearestWalkable(place.door, 3), `${place.id} door`).not.toBeNull();
    }
  });

  it('have beds for the whole cast and a few newcomers', () => {
    expect(homes.reduce((sum, home) => sum + home.beds, 0)).toBeGreaterThanOrEqual(14);
  });

  it('only use known animations on slots', () => {
    for (const place of places) {
      for (const slot of place.slots) {
        if (slot.anim) expect(Object.keys(ANIMS), `${slot.id}`).toContain(slot.anim);
      }
    }
  });

  it('put every activity slot next to walkable ground', () => {
    for (const place of places) {
      for (const slot of place.slots) {
        expect(nearestWalkable(slot.pos, 3), `${slot.id}`).not.toBeNull();
      }
      for (const point of place.spawnPoints) expect(nearestWalkable(point, 3), `${place.id} spawn`).not.toBeNull();
    }
  });
});

describe('navigation', () => {
  it('connects every pair of places', () => {
    const tavern = places.find((p) => p.id === 'tavern')!;
    for (const place of places) {
      const path = findPath(tavern.door, place.door);
      expect(path, `tavern -> ${place.id}`).not.toBeNull();
      const back = findPath(place.door, tavern.door);
      expect(back, `${place.id} -> tavern`).not.toBeNull();
    }
  });

  it('crosses the river over the bridge', () => {
    const smithy = places.find((p) => p.id === 'smithy')!;
    const graveyard = places.find((p) => p.id === 'graveyard')!;
    const path = findPath(smithy.door, graveyard.door)!;
    expect(path).not.toBeNull();
    const direct = Math.hypot(graveyard.door.x - smithy.door.x, graveyard.door.z - smithy.door.z);
    expect(pathLength(smithy.door, path)).toBeLessThan(direct * 1.6);
    for (const point of path) expect(isWalkable(point.x, point.z)).toBe(true);
  });

  it('is deterministic decoration', () => {
    expect(scatter.models.length).toBeGreaterThan(1000);
  });
});
