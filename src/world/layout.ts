/**
 * Hand-authored world layout. One character per hex; odd rows are shifted half
 * a hex to the right. Roads and rivers are hex paths (consecutive entries must
 * be neighbours; an entry outside the map just gives the first tile an open edge).
 *
 *   .  meadow            f  forest (walkable, scattered trees)
 *   ~  lake              g  grain field
 *   M  mountain          H  hills            T  dense trees   (all impassable)
 */
export const LAYOUT_ROWS: readonly string[] = [
  'M M M M M M M M M M M M M M . M M M M', // 0
  'M H f f f H M H f f f f f . f T H M M', // 1
  'M T f f f f f . . . f f f . f f f H M', // 2
  'T f f f . . . . . . . . . . f f f f M', // 3
  'T f f . . . . . . . . . . . . f f f T', // 4
  'f f . . . . . . . . . . . . . . f f T', // 5
  'T f . . . . . . . . . . . . . . . f T', // 6
  '. . . . . . . . . . . . . . . . . f T', // 7
  'T f . . . . . . . . . . . . . . . f T', // 8
  'f . . . . . . . . . . . . . . . f f T', // 9
  'T f g g . . . . . . . . . . . . f f T', // 10
  'f g g . . . . . . . ~ ~ ~ ~ . . f T M', // 11
  'T f . . . . . . . ~ ~ ~ ~ ~ ~ . f H M', // 12
  'T f f . . . . . ~ ~ ~ ~ ~ ~ ~ . H M M', // 13
  'M T T T H . . ~ ~ ~ ~ ~ ~ ~ ~ ~ M M M', // 14
];

type HexPath = ReadonlyArray<readonly [number, number]>;

export const ROADS: readonly HexPath[] = [
  // The main road: in from the west, through the village, over the bridge.
  [[-1, 7], [0, 7], [1, 7], [2, 7], [3, 7], [4, 7], [5, 7], [6, 7], [7, 7], [8, 7], [9, 7], [10, 7], [11, 7], [12, 7], [13, 7]],
  // East of the river, up to the graveyard.
  [[13, 7], [14, 6], [14, 5], [15, 4]],
  // North lane to the chapel.
  [[6, 7], [6, 6], [6, 5], [6, 4]],
  // South lane to the lake shore.
  [[6, 7], [6, 8], [6, 9], [6, 10], [6, 11], [7, 12]],
  // Farm track.
  [[4, 7], [4, 8], [3, 9]],
];

export const RIVERS: readonly HexPath[] = [
  [[14, -1], [14, 0], [13, 1], [13, 2], [12, 3], [12, 4], [11, 5], [11, 6], [10, 7], [10, 8], [10, 9], [11, 10], [11, 11]],
];

/** Seed for decorative scatter (trees, rocks, grass); independent of the sim seed. */
export const SCATTER_SEED = 20261001;
