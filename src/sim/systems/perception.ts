import { dist } from '../../core/math2d';
import { minutesToTicks, secondsToTicks } from '../../core/time';
import { MONSTERS } from '../data/monsters';
import type { Character, Monster } from '../state';
import {
  chronicle, firstName, hasRecentStimulus, isFighting, joinNames, living, monsterList, placeNameAt, remember, say, stimulate, type Ctx,
} from '../util';
import { maxHp, startFight, startFlee } from './combat';
import { abortConversation } from './conversation';
import { leaveBuilding } from './intents';
import { adjust } from './relationships';

const SIGHT = 18;
const WATCH_SIGHT = 48;
const RESPONDER_BRAVERY = 0.6;
const ALARM_RANGE = 150;

function nearestMonster(monsters: Monster[], c: Character, limit: number): Monster | null {
  let best: Monster | null = null;
  let bestDistance = limit;
  for (const m of monsters) {
    const d = dist(m.pos, c.pos);
    if (d < bestDistance) {
      bestDistance = d;
      best = m;
    }
  }
  return best;
}

/** Stand and fight, or run? Instinct only; an LLM "react" decision may overrule it. */
function standsGround(c: Character, monstersNear: number, alliesNear: number): boolean {
  const hpLeft = c.hp / maxHp(c);
  const score = c.profile.bravery * 0.75 + hpLeft * 0.35 + alliesNear * 0.08 - Math.max(0, monstersNear - 1) * 0.07;
  return score > 0.72;
}

function raiseAlarm(ctx: Ctx, c: Character, monster: Monster): void {
  const { state } = ctx;
  const where = placeNameAt(monster.pos);
  if (state.tick >= state.director.alarmUntilTick) {
    say(ctx, c, `To arms! The dead are at ${where}!`, null, true);
    chronicle(ctx, 'combat', `${firstName(c)} raised the alarm: the dead are walking at ${where}.`, [c.id]);
    ctx.events.push({ type: 'emote', id: c.id, emote: 'alert' });
  }
  state.director.alarmUntilTick = state.tick + minutesToTicks(40);
  state.director.alarmPos = { ...monster.pos };
}

/** When the last monster falls, those who fought remember it together. */
function standDown(ctx: Ctx): void {
  const { state } = ctx;
  const since = Math.max(1, state.tick - minutesToTicks(90));
  const fighters = living(state).filter((c) => c.combat.nextAttackTick > since);
  state.director.alarmUntilTick = state.tick;
  state.director.alarmPos = null;
  if (fighters.length === 0) return;
  chronicle(ctx, 'combat', `The attack is over. ${joinNames(fighters.map(firstName))} held the line.`, fighters.map((c) => c.id));
  for (const c of fighters) {
    const others = fighters.filter((o) => o.id !== c.id);
    for (const other of others) adjust(ctx, c, other.id, { affinity: 2, trust: 3, familiarity: 2 });
    remember(ctx, c, others.length > 0
      ? `Fought off the dead beside ${joinNames(others.map(firstName))}.`
      : 'Fought off the dead alone.', 6, 'event', others.map((o) => o.id));
    stimulate(ctx, c, 'victory', 'The fight is over and the dead are destroyed.', 1);
  }
  ctx.events.push({ type: 'save_point', reason: 'attack over' });
}

/** Notices danger and reacts on instinct: fight, flee, raise the alarm, answer it. */
export function updatePerception(ctx: Ctx): void {
  const { state } = ctx;
  const monsters = monsterList(state).filter((m) => m.phase !== 'dying');
  const alarm = state.tick < state.director.alarmUntilTick;

  if (monsters.length === 0) {
    if (state.director.alarmPos) standDown(ctx);
    return;
  }

  const everyone = living(state);
  for (let i = 0; i < everyone.length; i++) {
    const c = everyone[i] as Character;
    if ((state.tick + i * 3) % 10 !== 0 || c.combat.yielded) continue;
    const awakeOutside = !c.inside && !c.asleep;
    const onWatch = c.intent?.kind === 'activity' && c.intent.activity === 'watch' && c.act.phase === 'perform';
    const seen = awakeOutside ? nearestMonster(monsters, c, onWatch ? WATCH_SIGHT : SIGHT) : null;

    if (seen) {
      raiseAlarm(ctx, c, seen);
      if (c.convoId) abortConversation(ctx, c.convoId);
      if (!hasRecentStimulus(c, 'threat', state.tick - secondsToTicks(30))) {
        stimulate(ctx, c, 'threat', `A ${MONSTERS[seen.kind].label} is coming at you near ${placeNameAt(seen.pos)}!`, 3, seen.id);
      }
      if (isFighting(c)) continue;
      const monstersNear = monsters.filter((m) => dist(m.pos, c.pos) < 20).length;
      const alliesNear = everyone.filter((o) => o.id !== c.id && !o.inside && o.intent?.kind === 'fight' && dist(o.pos, c.pos) < 20).length;
      if (onWatch || standsGround(c, monstersNear, alliesNear)) startFight(ctx, c, seen.id, 'monster', 'reflex');
      else startFlee(ctx, c, 'reflex');
      continue;
    }

    if (!alarm || isFighting(c)) continue;
    const responder = c.profile.bravery >= RESPONDER_BRAVERY && c.hp > maxHp(c) * 0.4;
    if (responder) {
      const target = nearestMonster(monsters, c, ALARM_RANGE);
      if (!target) continue;
      if (c.asleep || c.inside) leaveBuilding(c);
      if (!hasRecentStimulus(c, 'alarm', state.tick - minutesToTicks(30))) {
        stimulate(ctx, c, 'alarm', `The alarm has been raised: the dead are attacking near ${placeNameAt(target.pos)}.`, 2, target.id);
      }
      startFight(ctx, c, target.id, 'monster', 'reflex');
    } else if (awakeOutside && nearestMonster(monsters, c, 45)) {
      startFlee(ctx, c, 'reflex');
    }
  }
}
