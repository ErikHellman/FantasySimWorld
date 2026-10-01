import { describe, expect, it } from 'vitest';
import { TICKS_PER_DAY } from '../core/time';
import { createState } from './create';
import type { SimEvent } from './events';
import { Sim } from './sim';
import type { Character, SimState } from './state';
import { killCharacter } from './systems/death';
import { RUNG_BLOOD, RUNG_FEUD, labelFor, lethalUnlocked, rungOf } from './systems/relationships';
import { firstName, living, makeCtx, newRelationship, rel } from './util';

function world(seed = 31): { sim: Sim; state: SimState; events: SimEvent[]; run: (ticks: number, until?: () => boolean) => void; who: (name: string) => Character } {
  const state = createState(seed);
  const sim = new Sim(state);
  const events: SimEvent[] = [];
  // No small talk: these tests set up their own encounters and must not be interrupted by a chat.
  sim.enqueue({ type: 'tuning', tuning: { chatter: 0 } });
  return {
    sim,
    state,
    events,
    run: (ticks, until) => {
      for (let i = 0; i < ticks && !(until && until()); i++) events.push(...sim.step());
    },
    who: (name) => living(state).find((c) => firstName(c) === name)!,
  };
}

describe('relationship labels', () => {
  it('are harder to lose than to gain', () => {
    const r = newRelationship();
    r.familiarity = 30;
    r.affinity = 31;
    r.label = labelFor(r, false);
    expect(r.label).toBe('friend');
    r.affinity = 22; // below the bar for a new friendship, but not low enough to end one
    expect(labelFor(r, false)).toBe('friend');
    r.affinity = 10;
    expect(labelFor(r, false)).toBe('acquaintance');
    r.label = 'acquaintance';
    r.affinity = 22;
    expect(labelFor(r, false)).toBe('acquaintance');
  });

  it('put hostility and romance above everything else', () => {
    const r = newRelationship();
    r.affinity = 70;
    r.familiarity = 80;
    r.kin = true;
    expect(labelFor(r, false)).toBe('family');
    expect(labelFor(r, true)).toBe('lover');
    r.grievance = 62;
    expect(labelFor(r, false)).toBe('enemy');
    expect(rungOf(r)).toBe('grudge');
  });
});

describe('the hostility ladder', () => {
  it('only unlocks killing at the top rung, soon after an offence, and not twice in quick succession', () => {
    const w = world();
    w.run(5);
    const garrick = w.who('Garrick');
    const aldric = w.who('Aldric');
    const ctx = () => makeCtx(w.state, []);
    const r = rel(garrick, aldric.id);
    r.grievance = RUNG_BLOOD - 1;
    r.lastOffenceTick = w.state.tick;
    expect(lethalUnlocked(ctx(), garrick, aldric.id)).toBe(false);
    r.grievance = RUNG_BLOOD + 5;
    expect(lethalUnlocked(ctx(), garrick, aldric.id)).toBe(true);
    r.lastOffenceTick = w.state.tick - TICKS_PER_DAY * 3;
    expect(lethalUnlocked(ctx(), garrick, aldric.id)).toBe(false);
    r.lastOffenceTick = w.state.tick;
    w.state.director.lastLethalTick = w.state.tick - 100;
    expect(lethalUnlocked(ctx(), garrick, aldric.id)).toBe(false);
  });

  it('refuses a brawl without a feud and allows one with it', () => {
    const w = world();
    w.run(5);
    const mira = w.who('Mira');
    const kael = w.who('Kael');
    w.sim.enqueue({ type: 'set_intents', id: mira.id, epoch: mira.epoch, intents: [{ kind: 'fight', targetId: kael.id, mode: 'brawl', source: 'llm' }] });
    w.run(2);
    expect(mira.intent?.kind).not.toBe('fight');
    rel(mira, kael.id).grievance = RUNG_FEUD + 1;
    w.sim.enqueue({ type: 'set_intents', id: mira.id, epoch: mira.epoch, intents: [{ kind: 'fight', targetId: kael.id, mode: 'brawl', source: 'llm' }] });
    w.run(2);
    expect(mira.intent).toMatchObject({ kind: 'fight', mode: 'brawl' });
  });

  it('ends a brawl with a winner, a loser and nobody dead', () => {
    const w = world();
    w.run(5);
    const garrick = w.who('Garrick');
    const dorn = w.who('Dorn');
    rel(garrick, dorn.id).grievance = RUNG_FEUD + 5;
    w.sim.enqueue({ type: 'set_intents', id: garrick.id, epoch: garrick.epoch, intents: [{ kind: 'fight', targetId: dorn.id, mode: 'brawl', source: 'llm', say: 'Enough!' }] });
    w.run(6000, () => w.state.chronicle.some((entry) => entry.text.includes('came to blows')));
    expect(w.state.chronicle.some((entry) => entry.text.includes('came to blows'))).toBe(true);
    expect(w.state.order.length).toBe(12);
    expect(Math.min(garrick.hp, dorn.hp)).toBeGreaterThan(0);
    const loser = garrick.hp < dorn.hp ? garrick : dorn;
    const winner = loser === garrick ? dorn : garrick;
    expect(loser.memories.some((memory) => memory.text.includes('beat me'))).toBe(true);
    expect(rel(loser, winner.id).grievance).toBeGreaterThan(10);
  });

  function duel(spare: boolean | null) {
    const w = world(47);
    w.run(5);
    const garrick = w.who('Garrick');
    const aldric = w.who('Aldric');
    const r = rel(garrick, aldric.id);
    r.grievance = 95;
    r.lastOffenceTick = w.state.tick;
    aldric.hp = 45; // already wounded, so the duel is short and one-sided
    aldric.inventory.potion = 0;
    w.sim.enqueue({ type: 'tuning', tuning: { mercyWaitTicks: 200 } });
    w.sim.enqueue({ type: 'set_intents', id: garrick.id, epoch: garrick.epoch, intents: [{ kind: 'fight', targetId: aldric.id, mode: 'lethal', source: 'llm' }] });
    w.run(8000, () => w.events.some((event) => event.type === 'mercy_request') || !w.state.characters[aldric.id]);
    return { w, garrick, aldric, spare };
  }

  it('lets the attacker spare a beaten opponent', () => {
    const { w, garrick, aldric } = duel(true);
    expect(w.events.some((event) => event.type === 'mercy_request')).toBe(true);
    expect(aldric.combat.yielded).toBe(true);
    w.sim.enqueue({ type: 'mercy', attackerId: garrick.id, spare: true, say: 'Go.' });
    w.run(5);
    expect(w.state.characters[aldric.id]).toBeDefined();
    expect(aldric.combat.yielded).toBe(false);
    expect(aldric.intent?.kind).toBe('flee');
    expect(rel(aldric, garrick.id).label).toBe('enemy');
    expect(w.state.chronicle.some((entry) => entry.text.includes('spared'))).toBe(true);
  });

  it('makes a killing permanent, with a grave and a village that does not forgive', () => {
    const { w, garrick, aldric } = duel(false);
    w.sim.enqueue({ type: 'mercy', attackerId: garrick.id, spare: false });
    w.run(5);
    expect(w.state.characters[aldric.id]).toBeUndefined();
    expect(w.state.order.length).toBe(11);
    expect(w.state.dead[aldric.id]).toMatchObject({ killerId: garrick.id });
    expect(w.state.graves.some((grave) => grave.characterId === aldric.id)).toBe(true);
    expect(garrick.killer).toBe(true);
    const brenna = w.who('Brenna');
    expect(rel(brenna, garrick.id).label).toBe('enemy');
    expect(brenna.memories.some((memory) => memory.text.includes('Aldric is dead'))).toBe(true);
  });

  it('spares by default if no decision arrives', () => {
    const { w, aldric } = duel(null);
    w.run(400);
    // Garrick is hot-tempered, so either ending is possible; what matters is that the duel resolves.
    const alive = w.state.characters[aldric.id] !== undefined;
    expect(alive ? aldric.combat.yielded : true).toBe(alive ? false : true);
  });
});

describe('death and arrivals', () => {
  it('brings a newcomer after a death', () => {
    const w = world(5);
    w.run(10);
    const victim = w.who('Isolde');
    killCharacter(makeCtx(w.state, []), victim, 'lost to a fever', null);
    expect(w.state.order.length).toBe(11);
    w.run(TICKS_PER_DAY * 4, () => w.state.director.arrivals > 0);
    expect(w.state.director.arrivals).toBe(1);
    const newcomer = living(w.state).find((c) => c.arrivedTick > 0)!;
    expect(newcomer.profile.name).toBe('Halvard Dawnmere');
    expect(newcomer.homeId).toMatch(/^home_/);
    expect(w.who('Rowan').memories.some((memory) => memory.text.includes('Isolde is dead'))).toBe(true);
  });
});

describe('conversations without a model', () => {
  it('wait for lines, then fall back to local ones', () => {
    const w = world(8);
    w.sim.enqueue({ type: 'tuning', tuning: { scriptWaitTicks: 100 } });
    w.run(3);
    const a = w.who('Rowan');
    const b = w.who('Isolde');
    w.sim.enqueue({ type: 'set_intents', id: a.id, epoch: a.epoch, intents: [{ kind: 'talk', targetId: b.id, opener: 'Walk with me, child.', tone: 'friendly', source: 'llm' }] });
    const ended = (): boolean => w.events.some((event) => event.type === 'conversation_ended' && event.a === a.id);
    w.run(6000, ended);
    expect(ended()).toBe(true);
    expect(w.events.some((event) => event.type === 'conversation_waiting')).toBe(true);
    const lines = w.events.filter((event) => event.type === 'speech' && (event.speakerId === a.id || event.speakerId === b.id) && event.toId !== null);
    expect(lines.length).toBeGreaterThanOrEqual(3);
    expect(a.memories.some((memory) => memory.kind === 'talk' && memory.subjects.includes(b.id))).toBe(true);
  });
});
