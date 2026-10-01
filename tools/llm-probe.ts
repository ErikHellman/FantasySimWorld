// Measures how well a model plays a villager: latency and how often its tool
// calls are usable, over real prompts taken from a running simulation.
//
//   bun tools/llm-probe.ts <base-url> <model> [runs] [api-key]
//   bun tools/llm-probe.ts http://localhost:11434/v1 qwen3:8b 20
//   LLM_API_KEY=sk-... bun tools/llm-probe.ts https://api.openai.com/v1 <model> 20
//
// Options (environment): LLM_PROTOCOL=auto|tools|json, LLM_HEADERS="Name: value\nOther: value"
import { LlmClient } from '../src/brain/llm/client';
import { LlmError } from '../src/brain/llm/types';
import { conversationPrompt, decidePrompt, planPrompt, reactPrompt, reflectPrompt, type Prompt } from '../src/brain/prompts';
import { interpretConversation, interpretDecision, interpretPlan } from '../src/brain/tools/validate';
import { DEFAULT_SETTINGS } from '../src/persist/settings';
import { createState } from '../src/sim/create';
import { Sim } from '../src/sim/sim';
import type { SimState } from '../src/sim/state';
import { living } from '../src/sim/util';

const [baseUrl, model, runsArg, keyArg] = process.argv.slice(2);
if (!baseUrl || !model) {
  console.error('usage: bun tools/llm-probe.ts <base-url> <model> [runs] [api-key]');
  process.exit(1);
}
const runs = Number(runsArg ?? 12);
const protocol = (process.env.LLM_PROTOCOL as 'auto' | 'tools' | 'json' | undefined) ?? 'auto';
const client = new LlmClient({
  ...DEFAULT_SETTINGS.llm, enabled: true, baseUrl, model, protocol,
  apiKey: keyArg ?? process.env.LLM_API_KEY ?? '', headers: process.env.LLM_HEADERS ?? '',
});

// A world a few days in, mid-afternoon, so prompts carry memories and relationships.
const sim = new Sim(createState(2024));
for (let i = 0; i < 28800 * 2 + 9600; i++) sim.step();
const state: SimState = sim.state;
const people = living(state);

interface Case {
  kind: string;
  prompt: Prompt;
  usable: (calls: Awaited<ReturnType<LlmClient['complete']>>['calls']) => string | null;
}

function buildCase(index: number): Case {
  const c = people[index % people.length]!;
  const other = people[(index + 3) % people.length]!;
  switch (index % 5) {
    case 0:
    case 1: {
      c.stimuli = [{ kind: 'block_ended', tick: state.tick, text: 'You finished what you were doing.', urgency: 0 }];
      return { kind: 'decide', prompt: decidePrompt(state, c, 1200), usable: (calls) => {
        const decision = interpretDecision(state, c, calls);
        return decision.intents.length > 0 || decision.keep ? null : decision.problems.join('; ') || 'no usable call';
      } };
    }
    case 2:
      return { kind: 'plan_day', prompt: planPrompt(state, c, 1200), usable: (calls) => (interpretPlan(calls) ? null : 'plan could not be read') };
    case 3: {
      const convo = { id: 'v0', a: c.id, b: other.id, tone: 'friendly' as const, phase: 'play' as const, lines: [{ speakerId: c.id, text: 'You look troubled. What is it?' }], index: 0, lineEndTick: 0, startedTick: state.tick, awaitUntilTick: 0, closed: false, author: 'local' as const, topic: '', feelings: {}, fallback: { lines: [], topic: '', feelings: {} } };
      return { kind: 'converse', prompt: conversationPrompt(state, convo, c, other, 1200), usable: (calls) => (interpretConversation(calls, c, other) ? null : 'lines could not be read') };
    }
    default:
      return index % 2 === 0
        ? { kind: 'reflect', prompt: reflectPrompt(state, c, 1200), usable: (calls) => (calls.some((call) => call.name === 'reflection' && typeof call.args.summary === 'string') ? null : 'no reflection') }
        : { kind: 'react', prompt: reactPrompt(state, c), usable: (calls) => (calls.some((call) => call.name === 'fight' || call.name === 'flee') ? null : 'neither fight nor flee') };
  }
}

const latencies: number[] = [];
const byKind: Record<string, { ok: number; total: number }> = {};
let tokens = 0;

for (let i = 0; i < runs; i++) {
  const test = buildCase(i);
  const tally = (byKind[test.kind] ??= { ok: 0, total: 0 });
  tally.total++;
  try {
    const response = await client.complete({ messages: test.prompt.messages, tools: test.prompt.tools, maxCalls: test.prompt.maxCalls });
    const problem = test.usable(response.calls);
    latencies.push(response.latencyMs);
    tokens += response.promptTokens + response.completionTokens;
    if (!problem) tally.ok++;
    console.log(`${String(i + 1).padStart(3)} ${test.kind.padEnd(9)} ${Math.round(response.latencyMs).toString().padStart(6)} ms  ${problem ? `UNUSABLE (${problem})` : 'ok'}  ${response.calls.map((call) => `${call.name}(${JSON.stringify(call.args).slice(0, 110)})`).join(' ')}`);
  } catch (error) {
    console.log(`${String(i + 1).padStart(3)} ${test.kind.padEnd(9)}   FAILED  ${error instanceof LlmError ? `${error.kind}: ${error.message}` : String(error)}`);
    if (process.env.LLM_DEBUG && error instanceof LlmError && error.raw) console.log(`      raw: ${error.raw.slice(0, 1200)}`);
    if (error instanceof LlmError && error.kind === 'network') break;
  }
}

latencies.sort((a, b) => a - b);
const at = (q: number): number => Math.round(latencies[Math.min(latencies.length - 1, Math.floor(latencies.length * q))] ?? 0);
const ok = Object.values(byKind).reduce((sum, entry) => sum + entry.ok, 0);
const total = Object.values(byKind).reduce((sum, entry) => sum + entry.total, 0);
console.log(`\nprotocol in use: ${client.protocol === 'tools' ? 'native tool calls' : 'JSON output'}`);
console.log(`usable answers: ${ok}/${total} (${total ? Math.round((ok / total) * 100) : 0}%)  ${Object.entries(byKind).map(([kind, entry]) => `${kind} ${entry.ok}/${entry.total}`).join(', ')}`);
console.log(`latency: median ${at(0.5)} ms, 90th percentile ${at(0.9)} ms; ${tokens} tokens in total`);
if (latencies.length > 0) console.log(`at this speed one request at a time serves about ${Math.round(60000 / at(0.5))} decisions a minute`);
