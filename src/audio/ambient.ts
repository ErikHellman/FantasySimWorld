import { isNight } from '../core/time';
import type { AmbientCounts, House, WireEvent } from '../generated/api';
import { worldMap } from '../world/map';
import { findPlace, getPlace } from '../world/places';
import type { AudioEngine } from './engine';

interface Listener {
  x: number;
  z: number;
  /** camera distance from the ground point it looks at */
  distance: number;
}

const forestCells = worldMap.cells.filter((cell) => cell.terrain === 'forest' || cell.terrain === 'trees');
const riverCells = worldMap.cells.filter((cell) => cell.riverMask !== 0);
const lakeCells = worldMap.cells.filter((cell) => cell.terrain === 'water');

function nearest(cells: typeof worldMap.cells, x: number, z: number): number {
  let best = Infinity;
  for (const cell of cells) best = Math.min(best, Math.hypot(cell.center.x - x, cell.center.z - z));
  return best;
}

/** 1 when on top of something, fading to 0 at `range`. */
function near(distance: number, range: number): number {
  return Math.max(0, Math.min(1, 1 - distance / range));
}

function placeDistance(id: string, listener: Listener): number {
  const place = getPlace(id);
  return Math.hypot(place.origin.x - listener.x, place.origin.z - listener.z);
}

/** How loud the nearest burning house crackles: louder the bigger the fire, and heard from further than a campfire. */
function blaze(houses: readonly House[], listener: Listener): number {
  let loudest = 0;
  for (const house of houses) {
    if (house.phase !== 'burning' || !findPlace(house.id)) continue;
    loudest = Math.max(loudest, near(placeDistance(house.id, listener), 70) * (0.5 + house.fire / 200));
  }
  return loudest;
}

/**
 * Decides how loud each ambient loop is from where the camera is looking, how
 * far it is zoomed out, the time of day, what the villagers are doing (the
 * server's counts, from `status`) and whether a house is on fire.
 */
export function ambientLevels(counts: AmbientCounts, hour: number, listener: Listener, houses: readonly House[] = []): Record<string, number> {
  const closeness = Math.max(0.25, Math.min(1, 1.35 - listener.distance / 130));
  const dark = isNight(hour) ? 1 : hour > 19.5 ? (hour - 19.5) / 1.5 : hour < 6.5 ? (6.5 - hour) / 1 : 0;
  const day = 1 - Math.min(1, dark);
  const awake = counts.awake;
  const atTavern = counts.eating + counts.drinking;
  return {
    birds_day: day * 0.8 * (0.5 + 0.5 * near(nearest(forestCells, listener.x, listener.z), 60)),
    wind: 0.25 + 0.5 * (1 - closeness),
    forest: near(nearest(forestCells, listener.x, listener.z), 30) * closeness * (0.6 + 0.4 * day),
    river: near(nearest(riverCells, listener.x, listener.z), 32) * closeness,
    shore: near(nearest(lakeCells, listener.x, listener.z), 30) * closeness,
    crickets_night: Math.min(1, dark) * 0.9,
    village_day: day * near(placeDistance('green', listener), 70) * Math.min(1, awake / 8) * closeness,
    tavern: near(placeDistance('tavern', listener), 32) * Math.min(1, atTavern / 3) * closeness,
    smithy: near(placeDistance('smithy', listener), 32) * Math.min(1, counts.smithing) * closeness,
    // The crackle of the glade's campfire is also the sound of a house burning.
    campfire: Math.max(near(placeDistance('glade', listener), 26) * closeness * (counts.relaxing > 0 || dark > 0.5 ? 1 : 0.4), blaze(houses, listener)),
  };
}

/** Plays effects for a tick's events, quieter the further they are from where the camera looks. */
export function playEffects(engine: AudioEngine, events: readonly WireEvent[], isVillager: (id: string) => boolean, listener: Listener): void {
  const volumeAt = (x: number, z: number): number => near(Math.hypot(x - listener.x, z - listener.z), 30 + listener.distance);
  for (const event of events) {
    if (event.type === 'attack') {
      if (event.ranged === 'magic') engine.play('magic_cast', volumeAt(event.from.x, event.from.z));
      else if (event.ranged) engine.play('arrow_shot', volumeAt(event.from.x, event.from.z));
    } else if (event.type === 'hit') {
      engine.play('sword_hit', volumeAt(event.pos.x, event.pos.z));
      if (isVillager(event.targetId)) engine.play('hurt', volumeAt(event.pos.x, event.pos.z) * 0.7);
    } else if (event.type === 'death') {
      engine.play(event.monster ? 'monster_death' : 'death', volumeAt(event.pos.x, event.pos.z));
    } else if (event.type === 'monster_spawn') {
      engine.play('monster_spawn', volumeAt(event.pos.x, event.pos.z));
    } else if (event.type === 'speech' && event.loud && (event.text.startsWith('To arms') || event.text.startsWith('Fire!'))) {
      engine.play('alarm', 0.9);
    } else if (event.type === 'ceremony' && event.ceremony.kind === 'wedding' && event.ceremony.phase === 'rite') {
      // The same church bells as the alarm, rung for a happier reason.
      engine.play('alarm', 0.9);
    }
  }
}
