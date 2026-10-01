import { describe, expect, it } from 'vitest';
import { DIR_COUNT, dirVector, directionTo, hexAt, hexCenter, hexDistance, maskOf, neighbor, rotateMask, HEX_WIDTH } from './hex';
import { crossingTile, riverTile, roadTile } from './tiles';
import { cellAt, groundAt, worldMap } from './map';

describe('hex grid', () => {
  it('round-trips centres through hexAt', () => {
    for (let row = -2; row < 12; row++) {
      for (let col = -2; col < 12; col++) {
        const c = hexCenter({ col, row });
        expect(hexAt(c.x, c.z)).toEqual({ col, row });
        expect(hexAt(c.x + 3, c.z - 3)).toEqual({ col, row });
      }
    }
  });

  it('places neighbours one hex width away in the edge direction', () => {
    for (const hex of [{ col: 3, row: 4 }, { col: 3, row: 5 }]) {
      const c = hexCenter(hex);
      for (let dir = 0; dir < DIR_COUNT; dir++) {
        const n = neighbor(hex, dir);
        const nc = hexCenter(n);
        const v = dirVector(dir);
        expect(nc.x - c.x).toBeCloseTo(v.x * HEX_WIDTH, 5);
        expect(nc.z - c.z).toBeCloseTo(v.z * HEX_WIDTH, 5);
        expect(directionTo(hex, n)).toBe(dir);
        expect(directionTo(n, hex)).toBe((dir + 3) % 6);
        expect(hexDistance(hex, n)).toBe(1);
      }
    }
  });

  it('rotates edge masks', () => {
    expect(rotateMask(maskOf([0, 3]), 1)).toBe(maskOf([1, 4]));
    expect(rotateMask(maskOf([5]), 2)).toBe(maskOf([1]));
  });
});

describe('tile selection', () => {
  it('finds a road and river tile for every edge combination', () => {
    for (let mask = 1; mask < 64; mask++) {
      expect(roadTile(mask), `road mask ${mask}`).not.toBeNull();
      const edges = mask.toString(2).replaceAll('0', '').length;
      if (edges >= 2) expect(riverTile(mask), `river mask ${mask}`).not.toBeNull();
    }
  });

  it('picks the crossing tile for a NE-SW river under an east-west road', () => {
    expect(crossingTile(maskOf([2, 5]), maskOf([0, 3]))).toEqual({ model: 'hex_river_crossing_A', rot: 2 });
  });
});

describe('world map', () => {
  it('builds every hex with a tile', () => {
    expect(worldMap.cells.length).toBe(worldMap.cols * worldMap.rows);
    for (const cell of worldMap.cells) expect(cell.tile.model).toMatch(/^hex_/);
  });

  it('has the bridge on the main road and blocks the river elsewhere', () => {
    const crossing = cellAt(10, 7)!;
    expect(crossing.bridge).toBe(true);
    expect(groundAt(crossing.center.x, crossing.center.z)).toBe('open');
    const river = cellAt(11, 5)!;
    expect(groundAt(river.center.x, river.center.z)).toBe('water');
    expect(groundAt(cellAt(12, 12)!.center.x, cellAt(12, 12)!.center.z)).toBe('water');
    expect(groundAt(cellAt(0, 0)!.center.x, cellAt(0, 0)!.center.z)).toBe('blocked');
    expect(groundAt(-50, -50)).toBe('void');
  });
});
