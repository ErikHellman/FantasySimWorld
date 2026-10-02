// Writes the TypeScript brain's prompts for a fixed world as a fixture the
// Rust prompts must match word for word (server/crates/brain/tests/fixtures/prompts.json).
//   bun tools/make-prompt-fixture.ts
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  conversationPrompt, decidePrompt, mercyPrompt, planPrompt, promptText, reactPrompt, reflectPrompt, replyPrompt,
} from '../src/brain/prompts';
import { createState } from '../src/sim/create';
import { Sim } from '../src/sim/sim';

const SEED = 1;
const TICKS = [600, 9000, 21000];
const cases: Array<{ tick: number; kind: string; ids: string[]; budget: number; text: string; tools: string[] }> = [];
const sim = new Sim(createState(SEED));
for (const tick of TICKS) {
  while (sim.state.tick < tick) sim.step();
  // Prompts may create default relationship records in the TypeScript sim; build them on a copy.
  const state = structuredClone(sim.state);
  const ids = state.order.slice(0, 4);
  for (const id of ids) {
    const c = state.characters[id]!;
    for (const budget of [900, 1500]) {
      const p = decidePrompt(state, c, budget);
      cases.push({ tick, kind: 'decide', ids: [id], budget, text: promptText(p), tools: p.tools.map((t) => t.name) });
    }
    const plan = planPrompt(state, c, 900);
    cases.push({ tick, kind: 'plan', ids: [id], budget: 900, text: promptText(plan), tools: plan.tools.map((t) => t.name) });
    const react = reactPrompt(state, c);
    cases.push({ tick, kind: 'react', ids: [id], budget: 0, text: promptText(react), tools: react.tools.map((t) => t.name) });
    const reflect = reflectPrompt(state, c, 900);
    cases.push({ tick, kind: 'reflect', ids: [id], budget: 900, text: promptText(reflect), tools: reflect.tools.map((t) => t.name) });
  }
  const [a, b] = [state.characters[ids[0]!]!, state.characters[ids[1]!]!];
  const mercy = mercyPrompt(state, a, b);
  cases.push({ tick, kind: 'mercy', ids: [a.id, b.id], budget: 0, text: promptText(mercy), tools: mercy.tools.map((t) => t.name) });
  const convo = { id: 'v0', a: a.id, b: b.id, tone: 'friendly' as const, phase: 'hail' as const, lines: [{ speakerId: a.id, text: 'Fine morning.' }], index: 0, lineEndTick: 0, startedTick: 0, awaitUntilTick: 0, closed: false, author: 'local' as const, topic: '', feelings: {}, fallback: { lines: [], topic: '', feelings: {} } };
  const conversation = conversationPrompt(state, convo, a, b, 1500);
  cases.push({ tick, kind: 'conversation', ids: [a.id, b.id], budget: 1500, text: promptText(conversation), tools: conversation.tools.map((t) => t.name) });
  const reply = replyPrompt(state, convo, convo.lines, b, a, 900);
  cases.push({ tick, kind: 'reply', ids: [b.id, a.id], budget: 900, text: promptText(reply), tools: reply.tools.map((t) => t.name) });
}
const path = join(import.meta.dirname, '..', 'server/crates/brain/tests/fixtures/prompts.json');
writeFileSync(path, `${JSON.stringify({ seed: SEED, cases }, null, 1)}\n`);
console.log(`wrote ${cases.length} prompts to ${path}`);
