import type { Vec2 } from '../core/math2d';
import { groundAt, isOnRoad, worldMap } from './map';
import { obstacleContains, places, type Obstacle } from './places';
import { scatter } from './scatter';

/** Square navigation grid over the whole map. */
export const CELL = 1;
/** Obstacles are grown by this so a character's body does not clip them. */
export const AGENT_RADIUS = 0.55;

const ROAD_COST = 1;
const GRASS_COST = 1.45;

export interface NavGrid {
  originX: number;
  originZ: number;
  width: number;
  height: number;
  /** 0 = blocked, otherwise the cost of entering the cell */
  cost: Float32Array;
}

function buildNavGrid(): NavGrid {
  const originX = Math.floor(worldMap.minX);
  const originZ = Math.floor(worldMap.minZ);
  const width = Math.ceil((worldMap.maxX - originX) / CELL);
  const height = Math.ceil((worldMap.maxZ - originZ) / CELL);
  const cost = new Float32Array(width * height);

  for (let iz = 0; iz < height; iz++) {
    for (let ix = 0; ix < width; ix++) {
      const x = originX + (ix + 0.5) * CELL;
      const z = originZ + (iz + 0.5) * CELL;
      if (groundAt(x, z) !== 'open') continue;
      // Keep a body-width away from water, cliffs and the map edge too.
      if (
        groundAt(x + AGENT_RADIUS, z) !== 'open' || groundAt(x - AGENT_RADIUS, z) !== 'open' ||
        groundAt(x, z + AGENT_RADIUS) !== 'open' || groundAt(x, z - AGENT_RADIUS) !== 'open'
      ) continue;
      cost[iz * width + ix] = isOnRoad(x, z) ? ROAD_COST : GRASS_COST;
    }
  }

  const block = (obstacle: Obstacle): void => {
    const reach = (obstacle.kind === 'circle' ? obstacle.radius : Math.hypot(obstacle.halfX, obstacle.halfZ)) + AGENT_RADIUS + CELL;
    const x0 = Math.max(0, Math.floor((obstacle.x - reach - originX) / CELL));
    const x1 = Math.min(width - 1, Math.ceil((obstacle.x + reach - originX) / CELL));
    const z0 = Math.max(0, Math.floor((obstacle.z - reach - originZ) / CELL));
    const z1 = Math.min(height - 1, Math.ceil((obstacle.z + reach - originZ) / CELL));
    for (let iz = z0; iz <= z1; iz++) {
      for (let ix = x0; ix <= x1; ix++) {
        const x = originX + (ix + 0.5) * CELL;
        const z = originZ + (iz + 0.5) * CELL;
        if (obstacleContains(obstacle, x, z, AGENT_RADIUS)) cost[iz * width + ix] = 0;
      }
    }
  };
  for (const place of places) place.obstacles.forEach(block);
  scatter.obstacles.forEach(block);

  return { originX, originZ, width, height, cost };
}

export const navGrid: NavGrid = buildNavGrid();

export function cellIndex(x: number, z: number): number {
  const ix = Math.floor((x - navGrid.originX) / CELL);
  const iz = Math.floor((z - navGrid.originZ) / CELL);
  if (ix < 0 || iz < 0 || ix >= navGrid.width || iz >= navGrid.height) return -1;
  return iz * navGrid.width + ix;
}

export function cellCenter(index: number): Vec2 {
  const ix = index % navGrid.width;
  const iz = (index - ix) / navGrid.width;
  return { x: navGrid.originX + (ix + 0.5) * CELL, z: navGrid.originZ + (iz + 0.5) * CELL };
}

export function isWalkable(x: number, z: number): boolean {
  const index = cellIndex(x, z);
  return index >= 0 && (navGrid.cost[index] as number) > 0;
}

/** Nearest walkable cell centre to a point, searching outwards in rings; null if none within `maxRadius` cells. */
export function nearestWalkable(pos: Vec2, maxRadius = 6): Vec2 | null {
  if (isWalkable(pos.x, pos.z)) return pos;
  const cx = Math.floor((pos.x - navGrid.originX) / CELL);
  const cz = Math.floor((pos.z - navGrid.originZ) / CELL);
  let best: Vec2 | null = null;
  let bestDistance = Infinity;
  for (let r = 1; r <= maxRadius; r++) {
    for (let dz = -r; dz <= r; dz++) {
      for (let dx = -r; dx <= r; dx++) {
        if (Math.max(Math.abs(dx), Math.abs(dz)) !== r) continue;
        const ix = cx + dx;
        const iz = cz + dz;
        if (ix < 0 || iz < 0 || ix >= navGrid.width || iz >= navGrid.height) continue;
        if ((navGrid.cost[iz * navGrid.width + ix] as number) <= 0) continue;
        const centre = { x: navGrid.originX + (ix + 0.5) * CELL, z: navGrid.originZ + (iz + 0.5) * CELL };
        const d = (centre.x - pos.x) ** 2 + (centre.z - pos.z) ** 2;
        if (d < bestDistance) {
          bestDistance = d;
          best = centre;
        }
      }
    }
    if (best) return best;
  }
  return null;
}

/** True when the straight line between two points only crosses walkable cells. */
export function lineOfWalk(a: Vec2, b: Vec2): boolean {
  const distance = Math.hypot(b.x - a.x, b.z - a.z);
  const steps = Math.max(1, Math.ceil(distance / (CELL * 0.4)));
  for (let i = 0; i <= steps; i++) {
    const t = i / steps;
    if (!isWalkable(a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t)) return false;
  }
  return true;
}
