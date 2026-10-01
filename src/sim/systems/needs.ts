import { GAME_MINUTES_PER_TICK, minutesToTicks } from '../../core/time';
import type { Character } from '../state';
import { hasRecentStimulus, living, stimulate, type Ctx } from '../util';
import { maxHp } from './combat';

/** Change per game minute while awake. A full belly lasts about 15 hours, a night's sleep about 18. */
const HUNGER_PER_MINUTE = -0.11;
const ENERGY_PER_MINUTE = -0.092;
const SOCIAL_PER_MINUTE = -0.075;
const HP_PER_MINUTE = 0.03;
const STIMULUS_LIFETIME = minutesToTicks(100);

function warn(ctx: Ctx, c: Character, text: string): void {
  if (!hasRecentStimulus(c, 'need', ctx.state.tick - minutesToTicks(90))) stimulate(ctx, c, 'need', text, 1);
}

export function updateNeeds(ctx: Ctx): void {
  const dt = GAME_MINUTES_PER_TICK;
  for (const c of living(ctx.state)) {
    c.needs.hunger = Math.max(0, c.needs.hunger + HUNGER_PER_MINUTE * dt * (c.asleep ? 0.5 : 1));
    if (!c.asleep) {
      c.needs.energy = Math.max(0, c.needs.energy + ENERGY_PER_MINUTE * dt);
      c.needs.social = Math.max(0, c.needs.social + SOCIAL_PER_MINUTE * dt * (0.5 + c.profile.sociability));
    }
    const fighting = c.intent?.kind === 'fight';
    if (!fighting && c.needs.hunger > 20) c.hp = Math.min(maxHp(c), c.hp + HP_PER_MINUTE * dt);

    if (ctx.state.tick % 40 !== 0) continue;
    // What happened hours ago is memory, not news: it no longer calls for a reaction.
    if (c.stimuli.length > 0) c.stimuli = c.stimuli.filter((stimulus) => ctx.state.tick - stimulus.tick < STIMULUS_LIFETIME);
    if (c.asleep) continue;
    if (c.needs.hunger < 18) warn(ctx, c, 'You are very hungry.');
    else if (c.needs.energy < 14) warn(ctx, c, 'You are exhausted and need sleep.');
    else if (c.needs.social < 12 && c.profile.sociability > 0.4) warn(ctx, c, 'You feel lonely and want company.');
  }
}
