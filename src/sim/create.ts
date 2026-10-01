import { range } from '../core/rng';
import { CAST, SEED_BONDS } from './data/cast';
import { SAVE_VERSION, type Character, type SimState } from './state';
import { freeHome, makeCharacter } from './systems/arrivals';
import { tickAt } from './systems/director';
import { labelFor } from './systems/relationships';
import { newRelationship } from './util';

export const DEFAULT_SEED = 1337;

/** A new world: the authored cast at their doors at six in the morning of day one. */
export function createState(seed: number = DEFAULT_SEED): SimState {
  const state: SimState = {
    version: SAVE_VERSION,
    seed,
    rng: seed >>> 0,
    tick: 0,
    nextId: 1,
    characters: {},
    order: [],
    monsters: {},
    conversations: {},
    graves: [],
    corpses: [],
    dead: {},
    chronicle: [],
    slots: {},
    director: {
      nextWaveTick: tickAt(1, 20.6),
      nextArrivalTick: 0,
      lastLethalTick: -1e9,
      alarmUntilTick: 0,
      alarmPos: null,
      watchDay: 0,
      deathTicks: [],
      arrivals: 0,
    },
    tuning: { chatter: 0.5, scriptWaitTicks: 0, mercyWaitTicks: 40 },
  };

  const byKey = new Map<string, Character>();
  for (const entry of CAST) {
    const c = makeCharacter(state, entry, entry.homeId ?? freeHome(state));
    state.characters[c.id] = c;
    state.order.push(c.id);
    byKey.set(entry.key, c);
  }

  // Everyone in a village this small knows everyone, at least a little.
  const cast = [...byKey.values()];
  for (const a of cast) {
    for (const b of cast) {
      if (a.id === b.id) continue;
      const r = newRelationship();
      r.familiarity = range(state, 18, 32);
      r.affinity = Math.round(range(state, 0, 14));
      r.trust = Math.round(range(state, 0, 12));
      a.relationships[b.id] = r;
    }
  }
  for (const bond of SEED_BONDS) {
    const from = byKey.get(bond.from);
    const to = byKey.get(bond.to);
    if (!from || !to) throw new Error(`Seed bond refers to unknown cast member ${bond.from} -> ${bond.to}`);
    const r = from.relationships[to.id]!;
    r.affinity = bond.affinity ?? r.affinity;
    r.trust = bond.trust ?? r.trust;
    r.attraction = bond.attraction ?? 0;
    r.grievance = bond.grievance ?? 0;
    r.familiarity = bond.familiarity ?? r.familiarity;
    r.kin = bond.kin ?? false;
    r.summary = bond.summary;
    if (r.grievance > 0) r.lastOffenceTick = 0;
  }
  for (const a of cast) {
    for (const id of Object.keys(a.relationships)) a.relationships[id]!.label = labelFor(a.relationships[id]!, false);
  }
  state.chronicle.push({ tick: 0, kind: 'world', text: 'A new day dawns over the valley.', ids: [] });
  return state;
}
