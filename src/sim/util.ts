import { clamp, dist, type Vec2 } from '../core/math2d';
import { gameTime, secondsToTicks, type GameTime } from '../core/time';
import { nearestPlace } from '../world/places';
import type { AnimId } from './data/anims';
import type { SimEvent } from './events';
import type {
  Character, ChronicleKind, Id, Memory, MemoryKind, Monster, Relationship, SimState, Stimulus, StimulusKind,
} from './state';

/** Per-tick context handed to every system. */
export interface Ctx {
  state: SimState;
  events: SimEvent[];
  time: GameTime;
}

export const MEMORY_CAP = 140;
export const CHRONICLE_CAP = 600;
const STIMULUS_CAP = 8;

export function makeCtx(state: SimState, events: SimEvent[]): Ctx {
  return { state, events, time: gameTime(state.tick) };
}

/** Living characters in a stable order. */
export function living(state: SimState): Character[] {
  const result: Character[] = [];
  for (const id of state.order) {
    const c = state.characters[id];
    if (c) result.push(c);
  }
  return result;
}

export function getCharacter(state: SimState, id: Id | null | undefined): Character | null {
  return id ? (state.characters[id] ?? null) : null;
}

export function getMonster(state: SimState, id: Id | null | undefined): Monster | null {
  if (!id) return null;
  const monster = state.monsters[id];
  return monster && monster.phase !== 'dying' ? monster : null;
}

export function firstName(c: Character): string {
  return c.profile.name.split(' ')[0] as string;
}

export function nameOf(state: SimState, id: Id): string {
  const c = state.characters[id];
  if (c) return firstName(c);
  const dead = state.dead[id];
  return dead ? (dead.name.split(' ')[0] as string) : 'someone';
}

export function newRelationship(): Relationship {
  return {
    affinity: 0, trust: 0, attraction: 0, grievance: 0, familiarity: 0,
    label: 'stranger', kin: false, summary: '', lastTalkTick: -1e9, lastOffenceTick: -1e9,
  };
}

/** The relationship record `from` keeps about `toId`, created on first use. */
export function rel(from: Character, toId: Id): Relationship {
  let r = from.relationships[toId];
  if (!r) {
    r = newRelationship();
    from.relationships[toId] = r;
  }
  return r;
}

export function setAnim(entity: { anim: AnimId; animSeq: number }, anim: AnimId, restart = false): void {
  if (entity.anim !== anim || restart) {
    entity.anim = anim;
    entity.animSeq++;
  }
}

/** How long a line stays up: reading time now, speaking time once there are voices. */
export function speechTicks(text: string): number {
  return secondsToTicks(clamp(1.3 + text.length * 0.055, 2, 9));
}

export function say(ctx: Ctx, speaker: Character, text: string, toId: Id | null = null, loud = false): number {
  const durationTicks = speechTicks(text);
  ctx.events.push({ type: 'speech', speakerId: speaker.id, toId, text, durationTicks, loud });
  return durationTicks;
}

export function remember(ctx: Ctx, c: Character, text: string, importance: number, kind: MemoryKind = 'event', subjects: Id[] = []): void {
  const memory: Memory = { tick: ctx.state.tick, day: ctx.time.day, text, importance: clamp(Math.round(importance), 1, 10), kind, subjects };
  c.memories.push(memory);
  if (c.memories.length > MEMORY_CAP) pruneMemories(c, ctx.state.tick);
}

/** Drops the least valuable memories: old and unimportant go first, reflections last. */
export function pruneMemories(c: Character, tick: number): void {
  const keep = Math.floor(MEMORY_CAP * 0.8);
  const scored = c.memories.map((memory, index) => {
    const ageDays = (tick - memory.tick) / (20 * 60 * 24);
    const score = memory.importance * 1.6 - ageDays * 1.2 + (memory.kind === 'reflection' ? 6 : 0);
    return { index, score };
  });
  scored.sort((a, b) => b.score - a.score);
  const kept = new Set(scored.slice(0, keep).map((entry) => entry.index));
  c.memories = c.memories.filter((_, index) => kept.has(index));
}

export function chronicle(ctx: Ctx, kind: ChronicleKind, text: string, ids: Id[] = []): void {
  const entry = { tick: ctx.state.tick, kind, text, ids };
  ctx.state.chronicle.push(entry);
  if (ctx.state.chronicle.length > CHRONICLE_CAP) ctx.state.chronicle.splice(0, ctx.state.chronicle.length - CHRONICLE_CAP);
  ctx.events.push({ type: 'chronicle', entry });
}

export function stimulate(ctx: Ctx, c: Character, kind: StimulusKind, text: string, urgency: number, subjectId?: Id): void {
  const stimulus: Stimulus = { kind, tick: ctx.state.tick, text, urgency, subjectId };
  c.stimuli.push(stimulus);
  if (c.stimuli.length > STIMULUS_CAP) c.stimuli.shift();
}

export function hasRecentStimulus(c: Character, kind: StimulusKind, sinceTick: number, subjectId?: Id): boolean {
  return c.stimuli.some((s) => s.kind === kind && s.tick >= sinceTick && (subjectId === undefined || s.subjectId === subjectId));
}

/** Out in the open and conscious: can be seen, spoken to or attacked. */
export function isPresent(c: Character): boolean {
  return !c.inside && !c.asleep;
}

export function isFighting(c: Character): boolean {
  return c.intent?.kind === 'fight' || c.intent?.kind === 'flee';
}

/** Free to be approached for a chat or a gift. */
export function isApproachable(c: Character): boolean {
  return isPresent(c) && !c.convoId && !isFighting(c) && !c.combat.yielded;
}

export function hpFraction(c: Character, maxHp: number): number {
  return clamp(c.hp / maxHp, 0, 1);
}

export function placeNameAt(pos: Vec2): string {
  const place = nearestPlace(pos, 16);
  if (place) return place.name;
  const landmark = nearestPlace(pos, 80);
  return landmark ? `the fields near ${landmark.name}` : 'the wilds';
}

export function nearbyCharacters(state: SimState, pos: Vec2, radius: number, exceptId?: Id): Character[] {
  return living(state).filter((c) => c.id !== exceptId && !c.inside && dist(c.pos, pos) <= radius);
}

export function nearbyMonsters(state: SimState, pos: Vec2, radius: number): Monster[] {
  const result: Monster[] = [];
  for (const id in state.monsters) {
    const m = state.monsters[id] as Monster;
    if (m.phase !== 'dying' && dist(m.pos, pos) <= radius) result.push(m);
  }
  return result;
}

export function monsterList(state: SimState): Monster[] {
  return Object.keys(state.monsters).map((id) => state.monsters[id] as Monster);
}

/** Invalidate any decision that was requested before now. */
export function bumpEpoch(c: Character): void {
  c.epoch++;
}

export function joinNames(names: string[]): string {
  if (names.length <= 1) return names[0] ?? '';
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/** Possessive pronoun for narration: "her", "his" or "their". */
export function their(c: Character): string {
  return c.profile.pronouns === 'she/her' ? 'her' : c.profile.pronouns === 'he/him' ? 'his' : 'their';
}

/**
 * Whether two people could fall for each other at all: a fixed property of
 * the pair, so romance grows between some and never between others.
 */
export function chemistry(a: Character, b: Character): boolean {
  const key = a.id < b.id ? `${a.id}|${b.id}` : `${b.id}|${a.id}`;
  let h = 2166136261;
  for (let i = 0; i < key.length; i++) h = Math.imul(h ^ key.charCodeAt(i), 16777619);
  return (h >>> 0) % 100 < 38;
}
