import { rotate, type Vec2 } from '../core/math2d';
import { assetManifest } from '../generated/assetManifest';
import { HEX_SCALE, hexCenter } from './hex';
import { PLACE_DEFS, type ActivityId, type PlaceDef, type PlaceKind } from './placeDefs';

export type { ActivityId, PlaceKind } from './placeDefs';

const DEG = Math.PI / 180;

export interface PlacedModel {
  bundle: string;
  model: string;
  x: number;
  y: number;
  z: number;
  /** rotation about y, radians */
  rotation: number;
  scale: number;
}

/** A solid thing characters walk around: a rotated box or a circle. */
export type Obstacle =
  | { kind: 'box'; x: number; z: number; halfX: number; halfZ: number; rotation: number }
  | { kind: 'circle'; x: number; z: number; radius: number };

export interface Slot {
  id: string;
  placeId: string;
  activities: readonly ActivityId[];
  pos: Vec2;
  facing: number;
  anim: string | null;
  ranged: boolean;
}

export interface Place {
  id: string;
  name: string;
  kind: PlaceKind;
  description: string;
  origin: Vec2;
  facing: number;
  /** where characters enter and leave, and where "go to this place" ends */
  door: Vec2;
  beds: number;
  clear: number;
  models: PlacedModel[];
  obstacles: Obstacle[];
  slots: Slot[];
  gravePlots: Array<{ pos: Vec2; facing: number }>;
  spawnPoints: Vec2[];
}

function modelBounds(ref: string): { bundle: string; model: string; bounds: number[] } {
  const [bundle, model] = ref.split('/') as [string, string];
  const entry = assetManifest.bundles[bundle]?.models[model];
  if (!entry) throw new Error(`Place refers to unknown model ${ref}`);
  return { bundle, model, bounds: entry.bounds };
}

function resolve(def: PlaceDef): Place {
  const centre = hexCenter({ col: def.hex[0], row: def.hex[1] });
  const origin = { x: centre.x + (def.offset?.[0] ?? 0), z: centre.z + (def.offset?.[1] ?? 0) };
  const facing = def.facing * DEG;
  const toWorld = (local: readonly [number, number]): Vec2 => {
    const r = rotate({ x: local[0], z: local[1] }, facing);
    return { x: origin.x + r.x, z: origin.z + r.z };
  };

  const models: PlacedModel[] = [];
  const obstacles: Obstacle[] = [];

  if (def.building) {
    const { bundle, model, bounds } = modelBounds(def.building.model);
    const scale = def.building.scale ?? HEX_SCALE;
    const at = def.building.at ?? [0, 0];
    const rotation = facing + (def.building.rot ?? 0) * DEG;
    // The footprint box is centred on the model's bounds, which are not always centred on its origin.
    const cx = ((bounds[0]! + bounds[3]!) / 2) * scale;
    const cz = ((bounds[2]! + bounds[5]!) / 2) * scale;
    const pos = toWorld(at);
    const centreOffset = rotate({ x: cx, z: cz }, rotation);
    models.push({ bundle, model, x: pos.x, y: 0, z: pos.z, rotation, scale });
    obstacles.push({
      kind: 'box',
      x: pos.x + centreOffset.x,
      z: pos.z + centreOffset.z,
      halfX: ((bounds[3]! - bounds[0]!) / 2) * scale * 0.94,
      halfZ: ((bounds[5]! - bounds[2]!) / 2) * scale * 0.94,
      rotation,
    });
  }

  for (const prop of def.props ?? []) {
    const { bundle, model, bounds } = modelBounds(prop.model);
    const scale = prop.scale ?? 1;
    const pos = toWorld(prop.at);
    const rotation = facing + (prop.rot ?? 0) * DEG;
    models.push({ bundle, model, x: pos.x, y: prop.y ?? 0, z: pos.z, rotation, scale });
    const width = (bounds[3]! - bounds[0]!) * scale;
    const depth = (bounds[5]! - bounds[2]!) * scale;
    const height = (bounds[4]! - bounds[1]!) * scale;
    if (prop.block !== undefined) {
      if (prop.block > 0) obstacles.push({ kind: 'circle', x: pos.x, z: pos.z, radius: prop.block });
    } else if (height >= 0.6 && Math.max(width, depth) >= 0.9) {
      if (Math.max(width, depth) > 2.4) {
        const c = rotate({ x: ((bounds[0]! + bounds[3]!) / 2) * scale, z: ((bounds[2]! + bounds[5]!) / 2) * scale }, rotation);
        obstacles.push({ kind: 'box', x: pos.x + c.x, z: pos.z + c.z, halfX: width / 2, halfZ: depth / 2, rotation });
      } else {
        obstacles.push({ kind: 'circle', x: pos.x, z: pos.z, radius: Math.max(width, depth) * 0.45 });
      }
    }
  }

  const slots: Slot[] = (def.slots ?? []).map((slot, index) => ({
    id: `${def.id}#${index}`,
    placeId: def.id,
    activities: slot.activities,
    pos: toWorld(slot.at),
    facing: facing + slot.face * DEG,
    anim: slot.anim ?? null,
    ranged: slot.ranged ?? false,
  }));

  const door = toWorld(def.door ?? [0, 0]);
  return {
    id: def.id,
    name: def.name,
    kind: def.kind,
    description: def.description,
    origin,
    facing,
    door,
    beds: def.beds ?? 0,
    clear: def.clear ?? 8,
    models,
    obstacles,
    slots,
    gravePlots: (def.gravePlots ?? []).map((p) => ({ pos: toWorld(p), facing })),
    spawnPoints: (def.spawnPoints ?? []).map(toWorld),
  };
}

export const places: readonly Place[] = PLACE_DEFS.map(resolve);

const placeById = new Map(places.map((place) => [place.id, place]));
const slotById = new Map(places.flatMap((place) => place.slots.map((slot) => [slot.id, slot] as const)));

export function getPlace(id: string): Place {
  const place = placeById.get(id);
  if (!place) throw new Error(`Unknown place ${id}`);
  return place;
}

export function findPlace(id: string): Place | null {
  return placeById.get(id) ?? null;
}

export function getSlot(id: string): Slot {
  const slot = slotById.get(id);
  if (!slot) throw new Error(`Unknown slot ${id}`);
  return slot;
}

export function slotsFor(activity: ActivityId): Slot[] {
  return places.flatMap((place) => place.slots.filter((slot) => slot.activities.includes(activity)));
}

export const homes: readonly Place[] = places.filter((place) => place.kind === 'home');

/** The place whose origin is nearest to a point, within `maxDistance`. */
export function nearestPlace(pos: Vec2, maxDistance = 14): Place | null {
  let best: Place | null = null;
  let bestDistance = maxDistance;
  for (const place of places) {
    const d = Math.hypot(place.origin.x - pos.x, place.origin.z - pos.z);
    if (d < bestDistance) {
      bestDistance = d;
      best = place;
    }
  }
  return best;
}

export function obstacleContains(obstacle: Obstacle, x: number, z: number, margin: number): boolean {
  if (obstacle.kind === 'circle') {
    const r = obstacle.radius + margin;
    const dx = x - obstacle.x;
    const dz = z - obstacle.z;
    return dx * dx + dz * dz <= r * r;
  }
  // Into the box frame: the inverse of rotate().
  const dx = x - obstacle.x;
  const dz = z - obstacle.z;
  const cos = Math.cos(obstacle.rotation);
  const sin = Math.sin(obstacle.rotation);
  const lx = dx * cos - dz * sin;
  const lz = dx * sin + dz * cos;
  return Math.abs(lx) <= obstacle.halfX + margin && Math.abs(lz) <= obstacle.halfZ + margin;
}
