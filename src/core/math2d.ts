/** Ground-plane vector. The sim is 2D: x is east, z is south, y (up) is render-only. */
export interface Vec2 {
  x: number;
  z: number;
}

export const vec = (x: number, z: number): Vec2 => ({ x, z });

export function dist(a: Vec2, b: Vec2): number {
  return Math.hypot(a.x - b.x, a.z - b.z);
}

export function distSq(a: Vec2, b: Vec2): number {
  const dx = a.x - b.x;
  const dz = a.z - b.z;
  return dx * dx + dz * dz;
}

export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

export function clamp(value: number, min: number, max: number): number {
  return value < min ? min : value > max ? max : value;
}

/** Facing angle (radians) for a direction; 0 faces +z, matching the KayKit models. */
export function angleOf(dx: number, dz: number): number {
  return Math.atan2(dx, dz);
}

export function facingTo(from: Vec2, to: Vec2): number {
  return angleOf(to.x - from.x, to.z - from.z);
}

export function wrapAngle(a: number): number {
  let r = a % (Math.PI * 2);
  if (r > Math.PI) r -= Math.PI * 2;
  if (r < -Math.PI) r += Math.PI * 2;
  return r;
}

export function turnToward(current: number, target: number, maxStep: number): number {
  const diff = wrapAngle(target - current);
  if (Math.abs(diff) <= maxStep) return target;
  return wrapAngle(current + Math.sign(diff) * maxStep);
}

export function lerpAngle(a: number, b: number, t: number): number {
  return a + wrapAngle(b - a) * t;
}

/** Rotates a local offset by a facing angle (0 = +z forward, +x is the local right). */
export function rotate(offset: Vec2, facing: number): Vec2 {
  const s = Math.sin(facing);
  const c = Math.cos(facing);
  return { x: offset.x * c + offset.z * s, z: -offset.x * s + offset.z * c };
}

export function distToSegment(p: Vec2, a: Vec2, b: Vec2): number {
  const abx = b.x - a.x;
  const abz = b.z - a.z;
  const lenSq = abx * abx + abz * abz;
  const t = lenSq === 0 ? 0 : clamp(((p.x - a.x) * abx + (p.z - a.z) * abz) / lenSq, 0, 1);
  return Math.hypot(p.x - (a.x + abx * t), p.z - (a.z + abz * t));
}
