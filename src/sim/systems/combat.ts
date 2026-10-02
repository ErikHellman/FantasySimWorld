import { clamp, dist, type Vec2 } from '../../core/math2d';
import { secondsToTicks } from '../../core/time';
import { chance, intRange, pick } from '../../core/rng';
import { ANIMS, animSeconds, type AnimDef, type AnimId } from '../../render/anims';
import { CLASSES, FISTS, RUN_SPEED, type Weapon } from '../data/classes';
import { MONSTERS } from '../data/monsters';
import type { Character, FightMode, Id, Intent, Monster } from '../state';
import {
  bumpEpoch, chronicle, firstName, getCharacter, getMonster, hasRecentStimulus, isFighting, nearbyCharacters,
  placeNameAt, remember, say, setAnim, stimulate, their, type Ctx,
} from '../util';
import { abortConversation } from './conversation';
import { detach, killCharacter } from './death';
import { advance, face, routeTo } from './movement';
import { adjust } from './relationships';

const BRAWL_YIELD = 0.3;
const LETHAL_YIELD = 0.25;
const POTION_HEAL = 55;

export function maxHp(c: Character): number {
  return CLASSES[c.classId].maxHp;
}

export function weaponOf(c: Character, mode: FightMode): Weapon {
  return mode === 'brawl' ? FISTS : CLASSES[c.classId].weapon;
}

function projectileOf(weapon: Weapon): 'arrow' | 'bolt' | 'magic' | null {
  return weapon.style === 'bow' ? 'arrow' : weapon.style === 'crossbow' ? 'bolt' : weapon.style === 'magic' ? 'magic' : null;
}

function isOneShotPlaying(c: { anim: AnimId }, startTick: number, tick: number): boolean {
  const def: AnimDef = ANIMS[c.anim];
  return !def.loop && tick < startTick + secondsToTicks(animSeconds(c.anim));
}

/** Replaces whatever the character was doing with a fight. */
export function startFight(ctx: Ctx, c: Character, targetId: Id, mode: FightMode, source: Intent['source'], say_?: string): void {
  detach(ctx, c);
  if (c.inside) {
    c.inside = null;
    c.asleep = false;
  }
  c.tool = null;
  c.queue = [];
  c.intent = { kind: 'fight', targetId, mode, source, say: say_ };
  c.act = { phase: 'start', path: [], dest: null, sinceTick: ctx.state.tick, untilTick: 0, repathTick: 0, stuck: 0 };
  c.combat.targetId = targetId;
  c.combat.mode = mode;
  c.combat.hitTick = 0;
  bumpEpoch(c);
}

export function startFlee(ctx: Ctx, c: Character, source: Intent['source']): void {
  detach(ctx, c);
  c.tool = null;
  c.queue = [];
  c.intent = { kind: 'flee', source };
  c.act = { phase: 'start', path: [], dest: null, sinceTick: ctx.state.tick, untilTick: 0, repathTick: 0, stuck: 0 };
  c.combat.targetId = null;
  c.combat.hitTick = 0;
  bumpEpoch(c);
}

function endFight(c: Character, outcome: string): void {
  c.outcome = outcome;
  c.act.phase = 'done';
  c.act.path = [];
  c.combat.targetId = null;
  c.combat.hitTick = 0;
}

/** Someone who is hit while minding their own business hits back, or runs. */
function reflexDefend(ctx: Ctx, victim: Character, attackerId: Id, mode: FightMode): void {
  if (isFighting(victim) || victim.combat.yielded) return;
  const hpLeft = victim.hp / maxHp(victim);
  const stands = victim.profile.bravery * 0.75 + hpLeft * 0.35 + (mode === 'brawl' ? 0.2 : 0) > 0.62;
  if (stands) startFight(ctx, victim, attackerId, mode, 'reflex');
  else startFlee(ctx, victim, 'reflex');
}

export function damageMonster(ctx: Ctx, monster: Monster, amount: number, attacker: Character): void {
  const def = MONSTERS[monster.kind];
  monster.hp -= amount;
  const fatal = monster.hp <= 0;
  ctx.events.push({ type: 'hit', attackerId: attacker.id, targetId: monster.id, damage: amount, pos: { ...monster.pos }, fatal });
  if (!fatal) {
    if (monster.hitTick === 0) setAnim(monster, 'hit', true);
    const current = getCharacter(ctx.state, monster.targetId);
    if (!current || dist(monster.pos, attacker.pos) < dist(monster.pos, current.pos)) monster.targetId = attacker.id;
    if (monster.phase === 'roam') monster.phase = 'chase';
    return;
  }
  monster.phase = 'dying';
  monster.sinceTick = ctx.state.tick;
  monster.hitTick = 0;
  monster.path = [];
  setAnim(monster, 'skeleton_death', true);
  attacker.kills++;
  const where = placeNameAt(monster.pos);
  remember(ctx, attacker, `Destroyed a ${def.label} at ${where}.`, monster.kind === 'necromancer' ? 8 : 4, 'event');
  if (monster.kind === 'necromancer') chronicle(ctx, 'combat', `${attacker.profile.name} struck down the necromancer at ${where}!`, [attacker.id]);
  ctx.events.push({ type: 'death', id: monster.id, name: def.label, pos: { ...monster.pos }, monster: true });
}

interface Assailant {
  id: Id;
  label: string;
  character: Character | null;
}

export function damageCharacter(ctx: Ctx, victim: Character, amount: number, by: Assailant, mode: FightMode): void {
  const { state } = ctx;
  const cap = maxHp(victim);
  const wasFresh = state.tick - victim.combat.lastHitByTick > secondsToTicks(8);
  victim.hp = mode === 'brawl' ? Math.max(1, victim.hp - amount) : victim.hp - amount;
  victim.combat.lastHitByTick = state.tick;
  victim.combat.lastHitById = by.id;
  const fatal = victim.hp <= 0;
  ctx.events.push({ type: 'hit', attackerId: by.id, targetId: victim.id, damage: amount, pos: { ...victim.pos }, fatal });

  if (fatal) {
    killCharacter(ctx, victim, by.character ? `killed by ${by.label}` : `slain by a ${by.label} at ${placeNameAt(victim.pos)}`, by.character);
    return;
  }

  if (victim.convoId) abortConversation(ctx, victim.convoId);
  if (victim.combat.hitTick === 0) setAnim(victim, 'hit', true);
  if (wasFresh) {
    bumpEpoch(victim);
    if (!hasRecentStimulus(victim, 'attacked', state.tick - secondsToTicks(20), by.id)) {
      stimulate(ctx, victim, 'attacked', by.character
        ? `${by.label} is attacking you${mode === 'brawl' ? ' with their fists' : ' with a weapon'}!`
        : `A ${by.label} is attacking you!`, 3, by.id);
    }
  }
  reflexDefend(ctx, victim, by.id, mode);

  const attacker = by.character;
  if (!attacker) return;

  if (mode === 'brawl' && victim.hp <= cap * BRAWL_YIELD) {
    endBrawl(ctx, attacker, victim);
  } else if (mode === 'lethal' && victim.hp <= cap * LETHAL_YIELD && !victim.combat.yielded) {
    // Beaten: the victim yields and the attacker must choose.
    victim.combat.yielded = true;
    victim.intent = null;
    victim.queue = [];
    victim.act.path = [];
    victim.combat.hitTick = 0;
    setAnim(victim, 'sit_floor');
    say(ctx, victim, 'Enough! I yield!', attacker.id, true);
    attacker.combat.mercyUntilTick = state.tick + Math.max(1, state.tuning.mercyWaitTicks);
    attacker.combat.mercyTargetId = victim.id;
    attacker.combat.hitTick = 0;
    setAnim(attacker, weaponOf(attacker, 'lethal').stance);
    bumpEpoch(attacker);
    ctx.events.push({ type: 'mercy_request', attackerId: attacker.id, victimId: victim.id });
  }
}

function witnesses(ctx: Ctx, pos: Vec2, exclude: Id[]): Character[] {
  return nearbyCharacters(ctx.state, pos, 22).filter((c) => !exclude.includes(c.id) && !c.asleep);
}

function endBrawl(ctx: Ctx, winner: Character, loser: Character): void {
  const starter = winner.intent?.source !== 'reflex' ? winner : loser;
  const where = placeNameAt(loser.pos);
  endFight(winner, `You beat ${firstName(loser)} in a brawl.`);
  endFight(loser, `${firstName(winner)} beat you in a brawl.`);
  loser.intent = { kind: 'wait', minutes: 20, source: 'reflex' };
  loser.act = { phase: 'start', path: [], dest: null, sinceTick: ctx.state.tick, untilTick: 0, repathTick: 0, stuck: 0 };
  setAnim(loser, 'sit_floor');
  adjust(ctx, loser, winner.id, { grievance: 14, affinity: -10, trust: -10 });
  adjust(ctx, winner, loser.id, { grievance: -22, affinity: -3 });
  remember(ctx, winner, `Fought ${firstName(loser)} with my fists at ${where}, and won.`, 7, 'event', [loser.id]);
  remember(ctx, loser, `${firstName(winner)} beat me bloody at ${where}.`, 8, 'event', [winner.id]);
  chronicle(ctx, 'conflict', `${firstName(starter)} came to blows with ${firstName(starter === winner ? loser : winner)} at ${where}; ${firstName(winner)} won.`, [winner.id, loser.id]);
  ctx.events.push({ type: 'emote', id: loser.id, emote: 'anger' });
  for (const witness of witnesses(ctx, loser.pos, [winner.id, loser.id])) {
    adjust(ctx, witness, starter.id, { affinity: -7, trust: -5 });
    remember(ctx, witness, `Watched ${firstName(winner)} and ${firstName(loser)} brawl at ${where}. ${firstName(starter)} started it.`, 6, 'event', [winner.id, loser.id]);
    stimulate(ctx, witness, 'witness', `You saw ${firstName(winner)} beat ${firstName(loser)} in a fist fight that ${firstName(starter)} started.`, 1, starter.id);
  }
}

/** The attacker's answer once a beaten opponent has yielded. */
export function resolveMercy(ctx: Ctx, attacker: Character, spare: boolean, line?: string): void {
  const victim = getCharacter(ctx.state, attacker.combat.mercyTargetId);
  attacker.combat.mercyUntilTick = 0;
  attacker.combat.mercyTargetId = null;
  if (!victim) {
    endFight(attacker, 'Your opponent is gone.');
    return;
  }
  const where = placeNameAt(victim.pos);
  if (!spare) {
    if (line) say(ctx, attacker, line, victim.id, true);
    endFight(attacker, `You killed ${firstName(victim)}.`);
    setAnim(attacker, pick(ctx.state, weaponOf(attacker, 'lethal').anims), true);
    killCharacter(ctx, victim, `killed by ${attacker.profile.name}`, attacker);
    return;
  }
  say(ctx, attacker, line || 'Get out of my sight.', victim.id);
  endFight(attacker, `You had ${firstName(victim)} at your mercy and let them live.`);
  victim.combat.yielded = false;
  ctx.state.director.lastLethalTick = ctx.state.tick;
  startFlee(ctx, victim, 'reflex');
  adjust(ctx, victim, attacker.id, { grievance: 48, affinity: -60, trust: -55 });
  adjust(ctx, attacker, victim.id, { grievance: -45 });
  remember(ctx, attacker, `I drew steel on ${firstName(victim)} at ${where}. They yielded, and I let them live.`, 9, 'event', [victim.id]);
  remember(ctx, victim, `${firstName(attacker)} tried to kill me at ${where}. I begged, and was spared.`, 10, 'event', [attacker.id]);
  chronicle(ctx, 'conflict', `${attacker.profile.name} attacked ${victim.profile.name} with a weapon at ${where}, but spared them.`, [attacker.id, victim.id]);
  for (const witness of witnesses(ctx, victim.pos, [attacker.id, victim.id])) {
    adjust(ctx, witness, attacker.id, { affinity: -25, trust: -30, grievance: 10 });
    remember(ctx, witness, `Saw ${firstName(attacker)} nearly kill ${firstName(victim)} at ${where}.`, 8, 'event', [attacker.id, victim.id]);
    stimulate(ctx, witness, 'witness', `You saw ${firstName(attacker)} attack ${firstName(victim)} with a weapon; ${firstName(victim)} yielded and was spared.`, 2, attacker.id);
  }
}

function landHit(ctx: Ctx, c: Character, weapon: Weapon, mode: FightMode): void {
  const { state } = ctx;
  const targetId = c.combat.hitTargetId;
  c.combat.hitTargetId = null;
  const reach = weapon.range * (weapon.style === 'melee' ? 1.45 : 1.5);
  const damage = intRange(state, weapon.damage[0], weapon.damage[1]);
  if (mode === 'monster') {
    const monster = getMonster(state, targetId);
    if (monster && dist(c.pos, monster.pos) <= reach) damageMonster(ctx, monster, damage, c);
    return;
  }
  const victim = getCharacter(state, targetId);
  if (victim && !victim.inside && dist(c.pos, victim.pos) <= reach) {
    damageCharacter(ctx, victim, damage, { id: c.id, label: firstName(c), character: c }, mode);
  }
}

/** Drives a character whose intent is to fight. */
export function runFight(ctx: Ctx, c: Character, intent: Extract<Intent, { kind: 'fight' }>): void {
  const { state } = ctx;
  const combat = c.combat;
  const mode = intent.mode;
  const weapon = weaponOf(c, mode);

  if (combat.mercyUntilTick > 0) {
    if (state.tick >= combat.mercyUntilTick) {
      resolveMercy(ctx, c, !(c.profile.temper > 0.8 && chance(state, 0.35)));
    }
    return;
  }

  const monster = mode === 'monster' ? getMonster(state, intent.targetId) : null;
  const foe = mode === 'monster' ? null : getCharacter(state, intent.targetId);
  const targetPos = monster?.pos ?? foe?.pos ?? null;
  if (!targetPos) {
    endFight(c, mode === 'monster' ? 'The monster is destroyed.' : 'Your opponent is down.');
    return;
  }
  if (foe && (foe.inside || foe.combat.yielded)) {
    endFight(c, `${firstName(foe)} got away from you.`);
    return;
  }

  if (c.act.phase === 'start') {
    c.act.phase = 'perform';
    c.act.sinceTick = state.tick;
    // Only the one who starts it is reported; hitting back is not "going for" anyone.
    const answering = foe?.intent?.kind === 'fight' && foe.intent.targetId === c.id;
    if (foe && mode !== 'monster' && intent.source !== 'reflex' && !answering) {
      const where = placeNameAt(c.pos);
      chronicle(ctx, 'conflict', mode === 'brawl'
        ? `${firstName(c)} went for ${firstName(foe)} with ${their(c)} fists at ${where}.`
        : `${c.profile.name} drew a weapon on ${foe.profile.name} at ${where}!`, [c.id, foe.id]);
      if (mode === 'lethal') state.director.lastLethalTick = state.tick;
      ctx.events.push({ type: 'emote', id: c.id, emote: 'anger' });
    }
  }

  // A swing already in the air lands (or misses) before anything else happens.
  if (combat.hitTick > 0) {
    face(c, targetPos);
    if (state.tick >= combat.hitTick) {
      combat.hitTick = 0;
      landHit(ctx, c, weapon, mode);
    }
    return;
  }

  // Drink a potion rather than die.
  if (c.hp < maxHp(c) * 0.38 && c.inventory.potion > 0) {
    c.inventory.potion--;
    c.hp = Math.min(maxHp(c), c.hp + POTION_HEAL);
    setAnim(c, 'use_item', true);
    combat.nextAttackTick = Math.max(combat.nextAttackTick, state.tick + secondsToTicks(1.3));
    return;
  }
  // Those fighting on instinct know when they are beaten.
  if (mode === 'monster' && intent.source !== 'llm' && c.hp < maxHp(c) * (0.34 - c.profile.bravery * 0.22)) {
    startFlee(ctx, c, 'reflex');
    return;
  }

  const d = dist(c.pos, targetPos);
  if (d > weapon.range) {
    if (c.act.path.length === 0 || state.tick >= c.act.repathTick) {
      const route = routeTo(c.pos, targetPos);
      c.act.repathTick = state.tick + secondsToTicks(0.6);
      if (!route) {
        if (++c.act.stuck > 8) endFight(c, 'You could not reach your opponent.');
        return;
      }
      c.act.path = route;
    }
    advance(c, c.act.path, RUN_SPEED);
    setAnim(c, 'run');
    return;
  }

  c.act.path = [];
  c.act.stuck = 0;
  face(c, targetPos);
  if (state.tick >= combat.nextAttackTick) {
    const anim = pick(state, weapon.anims);
    setAnim(c, anim, true);
    const windup = clamp(animSeconds(anim) * 0.42, 0.3, 0.75);
    combat.hitTick = state.tick + secondsToTicks(windup);
    combat.hitTargetId = intent.targetId;
    combat.nextAttackTick = state.tick + secondsToTicks(weapon.interval);
    ctx.events.push({ type: 'attack', attackerId: c.id, targetId: intent.targetId, ranged: projectileOf(weapon), from: { ...c.pos }, to: { ...targetPos } });
  } else if (!isOneShotPlaying(c, combat.nextAttackTick - secondsToTicks(weapon.interval), state.tick)) {
    setAnim(c, weapon.stance);
  }
}
