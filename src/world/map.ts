import { distToSegment, type Vec2 } from '../core/math2d';
import { hashString } from '../core/rng';
import { BRIDGE_HALF_WIDTH, bridgeHeight, tileHeight } from './height';
import {
  DIR_COUNT, HEX_APOTHEM, HEX_RADIUS, HEX_ROW_PITCH, HEX_SCALE, HEX_WIDTH,
  dirVector, directionTo, hexAt, hexCenter, hexKey, maskHas, neighbor, type Hex,
} from './hex';
import { LAYOUT_ROWS, RIVERS, ROADS } from './layout';
import { coastTile, crossingTile, riverTile, roadTile, type TileChoice } from './tiles';

export type Terrain = 'meadow' | 'forest' | 'water' | 'mountain' | 'hills' | 'trees' | 'grain';

const TERRAIN_BY_CHAR: Record<string, Terrain> = {
  '.': 'meadow', f: 'forest', '~': 'water', M: 'mountain', H: 'hills', T: 'trees', g: 'grain',
};

const IMPASSABLE: ReadonlySet<Terrain> = new Set(['water', 'mountain', 'hills', 'trees']);

export interface HexCell extends Hex {
  terrain: Terrain;
  center: Vec2;
  /** edges with a road / river connection */
  roadMask: number;
  riverMask: number;
  /** edges that face open water (lake) */
  waterMask: number;
  tile: TileChoice;
  /** hex-pack model standing on the tile (mountain, hills, trees, grain), scaled like the tile */
  decoration: TileChoice | null;
  /** true when a bridge model sits on this hex */
  bridge: boolean;
}

export interface WorldMap {
  cols: number;
  rows: number;
  cells: HexCell[];
  minX: number;
  minZ: number;
  maxX: number;
  maxZ: number;
}

/** Half-width of the road strip, in world units. */
export const ROAD_HALF_WIDTH = 1.6;

function parseLayout(): Terrain[][] {
  return LAYOUT_ROWS.map((line, row) => {
    const chars = line.replace(/\s+/g, '').split('');
    return chars.map((ch, col) => {
      const terrain = TERRAIN_BY_CHAR[ch];
      if (!terrain) throw new Error(`Unknown terrain '${ch}' at ${col},${row}`);
      return terrain;
    });
  });
}

function pickVariant(models: readonly string[], col: number, row: number): TileChoice {
  const h = hashString(`${col},${row}`);
  return { model: models[h % models.length] as string, rot: (h >>> 8) % 6 };
}

function buildMap(): WorldMap {
  const grid = parseLayout();
  const rows = grid.length;
  const cols = grid[0]?.length ?? 0;
  for (const [row, line] of grid.entries()) {
    if (line.length !== cols) throw new Error(`Layout row ${row} has ${line.length} hexes, expected ${cols}`);
  }
  const inMap = (h: Hex) => h.col >= 0 && h.row >= 0 && h.col < cols && h.row < rows;
  const terrainAt = (h: Hex): Terrain | null => (inMap(h) ? (grid[h.row]?.[h.col] ?? null) : null);

  const roadMasks = new Map<string, number>();
  const riverMasks = new Map<string, number>();
  const connect = (masks: Map<string, number>, paths: typeof ROADS, label: string) => {
    for (const path of paths) {
      for (let i = 0; i + 1 < path.length; i++) {
        const a = { col: path[i]![0], row: path[i]![1] };
        const b = { col: path[i + 1]![0], row: path[i + 1]![1] };
        const dir = directionTo(a, b);
        if (dir < 0) throw new Error(`${label} path: ${hexKey(a)} and ${hexKey(b)} are not neighbours`);
        if (inMap(a)) masks.set(hexKey(a), (masks.get(hexKey(a)) ?? 0) | (1 << dir));
        if (inMap(b)) masks.set(hexKey(b), (masks.get(hexKey(b)) ?? 0) | (1 << ((dir + 3) % 6)));
      }
    }
  };
  connect(roadMasks, ROADS, 'Road');
  connect(riverMasks, RIVERS, 'River');

  const cells: HexCell[] = [];
  for (let row = 0; row < rows; row++) {
    for (let col = 0; col < cols; col++) {
      const hex = { col, row };
      const terrain = grid[row]![col]!;
      const key = hexKey(hex);
      const roadMask = terrain === 'water' ? 0 : (roadMasks.get(key) ?? 0);
      const riverMask = terrain === 'water' ? 0 : (riverMasks.get(key) ?? 0);
      let waterMask = 0;
      if (terrain !== 'water') {
        for (let dir = 0; dir < DIR_COUNT; dir++) {
          if (terrainAt(neighbor(hex, dir)) === 'water') waterMask |= 1 << dir;
        }
      }

      let tile: TileChoice | null = null;
      let decoration: TileChoice | null = null;
      let bridge = false;
      if (terrain === 'water') {
        tile = { model: 'hex_water', rot: 0 };
      } else if (riverMask && roadMask) {
        tile = crossingTile(riverMask, roadMask);
        bridge = true;
        if (!tile) throw new Error(`No river crossing tile fits ${key}`);
      } else if (riverMask) {
        tile = riverTile(riverMask);
        if (!tile) throw new Error(`No river tile fits ${key} (mask ${riverMask.toString(2)})`);
      } else if (roadMask) {
        tile = roadTile(roadMask);
        if (!tile) throw new Error(`No road tile fits ${key} (mask ${roadMask.toString(2)})`);
      } else if (waterMask) {
        tile = coastTile(waterMask) ?? { model: 'hex_grass', rot: 0 };
      } else {
        tile = { model: 'hex_grass', rot: 0 };
        if (terrain === 'mountain') {
          decoration = pickVariant(['mountain_A', 'mountain_B', 'mountain_C', 'mountain_A_grass', 'mountain_B_grass', 'mountain_C_grass_trees'], col, row);
        } else if (terrain === 'hills') {
          decoration = pickVariant(['hills_A', 'hills_B', 'hills_C', 'hills_A_trees', 'hills_B_trees', 'hills_C_trees'], col, row);
        } else if (terrain === 'trees') {
          decoration = pickVariant(['trees_A_large', 'trees_B_large', 'trees_A_medium', 'trees_B_medium'], col, row);
        } else if (terrain === 'grain') {
          decoration = { model: 'building_grain', rot: 0 };
        }
      }

      cells.push({ col, row, terrain, center: hexCenter(hex), roadMask, riverMask, waterMask, tile, decoration, bridge });
    }
  }

  return {
    cols,
    rows,
    cells,
    minX: -HEX_APOTHEM,
    minZ: -HEX_RADIUS,
    maxX: HEX_WIDTH * (cols - 1) + HEX_APOTHEM * 2,
    maxZ: HEX_ROW_PITCH * (rows - 1) + HEX_RADIUS,
  };
}

export const worldMap: WorldMap = buildMap();

export function cellAt(col: number, row: number): HexCell | null {
  if (col < 0 || row < 0 || col >= worldMap.cols || row >= worldMap.rows) return null;
  return worldMap.cells[row * worldMap.cols + col] ?? null;
}

export function cellAtPoint(x: number, z: number): HexCell | null {
  const hex = hexAt(x, z);
  return cellAt(hex.col, hex.row);
}

/** Distance from a point to the centre line of the lines drawn on its hex for `mask`. */
function distanceToSpokes(cell: HexCell, mask: number, p: Vec2): number {
  let best = Infinity;
  for (let dir = 0; dir < DIR_COUNT; dir++) {
    if (!maskHas(mask, dir)) continue;
    const v = dirVector(dir);
    const edge = { x: cell.center.x + v.x * HEX_APOTHEM, z: cell.center.z + v.z * HEX_APOTHEM };
    best = Math.min(best, distToSegment(p, cell.center, edge));
  }
  return best;
}

export function roadDistance(x: number, z: number): number {
  const cell = cellAtPoint(x, z);
  if (!cell || !cell.roadMask) return Infinity;
  return distanceToSpokes(cell, cell.roadMask, { x, z });
}

export function isOnRoad(x: number, z: number): boolean {
  return roadDistance(x, z) <= ROAD_HALF_WIDTH;
}

export type Ground = 'void' | 'blocked' | 'water' | 'open';

const TILE_ROTATION_STEP = -Math.PI / 3;
/**
 * Surfaces lower than this (model units) are under water. River beds are flat
 * at -0.1; a beach slopes all the way down to the lake at -0.2.
 */
const RIVER_WATER_LEVEL = -0.075;
const LAKE_WATER_LEVEL = -0.16;

/**
 * The bridge model's deck runs between two opposite hex edges, along direction 1
 * in the model's own frame (60° from +x), not along z.
 */
const BRIDGE_MODEL_DIR = 1;

/** The edge direction the road (and so the bridge) on this hex runs along. */
function bridgeDir(cell: HexCell): number {
  for (let dir = 0; dir < DIR_COUNT; dir++) {
    if (maskHas(cell.roadMask, dir)) return dir;
  }
  return BRIDGE_MODEL_DIR;
}

/** The rotation about y that lines the bridge model on this hex up with the road. */
export function bridgeRotation(cell: HexCell): number {
  return (bridgeDir(cell) - BRIDGE_MODEL_DIR) * TILE_ROTATION_STEP;
}

/** A world offset from a hex centre, expressed in the frame of a model rotated by `rotation` about y. */
function toLocal(dx: number, dz: number, rotation: number): Vec2 {
  const cos = Math.cos(rotation);
  const sin = Math.sin(rotation);
  return { x: (dx * cos - dz * sin) / HEX_SCALE, z: (dx * sin + dz * cos) / HEX_SCALE };
}

function tileSurface(cell: HexCell, x: number, z: number): number | null {
  const local = toLocal(x - cell.center.x, z - cell.center.z, cell.tile.rot * TILE_ROTATION_STEP);
  // Right on a hex edge the sample can miss the tile; pull it slightly towards the centre.
  return tileHeight(cell.tile.model, local.x, local.z) ?? tileHeight(cell.tile.model, local.x * 0.93, local.z * 0.93);
}

function bridgeDeck(cell: HexCell, x: number, z: number): number | null {
  if (!cell.bridge) return null;
  const dx = x - cell.center.x;
  const dz = z - cell.center.z;
  // The deck runs along the road, so its width is measured square to the road.
  const along = dirVector(bridgeDir(cell));
  if (Math.abs(dx * along.z - dz * along.x) / HEX_SCALE > BRIDGE_HALF_WIDTH) return null;
  const local = toLocal(dx, dz, bridgeRotation(cell));
  return bridgeHeight(local.x, local.z);
}

/** What the terrain itself allows at a point (buildings and props are handled by the nav grid). */
export function groundAt(x: number, z: number): Ground {
  const cell = cellAtPoint(x, z);
  if (!cell) return 'void';
  if (cell.terrain === 'water') return 'water';
  if (IMPASSABLE.has(cell.terrain)) return 'blocked';
  if (bridgeDeck(cell, x, z) !== null) return 'open';
  const height = tileSurface(cell, x, z);
  return height !== null && height < (cell.riverMask ? RIVER_WATER_LEVEL : LAKE_WATER_LEVEL) ? 'water' : 'open';
}

export function isWalkableTerrain(x: number, z: number): boolean {
  return groundAt(x, z) === 'open';
}

/** Height of the ground (or bridge deck) in world units; 0 on plain grass. Used only for drawing. */
export function groundHeight(x: number, z: number): number {
  const cell = cellAtPoint(x, z);
  if (!cell) return 0;
  const surface = tileSurface(cell, x, z) ?? 0;
  const deck = bridgeDeck(cell, x, z);
  return Math.max(surface, deck ?? -Infinity) * HEX_SCALE;
}
