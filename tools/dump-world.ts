// Writes the world layer's output as golden fixtures for the Rust port
// (server/crates/world/tests/golden). The TypeScript world is the reference:
// the Rust world must reproduce these files exactly (categorical data, nav
// costs and paths) or to 1e-9 (coordinates).
//
//   bun tools/dump-world.ts [outDir]
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import type { Vec2 } from '../src/core/math2d';
import { nextFloat, pick, range, type RngHolder } from '../src/core/rng';
import { hexAt } from '../src/world/hex';
import { groundAt, isOnRoad, roadDistance, worldMap } from '../src/world/map';
import { CELL, navGrid, nearestWalkable, worldHash } from '../src/world/navgrid';
import { findPath, findRoute } from '../src/world/pathfind';
import { places } from '../src/world/places';
import { scatter } from '../src/world/scatter';

const OUT = resolve(process.argv[2] ?? join(import.meta.dirname, '..', 'server/crates/world/tests/golden'));
/** Sample spacing for the ground and road masks. */
const GROUND_STEP = 0.5;
const PATH_PAIRS = 300;
const POINT_SAMPLES = 2000;
/** Fixed seed for picking sample points and pairs; unrelated to any world seed. */
const FIXTURE_SEED = 7_000_001;

const GROUND_CHAR = { void: 'v', blocked: 'b', water: 'w', open: 'o' } as const;

function write(name: string, value: unknown): void {
  const path = join(OUT, name);
  writeFileSync(path, `${JSON.stringify(value)}\n`);
  console.log(`${name}: ${(JSON.stringify(value).length / 1024).toFixed(0)} KB`);
}

function randomPoints(rng: RngHolder, count: number): Vec2[] {
  const points: Vec2[] = [];
  for (let i = 0; i < count; i++) {
    points.push({ x: range(rng, worldMap.minX - 4, worldMap.maxX + 4), z: range(rng, worldMap.minZ - 4, worldMap.maxZ + 4) });
  }
  return points;
}

mkdirSync(OUT, { recursive: true });
const rng: RngHolder = { rng: FIXTURE_SEED };

// Hexes: terrain, edge masks, tile choice per cell, plus point -> hex lookups.
write('map.json', {
  ...worldMap,
  hexAt: randomPoints(rng, POINT_SAMPLES).map((p) => ({ ...p, hex: hexAt(p.x, p.z) })),
});

// What the terrain allows, and where the roads run, on a regular grid.
{
  const originX = Math.floor(worldMap.minX) - 1;
  const originZ = Math.floor(worldMap.minZ) - 1;
  const nx = Math.ceil((worldMap.maxX + 1 - originX) / GROUND_STEP);
  const nz = Math.ceil((worldMap.maxZ + 1 - originZ) / GROUND_STEP);
  let ground = '';
  let road = '';
  for (let iz = 0; iz < nz; iz++) {
    for (let ix = 0; ix < nx; ix++) {
      const x = originX + ix * GROUND_STEP;
      const z = originZ + iz * GROUND_STEP;
      ground += GROUND_CHAR[groundAt(x, z)];
      road += isOnRoad(x, z) ? '1' : '0';
    }
  }
  const roadDistances = randomPoints(rng, POINT_SAMPLES)
    .map((p) => ({ ...p, d: roadDistance(p.x, p.z) }))
    // JSON has no Infinity; points off any road hex are left out.
    .filter((s) => Number.isFinite(s.d));
  write('ground.json', { step: GROUND_STEP, originX, originZ, nx, nz, legend: GROUND_CHAR, ground, road, roadDistances });
}

// Nav grid costs as raw little-endian f32 bit patterns.
write('navgrid.json', {
  cell: CELL,
  originX: navGrid.originX,
  originZ: navGrid.originZ,
  width: navGrid.width,
  height: navGrid.height,
  walkable: navGrid.cost.reduce((n, c) => n + (c > 0 ? 1 : 0), 0),
  hash: worldHash(),
  costF32Base64: Buffer.from(navGrid.cost.buffer, navGrid.cost.byteOffset, navGrid.cost.byteLength).toString('base64'),
});

write('places.json', places);
write('scatter.json', scatter);

// Routes between doors, activity slots and spawn points, raw and smoothed, plus nearest-walkable lookups.
{
  const targets: Array<{ label: string; pos: Vec2 }> = [];
  for (const place of places) {
    targets.push({ label: `${place.id}:door`, pos: place.door });
    for (const slot of place.slots) targets.push({ label: slot.id, pos: slot.pos });
    place.spawnPoints.forEach((pos, i) => targets.push({ label: `${place.id}:spawn${i}`, pos }));
  }
  const paths = [];
  for (let i = 0; i < PATH_PAIRS; i++) {
    const from = pick(rng, targets);
    const to = pick(rng, targets);
    paths.push({ from: from.label, to: to.label, fromPos: from.pos, toPos: to.pos, route: findRoute(from.pos, to.pos), path: findPath(from.pos, to.pos) });
  }
  // A few routes from random points, which exercise nearestWalkable's ring search and unreachable cases.
  for (let i = 0; i < 40; i++) {
    const from = { x: range(rng, worldMap.minX, worldMap.maxX), z: range(rng, worldMap.minZ, worldMap.maxZ) };
    const to = pick(rng, targets);
    paths.push({ from: 'random', to: to.label, fromPos: from, toPos: to.pos, route: findRoute(from, to.pos), path: findPath(from, to.pos) });
  }
  const nearest = randomPoints(rng, POINT_SAMPLES).map((p) => {
    const maxRadius = 1 + Math.floor(nextFloat(rng) * 8);
    return { ...p, maxRadius, nearest: nearestWalkable(p, maxRadius) };
  });
  write('paths.json', { pairs: paths, nearest });
}
