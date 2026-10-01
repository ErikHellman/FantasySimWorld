import { dist, facingTo, turnToward, type Vec2 } from '../../core/math2d';
import { TICK_SECONDS } from '../../core/time';
import { isWalkable } from '../../world/navgrid';
import { findPath } from '../../world/pathfind';
import type { SimState } from '../state';
import { living, monsterList } from '../util';

const TURN_RATE = 9; // radians per second
const BODY_RADIUS = 0.55;

interface Mover {
  pos: Vec2;
  facing: number;
  moving: boolean;
}

/** Route to a point; an empty array means "already there", null means unreachable. */
export function routeTo(from: Vec2, to: Vec2): Vec2[] | null {
  if (dist(from, to) < 0.05) return [];
  return findPath(from, to);
}

export function face(mover: Mover, target: Vec2): void {
  if (dist(mover.pos, target) < 0.01) return;
  mover.facing = turnToward(mover.facing, facingTo(mover.pos, target), TURN_RATE * TICK_SECONDS);
}

/**
 * Moves along a path at `speed` units per second, consuming waypoints.
 * Returns true once the path is finished.
 */
export function advance(mover: Mover, path: Vec2[], speed: number): boolean {
  let budget = speed * TICK_SECONDS;
  while (budget > 0 && path.length > 0) {
    const next = path[0] as Vec2;
    const d = dist(mover.pos, next);
    if (d <= budget) {
      mover.pos = { x: next.x, z: next.z };
      path.shift();
      budget -= d;
    } else {
      face(mover, next);
      mover.pos = { x: mover.pos.x + ((next.x - mover.pos.x) / d) * budget, z: mover.pos.z + ((next.z - mover.pos.z) / d) * budget };
      budget = 0;
    }
    mover.moving = true;
  }
  return path.length === 0;
}

/** Steps straight towards a point (used for the last stretch onto a seat or up to a person). */
export function stepToward(mover: Mover, target: Vec2, speed: number): boolean {
  const d = dist(mover.pos, target);
  const step = speed * TICK_SECONDS;
  if (d <= step) {
    mover.pos = { x: target.x, z: target.z };
    mover.moving = d > 0.001;
    return true;
  }
  face(mover, target);
  mover.pos = { x: mover.pos.x + ((target.x - mover.pos.x) / d) * step, z: mover.pos.z + ((target.z - mover.pos.z) / d) * step };
  mover.moving = true;
  return false;
}

/** Nudges moving bodies apart so walkers do not pass through each other. */
export function separate(state: SimState): void {
  const bodies: Mover[] = [];
  for (const c of living(state)) if (!c.inside) bodies.push(c);
  for (const m of monsterList(state)) if (m.phase !== 'dying') bodies.push(m);
  for (const mover of bodies) {
    if (!mover.moving) continue;
    let pushX = 0;
    let pushZ = 0;
    for (const other of bodies) {
      if (other === mover) continue;
      const dx = mover.pos.x - other.pos.x;
      const dz = mover.pos.z - other.pos.z;
      const d = Math.hypot(dx, dz);
      if (d >= BODY_RADIUS * 2 || d < 1e-4) continue;
      const overlap = BODY_RADIUS * 2 - d;
      pushX += (dx / d) * overlap * 0.35;
      pushZ += (dz / d) * overlap * 0.35;
    }
    if (pushX === 0 && pushZ === 0) continue;
    const x = mover.pos.x + pushX;
    const z = mover.pos.z + pushZ;
    if (isWalkable(x, z)) mover.pos = { x, z };
  }
}
