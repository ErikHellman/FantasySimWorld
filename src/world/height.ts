import { BRIDGE_GRID, BRIDGE_HEIGHTS, NO_GROUND, TILE_GRID, TILE_HEIGHTS } from '../generated/tileHeights';

/**
 * Ground heights sampled from the hex tile models by the asset build, so the
 * world layer knows where water, sand and road are without any 3D geometry.
 * All values here are in model units (multiply by HEX_SCALE for world units).
 */
interface Grid {
  nx: number;
  nz: number;
  minX: number;
  minZ: number;
  maxX: number;
  maxZ: number;
}

function decode(base64: string): Int8Array {
  const binary = atob(base64);
  const data = new Int8Array(binary.length);
  for (let i = 0; i < binary.length; i++) data[i] = (binary.charCodeAt(i) << 24) >> 24;
  return data;
}

const tiles = new Map<string, Int8Array>();
const bridge = decode(BRIDGE_HEIGHTS);

function sample(data: Int8Array, grid: Grid, x: number, z: number): number | null {
  const ix = Math.floor(((x - grid.minX) / (grid.maxX - grid.minX)) * grid.nx);
  const iz = Math.floor(((z - grid.minZ) / (grid.maxZ - grid.minZ)) * grid.nz);
  if (ix < 0 || iz < 0 || ix >= grid.nx || iz >= grid.nz) return null;
  const value = data[iz * grid.nx + ix] as number;
  return value === NO_GROUND ? null : value / 100;
}

/** Height of a tile's surface at a point in the tile's own (unrotated) frame; null off the tile. */
export function tileHeight(model: string, x: number, z: number): number | null {
  let data = tiles.get(model);
  if (!data) {
    const encoded = TILE_HEIGHTS[model];
    if (!encoded) return null;
    data = decode(encoded);
    tiles.set(model, data);
  }
  return sample(data, TILE_GRID, x, z);
}

/** Height of the bridge deck at a point in the bridge model's own frame; null off the bridge. */
export function bridgeHeight(x: number, z: number): number | null {
  return sample(bridge, BRIDGE_GRID, x, z);
}

/** Half the walkable width of the bridge deck, in model units. */
export const BRIDGE_HALF_WIDTH = 0.42;
