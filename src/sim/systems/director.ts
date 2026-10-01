import { MINUTES_PER_DAY, START_MINUTE, TICKS_PER_DAY, TICKS_PER_GAME_MINUTE, minutesToTicks } from '../../core/time';
import { chance, nextFloat, range } from '../../core/rng';
import { nearestWalkable } from '../../world/navgrid';
import { getPlace } from '../../world/places';
import { firstName, chronicle, joinNames, living, monsterList, remember, type Ctx } from '../util';
import { arrive } from './arrivals';
import { randomMonsterKind, spawnMonster } from './monsters';

export const TARGET_POPULATION = 12;
const MAX_WAVE = 5;

/** Tick at which a given hour of a given day begins. */
export function tickAt(day: number, hour: number): number {
  return Math.round(((day - 1) * MINUTES_PER_DAY + hour * 60 - START_MINUTE) * TICKS_PER_GAME_MINUTE);
}

function recentDeaths(ctx: Ctx, days: number): number {
  const since = ctx.state.tick - days * TICKS_PER_DAY;
  return ctx.state.director.deathTicks.filter((tick) => tick >= since).length;
}

function spawnWave(ctx: Ctx): void {
  const { state, time } = ctx;
  const alive = state.order.length;
  const night = time.hour < 12 ? time.day - 1 : time.day;
  let size = 1 + Math.floor(alive / 5) + Math.min(2, Math.floor(night / 5));
  if (night === 1) size = 2;
  if (recentDeaths(ctx, 3) >= 2) size -= 1;
  if (alive < 8) size -= 1;
  size = Math.max(1, Math.min(MAX_WAVE, size));
  size = Math.min(size, 9 - monsterList(state).length);
  if (size <= 0) return;

  const firstOfNight = time.hour >= 19 && time.hour < 22.6;
  const fromRuins = night > 2 && chance(state, 0.25);
  const place = getPlace(fromRuins ? 'ruins' : 'graveyard');
  const points = [...place.spawnPoints];
  const withNecromancer = firstOfNight && night >= 4 && night % 4 === 0 && alive >= 9;

  for (let i = 0; i < size; i++) {
    const point = points.length > 0 ? points.splice(Math.floor(nextFloat(state) * points.length), 1)[0]! : place.origin;
    const pos = nearestWalkable({ x: point.x + range(state, -0.6, 0.6), z: point.z + range(state, -0.6, 0.6) }, 5);
    if (!pos) continue;
    spawnMonster(ctx, withNecromancer && i === 0 ? 'necromancer' : randomMonsterKind(ctx), pos, place.facing);
  }
  if (firstOfNight) {
    chronicle(ctx, 'world', withNecromancer
      ? 'A cold light burns in the graveyard: a necromancer walks among the risen dead tonight.'
      : fromRuins ? 'Something is stirring in the old ruins.' : 'As darkness falls, the graves in the old graveyard begin to stir.');
  }
}

function scheduleNextWave(ctx: Ctx, spawned: boolean): void {
  const { state, time } = ctx;
  const stillDark = time.hour >= 19.5 || time.hour < 1.5;
  // Now and then a second wave follows later the same night.
  if (spawned && stillDark && chance(state, 0.4)) {
    state.director.nextWaveTick = state.tick + minutesToTicks(range(state, 100, 200));
    return;
  }
  // Otherwise the next attack comes on a later evening; some nights are quiet.
  let day = time.hour < 12 ? time.day : time.day + 1;
  if (chance(state, 0.22)) day++;
  state.director.nextWaveTick = tickAt(day, range(state, 20.2, 22.4));
}

/** Two of those who can stand watch take tonight's duty, in rotation. */
function assignWatch(ctx: Ctx): void {
  const { state, time } = ctx;
  state.director.watchDay = time.day;
  const able = living(state).filter((c) => c.jobs.includes('watch') && c.hp > 30);
  if (able.length === 0) return;
  const count = Math.min(2, able.length);
  const chosen = [];
  for (let i = 0; i < count; i++) chosen.push(able[(time.day + i * Math.max(1, Math.floor(able.length / 2))) % able.length]!);
  const unique = [...new Set(chosen)];
  for (const c of unique) {
    c.watchUntilTick = tickAt(time.day + 1, 5);
    c.sleepInUntilTick = tickAt(time.day + 1, 11);
    remember(ctx, c, 'I have the night watch at the watchtower tonight. I should rest in the afternoon.', 3, 'event');
  }
  chronicle(ctx, 'world', `${joinNames(unique.map(firstName))} ${unique.length > 1 ? 'have' : 'has'} tonight's watch.`, unique.map((c) => c.id));
}

/** Keeps the world neither empty nor quiet: night attacks, the watch rota, and newcomers. */
export function updateDirector(ctx: Ctx): void {
  const { state, time } = ctx;
  const director = state.director;

  if (director.watchDay !== time.day && time.hour >= 6.5) assignWatch(ctx);

  if (state.tick >= director.nextWaveTick) {
    const dark = time.hour >= 19.5 || time.hour < 4;
    if (dark) spawnWave(ctx);
    scheduleNextWave(ctx, dark);
  }

  if (state.order.length < TARGET_POPULATION && state.tick >= director.nextArrivalTick && time.hour >= 8 && time.hour < 17) {
    arrive(ctx);
    director.nextArrivalTick = state.tick + Math.round(range(state, 0.8, 1.6) * TICKS_PER_DAY);
  }

  if (state.tick % 200 === 0) {
    director.deathTicks = director.deathTicks.filter((tick) => state.tick - tick < TICKS_PER_DAY * 6);
    state.corpses = state.corpses.filter((corpse) => corpse.untilTick > state.tick);
  }
}

/** A grave to visit: used when someone who lost a friend has an idle moment. */
export function hasSomeoneToMourn(ctx: Ctx, characterId: string): boolean {
  const c = ctx.state.characters[characterId];
  if (!c) return false;
  return ctx.state.graves.some((grave) => (c.relationships[grave.characterId]?.affinity ?? 0) >= 25 && ctx.time.day - grave.day <= 4);
}
