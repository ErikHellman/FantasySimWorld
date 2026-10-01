import { gameTime } from '../core/time';
import { findPlace, type ActivityId } from '../world/places';
import { ACTIVITIES } from './data/activities';
import type { Command } from './commands';
import type { SimEvent } from './events';
import type { Character, Intent, PlanBlock, SimState } from './state';
import { resolveMercy, startFight, startFlee } from './systems/combat';
import { addLines, forceLocal, updateConversations } from './systems/conversation';
import { detach } from './systems/death';
import { updateDirector } from './systems/director';
import { freshAct, leaveBuilding, updateIntents } from './systems/intents';
import { updateMonsters } from './systems/monsters';
import { separate } from './systems/movement';
import { updateNeeds } from './systems/needs';
import { updatePerception } from './systems/perception';
import { RUNG_FEUD, dailyRelationships, lethalUnlocked } from './systems/relationships';
import { blockAt, updateRoutine } from './systems/routine';
import { chronicle, getCharacter, getMonster, makeCtx, rel, remember, setAnim, type Ctx } from './util';

const MAX_QUEUE = 3;

/** Whether an intent still makes sense for this character right now. */
function intentAllowed(ctx: Ctx, c: Character, intent: Intent): boolean {
  switch (intent.kind) {
    case 'go_to':
      return findPlace(intent.placeId) !== null;
    case 'activity':
      return intent.activity in ACTIVITIES && (!intent.placeId || findPlace(intent.placeId) !== null);
    case 'talk':
    case 'give':
      return intent.targetId !== c.id && getCharacter(ctx.state, intent.targetId) !== null;
    case 'fight': {
      if (intent.mode === 'monster') return getMonster(ctx.state, intent.targetId) !== null;
      const foe = getCharacter(ctx.state, intent.targetId);
      if (!foe || foe.id === c.id) return false;
      // Being attacked always allows fighting back; starting it needs the ladder.
      if (foe.intent?.kind === 'fight' && foe.intent.targetId === c.id) return true;
      return intent.mode === 'brawl' ? rel(c, foe.id).grievance >= RUNG_FEUD : lethalUnlocked(ctx, c, foe.id);
    }
    case 'flee':
    case 'wait':
      return true;
  }
}

function applyIntents(ctx: Ctx, c: Character, intents: Intent[]): void {
  const valid = intents.filter((intent) => intentAllowed(ctx, c, intent)).slice(0, MAX_QUEUE);
  const first = valid[0];
  if (!first) return;
  if (first.kind === 'fight') {
    startFight(ctx, c, first.targetId, first.mode, first.source, first.say);
    if (first.thought) c.thought = first.thought;
    return;
  }
  if (first.kind === 'flee') {
    startFlee(ctx, c, first.source);
    if (first.thought) c.thought = first.thought;
    return;
  }
  detach(ctx, c);
  if (c.asleep) leaveBuilding(c);
  c.tool = null;
  c.combat.targetId = null;
  c.combat.hitTick = 0;
  c.intent = first;
  c.queue = valid.slice(1);
  c.act = freshAct(ctx.state.tick);
  if (c.anim !== 'idle' && (c.anim === 'sit_chair' || c.anim === 'sit_floor')) setAnim(c, 'idle');
}

function applyPlan(ctx: Ctx, c: Character, blocks: PlanBlock[], thought?: string): void {
  const valid = blocks
    .filter((block) => block.activity in ACTIVITIES && Number.isFinite(block.hour) && block.hour >= 0 && block.hour < 24 && (!block.placeId || findPlace(block.placeId)))
    .map((block) => ({ hour: block.hour, activity: block.activity as ActivityId, placeId: block.placeId, note: block.note?.slice(0, 120) }))
    .sort((a, b) => a.hour - b.hour);
  if (valid.length < 2) return;
  // Everyone sleeps: a plan that forgets to is given a bedtime.
  if (!valid.some((block) => block.activity === 'sleep')) valid.push({ hour: 22, activity: 'sleep', placeId: undefined, note: undefined });
  c.plan = valid;
  c.planDay = ctx.time.day;
  c.planSource = 'llm';
  c.planBlock = blockAt(valid, ctx.time.hour);
  if (thought) {
    c.thought = thought;
    remember(ctx, c, `My plan for today: ${thought}`, 3, 'thought');
  }
}

function applyCommand(ctx: Ctx, command: Command): void {
  const { state } = ctx;
  switch (command.type) {
    case 'set_plan': {
      const c = getCharacter(state, command.id);
      if (c) applyPlan(ctx, c, command.blocks, command.thought);
      break;
    }
    case 'set_intents': {
      const c = getCharacter(state, command.id);
      // A decision made for a situation that has since changed is dropped.
      if (!c || c.epoch !== command.epoch || c.convoId || c.combat.yielded || c.combat.mercyUntilTick > 0) break;
      applyIntents(ctx, c, command.intents);
      break;
    }
    case 'conversation_lines':
      addLines(ctx, command.convoId, command.lines, command.closed, command.topic, command.feelings);
      break;
    case 'conversation_local':
      forceLocal(ctx, command.convoId);
      break;
    case 'mercy': {
      const attacker = getCharacter(state, command.attackerId);
      if (attacker && attacker.combat.mercyUntilTick > 0) resolveMercy(ctx, attacker, command.spare, command.say);
      break;
    }
    case 'reflection': {
      const c = getCharacter(state, command.id);
      if (!c) break;
      if (command.summary.trim()) c.summary = command.summary.trim().slice(0, 600);
      for (const insight of command.insights.slice(0, 4)) {
        if (insight.trim()) remember(ctx, c, insight.trim().slice(0, 240), 7, 'reflection');
      }
      for (const entry of command.relationships) {
        if (state.characters[entry.id] && entry.id !== c.id && entry.summary.trim()) rel(c, entry.id).summary = entry.summary.trim().slice(0, 200);
      }
      c.lastReflectionDay = ctx.time.day;
      break;
    }
    case 'thought': {
      const c = getCharacter(state, command.id);
      if (c) c.thought = command.text.slice(0, 240);
      break;
    }
    case 'tuning':
      Object.assign(state.tuning, command.tuning);
      break;
  }
}

/**
 * The simulation. `step()` advances one tick (1/20 s of real time at 1x) and
 * returns that tick's one-shot events. Commands are applied at tick boundaries.
 */
export class Sim {
  private pending: Command[] = [];

  constructor(public state: SimState) {}

  enqueue(command: Command): void {
    this.pending.push(command);
  }

  step(): SimEvent[] {
    const state = this.state;
    const previousDay = gameTime(state.tick).day;
    state.tick++;
    const events: SimEvent[] = [];
    const ctx = makeCtx(state, events);

    const commands = this.pending;
    this.pending = [];
    for (const command of commands) applyCommand(ctx, command);

    if (ctx.time.day !== previousDay) {
      dailyRelationships(ctx);
      events.push({ type: 'new_day', day: ctx.time.day }, { type: 'save_point', reason: 'new day' });
    }
    if (Math.abs(ctx.time.minuteOfDay - 360) < 0.026 && state.tick > 1) {
      chronicle(ctx, 'world', `Day ${ctx.time.day} dawns over the valley.`);
    }

    updateDirector(ctx);
    updateRoutine(ctx);
    updateIntents(ctx);
    updateConversations(ctx);
    updateMonsters(ctx);
    updatePerception(ctx);
    separate(state);
    updateNeeds(ctx);
    return events;
  }
}
