/**
 * Small seeded RNG (mulberry32). The state is a single uint32 so it can live in
 * the sim's JSON state; every function takes the holder and advances it.
 */
export interface RngHolder {
  rng: number;
}

export function nextFloat(holder: RngHolder): number {
  let t = (holder.rng = (holder.rng + 0x6d2b79f5) >>> 0);
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}

export function range(holder: RngHolder, min: number, max: number): number {
  return min + nextFloat(holder) * (max - min);
}

/** Integer in [min, max] inclusive. */
export function intRange(holder: RngHolder, min: number, max: number): number {
  return Math.floor(range(holder, min, max + 1));
}

export function chance(holder: RngHolder, probability: number): boolean {
  return nextFloat(holder) < probability;
}

export function pick<T>(holder: RngHolder, items: readonly T[]): T {
  if (items.length === 0) throw new Error('pick() from empty list');
  return items[Math.floor(nextFloat(holder) * items.length)] as T;
}

export function weightedPick<T>(holder: RngHolder, items: readonly T[], weight: (item: T) => number): T | null {
  let total = 0;
  for (const item of items) total += Math.max(0, weight(item));
  if (total <= 0) return null;
  let roll = nextFloat(holder) * total;
  for (const item of items) {
    roll -= Math.max(0, weight(item));
    if (roll <= 0) return item;
  }
  return items[items.length - 1] ?? null;
}

export function hashString(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}
