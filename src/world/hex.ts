import type { Vec2 } from '../core/math2d';

/**
 * Hex grid geometry. KayKit hex tiles are pointy-top along z (flat sides face ±x),
 * 2 units flat-to-flat, and are scaled by HEX_SCALE to match character size.
 * Rows run along x; odd rows are shifted half a hex to the right ("odd-r" offset).
 */
export const HEX_SCALE = 7;
export const HEX_WIDTH = 2 * HEX_SCALE;
export const HEX_ROW_PITCH = Math.sqrt(3) * HEX_SCALE;
/** centre to corner */
export const HEX_RADIUS = (2 / Math.sqrt(3)) * HEX_SCALE;
/** centre to edge midpoint */
export const HEX_APOTHEM = HEX_SCALE;

export interface Hex {
  col: number;
  row: number;
}

/**
 * Edge directions 0..5 at 60° steps from +x towards +z:
 * 0 east, 1 south-east, 2 south-west, 3 west, 4 north-west, 5 north-east.
 */
export const DIR_COUNT = 6;
const EVEN_ROW: ReadonlyArray<readonly [number, number]> = [[1, 0], [0, 1], [-1, 1], [-1, 0], [-1, -1], [0, -1]];
const ODD_ROW: ReadonlyArray<readonly [number, number]> = [[1, 0], [1, 1], [0, 1], [-1, 0], [0, -1], [1, -1]];

export function neighbor(hex: Hex, dir: number): Hex {
  const table = hex.row & 1 ? ODD_ROW : EVEN_ROW;
  const [dc, dr] = table[((dir % 6) + 6) % 6] as readonly [number, number];
  return { col: hex.col + dc, row: hex.row + dr };
}

/** Direction from a hex to an adjacent hex, or -1 if they are not neighbours. */
export function directionTo(from: Hex, to: Hex): number {
  for (let dir = 0; dir < DIR_COUNT; dir++) {
    const n = neighbor(from, dir);
    if (n.col === to.col && n.row === to.row) return dir;
  }
  return -1;
}

export function dirVector(dir: number): Vec2 {
  const angle = (Math.PI / 3) * dir;
  return { x: Math.cos(angle), z: Math.sin(angle) };
}

export function hexCenter(hex: Hex): Vec2 {
  return { x: HEX_WIDTH * (hex.col + 0.5 * (hex.row & 1)), z: HEX_ROW_PITCH * hex.row };
}

export function hexAt(x: number, z: number): Hex {
  const q = ((Math.sqrt(3) / 3) * x - z / 3) / HEX_RADIUS;
  const r = ((2 / 3) * z) / HEX_RADIUS;
  let rx = Math.round(q);
  let rz = Math.round(r);
  const ry = Math.round(-q - r);
  const dx = Math.abs(rx - q);
  const dz = Math.abs(rz - r);
  const dy = Math.abs(ry - (-q - r));
  if (dx > dy && dx > dz) rx = -ry - rz;
  else if (dz > dy) rz = -rx - ry;
  // `+ 0` turns -0 into 0 so hexes compare and print cleanly.
  return { col: rx + (rz - (rz & 1)) / 2 + 0, row: rz + 0 };
}

export function hexKey(hex: Hex): string {
  return `${hex.col},${hex.row}`;
}

export function hexDistance(a: Hex, b: Hex): number {
  const aq = a.col - (a.row - (a.row & 1)) / 2;
  const bq = b.col - (b.row - (b.row & 1)) / 2;
  const dq = aq - bq;
  const dr = a.row - b.row;
  return (Math.abs(dq) + Math.abs(dr) + Math.abs(dq + dr)) / 2;
}

export function maskOf(dirs: readonly number[]): number {
  let mask = 0;
  for (const dir of dirs) mask |= 1 << dir;
  return mask;
}

export function maskHas(mask: number, dir: number): boolean {
  return (mask & (1 << dir)) !== 0;
}

/** Rotates an edge mask so that direction k becomes k + steps. */
export function rotateMask(mask: number, steps: number): number {
  const s = ((steps % 6) + 6) % 6;
  return ((mask << s) | (mask >> (6 - s))) & 0b111111;
}
