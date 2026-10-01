import type { Vec2 } from '../core/math2d';
import { CELL, cellCenter, cellIndex, lineOfWalk, navGrid, nearestWalkable } from './navgrid';

const SQRT2 = Math.SQRT2;
const NEIGHBOURS: ReadonlyArray<readonly [number, number, number]> = [
  [1, 0, 1], [-1, 0, 1], [0, 1, 1], [0, -1, 1],
  [1, 1, SQRT2], [1, -1, SQRT2], [-1, 1, SQRT2], [-1, -1, SQRT2],
];

// Scratch buffers reused across searches; `stamp` marks which entries belong to the current one.
const size = navGrid.width * navGrid.height;
const gScore = new Float32Array(size);
const cameFrom = new Int32Array(size);
const visited = new Uint32Array(size);
const closed = new Uint32Array(size);
let stamp = 0;

/** Binary min-heap of cell indices keyed by f-score. */
class Heap {
  private readonly items: number[] = [];
  private readonly keys: number[] = [];

  get length(): number {
    return this.items.length;
  }

  push(item: number, key: number): void {
    let i = this.items.length;
    this.items.push(item);
    this.keys.push(key);
    while (i > 0) {
      const parent = (i - 1) >> 1;
      if ((this.keys[parent] as number) <= key) break;
      this.items[i] = this.items[parent] as number;
      this.keys[i] = this.keys[parent] as number;
      i = parent;
    }
    this.items[i] = item;
    this.keys[i] = key;
  }

  pop(): number {
    const top = this.items[0] as number;
    const last = this.items.pop() as number;
    const lastKey = this.keys.pop() as number;
    const n = this.items.length;
    if (n > 0) {
      let i = 0;
      for (;;) {
        let child = 2 * i + 1;
        if (child >= n) break;
        if (child + 1 < n && (this.keys[child + 1] as number) < (this.keys[child] as number)) child++;
        if ((this.keys[child] as number) >= lastKey) break;
        this.items[i] = this.items[child] as number;
        this.keys[i] = this.keys[child] as number;
        i = child;
      }
      this.items[i] = last;
      this.keys[i] = lastKey;
    }
    return top;
  }
}

/**
 * A* over the nav grid, then string-pulled into a few straight legs.
 * Returns waypoints from (but not including) `from` to the reachable point
 * nearest `to`, or null when there is no route.
 */
export function findPath(from: Vec2, to: Vec2): Vec2[] | null {
  const start = nearestWalkable(from);
  const goal = nearestWalkable(to);
  if (!start || !goal) return null;
  const startIndex = cellIndex(start.x, start.z);
  const goalIndex = cellIndex(goal.x, goal.z);
  if (startIndex < 0 || goalIndex < 0) return null;
  if (startIndex === goalIndex || lineOfWalk(start, goal)) return [goal];

  stamp++;
  const { width, height, cost } = navGrid;
  const goalX = goalIndex % width;
  const goalZ = (goalIndex - goalX) / width;
  const heuristic = (index: number): number => {
    const dx = Math.abs((index % width) - goalX);
    const dz = Math.abs(Math.floor(index / width) - goalZ);
    return Math.max(dx, dz) + (SQRT2 - 1) * Math.min(dx, dz);
  };

  const open = new Heap();
  gScore[startIndex] = 0;
  visited[startIndex] = stamp;
  cameFrom[startIndex] = -1;
  open.push(startIndex, heuristic(startIndex));

  let found = false;
  while (open.length > 0) {
    const current = open.pop();
    if (current === goalIndex) {
      found = true;
      break;
    }
    if (closed[current] === stamp) continue;
    closed[current] = stamp;
    const cx = current % width;
    const cz = (current - cx) / width;
    for (const [dx, dz, step] of NEIGHBOURS) {
      const nx = cx + dx;
      const nz = cz + dz;
      if (nx < 0 || nz < 0 || nx >= width || nz >= height) continue;
      const next = nz * width + nx;
      const cellCost = cost[next] as number;
      if (cellCost <= 0 || closed[next] === stamp) continue;
      // No cutting corners between two blocked cells.
      if (dx !== 0 && dz !== 0 && ((cost[cz * width + nx] as number) <= 0 || (cost[nz * width + cx] as number) <= 0)) continue;
      const tentative = (gScore[current] as number) + step * cellCost;
      if (visited[next] === stamp && tentative >= (gScore[next] as number)) continue;
      visited[next] = stamp;
      gScore[next] = tentative;
      cameFrom[next] = current;
      open.push(next, tentative + heuristic(next));
    }
  }
  if (!found) return null;

  const cells: Vec2[] = [];
  for (let index = goalIndex; index !== -1; index = cameFrom[index] as number) cells.push(cellCenter(index));
  cells.reverse();
  cells[cells.length - 1] = goal;
  return smooth(start, cells);
}

/** Drops waypoints that can be skipped in a straight walkable line. */
function smooth(start: Vec2, cells: Vec2[]): Vec2[] {
  const result: Vec2[] = [];
  let anchor = start;
  let i = 0;
  while (i < cells.length) {
    // Furthest waypoint visible from the anchor (look ahead a bounded distance to keep this cheap).
    let furthest = i;
    const limit = Math.min(cells.length - 1, i + 40);
    for (let j = limit; j > i; j--) {
      if (lineOfWalk(anchor, cells[j] as Vec2)) {
        furthest = j;
        break;
      }
    }
    anchor = cells[furthest] as Vec2;
    result.push(anchor);
    i = furthest + 1;
  }
  return result;
}

export function pathLength(from: Vec2, path: readonly Vec2[]): number {
  let total = 0;
  let previous = from;
  for (const point of path) {
    total += Math.hypot(point.x - previous.x, point.z - previous.z);
    previous = point;
  }
  return total;
}

export { CELL };
