import { dist } from '../../core/math2d';
import { minutesToTicks, TICKS_PER_DAY } from '../../core/time';
import { chance, intRange, nextFloat, pick, weightedPick } from '../../core/rng';
import type { ActivityId } from '../../world/places';
import { ACTIVITIES } from '../data/activities';
import type { Character, Intent, PlanBlock, TalkTone } from '../state';
import { firstName, isApproachable, living, rel, stimulate, type Ctx } from '../util';
import { maxHp, startFight } from './combat';
import { chatterRoll, talkedRecently } from './conversation';
import { drinkPotionIfHurt, freshAct } from './intents';
import { RUNG_FEUD, RUNG_GRUDGE, lethalUnlocked } from './relationships';

/**
 * The local brain. It gives every character a daily plan and, whenever they
 * have nothing to do, the next thing to do. It never waits for anything, so
 * the village lives with no LLM at all; LLM decisions simply replace its
 * choices when they arrive.
 */

const SOCIAL_RADIUS = 15;

/** A plausible day for this character, from their jobs and hobbies. */
export function routinePlan(ctx: Ctx, c: Character): PlanBlock[] {
  const { state } = ctx;
  const jobs = c.jobs.length > 0 ? c.jobs : (['stroll'] as ActivityId[]);
  const hobbies = c.hobbies.length > 0 ? c.hobbies : (['stroll'] as ActivityId[]);
  const wake = 5.8 + nextFloat(state) * 1.4;
  const morningJob = pick(state, jobs);
  const afternoonJob = chance(state, 0.6) ? pick(state, jobs) : pick(state, hobbies);
  const leisure = pick(state, hobbies);
  const evening: ActivityId = c.profile.sociability > 0.35 || chance(state, 0.4) ? 'drink' : pick(state, hobbies);
  return [
    { hour: wake, activity: 'eat', note: 'breakfast' },
    { hour: wake + 0.9, activity: morningJob },
    { hour: 12 + nextFloat(state) * 0.6, activity: 'eat', note: 'midday meal' },
    { hour: 13.2 + nextFloat(state) * 0.4, activity: afternoonJob },
    { hour: 16.6 + nextFloat(state) * 0.6, activity: leisure },
    { hour: 18.4 + nextFloat(state) * 0.5, activity: 'eat', note: 'supper' },
    { hour: 19.3 + nextFloat(state) * 0.4, activity: evening },
    { hour: 21.4 + nextFloat(state) * 1.1, activity: 'sleep' },
  ];
}

/** Index of the plan block in force at `hour` (-1 before the first one). */
export function blockAt(plan: readonly PlanBlock[], hour: number): number {
  let index = -1;
  for (let i = 0; i < plan.length; i++) {
    if ((plan[i] as PlanBlock).hour <= hour) index = i;
  }
  return index;
}

function minutesUntilNextBlock(plan: readonly PlanBlock[], index: number, hour: number): number {
  const next = plan[index + 1];
  const endHour = next ? next.hour : 30; // the last block (sleep) runs past midnight
  return Math.max(10, (endHour - hour) * 60);
}

function act(activity: ActivityId, minutes?: number, placeId?: string): Intent {
  return { kind: 'activity', activity, minutes, placeId, source: 'routine' };
}

/** Someone nearby worth walking over to, and in what spirit. */
function socialIntent(ctx: Ctx, c: Character): Intent | null {
  const { state } = ctx;
  if (state.tick < c.nextSocialTick || !chatterRoll(ctx, c)) return null;
  const candidates = living(state).filter((other) =>
    other.id !== c.id && isApproachable(other) && dist(other.pos, c.pos) <= SOCIAL_RADIUS &&
    state.tick >= other.nextSocialTick && !talkedRecently(ctx, c, other));
  const other = weightedPick(state, candidates, (o) => {
    const r = rel(c, o.id);
    return 1 + Math.abs(r.affinity) / 25 + r.attraction / 30 + r.grievance / 12;
  });
  if (!other) return null;

  const r = rel(c, other.id);
  let tone: TalkTone = 'friendly';
  if (r.grievance >= RUNG_GRUDGE && chance(state, 0.35 + c.profile.temper * 0.5)) tone = 'hostile';
  else if (r.grievance >= 22 && r.affinity > -30 && chance(state, 0.12 + (1 - c.profile.temper) * 0.25)) tone = 'apology';
  else if (!r.kin && r.attraction >= 50 && r.affinity >= 35 && (!c.partnerId || c.partnerId === other.id) && chance(state, 0.3)) tone = 'romantic';
  c.nextSocialTick = state.tick + minutesToTicks(40);
  return { kind: 'talk', targetId: other.id, opener: '', tone, source: 'routine' };
}

/** Without an LLM to hold them back, a long feud eventually comes to blows on its own. */
function hostileIntent(ctx: Ctx, c: Character): boolean {
  const { state } = ctx;
  if (c.hp < maxHp(c) * 0.6) return false;
  for (const other of living(state)) {
    if (other.id === c.id || !isApproachable(other) || dist(other.pos, c.pos) > 18) continue;
    const r = rel(c, other.id);
    if (r.grievance < RUNG_FEUD) continue;
    if (lethalUnlocked(ctx, c, other.id) && chance(state, 0.04 * c.profile.temper)) {
      startFight(ctx, c, other.id, 'lethal', 'routine', `${firstName(other)}! This ends now!`);
      return true;
    }
    if (state.tick - r.lastOffenceTick < TICKS_PER_DAY * 2 && chance(state, 0.07 * c.profile.temper)) {
      startFight(ctx, c, other.id, 'brawl', 'routine', `I have had enough of you, ${firstName(other)}!`);
      return true;
    }
  }
  return false;
}

/** Picks what a character with nothing to do does next. */
function choose(ctx: Ctx, c: Character): Intent {
  const { state, time } = ctx;
  const hour = time.hour;

  const hasWatchTonight = state.tick < c.watchUntilTick;
  const onWatch = hasWatchTonight && (hour >= 20.5 || hour < 5.2);

  // Starving comes before anything, even duty.
  if (c.needs.hunger < 12 || (c.needs.hunger < 24 && !onWatch)) {
    if (c.inventory.food > 0 && (onWatch || c.needs.hunger < 10 || chance(state, 0.4))) {
      c.inventory.food--;
      c.needs.hunger = Math.min(100, c.needs.hunger + 45);
      return { kind: 'wait', minutes: 6, source: 'routine' };
    }
    return act('eat');
  }
  if (onWatch) return act('watch', Math.max(5, Math.min(150, (c.watchUntilTick - state.tick) / 20)));
  // After a night on watch, sleep in; before one, take an afternoon nap.
  if (!hasWatchTonight && state.tick < c.sleepInUntilTick && c.needs.energy < 90) {
    return act('sleep', Math.max(20, (c.sleepInUntilTick - state.tick) / 20));
  }
  if (hasWatchTonight && hour >= 13.5 && hour < 18.5 && c.needs.energy < 88) return act('sleep', (18.6 - hour) * 60);
  if (c.needs.energy < 9) return act('sleep', 300);
  if (c.hp < maxHp(c) * 0.5) {
    drinkPotionIfHurt(ctx, c);
    if (c.hp < maxHp(c) * 0.5) return act('rest', 45, c.homeId);
  }

  if (hostileIntent(ctx, c)) return c.intent as Intent;
  const social = socialIntent(ctx, c);
  if (social) return social;

  const index = blockAt(c.plan, hour);
  if (index < 0) {
    // Before the day's first block: stay asleep, or potter about if already up.
    return hour < 5 ? act('sleep', (c.plan[0]?.hour ?? 6) * 60 - hour * 60) : act('rest', 20);
  }
  const block = c.plan[index] as PlanBlock;
  const remaining = minutesUntilNextBlock(c.plan, index, hour);
  if (block.activity === 'sleep') return act('sleep', Math.max(60, ((24 - hour) % 24) * 60 + (c.plan[0]?.hour ?? 6) * 60));
  if (block.activity === 'eat' && c.needs.hunger > 78) return act('stroll', Math.min(remaining, 25));
  const def = ACTIVITIES[block.activity];
  const minutes = Math.min(remaining, intRange(state, def.minutes[0], def.minutes[1]));
  return act(block.activity, minutes, block.placeId);
}

export function updateRoutine(ctx: Ctx): void {
  const { state, time } = ctx;
  for (const c of living(state)) {
    // A fresh plan each morning (an LLM plan for the day, if one arrives, replaces it).
    if (c.planDay !== time.day && time.hour >= 4.5) {
      c.plan = routinePlan(ctx, c);
      c.planDay = time.day;
      c.planSource = 'routine';
      c.planBlock = -1;
    }

    const index = blockAt(c.plan, time.hour);
    if (index !== c.planBlock) {
      c.planBlock = index;
      const block = c.plan[index];
      if (block && !c.asleep) {
        stimulate(ctx, c, 'block_ended', `It is time for the next part of your day: you had planned to ${ACTIVITIES[block.activity].label}${block.note ? ` (${block.note})` : ''}.`, 0);
      }
      // A routine activity from the previous block gives way to the new one.
      if (c.intent?.source === 'routine' && c.intent.kind === 'activity' && c.act.phase === 'perform' && !c.convoId) {
        const sleepingOn = c.intent.activity === 'sleep' && block?.activity !== 'sleep' && c.needs.energy < 60;
        const onDuty = c.intent.activity === 'watch' && state.tick < c.watchUntilTick;
        if (!sleepingOn && !onDuty && c.intent.activity !== block?.activity) c.act.untilTick = Math.min(c.act.untilTick, state.tick);
      }
    }

    // Watch duty starts on time, whatever the plan said.
    const dueOnWatch = state.tick < c.watchUntilTick && (time.hour >= 20.5 || time.hour < 5.2);
    if (dueOnWatch && c.intent?.source === 'routine' && c.intent.kind === 'activity' && c.intent.activity !== 'watch' && c.act.phase === 'perform') {
      c.act.untilTick = Math.min(c.act.untilTick, state.tick);
    }

    if (c.intent || c.queue.length > 0 || c.convoId || c.combat.yielded) continue;
    const intent = choose(ctx, c);
    if (c.intent) continue; // choose() may have started a fight directly
    c.intent = intent;
    c.act = freshAct(state.tick);
  }
}
