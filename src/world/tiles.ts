import { maskOf, rotateMask } from './hex';

/**
 * Which hex edges each KayKit road / river / coast tile connects, measured from
 * the tile geometry (edge directions as in hex.ts, tile unrotated).
 */
const ROAD_TILES: ReadonlyArray<readonly [string, number]> = [
  ['hex_road_M', maskOf([3])],
  ['hex_road_A', maskOf([0, 3])],
  ['hex_road_B', maskOf([3, 5])],
  ['hex_road_C', maskOf([3, 4])],
  ['hex_road_D', maskOf([1, 3, 5])],
  ['hex_road_E', maskOf([0, 3, 5])],
  ['hex_road_F', maskOf([0, 1, 3])],
  ['hex_road_G', maskOf([2, 3, 4])],
  ['hex_road_H', maskOf([0, 2, 3, 4])],
  ['hex_road_I', maskOf([1, 2, 4, 5])],
  ['hex_road_J', maskOf([0, 1, 2, 3])],
  ['hex_road_K', maskOf([1, 2, 3, 4, 5])],
  ['hex_road_L', maskOf([0, 1, 2, 3, 4, 5])],
];

const RIVER_TILES: ReadonlyArray<readonly [string, number]> = [
  ['hex_river_A', maskOf([0, 3])],
  ['hex_river_B', maskOf([3, 5])],
  ['hex_river_C', maskOf([3, 4])],
  ['hex_river_D', maskOf([1, 3, 5])],
  ['hex_river_E', maskOf([0, 3, 5])],
  ['hex_river_F', maskOf([0, 1, 3])],
  ['hex_river_G', maskOf([2, 3, 4])],
  ['hex_river_H', maskOf([0, 2, 3, 4])],
  ['hex_river_I', maskOf([1, 2, 4, 5])],
  ['hex_river_J', maskOf([0, 1, 2, 3])],
  ['hex_river_K', maskOf([1, 2, 3, 4, 5])],
  ['hex_river_L', maskOf([0, 1, 2, 3, 4, 5])],
];

/** Coast tiles by which edges face water. */
const COAST_TILES: ReadonlyArray<readonly [string, number]> = [
  ['hex_coast_A', maskOf([1])],
  ['hex_coast_B', maskOf([1, 2])],
  ['hex_coast_C', maskOf([0, 1, 2])],
  ['hex_coast_D', maskOf([0, 1, 2, 5])],
];

/** River runs 0-3 on both; the road crosses on 1-4 (A) or 2-5 (B). */
const CROSSING_TILES: ReadonlyArray<readonly [string, number, number]> = [
  ['hex_river_crossing_A', maskOf([0, 3]), maskOf([1, 4])],
  ['hex_river_crossing_B', maskOf([0, 3]), maskOf([2, 5])],
];

export interface TileChoice {
  model: string;
  /** rotation in 60° steps: tile edge k ends up at k + rot */
  rot: number;
}

function match(tiles: ReadonlyArray<readonly [string, number]>, mask: number): TileChoice | null {
  for (const [model, base] of tiles) {
    for (let rot = 0; rot < 6; rot++) {
      if (rotateMask(base, rot) === mask) return { model, rot };
    }
  }
  return null;
}

export const roadTile = (mask: number): TileChoice | null => match(ROAD_TILES, mask);
export const riverTile = (mask: number): TileChoice | null => match(RIVER_TILES, mask);
export const coastTile = (waterMask: number): TileChoice | null => match(COAST_TILES, waterMask);

export function crossingTile(riverMask: number, roadMask: number): TileChoice | null {
  for (const [model, river, road] of CROSSING_TILES) {
    for (let rot = 0; rot < 6; rot++) {
      if (rotateMask(river, rot) === riverMask && rotateMask(road, rot) === roadMask) return { model, rot };
    }
  }
  return null;
}
