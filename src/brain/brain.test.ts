import { describe, expect, it } from 'vitest';
import { DEFAULT_SETTINGS, type LlmSettings } from '../persist/settings';
import { createState } from '../sim/create';
import type { SimEvent } from '../sim/events';
import { Sim } from '../sim/sim';
import { firstName, living } from '../sim/util';
import { Brain, type BrainLogRecord } from './brain';
import { decidePrompt, planPrompt, promptText } from './prompts';

const settings: LlmSettings = { ...DEFAULT_SETTINGS.llm, enabled: true, baseUrl: 'http://llm.test/v1', model: 'mock', concurrency: 4, conversations: 'batched', protocol: 'tools' };
const wait = (ms = 8): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

type Responder = (toolNames: string[], body: { messages: Array<{ role: string; content: string }> }) => Array<{ name: string; args: unknown }> | string;

function harness(responder: Responder, overrides: Partial<LlmSettings> = {}) {
  const sim = new Sim(createState(21));
  const logs: BrainLogRecord[] = [];
  const requests: Array<{ tools: string[]; text: string }> = [];
  const brain = new Brain(sim, { speed: () => 1, log: (record) => logs.push(record), status: () => {} }, async (_url, init) => {
    const body = JSON.parse(String(init?.body));
    const toolNames = (body.tools as Array<{ function: { name: string } }>).map((tool) => tool.function.name);
    requests.push({ tools: toolNames, text: body.messages.map((m: { content: string }) => m.content).join('\n') });
    const reply = responder(toolNames, body);
    const message = typeof reply === 'string'
      ? { content: reply }
      : { content: null, tool_calls: reply.map((call) => ({ type: 'function', function: { name: call.name, arguments: JSON.stringify(call.args) } })) };
    return new Response(JSON.stringify({ choices: [{ message }], usage: { prompt_tokens: 100, completion_tokens: 20 } }), { status: 200 });
  }, 1000);
  brain.configure({ ...settings, ...overrides });
  const events: SimEvent[] = [];
  const step = (ticks = 1): void => {
    for (let i = 0; i < ticks; i++) {
      const tick = sim.step();
      events.push(...tick);
      brain.handle(tick);
    }
  };
  /** Steps until the conversation between two characters has ended; returns what the two said to each other. */
  const talk = async (aId: string, bId: string, pause = 20): Promise<Array<{ speakerId: string; text: string }>> => {
    const between = (event: SimEvent): boolean => event.type === 'conversation_ended' && event.a === aId && event.b === bId;
    for (let i = 0; i < 6000 && !events.some(between); i++) {
      step(1);
      if (i % pause === 0) await wait(1);
    }
    expect(events.some(between)).toBe(true);
    const lines: Array<{ speakerId: string; text: string }> = [];
    for (const event of events) {
      if (event.type === 'speech' && event.toId !== null && [aId, bId].includes(event.speakerId) && [aId, bId].includes(event.toId)) lines.push({ speakerId: event.speakerId, text: event.text });
    }
    return lines;
  };
  return { sim, brain, logs, requests, events, step, talk };
}

describe('Brain', () => {
  it('turns a decide reply into intents for the character', async () => {
    const h = harness((tools) => (tools.includes('go_to') ? [{ name: 'go_to', args: { place: 'the chapel', thought: 'I need quiet.' } }, { name: 'do_activity', args: { activity: 'pray' } }] : []));
    h.step(2);
    const c = living(h.sim.state)[0]!;
    c.stimuli.push({ kind: 'gift', tick: h.sim.state.tick, text: 'Someone left flowers at your door.', urgency: 1 });
    h.brain.update();
    await wait();
    h.step(1);
    expect(c.intent).toMatchObject({ kind: 'go_to', placeId: 'chapel', source: 'llm' });
    expect(c.queue[0]).toMatchObject({ kind: 'activity', activity: 'pray' });
    expect(c.thought).toBe('I need quiet.');
    expect(c.stimuli.length).toBe(0);
    expect(h.logs.find((record) => record.character === firstName(c) && record.kind === 'decide')).toMatchObject({ status: 'ok' });
    const sent = h.requests.find((request) => request.text.includes('Someone left flowers at your door.'))!;
    expect(sent.text).toContain(`You are ${c.profile.name}`);
  });

  it('drops an answer when the situation changed while the model was thinking', async () => {
    const h = harness(() => [{ name: 'go_to', args: { place: 'market' } }]);
    h.step(2);
    const c = living(h.sim.state).find((x) => !x.convoId && x.intent?.kind !== 'talk')!;
    c.stimuli.push({ kind: 'talked', tick: h.sim.state.tick, text: 'x', urgency: 1 });
    h.brain.update();
    c.epoch += 1; // something happened to them in the meantime
    await wait();
    h.step(1);
    expect(c.intent?.source).not.toBe('llm');
    expect(h.logs.find((record) => record.character === firstName(c) && record.kind === 'decide')).toMatchObject({ status: 'stale' });
  });

  it('repairs sloppy arguments and reports what it had to drop', async () => {
    const h = harness(() => [
      { name: 'talk_to', args: { person: 'garrik', message: '"Fancy a drink?"', tone: 'cheerful' } },
      { name: 'go_to', args: { place: 'the moon' } },
    ]);
    h.step(2);
    const c = living(h.sim.state).find((x) => firstName(x) === 'Dorn')!;
    c.stimuli.push({ kind: 'need', tick: h.sim.state.tick, text: 'You feel lonely.', urgency: 1 });
    h.brain.update();
    await wait();
    h.step(1);
    const garrick = living(h.sim.state).find((x) => firstName(x) === 'Garrick')!;
    expect(c.intent).toMatchObject({ kind: 'talk', targetId: garrick.id, opener: 'Fancy a drink?', tone: 'friendly' });
    expect(h.logs.find((record) => record.character === 'Dorn' && record.kind === 'decide')!.summary).toContain('unknown place');
  });

  it('writes a whole conversation in one call and applies the feelings', async () => {
    const h = harness((tools) => tools.includes('conversation')
      ? [{ name: 'conversation', args: {
          lines: [{ speaker: 'Dorn', text: 'You look like a man with a plan, and I do not like it.' }, { speaker: 'Garrick', text: 'Only a plan for ale.' }],
          topic: 'ale and bad ideas', initiator_feels: 'much_better', other_feels: 'better',
        } }]
      : [{ name: 'continue', args: {} }]);
    h.step(2);
    const garrick = living(h.sim.state).find((x) => firstName(x) === 'Garrick')!;
    const dorn = living(h.sim.state).find((x) => firstName(x) === 'Dorn')!;
    const before = garrick.relationships[dorn.id]!.affinity;
    h.sim.enqueue({ type: 'set_intents', id: garrick.id, epoch: garrick.epoch, intents: [{ kind: 'talk', targetId: dorn.id, opener: 'Dorn! A word.', tone: 'friendly', source: 'llm' }] });
    const spoken = (await h.talk(garrick.id, dorn.id)).map((line) => line.text);
    expect(spoken).toEqual(['Dorn! A word.', 'You look like a man with a plan, and I do not like it.', 'Only a plan for ale.']);
    expect(garrick.relationships[dorn.id]!.affinity).toBeGreaterThan(before);
    expect(garrick.memories.some((memory) => memory.text.includes('ale and bad ideas'))).toBe(true);
    expect(h.logs.some((record) => record.kind === 'converse' && record.status === 'ok')).toBe(true);
  });

  it('falls back to local lines when the model answers with nonsense', async () => {
    const h = harness(() => 'I would rather write a poem.');
    h.step(2);
    const [a, b] = living(h.sim.state);
    h.sim.enqueue({ type: 'set_intents', id: a!.id, epoch: a!.epoch, intents: [{ kind: 'talk', targetId: b!.id, opener: 'Morning.', tone: 'friendly', source: 'llm' }] });
    const lines = await h.talk(a!.id, b!.id);
    expect(lines.length).toBeGreaterThanOrEqual(2);
    expect(lines[0]!.text).toBe('Morning.');
    expect(h.logs.some((record) => record.status === 'error')).toBe(true);
  });

  it('takes turns when asked to', async () => {
    // Each speaker ends the conversation once three lines have been said after the opener.
    const h = harness((tools, body) => {
      if (!tools.includes('reply')) return [{ name: 'continue', args: {} }];
      const said = (body.messages[1]!.content.split('The conversation so far:')[1] ?? '').split('\n').filter((line) => /^(You|\w+): /.test(line)).length;
      return [{ name: 'reply', args: { text: `Line ${said}.`, feeling: 'better', end: said >= 3 } }];
    }, { conversations: 'turns' });
    h.step(2);
    const [a, b] = living(h.sim.state);
    h.sim.enqueue({ type: 'set_intents', id: a!.id, epoch: a!.epoch, intents: [{ kind: 'talk', targetId: b!.id, opener: 'Hello.', tone: 'friendly', source: 'llm' }] });
    const lines = await h.talk(a!.id, b!.id, 10);
    expect(lines.map((line) => line.speakerId)).toEqual([a!.id, b!.id, a!.id, b!.id]);
    expect(lines[0]!.text).toBe('Hello.');
    expect(lines.slice(1).every((line) => /^Line \d+\.$/.test(line.text))).toBe(true);
  });

  it('does nothing and keeps the world local when disabled', () => {
    const h = harness(() => []);
    h.brain.configure({ ...settings, enabled: false });
    h.step(3);
    expect(h.brain.enabled).toBe(false);
    expect(h.sim.state.tuning.scriptWaitTicks).toBe(0);
  });
});

describe('prompts', () => {
  it('fit the budget and keep the essentials', () => {
    const state = createState(4);
    const sim = new Sim(state);
    for (let i = 0; i < 3000; i++) sim.step();
    const c = living(state)[2]!;
    c.stimuli.push({ kind: 'need', tick: state.tick, text: 'You are very hungry.', urgency: 1 });
    const small = promptText(decidePrompt(state, c, 500));
    const large = promptText(decidePrompt(state, c, 3000));
    expect(small.length).toBeLessThan(large.length);
    for (const text of [small, large]) {
      expect(text).toContain('You are very hungry.');
      expect(text).toContain('Decide what you do next');
      expect(text).toContain('Activities:');
    }
    expect(promptText(planPrompt(state, c, 1500))).toContain('set_plan');
  });
});
