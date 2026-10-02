import { dist, facingTo, type Vec2 } from '../../core/math2d';
import { GAME_MINUTES_PER_TICK, minutesToTicks, secondsToTicks, TICKS_PER_GAME_MINUTE } from '../../core/time';
import { chance, intRange, pick, range } from '../../core/rng';
import { nearestWalkable } from '../../world/navgrid';
import { findPlace, getPlace, getSlot, slotsFor, type ActivityId, type Slot } from '../../world/places';
import { ACTIVITIES, ITEM_CAP } from '../data/activities';
import { ANIMS, type AnimId } from '../../render/anims';
import { CLASSES, RUN_SPEED, WALK_SPEED } from '../data/classes';
import type { Act, Character, Intent, ItemId, Relationship } from '../state';
import {
  chronicle, firstName, getCharacter, isApproachable, living, monsterList, rel, remember, say, setAnim, stimulate, type Ctx,
} from '../util';
import { maxHp, runFight } from './combat';
import { startConversation } from './conversation';
import { advance, routeTo, stepToward } from './movement';
import { adjust } from './relationships';

const TALK_REACH = 9;
const GIVE_REACH = 2.3;
const STROLL_STOPS = ['green', 'tavern', 'market', 'smithy', 'chapel', 'training_yard', 'docks', 'west_road'];

export function freshAct(tick: number): Act {
  return { phase: 'start', path: [], dest: null, sinceTick: tick, untilTick: 0, repathTick: 0, stuck: 0 };
}

function walkAnim(c: Character): AnimId {
  const style = c.id.charCodeAt(c.id.length - 1) % 3;
  return style === 0 ? 'walk' : style === 1 ? 'walk_b' : 'walk_c';
}

function speedOf(c: Character): number {
  return WALK_SPEED * (c.needs.energy < 15 ? 0.8 : 1);
}

/** Steps out of a building back onto the street. */
export function leaveBuilding(c: Character): void {
  if (!c.inside) return;
  const place = findPlace(c.inside);
  if (place) c.pos = nearestWalkable(place.door, 4) ?? { ...place.door };
  c.inside = null;
  c.asleep = false;
}

function releaseSlot(ctx: Ctx, c: Character): void {
  if (c.slotId && ctx.state.slots[c.slotId] === c.id) delete ctx.state.slots[c.slotId];
  c.slotId = null;
}

function finish(ctx: Ctx, c: Character, outcome: string): void {
  releaseSlot(ctx, c);
  c.tool = null;
  c.outcome = outcome;
  c.act.phase = 'done';
}

/** Sets a route; false when there is none. */
function travelTo(ctx: Ctx, c: Character, dest: Vec2): boolean {
  leaveBuilding(c);
  const route = routeTo(c.pos, dest);
  if (!route) return false;
  c.act.path = route;
  c.act.dest = { ...dest };
  c.act.phase = 'travel';
  c.act.sinceTick = ctx.state.tick;
  return true;
}

/** Walks the current route; true on arrival. */
function travel(c: Character, speed = speedOf(c)): boolean {
  if (c.act.path.length === 0) return true;
  const arrived = advance(c, c.act.path, speed);
  setAnim(c, speed > WALK_SPEED * 1.3 ? 'run' : walkAnim(c));
  return arrived;
}

function pickSlot(ctx: Ctx, c: Character, activity: ActivityId, placeId?: string): Slot | null {
  const ranged = CLASSES[c.classId].weapon.style !== 'melee';
  let best: Slot | null = null;
  let bestScore = Infinity;
  for (const slot of slotsFor(activity)) {
    const holder = ctx.state.slots[slot.id];
    if (holder && holder !== c.id) continue;
    let score = dist(c.pos, slot.pos);
    if (placeId && slot.placeId !== placeId) score += 400;
    // At home, people use their own bench.
    if (slot.placeId.startsWith('home_') && slot.placeId !== c.homeId) score += 150;
    if (activity === 'train' && slot.ranged !== ranged) score += 60;
    if (score < bestScore) {
      bestScore = score;
      best = slot;
    }
  }
  return best;
}

function give(ctx: Ctx, c: Character, item: ItemId): void {
  c.inventory[item] = Math.min(ITEM_CAP, c.inventory[item] + 1);
}

/** Per-game-minute effects of doing an activity. */
function activityMinute(ctx: Ctx, c: Character, activity: ActivityId, minutesDone: number): void {
  const def = ACTIVITIES[activity];
  if (def.needs) {
    c.needs.hunger = Math.min(100, Math.max(0, c.needs.hunger + (def.needs.hunger ?? 0)));
    c.needs.energy = Math.min(100, Math.max(0, c.needs.energy + (def.needs.energy ?? 0)));
    c.needs.social = Math.min(100, Math.max(0, c.needs.social + (def.needs.social ?? 0)));
  }
  if (def.heal) c.hp = Math.min(maxHp(c), c.hp + def.heal);
  if (def.yields && minutesDone > 0 && minutesDone % def.yields.everyMinutes === 0) give(ctx, c, def.yields.item);
  if (activity === 'pray' || activity === 'mourn') {
    for (const id in c.relationships) {
      const r = c.relationships[id] as Relationship;
      if (r.grievance > 0) r.grievance = Math.max(0, r.grievance - 0.12);
    }
  }
}

function runActivity(ctx: Ctx, c: Character, intent: Extract<Intent, { kind: 'activity' }>): void {
  const { state } = ctx;
  const act = c.act;
  const def = ACTIVITIES[intent.activity];
  const minutes = intent.minutes ?? intRange(state, def.minutes[0], def.minutes[1]);

  if (act.phase === 'start') {
    intent.minutes = minutes;
    if (intent.activity === 'sleep') {
      if (c.inside === c.homeId) {
        act.phase = 'perform';
        act.sinceTick = state.tick;
        act.untilTick = state.tick + minutesToTicks(minutes);
        c.asleep = true;
        return;
      }
      if (!travelTo(ctx, c, getPlace(c.homeId).door)) finish(ctx, c, 'You could not find a way home.');
      return;
    }
    if (intent.activity === 'stroll') {
      const stop = getPlace(pick(state, STROLL_STOPS));
      const goal = nearestWalkable({ x: stop.door.x + range(state, -5, 5), z: stop.door.z + range(state, -5, 5) }, 6) ?? stop.door;
      act.untilTick = state.tick + minutesToTicks(minutes);
      if (!travelTo(ctx, c, goal)) finish(ctx, c, 'You wandered a little.');
      return;
    }
    const slot = pickSlot(ctx, c, intent.activity, intent.placeId);
    if (!slot) {
      finish(ctx, c, `There was no room to ${def.label}.`);
      return;
    }
    state.slots[slot.id] = c.id;
    c.slotId = slot.id;
    const approach = nearestWalkable(slot.pos, 4) ?? slot.pos;
    if (!travelTo(ctx, c, approach)) finish(ctx, c, `You could not get to ${getPlace(slot.placeId).name}.`);
    return;
  }

  if (act.phase === 'travel') {
    if (!travel(c)) return;
    if (intent.activity === 'sleep') {
      c.inside = c.homeId;
      c.asleep = true;
      act.phase = 'perform';
      act.sinceTick = state.tick;
      act.untilTick = state.tick + minutesToTicks(minutes);
      ctx.events.push({ type: 'emote', id: c.id, emote: 'sleep' });
      return;
    }
    if (intent.activity === 'stroll') {
      setAnim(c, 'idle');
      if (state.tick >= act.untilTick) {
        finish(ctx, c, 'You finished your stroll.');
      } else if (act.dest) {
        // Arrived at a stop: linger a moment before wandering on.
        act.dest = null;
        act.repathTick = state.tick + secondsToTicks(range(state, 3, 8));
      } else if (state.tick >= act.repathTick) {
        const stop = getPlace(pick(state, STROLL_STOPS));
        const goal = nearestWalkable({ x: stop.door.x + range(state, -5, 5), z: stop.door.z + range(state, -5, 5) }, 6) ?? stop.door;
        if (!travelTo(ctx, c, goal)) finish(ctx, c, 'You finished your stroll.');
      }
      return;
    }
    // The last stretch onto the seat or up to the bench ignores the nav grid.
    const slot = c.slotId ? getSlot(c.slotId) : null;
    if (!slot) {
      finish(ctx, c, 'You lost your place.');
      return;
    }
    if (!stepToward(c, slot.pos, speedOf(c))) {
      setAnim(c, walkAnim(c));
      return;
    }
    c.facing = slot.facing;
    act.phase = 'perform';
    act.sinceTick = state.tick;
    act.untilTick = state.tick + minutesToTicks(minutes);
    c.tool = def.tool ?? null;
    const anim = slot.anim && slot.anim in ANIMS ? (slot.anim as AnimId) : def.anim;
    setAnim(c, anim, true);
    return;
  }

  // Performing.
  const elapsed = state.tick - act.sinceTick;
  if (elapsed > 0 && elapsed % TICKS_PER_GAME_MINUTE === 0) activityMinute(ctx, c, intent.activity, elapsed / TICKS_PER_GAME_MINUTE);

  if (intent.activity === 'train') {
    const weapon = CLASSES[c.classId].weapon;
    const every = secondsToTicks(weapon.interval + 1.2);
    if (elapsed % every === 0) setAnim(c, pick(state, weapon.anims), true);
  } else if (intent.activity === 'forage' || intent.activity === 'shop') {
    if (elapsed % secondsToTicks(5) === 0) setAnim(c, ACTIVITIES[intent.activity].anim, true);
  }

  const rested = intent.activity === 'sleep' && c.needs.energy >= 99 && elapsed > minutesToTicks(240) && state.tick >= c.sleepInUntilTick;
  if (state.tick < act.untilTick && !rested) return;

  if (intent.activity === 'sleep') {
    leaveBuilding(c);
    setAnim(c, 'idle');
    stimulate(ctx, c, 'woke', 'You have just woken up.', 0);
    ctx.events.push({ type: 'woke', id: c.id });
    finish(ctx, c, 'You slept and feel rested.');
    return;
  }
  if (intent.activity === 'shop') {
    const item: ItemId = c.inventory.food < 2 ? 'food' : 'gift';
    give(ctx, c, item);
    finish(ctx, c, `You picked up ${item === 'food' ? 'some food' : 'a small gift'} at the market.`);
    return;
  }
  setAnim(c, 'idle');
  finish(ctx, c, `You finished ${def.verb}.`);
}

function runTalk(ctx: Ctx, c: Character, intent: Extract<Intent, { kind: 'talk' }>): void {
  const { state } = ctx;
  const act = c.act;
  const target = getCharacter(state, intent.targetId);
  if (!target) {
    finish(ctx, c, 'The person you wanted to talk to is gone.');
    return;
  }
  if (act.phase === 'start') {
    leaveBuilding(c);
    act.phase = 'travel';
    act.sinceTick = state.tick;
    act.untilTick = state.tick + minutesToTicks(45);
    act.repathTick = 0;
  }
  const d = dist(c.pos, target.pos);
  if (d <= TALK_REACH && isApproachable(target)) {
    act.path = [];
    startConversation(ctx, c, target, intent.tone, intent.opener);
    return;
  }
  if (state.tick >= act.untilTick || (d <= TALK_REACH && target.asleep)) {
    finish(ctx, c, `You could not get a word with ${firstName(target)}.`);
    return;
  }
  if (target.inside) {
    // Wait near their door for a while rather than walking through walls.
    const door = getPlace(target.inside).door;
    if (dist(c.pos, door) < 4) {
      setAnim(c, 'idle');
      if (state.tick - act.sinceTick > minutesToTicks(8)) finish(ctx, c, `${firstName(target)} is indoors and did not come out.`);
      return;
    }
  }
  if (act.path.length === 0 || state.tick >= act.repathTick) {
    const goal = target.inside ? getPlace(target.inside).door : target.pos;
    act.path = routeTo(c.pos, goal) ?? [];
    act.repathTick = state.tick + secondsToTicks(1.5);
    if (act.path.length === 0 && d > TALK_REACH) {
      finish(ctx, c, `You could not reach ${firstName(target)}.`);
      return;
    }
  }
  travel(c);
}

function runGive(ctx: Ctx, c: Character, intent: Extract<Intent, { kind: 'give' }>): void {
  const { state } = ctx;
  const act = c.act;
  const target = getCharacter(state, intent.targetId);
  if (!target || c.inventory[intent.item] <= 0) {
    finish(ctx, c, target ? `You had no ${intent.item} to give.` : 'The person you wanted to give something to is gone.');
    return;
  }
  if (act.phase === 'start') {
    leaveBuilding(c);
    act.phase = 'travel';
    act.sinceTick = state.tick;
    act.untilTick = state.tick + minutesToTicks(40);
  }
  if (act.phase === 'perform') {
    if (state.tick >= act.untilTick) {
      setAnim(c, 'idle');
      finish(ctx, c, `You gave ${firstName(target)} ${describeItem(intent.item)}.`);
    }
    return;
  }
  const d = dist(c.pos, target.pos);
  if (d <= GIVE_REACH && isApproachable(target)) {
    act.path = [];
    c.facing = facingTo(c.pos, target.pos);
    c.inventory[intent.item]--;
    give(ctx, target, intent.item);
    setAnim(c, 'interact', true);
    act.phase = 'perform';
    act.untilTick = state.tick + secondsToTicks(1.6);
    const romantic = intent.item === 'gift' && rel(c, target.id).attraction >= 35;
    adjust(ctx, target, c.id, { affinity: 8, trust: 4, grievance: -10, attraction: romantic && !rel(target, c.id).kin ? 4 : 0 });
    adjust(ctx, c, target.id, { affinity: 3 });
    remember(ctx, target, `${firstName(c)} gave me ${describeItem(intent.item)}.`, 5, 'event', [c.id]);
    remember(ctx, c, `Gave ${firstName(target)} ${describeItem(intent.item)}.`, 3, 'event', [target.id]);
    stimulate(ctx, target, 'gift', `${firstName(c)} just gave you ${describeItem(intent.item)}.`, 1, c.id);
    ctx.events.push({ type: 'emote', id: target.id, emote: 'gift' });
    if (!intent.say) say(ctx, c, intent.item === 'potion' ? 'Take this. You may need it.' : 'Here. This is for you.', target.id);
    if (intent.item === 'gift') chronicle(ctx, 'social', `${firstName(c)} gave ${firstName(target)} a gift.`, [c.id, target.id]);
    return;
  }
  if (state.tick >= act.untilTick || target.inside) {
    finish(ctx, c, `You could not find ${firstName(target)} to give them anything.`);
    return;
  }
  if (act.path.length === 0 || state.tick >= act.repathTick) {
    act.path = routeTo(c.pos, target.pos) ?? [];
    act.repathTick = state.tick + secondsToTicks(1.5);
  }
  if (d > GIVE_REACH * 0.8) travel(c);
}

function describeItem(item: ItemId): string {
  return item === 'food' ? 'some food' : item === 'potion' ? 'a healing potion' : 'a small gift';
}

function runFlee(ctx: Ctx, c: Character): void {
  const { state } = ctx;
  const act = c.act;
  const home = getPlace(c.homeId);
  if (act.phase === 'start') {
    if (c.inside) {
      act.phase = 'perform';
      act.sinceTick = state.tick;
    } else if (!travelTo(ctx, c, home.door)) {
      finish(ctx, c, 'There was nowhere to run.');
    }
    return;
  }
  if (act.phase === 'travel') {
    if (!travel(c, RUN_SPEED)) return;
    c.inside = c.homeId;
    act.phase = 'perform';
    act.sinceTick = state.tick;
    return;
  }
  // Hiding indoors until the danger has passed.
  const waited = state.tick - act.sinceTick;
  const danger = monsterList(state).some((m) => m.phase !== 'dying' && dist(m.pos, home.door) < 45);
  const hunted = living(state).some((other) => other.intent?.kind === 'fight' && other.intent.targetId === c.id);
  if (waited > minutesToTicks(12) && !danger && !hunted) {
    leaveBuilding(c);
    setAnim(c, 'idle');
    finish(ctx, c, 'You hid until it was safe to come out.');
  }
}

/** Says the line and records the thought that came with a new intent. */
function begin(ctx: Ctx, c: Character, intent: Intent): void {
  if (intent.thought) {
    c.thought = intent.thought;
    if (intent.source === 'llm') remember(ctx, c, intent.thought, 3, 'thought');
  }
  if (intent.say && intent.kind !== 'talk') {
    const targetId = 'targetId' in intent && ctx.state.characters[intent.targetId] ? intent.targetId : null;
    say(ctx, c, intent.say, targetId, intent.kind === 'fight');
  }
}

/** Carries out each character's current intent, one tick at a time. */
export function updateIntents(ctx: Ctx): void {
  const { state } = ctx;
  for (const c of living(state)) {
    c.moving = false;
    if (c.convoId || c.combat.yielded) continue;

    // Finished elsewhere (a conversation ending, a fight resolving): clear it before picking the next.
    if (c.intent && c.act.phase === 'done') completeIntent(ctx, c, c.intent);
    if (!c.intent) {
      const next = c.queue.shift();
      if (!next) continue;
      c.intent = next;
      c.act = freshAct(state.tick);
    }
    const intent = c.intent;
    if (c.act.phase === 'start') begin(ctx, c, intent);

    switch (intent.kind) {
      case 'go_to': {
        const place = findPlace(intent.placeId);
        if (!place) {
          finish(ctx, c, 'You did not know the way.');
        } else if (c.act.phase === 'start') {
          if (!travelTo(ctx, c, place.door)) finish(ctx, c, `You could not find a way to ${place.name}.`);
        } else if (travel(c)) {
          setAnim(c, 'idle');
          finish(ctx, c, `You arrived at ${place.name}.`);
        }
        break;
      }
      case 'activity':
        runActivity(ctx, c, intent);
        break;
      case 'talk':
        runTalk(ctx, c, intent);
        break;
      case 'give':
        runGive(ctx, c, intent);
        break;
      case 'fight':
        runFight(ctx, c, intent);
        break;
      case 'flee':
        runFlee(ctx, c);
        break;
      case 'wait':
        if (c.act.phase === 'start') {
          c.act.phase = 'perform';
          c.act.untilTick = state.tick + minutesToTicks(intent.minutes);
          if (!ANIMS[c.anim].loop || c.anim.startsWith('walk') || c.anim === 'run') setAnim(c, 'idle');
        } else if (state.tick >= c.act.untilTick) {
          if (c.anim === 'sit_floor') setAnim(c, 'idle');
          finish(ctx, c, 'You waited a while.');
        }
        break;
    }

    // The intent may have been replaced (a reflex, a conversation) while it ran.
    if (c.intent === intent && c.act.phase === 'done') completeIntent(ctx, c, intent);
  }
}

function completeIntent(ctx: Ctx, c: Character, intent: Intent): void {
  c.intent = null;
  if (c.queue.length === 0 && intent.source === 'llm') {
    stimulate(ctx, c, 'block_ended', `You finished what you set out to do: ${c.outcome}`, 0);
  }
}

/** Game minutes an activity has been going, for status lines. */
export function minutesInto(ctx: Ctx, c: Character): number {
  return (ctx.state.tick - c.act.sinceTick) * GAME_MINUTES_PER_TICK;
}

/** Used when a character is wounded and idle: is there a potion worth drinking? */
export function drinkPotionIfHurt(ctx: Ctx, c: Character): boolean {
  if (c.inventory.potion > 0 && c.hp < maxHp(c) * 0.5 && chance(ctx.state, 0.5)) {
    c.inventory.potion--;
    c.hp = Math.min(maxHp(c), c.hp + 55);
    setAnim(c, 'use_item', true);
    return true;
  }
  return false;
}
