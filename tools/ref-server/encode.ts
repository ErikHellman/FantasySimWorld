// The stream encoder: what each entity looks like on the wire, how a tick's
// changes become a `tick` frame, and which sim events viewers get.
//
// This is the reference the Rust server's encoder is ported from, so the rules
// are spelled out here rather than left to the code:
//
// - Ids: villagers and monsters keep their sim ids (`c3`, `m12`). A corpse is
//   `corpse:<id of the dead>`, because a villager's corpse appears in the same
//   tick the villager leaves and both would otherwise share an id.
// - Order: villagers in `state.order`, then monsters in insertion order, then
//   corpses in array order. `add`, `del` and `d` follow that order.
// - Quantising: positions and facing are multiplied by 100 and rounded, hit
//   points rounded, all with JavaScript rounding (halves go toward +infinity;
//   `js_round` in Rust). Diffs compare the quantised values, so an idle entity
//   produces nothing.
// - The "at ease" rule: weapons are put away while sitting or lying, and a
//   tool held for work replaces the main-hand weapon and empties the off hand.
// - Event positions are rounded to centimetres and damage to a tenth; nothing
//   on the client needs more.
import { TICK_SECONDS } from '../../src/core/time';
import type {
  ChronicleEntry, Dyn, DynDelta, Entity, EntityStatic, Grave, TickFrame, Utterance, Vec2, WireEvent,
} from '../../src/generated/api';
import { CLASSES } from '../../src/sim/data/classes';
import { MONSTERS } from '../../src/sim/data/monsters';
import type { SimEvent } from '../../src/sim/events';
import type { Character, Corpse, Monster, SimState } from '../../src/sim/state';
import { firstName } from '../../src/sim/util';

export const corpseId = (id: string): string => `corpse:${id}`;

/** Animations during which weapons are put away. */
const AT_EASE = new Set<string>(['sit_chair', 'sit_floor', 'lie']);

/** ×100, rounded the JavaScript way. */
const centi = (value: number): number => Math.round(value * 100);
const point = (v: { x: number; z: number }): Vec2 => ({ x: centi(v.x) / 100, z: centi(v.z) / 100 });

export function characterStatic(c: Character): EntityStatic {
  return {
    id: c.id, kind: 'villager', name: firstName(c), look: structuredClone(c.look),
    maxHp: CLASSES[c.classId].maxHp, hostile: false, voice: { ...c.profile.voice },
  };
}

export function characterDyn(c: Character): Dyn {
  const atEase = AT_EASE.has(c.anim);
  return {
    p: [centi(c.pos.x), centi(c.pos.z)],
    f: centi(c.facing),
    a: [c.anim, c.animSeq],
    hp: Math.round(c.hp),
    v: !c.inside,
    mh: c.tool ?? (atEase ? null : c.look.mainHand),
    oh: c.tool || atEase ? null : c.look.offHand,
    act: c.intent?.kind === 'activity' && c.act.phase === 'perform' ? c.intent.activity : null,
    z: c.asleep,
  };
}

export function monsterStatic(m: Monster): EntityStatic {
  const def = MONSTERS[m.kind];
  return { id: m.id, kind: 'monster', name: def.label, look: structuredClone(m.look), maxHp: def.maxHp, hostile: true, voice: null };
}

export function monsterDyn(m: Monster): Dyn {
  return {
    p: [centi(m.pos.x), centi(m.pos.z)], f: centi(m.facing), a: [m.anim, m.animSeq], hp: Math.round(Math.max(0, m.hp)),
    v: true, mh: m.look.mainHand, oh: m.look.offHand, act: null, z: false,
  };
}

/** A body has no name (so no label), no health and never moves. */
export function corpseStatic(corpse: Corpse): EntityStatic {
  return { id: corpseId(corpse.id), kind: 'corpse', name: '', look: structuredClone(corpse.look), maxHp: 1, hostile: false, voice: null };
}

export function corpseDyn(corpse: Corpse): Dyn {
  return {
    p: [centi(corpse.pos.x), centi(corpse.pos.z)], f: centi(corpse.facing), a: [corpse.anim, 0], hp: 0,
    v: true, mh: corpse.look.mainHand, oh: corpse.look.offHand, act: null, z: false,
  };
}

/** Every entity a viewer draws, in wire order. */
export function entitiesOf(state: SimState): Entity[] {
  const result: Entity[] = [];
  for (const id of state.order) {
    const c = state.characters[id];
    if (c) result.push({ static: characterStatic(c), dyn: characterDyn(c) });
  }
  for (const id in state.monsters) {
    const m = state.monsters[id] as Monster;
    result.push({ static: monsterStatic(m), dyn: monsterDyn(m) });
  }
  for (const corpse of state.corpses) result.push({ static: corpseStatic(corpse), dyn: corpseDyn(corpse) });
  return result;
}

/** The fields of `next` that differ from `prev`, or null when nothing did. */
export function diffDyn(id: string, prev: Dyn, next: Dyn): DynDelta | null {
  const delta: DynDelta = { id };
  let changed = false;
  if (prev.p[0] !== next.p[0] || prev.p[1] !== next.p[1]) (delta.p = next.p), (changed = true);
  if (prev.f !== next.f) (delta.f = next.f), (changed = true);
  if (prev.a[0] !== next.a[0] || prev.a[1] !== next.a[1]) (delta.a = next.a), (changed = true);
  if (prev.hp !== next.hp) (delta.hp = next.hp), (changed = true);
  if (prev.v !== next.v) (delta.v = next.v), (changed = true);
  // These three may change to null, which is sent as null; an absent field means unchanged.
  if (prev.mh !== next.mh) (delta.mh = next.mh), (changed = true);
  if (prev.oh !== next.oh) (delta.oh = next.oh), (changed = true);
  if (prev.act !== next.act) (delta.act = next.act), (changed = true);
  if (prev.z !== next.z) (delta.z = next.z), (changed = true);
  return changed ? delta : null;
}

/** Applies a delta to a dyn (the client does the same; used here by the tests). */
export function applyDelta(dyn: Dyn, delta: DynDelta): Dyn {
  const next = { ...dyn };
  for (const key of ['p', 'f', 'a', 'hp', 'v', 'mh', 'oh', 'act', 'z'] as const) {
    if (key in delta) (next as Record<string, unknown>)[key] = delta[key];
  }
  return next;
}

export const graveKey = (grave: { plot: number; characterId: string }): string => `${grave.plot}:${grave.characterId}`;

/** The sim's one-shot events that viewers see; the rest (conversation, mercy, save points…) stay on the server. */
export function wireEvents(state: SimState, events: readonly SimEvent[]): WireEvent[] {
  const result: WireEvent[] = [];
  for (const event of events) {
    switch (event.type) {
      case 'speech': {
        const utterance = utteranceOf(state, event);
        if (utterance) result.push({ type: 'speech', ...utterance });
        break;
      }
      case 'emote':
        result.push({ type: 'emote', id: event.id, emote: event.emote });
        break;
      case 'attack':
        result.push({ type: 'attack', attackerId: event.attackerId, targetId: event.targetId, ranged: event.ranged, from: point(event.from), to: point(event.to) });
        break;
      case 'hit':
        result.push({
          type: 'hit', attackerId: event.attackerId, targetId: event.targetId, damage: Math.round(event.damage * 10) / 10, pos: point(event.pos), fatal: event.fatal,
        });
        break;
      case 'death':
        result.push({ type: 'death', id: event.id, name: event.name, pos: point(event.pos), monster: event.monster });
        break;
      case 'monster_spawn':
        result.push({ type: 'monster_spawn', id: event.id, pos: point(event.pos) });
        break;
      case 'chronicle':
        result.push({ type: 'chronicle', entry: chronicleEntry(event.entry) });
        break;
      default:
        break;
    }
  }
  return result;
}

/**
 * A spoken line with everything a sink needs. The speaker must still be alive
 * (a line from someone who died in the same tick is dropped, as the browser's
 * speech bus did); a listener who is gone is left out.
 */
export function utteranceOf(state: SimState, event: Extract<SimEvent, { type: 'speech' }>): Utterance | null {
  const speaker = state.characters[event.speakerId];
  if (!speaker) return null;
  const listener = event.toId ? state.characters[event.toId] : undefined;
  return {
    tick: state.tick,
    speakerId: speaker.id,
    speakerName: firstName(speaker),
    toId: listener?.id ?? null,
    toName: listener ? firstName(listener) : null,
    text: event.text,
    loud: event.loud,
    durationMs: Math.round(event.durationTicks * TICK_SECONDS * 1000),
    voice: { ...speaker.profile.voice },
    position: point(speaker.pos),
  };
}

export function chronicleEntry(entry: SimState['chronicle'][number]): ChronicleEntry {
  return { tick: entry.tick, kind: entry.kind, text: entry.text, ids: [...entry.ids] };
}

export function graveOf(grave: SimState['graves'][number]): Grave {
  return { plot: grave.plot, characterId: grave.characterId, name: grave.name, day: grave.day, cause: grave.cause };
}

/**
 * Remembers what every viewer has been told, and turns the next tick into a
 * frame of differences. One tracker serves all viewers: a new viewer gets
 * `entities()` as its snapshot, which is exactly the state the next frame is
 * a difference from.
 */
export class DeltaTracker {
  private readonly known = new Map<string, Entity>();
  private readonly graves = new Set<string>();

  constructor(state: SimState) {
    this.reset(state);
  }

  reset(state: SimState): void {
    this.known.clear();
    for (const entity of entitiesOf(state)) this.known.set(entity.static.id, entity);
    this.graves.clear();
    for (const grave of state.graves) this.graves.add(graveKey(grave));
  }

  /** What viewers currently know, in wire order (for a snapshot). */
  entities(): Entity[] {
    return [...this.known.values()];
  }

  /** The frame for the tick the sim has just run; `events` are what it returned. */
  frame(state: SimState, events: readonly SimEvent[]): TickFrame {
    const frame: TickFrame = { t: state.tick };
    const add: Entity[] = [];
    const d: DynDelta[] = [];
    // Rebuilt in wire order so `entities()` matches what a fresh snapshot would hold.
    const next = new Map<string, Entity>();
    for (const entity of entitiesOf(state)) {
      const id = entity.static.id;
      const before = this.known.get(id);
      if (!before) {
        add.push(entity);
        next.set(id, entity);
      } else {
        const delta = diffDyn(id, before.dyn, entity.dyn);
        if (delta) d.push(delta);
        // The static part never changes while an entity exists; keep the one viewers were sent.
        next.set(id, { static: before.static, dyn: entity.dyn });
      }
    }
    const del = [...this.known.keys()].filter((id) => !next.has(id));
    this.known.clear();
    for (const [id, entity] of next) this.known.set(id, entity);

    const ev = wireEvents(state, events);
    // New graves are not a sim event; they show up as a change in state.graves.
    for (const grave of state.graves) {
      const key = graveKey(grave);
      if (this.graves.has(key)) continue;
      this.graves.add(key);
      ev.push({ type: 'grave', grave: graveOf(grave) });
    }
    for (const key of [...this.graves]) if (!state.graves.some((grave) => graveKey(grave) === key)) this.graves.delete(key);

    if (add.length) frame.add = add;
    if (del.length) frame.del = del;
    if (d.length) frame.d = d;
    if (ev.length) frame.ev = ev;
    return frame;
  }
}
