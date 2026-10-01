import { clamp } from '../../core/math2d';
import { chance, intRange, nextFloat, pick, range } from '../../core/rng';
import { nearestWalkable } from '../../world/navgrid';
import { getPlace, homes } from '../../world/places';
import { CLASSES, type ClassId } from '../data/classes';
import { NEWCOMERS, type CastEntry } from '../data/cast';
import type { Character, SimState } from '../state';
import { newId } from '../state';
import { chronicle, firstName, living, newRelationship, remember, stimulate, type Ctx } from '../util';
import { freshAct } from './intents';
import { labelFor } from './relationships';

const FIRST_NAMES = ['Edda', 'Bram', 'Wren', 'Osric', 'Tamsin', 'Hale', 'Ysolt', 'Garnet', 'Perrin', 'Linnea', 'Cade', 'Marta', 'Jory', 'Senna', 'Alder'];
const LAST_NAMES = ['Underhill', 'Blackwater', 'Fairweather', 'Stonebridge', 'Larkspur', 'Holt', 'Greenmantle', 'Ashby', 'Wolfe', 'Tanner'];
const TRAIT_POOL = ['cheerful', 'wary', 'stubborn', 'generous', 'sharp-tongued', 'patient', 'restless', 'superstitious', 'ambitious', 'soft-hearted', 'boastful', 'thoughtful'];
const WANDERER_CLASSES: ClassId[] = ['knight', 'barbarian', 'mage', 'ranger', 'rogue', 'druid', 'engineer', 'farmer'];

/** A home with a free bed (the emptiest one, so newcomers spread out). */
export function freeHome(state: SimState): string {
  let best = homes[0]!.id;
  let bestFree = -Infinity;
  for (const home of homes) {
    const residents = living(state).filter((c) => c.homeId === home.id).length;
    const free = home.beds - residents;
    if (free > bestFree) {
      bestFree = free;
      best = home.id;
    }
  }
  return best;
}

/** Builds a character from an authored entry. */
export function makeCharacter(state: SimState, entry: CastEntry, homeId: string): Character {
  const def = CLASSES[entry.classId];
  const home = getPlace(homeId);
  const pos = nearestWalkable({ x: home.door.x + range(state, -1.5, 1.5), z: home.door.z + range(state, -0.5, 1.5) }, 5) ?? { ...home.door };
  const fighter = entry.profile.bravery >= 0.6;
  return {
    id: newId(state, 'c'),
    profile: structuredClone(entry.profile),
    classId: entry.classId,
    look: {
      model: entry.model ?? def.model,
      texture: entry.texture ?? null,
      hide: entry.hide ?? [],
      mainHand: entry.mainHand === undefined ? def.mainHand : entry.mainHand,
      offHand: entry.offHand === undefined ? def.offHand : entry.offHand,
    },
    homeId,
    jobs: [...(entry.jobs ?? def.jobs)],
    hobbies: [...(entry.hobbies ?? def.hobbies)],
    arrivedTick: 0,
    pos,
    facing: home.facing,
    moving: false,
    hp: def.maxHp,
    needs: { hunger: range(state, 50, 70), energy: range(state, 88, 100), social: range(state, 45, 70) },
    inventory: { food: 1, potion: fighter ? 1 : 0, gift: 0 },
    plan: [],
    planDay: 0,
    planSource: 'routine',
    planBlock: -1,
    intent: null,
    queue: [],
    act: freshAct(state.tick),
    slotId: null,
    inside: null,
    asleep: false,
    watchUntilTick: 0,
    sleepInUntilTick: 0,
    anim: 'idle',
    animSeq: 0,
    tool: null,
    convoId: null,
    partnerId: null,
    combat: {
      targetId: null, mode: 'monster', nextAttackTick: 0, hitTick: 0, hitTargetId: null,
      lastHitByTick: -1e9, lastHitById: null, yielded: false, mercyUntilTick: 0, mercyTargetId: null,
    },
    epoch: 0,
    thought: '',
    outcome: '',
    stimuli: [],
    relationships: {},
    memories: [],
    summary: '',
    lastReflectionDay: 0,
    kills: 0,
    killer: false,
    nextSocialTick: 0,
    nextPerceptionTick: 0,
  };
}

/** When the authored newcomers run out, wanderers are made up from name and trait lists. */
function wanderer(state: SimState): CastEntry {
  const classId = pick(state, WANDERER_CLASSES);
  const used = new Set([...living(state).map((c) => c.profile.name), ...Object.values(state.dead).map((d) => d.name)]);
  let name = '';
  for (let i = 0; i < 20 && (!name || used.has(name)); i++) name = `${pick(state, FIRST_NAMES)} ${pick(state, LAST_NAMES)}`;
  const traits = [pick(state, TRAIT_POOL), pick(state, TRAIT_POOL), pick(state, TRAIT_POOL)].filter((t, i, all) => all.indexOf(t) === i);
  const textures = ['A', 'B', 'C'];
  const base = CLASSES[classId].model.toLowerCase();
  const recolour = ['knight', 'barbarian', 'mage', 'ranger', 'rogue', 'druid', 'engineer'].includes(base);
  return {
    key: `wanderer${state.director.arrivals}`,
    classId,
    texture: recolour ? `${base}_texture_alt_${pick(state, textures)}` : undefined,
    profile: {
      name,
      pronouns: pick(state, ['she/her', 'he/him', 'they/them'] as const),
      age: intRange(state, 20, 55),
      occupation: `travelling ${CLASSES[classId].label}`,
      backstory: 'A traveller who came up the west road looking for a place to settle, and decided to stay a while.',
      traits,
      goals: ['find a place in the village', 'make a friend or two', 'stay alive through the nights'],
      speechStyle: 'Plain and direct.',
      temper: clamp(nextFloat(state), 0.15, 0.85),
      bravery: clamp(nextFloat(state), 0.25, 0.9),
      sociability: clamp(nextFloat(state), 0.25, 0.9),
      voice: { pitch: range(state, 0.7, 1.3), rate: range(state, 0.85, 1.15), timbre: 'plain' },
    },
  };
}

/** A new face walks in by the west road. */
export function arrive(ctx: Ctx): Character {
  const { state } = ctx;
  const entry = NEWCOMERS[state.director.arrivals] ?? wanderer(state);
  state.director.arrivals++;
  const c = makeCharacter(state, entry, freeHome(state));
  const road = getPlace('west_road');
  c.pos = nearestWalkable({ x: road.origin.x - 2, z: road.origin.z + range(state, -1, 1) }, 6) ?? { ...road.door };
  c.arrivedTick = state.tick;
  c.needs = { hunger: 40, energy: 60, social: 30 };
  c.queue = [
    { kind: 'go_to', placeId: 'tavern', source: 'routine' },
    { kind: 'activity', activity: 'eat', source: 'routine' },
  ];
  state.characters[c.id] = c;
  state.order.push(c.id);

  remember(ctx, c, `Arrived in the valley by the west road. I know no one here yet. I have been given a bed at ${getPlace(c.homeId).name}.`, 7, 'event');
  for (const other of living(state)) {
    if (other.id === c.id) continue;
    other.relationships[c.id] = newRelationship();
    c.relationships[other.id] = newRelationship();
    other.relationships[c.id]!.label = labelFor(other.relationships[c.id]!, false);
    stimulate(ctx, other, 'arrival', `A newcomer has arrived in the village: ${c.profile.name}, ${c.profile.occupation}.`, 0, c.id);
    if (chance(state, 0.5)) remember(ctx, other, `A newcomer, ${c.profile.name}, has come to the village: ${c.profile.occupation}.`, 4, 'event', [c.id]);
  }
  chronicle(ctx, 'arrival', `${c.profile.name}, ${c.profile.occupation}, arrived by the west road.`, [c.id]);
  ctx.events.push({ type: 'arrival', id: c.id }, { type: 'save_point', reason: 'arrival' });
  stimulate(ctx, c, 'arrival', `You have just arrived in the village and know nobody. ${firstName(c)}, find your feet.`, 1);
  return c;
}
