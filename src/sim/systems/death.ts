import { dist } from '../../core/math2d';
import { minutesToTicks, TICKS_PER_DAY } from '../../core/time';
import { range } from '../../core/rng';
import { getPlace } from '../../world/places';
import type { Character } from '../state';
import { chronicle, firstName, living, placeNameAt, rel, remember, stimulate, type Ctx } from '../util';
import { abortConversation } from './conversation';
import { adjust } from './relationships';

const CORPSE_MINUTES = 50;

/** Releases whatever a character is holding on to in the world (seat, conversation). */
export function detach(ctx: Ctx, c: Character): void {
  if (c.convoId) abortConversation(ctx, c.convoId);
  if (c.slotId) {
    if (ctx.state.slots[c.slotId] === c.id) delete ctx.state.slots[c.slotId];
    c.slotId = null;
  }
}

function freeGravePlot(ctx: Ctx): number {
  const plots = getPlace('graveyard').gravePlots.length;
  const used = new Set(ctx.state.graves.map((grave) => grave.plot));
  for (let i = 0; i < plots; i++) if (!used.has(i)) return i;
  // The graveyard is full: the oldest grave gives up its place.
  const oldest = ctx.state.graves.shift();
  return oldest ? oldest.plot : 0;
}

/**
 * Permadeath. The character leaves the living, gets a grave, and everyone who
 * knew them reacts. `killer` is set when another villager did it.
 */
export function killCharacter(ctx: Ctx, victim: Character, cause: string, killer: Character | null): void {
  const { state } = ctx;
  detach(ctx, victim);
  const name = firstName(victim);
  const where = placeNameAt(victim.pos);

  state.corpses.push({
    id: victim.id, look: victim.look, pos: { ...victim.pos }, facing: victim.facing, anim: 'death',
    untilTick: state.tick + minutesToTicks(CORPSE_MINUTES),
  });
  state.graves.push({ plot: freeGravePlot(ctx), characterId: victim.id, name: victim.profile.name, day: ctx.time.day, cause });
  state.dead[victim.id] = { name: victim.profile.name, day: ctx.time.day, cause, killerId: killer?.id ?? null };
  delete state.characters[victim.id];
  state.order = state.order.filter((id) => id !== victim.id);

  for (const other of living(state)) {
    if (other.combat.targetId === victim.id) other.combat.targetId = null;
    // Whoever had yielded to the dead character is free again.
    if (other.combat.yielded && other.combat.lastHitById === victim.id) other.combat.yielded = false;
    if (other.combat.mercyTargetId === victim.id) {
      other.combat.mercyTargetId = null;
      other.combat.mercyUntilTick = 0;
    }
    const r = rel(other, victim.id);
    const wasPartner = other.partnerId === victim.id;
    if (wasPartner) other.partnerId = null;
    const close = wasPartner || r.kin || r.affinity >= 25;
    const saw = !other.inside && dist(other.pos, victim.pos) < 28;
    const byWhom = killer ? `killed by ${killer.id === other.id ? 'my own hand' : firstName(killer)}` : cause;

    if (killer && other.id === killer.id) {
      remember(ctx, other, `I killed ${name} at ${where}. There is no taking it back.`, 10, 'event', [victim.id]);
      continue;
    }
    if (close) {
      remember(ctx, other, `${name} is dead, ${byWhom}. ${wasPartner ? 'My love is gone.' : r.kin ? 'My own blood.' : 'I have lost a friend.'}`, wasPartner || r.kin ? 10 : 8, 'event', [victim.id]);
      stimulate(ctx, other, 'death', `${name}, who was dear to you, has died: ${byWhom}.`, 2, victim.id);
      ctx.events.push({ type: 'emote', id: other.id, emote: 'grief' });
    } else if (saw) {
      remember(ctx, other, `Saw ${name} die at ${where}, ${byWhom}.`, 7, 'event', [victim.id]);
      stimulate(ctx, other, 'death', `You saw ${name} die: ${byWhom}.`, 1, victim.id);
    } else if (r.affinity <= -25) {
      remember(ctx, other, `${name} is dead, ${byWhom}. I feel less than I thought I would.`, 5, 'event', [victim.id]);
    } else {
      remember(ctx, other, `Heard that ${name} died, ${byWhom}.`, 5, 'event', [victim.id]);
    }

    if (killer) {
      // The whole village turns on a killer; those who loved the victim most of all.
      adjust(ctx, other, killer.id, close
        ? { affinity: -70, trust: -60, grievance: 70 }
        : saw
          ? { affinity: -45, trust: -50, grievance: 35 }
          : { affinity: -28, trust: -35, grievance: 12 });
    }
  }

  if (killer) {
    killer.killer = true;
    state.director.lastLethalTick = state.tick;
  }
  chronicle(ctx, 'death', killer
    ? `${victim.profile.name} was killed by ${killer.profile.name} at ${where}.`
    : `${victim.profile.name} has died, ${cause}.`, killer ? [victim.id, killer.id] : [victim.id]);
  ctx.events.push({ type: 'death', id: victim.id, name: victim.profile.name, pos: { ...victim.pos }, monster: false });
  ctx.events.push({ type: 'save_point', reason: 'death' });

  state.director.deathTicks.push(state.tick);
  const wait = state.order.length < 8 ? range(state, 0.4, 0.9) : range(state, 1.2, 2.4);
  state.director.nextArrivalTick = Math.max(state.director.nextArrivalTick, state.tick + Math.round(wait * TICKS_PER_DAY));
}
