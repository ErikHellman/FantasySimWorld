import type { Vec2 } from '../core/math2d';
import type { ActivityId } from '../world/places';
import type { AnimId } from '../render/anims';
import type { ClassId } from './data/classes';
import type { MonsterKind } from './data/monsters';

/**
 * The whole simulation state. Plain JSON: ids instead of references, no
 * classes, RNG state included, so it can be saved, loaded and compared.
 */
export const SAVE_VERSION = 1;

export type Id = string;

/** Reserved for text-to-speech: how this character should sound. */
export interface VoiceProfile {
  /** 0.5 (deep) .. 1.5 (high) */
  pitch: number;
  /** 0.7 (slow) .. 1.3 (fast) */
  rate: number;
  timbre: string;
}

export interface Profile {
  name: string;
  pronouns: 'she/her' | 'he/him' | 'they/them';
  age: number;
  occupation: string;
  backstory: string;
  traits: string[];
  goals: string[];
  speechStyle: string;
  /** 0 calm .. 1 hot-headed */
  temper: number;
  /** 0 timid .. 1 fearless */
  bravery: number;
  /** 0 solitary .. 1 gregarious */
  sociability: number;
  voice: VoiceProfile;
}

export interface Look {
  model: string;
  /** texture id override (recolour); null keeps the model's own */
  texture: string | null;
  /** mesh names to hide (helmet, cape, hat) */
  hide: string[];
  mainHand: string | null;
  offHand: string | null;
}

/** 0..100, higher is better: fed, rested, in good company. */
export interface Needs {
  hunger: number;
  energy: number;
  social: number;
}

export type ItemId = 'food' | 'potion' | 'gift';
export type Inventory = Record<ItemId, number>;

export type RelationLabel = 'stranger' | 'acquaintance' | 'friend' | 'close friend' | 'lover' | 'rival' | 'enemy' | 'family';

export interface Relationship {
  /** -100 (hatred) .. 100 (devotion) */
  affinity: number;
  /** -100 .. 100 */
  trust: number;
  /** 0 .. 100 romantic interest */
  attraction: number;
  /** 0 .. 100 unresolved anger; drives the hostility ladder */
  grievance: number;
  /** 0 .. 100 how well they know each other */
  familiarity: number;
  label: RelationLabel;
  kin: boolean;
  /** one line the character would use to describe the other; updated by reflection */
  summary: string;
  lastTalkTick: number;
  /** tick of the last thing that made this character angry at the other */
  lastOffenceTick: number;
}

export type MemoryKind = 'event' | 'talk' | 'thought' | 'reflection';

export interface Memory {
  tick: number;
  day: number;
  text: string;
  /** 1 (trivial) .. 10 (life-changing) */
  importance: number;
  kind: MemoryKind;
  subjects: Id[];
}

export interface PlanBlock {
  /** hour of day the block starts, 0..24 */
  hour: number;
  activity: ActivityId;
  placeId?: string;
  note?: string;
}

export type TalkTone = 'friendly' | 'romantic' | 'hostile' | 'apology';
export type FightMode = 'monster' | 'brawl' | 'lethal';

export type IntentAction =
  | { kind: 'go_to'; placeId: string }
  | { kind: 'activity'; activity: ActivityId; placeId?: string; minutes?: number }
  | { kind: 'talk'; targetId: Id; opener: string; tone: TalkTone }
  | { kind: 'give'; targetId: Id; item: ItemId }
  | { kind: 'fight'; targetId: Id; mode: FightMode }
  | { kind: 'flee' }
  | { kind: 'wait'; minutes: number };

export type Intent = IntentAction & {
  source: 'routine' | 'llm' | 'reflex';
  /** said aloud when the intent starts */
  say?: string;
  /** private reasoning, kept as a memory */
  thought?: string;
};

/** Progress of the current intent. */
export interface Act {
  phase: 'start' | 'travel' | 'perform' | 'done';
  path: Vec2[];
  dest: Vec2 | null;
  /** tick the current phase began */
  sinceTick: number;
  /** when a timed phase ends */
  untilTick: number;
  repathTick: number;
  /** ticks spent making no progress */
  stuck: number;
}

export type StimulusKind =
  | 'threat' | 'attacked' | 'alarm' | 'need' | 'block_ended' | 'witness' | 'death' | 'gift'
  | 'talked' | 'arrival' | 'woke' | 'victory' | 'hostility' | 'romance';

/** Something that just happened to a character and may deserve a decision. */
export interface Stimulus {
  kind: StimulusKind;
  tick: number;
  text: string;
  subjectId?: Id;
  /** 0 (idle curiosity) .. 3 (life or death) */
  urgency: number;
}

export interface CombatState {
  targetId: Id | null;
  mode: FightMode;
  nextAttackTick: number;
  /** a swing in flight: lands on this tick */
  hitTick: number;
  hitTargetId: Id | null;
  lastHitByTick: number;
  lastHitById: Id | null;
  /** hp at or below this ends a brawl or lethal duel with a yield */
  yielded: boolean;
  /** attacker waiting for a spare / finish decision until this tick */
  mercyUntilTick: number;
  mercyTargetId: Id | null;
}

export interface Character {
  id: Id;
  profile: Profile;
  classId: ClassId;
  look: Look;
  homeId: string;
  /** work this character usually does */
  jobs: ActivityId[];
  /** leisure this character enjoys */
  hobbies: ActivityId[];
  arrivedTick: number;

  pos: Vec2;
  facing: number;
  /** moved this tick (for separation and footsteps) */
  moving: boolean;
  hp: number;
  needs: Needs;
  inventory: Inventory;

  plan: PlanBlock[];
  planDay: number;
  planSource: 'routine' | 'llm';
  /** index of the plan block that last produced an intent */
  planBlock: number;
  intent: Intent | null;
  queue: Intent[];
  act: Act;
  slotId: string | null;
  /** place id while indoors (not visible, safe from monsters) */
  inside: string | null;
  asleep: boolean;
  /** on night watch until this tick */
  watchUntilTick: number;
  /** sleeps in after a night watch until this tick */
  sleepInUntilTick: number;

  anim: AnimId;
  animSeq: number;
  /** tool held instead of the weapon while working ("bundle/model") */
  tool: string | null;

  convoId: Id | null;
  partnerId: Id | null;
  combat: CombatState;
  /** bumped whenever the situation changes enough to invalidate a pending decision */
  epoch: number;
  thought: string;
  /** what came of the last intent, for the next decision */
  outcome: string;
  stimuli: Stimulus[];
  relationships: Record<Id, Relationship>;
  memories: Memory[];
  /** rolling self-summary written by reflection */
  summary: string;
  lastReflectionDay: number;
  kills: number;
  /** set when this character has killed another villager */
  killer: boolean;
  nextSocialTick: number;
  nextPerceptionTick: number;
}

export interface Monster {
  id: Id;
  kind: MonsterKind;
  look: Look;
  pos: Vec2;
  facing: number;
  moving: boolean;
  hp: number;
  phase: 'rising' | 'roam' | 'chase' | 'dying';
  sinceTick: number;
  targetId: Id | null;
  path: Vec2[];
  repathTick: number;
  nextAttackTick: number;
  hitTick: number;
  hitTargetId: Id | null;
  nextSummonTick: number;
  anim: AnimId;
  animSeq: number;
  stuck: number;
}

export type Feeling = 'much_worse' | 'worse' | 'same' | 'better' | 'much_better';

export interface ConversationLine {
  speakerId: Id;
  text: string;
}

export interface Conversation {
  id: Id;
  /** initiator */
  a: Id;
  b: Id;
  tone: TalkTone;
  phase: 'hail' | 'play' | 'await';
  lines: ConversationLine[];
  /** index of the line being spoken, or the next one to speak */
  index: number;
  lineEndTick: number;
  startedTick: number;
  /** while awaiting: give up and use local lines at this tick */
  awaitUntilTick: number;
  /** no more lines will be added */
  closed: boolean;
  /** who wrote the lines so far */
  author: 'local' | 'llm';
  topic: string;
  feelings: Record<Id, Feeling>;
  /** locally written continuation, used if no LLM lines arrive in time */
  fallback: { lines: ConversationLine[]; topic: string; feelings: Record<Id, Feeling> };
}

export interface Grave {
  plot: number;
  characterId: Id;
  name: string;
  day: number;
  cause: string;
}

/** A body left where someone fell; shown for a while, then removed. */
export interface Corpse {
  id: Id;
  look: Look;
  pos: Vec2;
  facing: number;
  anim: AnimId;
  untilTick: number;
}

export interface DeadRecord {
  name: string;
  day: number;
  cause: string;
  killerId: Id | null;
}

export type ChronicleKind = 'world' | 'social' | 'romance' | 'conflict' | 'combat' | 'death' | 'arrival';

export interface ChronicleEntry {
  tick: number;
  kind: ChronicleKind;
  text: string;
  ids: Id[];
}

export interface Director {
  nextWaveTick: number;
  nextArrivalTick: number;
  lastLethalTick: number;
  /** fighters keep responding to an alarm until this tick */
  alarmUntilTick: number;
  alarmPos: Vec2 | null;
  watchDay: number;
  deathTicks: number[];
  arrivals: number;
}

/** Knobs the brain layer may turn (through a command) without breaking determinism. */
export interface Tuning {
  /** how eagerly characters start conversations on their own, 0..1 */
  chatter: number;
  /** ticks a conversation waits for LLM-written lines before using local ones; 0 = never wait */
  scriptWaitTicks: number;
  /** ticks an attacker waits for a spare / finish decision */
  mercyWaitTicks: number;
}

export interface SimState {
  version: number;
  seed: number;
  rng: number;
  tick: number;
  nextId: number;
  characters: Record<Id, Character>;
  /** ids of living characters in a stable order */
  order: Id[];
  monsters: Record<Id, Monster>;
  conversations: Record<Id, Conversation>;
  graves: Grave[];
  corpses: Corpse[];
  dead: Record<Id, DeadRecord>;
  chronicle: ChronicleEntry[];
  /** slot id -> character id */
  slots: Record<string, Id>;
  director: Director;
  tuning: Tuning;
}

export function newId(state: SimState, prefix: string): Id {
  return `${prefix}${state.nextId++}`;
}
