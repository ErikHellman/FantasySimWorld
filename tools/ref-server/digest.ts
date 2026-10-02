// The HUD's view of the world, computed on the server because the client no
// longer has the sim helpers: the roster, the status line, and the inspector's
// `CharacterDetail` with every label already rendered. The Rust digest task is
// ported from here.
import { formatClock, gameTime, partOfDay } from '../../src/core/time';
import type {
  AmbientCounts, CharacterDetail, ChronicleEntry, LlmStatus, Mood, RelationRow, RosterEntry, Status,
} from '../../src/generated/api';
import { ACTIVITIES } from '../../src/sim/data/activities';
import { CLASSES } from '../../src/sim/data/classes';
import { describeActivity } from '../../src/sim/describe';
import type { Character, SimState } from '../../src/sim/state';
import { rungOf } from '../../src/sim/systems/relationships';
import { blockAt } from '../../src/sim/systems/routine';
import { living } from '../../src/sim/util';
import { findPlace } from '../../src/world/places';
import { chronicleEntry } from './encode';

/** How many chronicle entries a snapshot carries; older ones come from `GET /api/chronicle`. */
export const CHRONICLE_TAIL = 120;
/** How many memories the inspector shows, newest first. */
export const DETAIL_MEMORIES = 40;

export function moodOf(c: Character): Mood {
  if (c.intent?.kind === 'fight' || c.intent?.kind === 'flee') return 'fight';
  if (c.asleep) return 'sleep';
  if (c.convoId) return 'talk';
  if (c.hp < CLASSES[c.classId].maxHp * 0.5) return 'hurt';
  return 'ok';
}

export function rosterOf(state: SimState): RosterEntry[] {
  return living(state).map((c) => ({
    id: c.id,
    name: c.profile.name,
    role: c.profile.occupation,
    status: describeActivity(state, c),
    // Two decimals: enough for a health bar, and slow regeneration does not resend the roster every second.
    hp: Math.round((c.hp / CLASSES[c.classId].maxHp) * 100) / 100,
    mood: moodOf(c),
    partner: c.partnerId !== null,
  }));
}

export const isAlarm = (state: SimState): boolean => state.tick < state.director.alarmUntilTick;

/** Monsters still in the fight (a dying one is not counted). */
export const monsterCount = (state: SimState): number => Object.values(state.monsters).filter((m) => m.phase !== 'dying').length;

/** Villagers performing an activity (not on their way to it). */
function doing(state: SimState, activity: string): number {
  return living(state).filter((c) => c.intent?.kind === 'activity' && c.intent.activity === activity && c.act.phase === 'perform').length;
}

/** What the ambient mixer needs; the rest of its maths depends on each viewer's camera. */
export function ambientOf(state: SimState): AmbientCounts {
  return {
    awake: living(state).filter((c) => !c.asleep).length,
    eating: doing(state, 'eat'),
    drinking: doing(state, 'drink'),
    smithing: doing(state, 'smith'),
    relaxing: doing(state, 'relax'),
  };
}

export interface StatusExtras {
  llm: LlmStatus;
  viewers: number;
  speed: number;
  throttled: boolean;
}

export function statusOf(state: SimState, extras: StatusExtras): Status {
  return {
    llm: extras.llm,
    viewers: extras.viewers,
    speed: extras.speed,
    throttled: extras.throttled,
    population: state.order.length,
    monsters: monsterCount(state),
    alarm: isAlarm(state),
    part: partOfDay(gameTime(state.tick).hour),
    ambient: ambientOf(state),
  };
}

/**
 * The relationships worth showing: everyone alive the character has a record
 * of, and the dead they felt strongly about, strongest feelings first. The
 * hostility rung is rendered with a space ("blood feud"); "calm" means none.
 */
function relationsOf(state: SimState, c: Character): RelationRow[] {
  return Object.keys(c.relationships)
    .map((id): RelationRow => {
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
        rung: rungOf(r).replace('_', ' '),
        summary: r.summary,
        alive: other !== undefined,
      };
    })
    .filter((row) => row.name && (row.alive || Math.abs(row.affinity) >= 25))
    .sort((a, b) => Math.abs(b.affinity) + b.grievance + b.attraction - (Math.abs(a.affinity) + a.grievance + a.attraction));
}

/** The inspector for one living character, or null if there is no such character. */
export function detailOf(state: SimState, id: string): CharacterDetail | null {
  const c = state.characters[id];
  if (!c) return null;
  const current = blockAt(c.plan, gameTime(state.tick).hour);
  return {
    id: c.id,
    name: c.profile.name,
    pronouns: c.profile.pronouns,
    age: c.profile.age,
    occupation: c.profile.occupation,
    className: CLASSES[c.classId].label,
    backstory: c.profile.backstory,
    traits: [...c.profile.traits],
    goals: [...c.profile.goals],
    status: describeActivity(state, c),
    thought: c.thought,
    summary: c.summary,
    hp: c.hp,
    maxHp: CLASSES[c.classId].maxHp,
    needs: { ...c.needs },
    inventory: { ...c.inventory },
    home: findPlace(c.homeId)?.name ?? c.homeId,
    kills: c.kills,
    planSource: c.planSource,
    plan: c.plan.map((block, index) => ({
      time: formatClock(block.hour * 60),
      text: `${ACTIVITIES[block.activity].label}${block.note ? ` — ${block.note}` : ''}`,
      current: index === current,
    })),
    relations: relationsOf(state, c),
    memories: c.memories.slice(-DETAIL_MEMORIES).reverse().map((memory) => ({ ...memory, subjects: [...memory.subjects] })),
  };
}

export function chronicleTail(state: SimState): ChronicleEntry[] {
  return state.chronicle.slice(-CHRONICLE_TAIL).map(chronicleEntry);
}

/**
 * One page of history, oldest first: the newest `limit` entries strictly
 * before `before` (a tick), and whether older ones exist.
 */
export function chroniclePage(entries: readonly ChronicleEntry[], before: number, limit: number): { entries: ChronicleEntry[]; more: boolean } {
  let end = entries.length;
  while (end > 0 && (entries[end - 1] as ChronicleEntry).tick >= before) end--;
  const start = Math.max(0, end - limit);
  return { entries: entries.slice(start, end), more: start > 0 };
}
