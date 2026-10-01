import { signal } from '@preact/signals';
import { formatClock, gameTime, partOfDay } from '../core/time';
import { DEFAULT_SETTINGS, type Settings } from '../persist/settings';
import { ACTIVITIES } from '../sim/data/activities';
import { CLASSES } from '../sim/data/classes';
import { describeActivity } from '../sim/describe';
import type { ChronicleEntry, Character, Memory, RelationLabel, SimState } from '../sim/state';
import { rungOf } from '../sim/systems/relationships';
import { blockAt } from '../sim/systems/routine';
import { living } from '../sim/util';

export interface RosterEntry {
  id: string;
  name: string;
  role: string;
  status: string;
  hp: number;
  mood: 'ok' | 'fight' | 'sleep' | 'talk' | 'hurt';
  partner: boolean;
}

export interface RelationRow {
  id: string;
  name: string;
  label: RelationLabel;
  affinity: number;
  attraction: number;
  grievance: number;
  rung: string;
  summary: string;
  alive: boolean;
}

export interface CharacterDetail {
  id: string;
  name: string;
  pronouns: string;
  age: number;
  occupation: string;
  className: string;
  backstory: string;
  traits: string[];
  goals: string[];
  status: string;
  thought: string;
  summary: string;
  hp: number;
  maxHp: number;
  needs: { hunger: number; energy: number; social: number };
  inventory: { food: number; potion: number; gift: number };
  home: string;
  kills: number;
  planSource: string;
  plan: Array<{ time: string; text: string; current: boolean }>;
  relations: RelationRow[];
  memories: Memory[];
}

export interface LlmStatus {
  state: 'off' | 'ready' | 'busy' | 'error';
  inFlight: number;
  queued: number;
  requests: number;
  failures: number;
  tokens: number;
  latencyMs: number;
  protocol: string;
  message: string;
}

export interface BrainLogEntry {
  id: number;
  time: string;
  character: string;
  kind: string;
  status: 'ok' | 'fallback' | 'error' | 'stale';
  latencyMs: number;
  tokens: number;
  summary: string;
  prompt: string;
  response: string;
}

export interface AudioSlotStatus {
  id: string;
  label: string;
  loaded: boolean;
}

export const ui = {
  loading: signal<string | null>('Loading the valley…'),
  started: signal(false),
  backend: signal(''),
  clock: signal({ day: 1, time: '06:00', part: 'early morning' }),
  speed: signal(1),
  population: signal(0),
  monsters: signal(0),
  alarm: signal(false),
  roster: signal<RosterEntry[]>([]),
  selectedId: signal<string | null>(null),
  detail: signal<CharacterDetail | null>(null),
  chronicle: signal<ChronicleEntry[]>([]),
  graves: signal<Array<{ name: string; day: number; cause: string }>>([]),
  panel: signal<'none' | 'settings' | 'brain' | 'graves'>('none'),
  settings: signal<Settings>(DEFAULT_SETTINGS),
  llm: signal<LlmStatus>({ state: 'off', inFlight: 0, queued: 0, requests: 0, failures: 0, tokens: 0, latencyMs: 0, protocol: '', message: '' }),
  brainLog: signal<BrainLogEntry[]>([]),
  audioSlots: signal<AudioSlotStatus[]>([]),
  notice: signal<string | null>(null),
  savedAt: signal(0),
  canSave: signal(true),
};

/** Things the UI can ask the app to do; filled in by main.ts. */
export interface UiActions {
  start(): void;
  setSpeed(speed: number): void;
  select(id: string | null): void;
  applySettings(settings: Settings): void;
  testConnection(settings: Settings): Promise<string>;
  listModels(settings: Settings): Promise<string[]>;
  exportSave(): void;
  importSave(file: File): void;
  resetWorld(): void;
}

export const actions: UiActions = {
  start: () => {},
  setSpeed: () => {},
  select: () => {},
  applySettings: () => {},
  testConnection: async () => '',
  listModels: async () => [],
  exportSave: () => {},
  importSave: () => {},
  resetWorld: () => {},
};

function moodOf(c: Character): RosterEntry['mood'] {
  if (c.intent?.kind === 'fight' || c.intent?.kind === 'flee') return 'fight';
  if (c.asleep) return 'sleep';
  if (c.convoId) return 'talk';
  if (c.hp < CLASSES[c.classId].maxHp * 0.5) return 'hurt';
  return 'ok';
}

function detailOf(state: SimState, c: Character): CharacterDetail {
  const hour = gameTime(state.tick).hour;
  const current = blockAt(c.plan, hour);
  const relations: RelationRow[] = Object.keys(c.relationships)
    .map((id) => {
      const r = c.relationships[id]!;
      const other = state.characters[id];
      const dead = state.dead[id];
      return {
        id,
        name: other ? other.profile.name : dead ? `${dead.name} †` : '',
        label: r.label,
        affinity: r.affinity,
        attraction: r.attraction,
        grievance: r.grievance,
        rung: rungOf(r),
        summary: r.summary,
        alive: other !== undefined,
      };
    })
    .filter((row) => row.name && (row.alive || Math.abs(row.affinity) >= 25))
    .sort((a, b) => Math.abs(b.affinity) + b.grievance + b.attraction - (Math.abs(a.affinity) + a.grievance + a.attraction));
  return {
    id: c.id,
    name: c.profile.name,
    pronouns: c.profile.pronouns,
    age: c.profile.age,
    occupation: c.profile.occupation,
    className: CLASSES[c.classId].label,
    backstory: c.profile.backstory,
    traits: c.profile.traits,
    goals: c.profile.goals,
    status: describeActivity(state, c),
    thought: c.thought,
    summary: c.summary,
    hp: c.hp,
    maxHp: CLASSES[c.classId].maxHp,
    needs: { ...c.needs },
    inventory: { ...c.inventory },
    home: c.homeId,
    kills: c.kills,
    planSource: c.planSource,
    plan: c.plan.map((block, index) => ({
      time: formatClock(block.hour * 60),
      text: `${ACTIVITIES[block.activity].label}${block.note ? ` — ${block.note}` : ''}`,
      current: index === current,
    })),
    relations,
    memories: c.memories.slice(-40).reverse(),
  };
}

/** Copies what the HUD shows out of the sim state. Called a few times a second, not every tick. */
export function refreshUi(state: SimState): void {
  const time = gameTime(state.tick);
  ui.clock.value = { day: time.day, time: formatClock(time.minuteOfDay), part: partOfDay(time.hour) };
  const everyone = living(state);
  ui.population.value = everyone.length;
  ui.monsters.value = Object.values(state.monsters).filter((m) => m.phase !== 'dying').length;
  ui.alarm.value = state.tick < state.director.alarmUntilTick;
  ui.roster.value = everyone.map((c) => ({
    id: c.id,
    name: c.profile.name,
    role: c.profile.occupation,
    status: describeActivity(state, c),
    hp: c.hp / CLASSES[c.classId].maxHp,
    mood: moodOf(c),
    partner: c.partnerId !== null,
  }));
  const selected = ui.selectedId.value ? state.characters[ui.selectedId.value] : undefined;
  if (ui.selectedId.value && !selected) ui.selectedId.value = null;
  ui.detail.value = selected ? detailOf(state, selected) : null;
  if (ui.chronicle.value.length !== state.chronicle.length || ui.chronicle.value[ui.chronicle.value.length - 1] !== state.chronicle[state.chronicle.length - 1]) {
    ui.chronicle.value = state.chronicle.slice(-120);
  }
  if (ui.graves.value.length !== state.graves.length) {
    ui.graves.value = state.graves.map((grave) => ({ name: grave.name, day: grave.day, cause: grave.cause }));
  }
}
