import { distSq, type Vec2 } from '../core/math2d';
import { chance, intRange, nextFloat, pick, range, type RngHolder } from '../core/rng';
import { assetManifest } from '../generated/assetManifest';
import { HEX_APOTHEM, HEX_RADIUS, HEX_SCALE, hexAt } from './hex';
import { SCATTER_SEED } from './layout';
import { groundAt, roadDistance, worldMap, type HexCell } from './map';
import { places, type Obstacle, type PlacedModel } from './places';

export interface Scatter {
  models: PlacedModel[];
  obstacles: Obstacle[];
}

const TREES = [
  'Tree_1_A', 'Tree_1_B', 'Tree_1_C', 'Tree_2_A', 'Tree_2_B', 'Tree_2_C', 'Tree_2_D', 'Tree_3_A', 'Tree_3_B',
  'Tree_4_A', 'Tree_4_B', 'Tree_5_A', 'Tree_5_B', 'Tree_5_C', 'Tree_6_A', 'Tree_6_B', 'Tree_7_A', 'Tree_7_B',
];
const MEADOW_TREES = ['Tree_1_A', 'Tree_1_B', 'Tree_2_A', 'Tree_2_B', 'Tree_5_A', 'Tree_6_A', 'Tree_7_A'];
const BUSHES = ['Bush_1_A', 'Bush_1_B', 'Bush_1_C', 'Bush_2_A', 'Bush_2_B', 'Bush_3_A', 'Bush_3_B', 'Bush_4_A', 'Bush_4_B', 'Bush_4_C'];
const ROCKS = ['Rock_1_A', 'Rock_1_B', 'Rock_1_C', 'Rock_2_A', 'Rock_2_B', 'Rock_3_A', 'Rock_3_B', 'Rock_3_C', 'Rock_4_A', 'Rock_5_A', 'Rock_6_A'];
const GRASS = ['Grass_1_A', 'Grass_1_B', 'Grass_1_C', 'Grass_1_D', 'Grass_2_A', 'Grass_2_B', 'Grass_2_C', 'Grass_2_D'];
const FLOWERS = ['flax_flower_A', 'flax_flower_B'];
const WATER_PLANTS = ['waterlily_A', 'waterlily_B', 'waterplant_A', 'waterplant_B', 'waterplant_C'];

function sizeOf(bundle: string, model: string): { width: number; height: number } {
  const bounds = assetManifest.bundles[bundle]?.models[model]?.bounds;
  if (!bounds) throw new Error(`Scatter refers to unknown model ${bundle}/${model}`);
  return { width: Math.max(bounds[3]! - bounds[0]!, bounds[5]! - bounds[2]!), height: bounds[4]! - bounds[1]! };
}

function randomPointInHex(rng: RngHolder, cell: HexCell): Vec2 {
  for (;;) {
    const x = cell.center.x + range(rng, -HEX_APOTHEM, HEX_APOTHEM);
    const z = cell.center.z + range(rng, -HEX_RADIUS, HEX_RADIUS);
    const hex = hexAt(x, z);
    if (hex.col === cell.col && hex.row === cell.row) return { x, z };
  }
}

function nearPlace(p: Vec2, extra: number): boolean {
  for (const place of places) {
    const r = place.clear + extra;
    if (distSq(p, place.origin) < r * r) return true;
  }
  return false;
}

/** Deterministic decoration: the same world every time, independent of the sim seed. */
function buildScatter(): Scatter {
  const rng: RngHolder = { rng: SCATTER_SEED };
  const models: PlacedModel[] = [];
  const obstacles: Obstacle[] = [];
  const solid: Array<{ pos: Vec2; radius: number }> = [];

  const tooClose = (p: Vec2, radius: number): boolean =>
    solid.some((other) => distSq(p, other.pos) < (radius + other.radius) ** 2);

  const place = (
    cell: HexCell, bundle: string, list: readonly string[], count: number,
    options: { spacing: number; roadGap: number; solid: boolean; scale?: [number, number]; placeGap?: number },
  ): void => {
    for (let i = 0; i < count; i++) {
      for (let attempt = 0; attempt < 8; attempt++) {
        const p = randomPointInHex(rng, cell);
        if (groundAt(p.x, p.z) !== 'open') continue;
        if (roadDistance(p.x, p.z) < options.roadGap) continue;
        if (nearPlace(p, options.placeGap ?? 0)) continue;
        if (options.solid && tooClose(p, options.spacing)) continue;
        const model = pick(rng, list);
        const scale = options.scale ? range(rng, options.scale[0], options.scale[1]) : 1;
        models.push({ bundle, model, x: p.x, y: 0, z: p.z, rotation: nextFloat(rng) * Math.PI * 2, scale });
        if (options.solid) {
          const size = sizeOf(bundle, model);
          solid.push({ pos: p, radius: options.spacing });
          if (size.height * scale > 0.9) {
            // Trunks are much thinner than the canopy; rocks and bushes block most of their width.
            const radius = model.startsWith('Tree') ? 0.7 : Math.min(2.2, size.width * scale * 0.4);
            obstacles.push({ kind: 'circle', x: p.x, z: p.z, radius });
          }
        }
        break;
      }
    }
  };

  for (const cell of worldMap.cells) {
    if (cell.terrain === 'forest') {
      place(cell, 'forest', TREES, intRange(rng, 9, 13), { spacing: 1.7, roadGap: 3.4, solid: true, scale: [0.9, 1.2] });
      place(cell, 'forest', BUSHES, intRange(rng, 3, 6), { spacing: 1.0, roadGap: 2.6, solid: true });
      place(cell, 'forest', ROCKS, intRange(rng, 0, 2), { spacing: 1.0, roadGap: 2.6, solid: true });
      place(cell, 'forest', GRASS, intRange(rng, 6, 10), { spacing: 0, roadGap: 2.0, solid: false });
    } else if (cell.terrain === 'meadow') {
      if (cell.waterMask === 0) {
        place(cell, 'forest', MEADOW_TREES, chance(rng, 0.45) ? intRange(rng, 1, 2) : 0, { spacing: 1.7, roadGap: 4, solid: true, placeGap: 2 });
        place(cell, 'forest', BUSHES, intRange(rng, 0, 3), { spacing: 1.0, roadGap: 2.8, solid: true, placeGap: 1 });
        place(cell, 'props', FLOWERS, intRange(rng, 0, 4), { spacing: 0, roadGap: 2.2, solid: false, placeGap: -2 });
      }
      place(cell, 'forest', ROCKS, chance(rng, 0.35) ? 1 : 0, { spacing: 1.0, roadGap: 2.8, solid: true, placeGap: 1 });
      place(cell, 'forest', GRASS, intRange(rng, 7, 13), { spacing: 0, roadGap: 2.0, solid: false, placeGap: -3 });
    } else if (cell.terrain === 'water') {
      const count = chance(rng, 0.5) ? intRange(rng, 1, 3) : 0;
      for (let i = 0; i < count; i++) {
        const p = randomPointInHex(rng, cell);
        models.push({ bundle: 'hex', model: pick(rng, WATER_PLANTS), x: p.x, y: -0.2 * HEX_SCALE, z: p.z, rotation: nextFloat(rng) * Math.PI * 2, scale: HEX_SCALE });
      }
    }
  }
  return { models, obstacles };
}

export const scatter: Scatter = buildScatter();
