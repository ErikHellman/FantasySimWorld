// A stand-in for an OpenAI-compatible chat completions server, for trying the
// LLM path end to end without a model. It answers every request with plausible
// tool calls picked from the tools and options in the prompt.
//
//   node tools/mock-llm.mjs [port]          (default 8787; base URL http://localhost:8787/v1)
//   MOCK_DELAY=1500 node tools/mock-llm.mjs  simulated latency in ms (default 700)
//   MOCK_MODE=text node tools/mock-llm.mjs   answer with JSON in the text instead of native tool calls
import http from 'node:http';

const port = Number(process.argv[2] ?? 8787);
const delay = Number(process.env.MOCK_DELAY ?? 700);
const textMode = process.env.MOCK_MODE === 'text';

const pick = (list) => list[Math.floor(Math.random() * list.length)];
const OPENERS = ['Have you a moment? I have been meaning to ask you something.', 'You look troubled. What is it?', 'Strange days. Did you sleep at all last night?', 'I heard something at the market you will want to know.'];
const REPLIES = ['I was thinking the very same thing.', 'Hm. I am not sure I agree, but go on.', 'Ha! You always say that.', 'That is kind of you. Truly.', 'Mind your own affairs, for once.', 'We should speak of this again, somewhere quieter.'];
const THOUGHTS = ['I should make the most of the daylight.', 'Something about today feels off.', 'A little company would do me good.', 'Work first; the rest can wait.'];

function between(text, start, end) {
  const from = text.indexOf(start);
  if (from < 0) return '';
  const rest = text.slice(from + start.length);
  const to = end ? rest.indexOf(end) : -1;
  return to < 0 ? rest : rest.slice(0, to);
}

function decide(names, user) {
  const nearby = [...between(user, 'People near you:', '\n\n').matchAll(/^- (\w+),/gm)].map((m) => m[1]);
  const placeList = between(user, 'Places: ', '.').split(', ').filter(Boolean);
  const activities = [...between(user, 'Activities: ', '\n').matchAll(/(\w+) \(/g)].map((m) => m[1]);
  const roll = Math.random();
  if (names.includes('talk_to') && nearby.length > 0 && roll < 0.35) {
    return [{ name: 'talk_to', args: { person: pick(nearby), message: pick(OPENERS), tone: Math.random() < 0.12 ? 'hostile' : 'friendly', thought: pick(THOUGHTS) } }];
  }
  if (names.includes('do_activity') && activities.length > 0 && roll < 0.7) {
    return [{ name: 'do_activity', args: { activity: pick(activities), thought: pick(THOUGHTS) } }];
  }
  if (names.includes('go_to') && placeList.length > 0 && roll < 0.85) {
    return [
      { name: 'go_to', args: { place: pick(placeList), thought: pick(THOUGHTS) } },
      { name: 'do_activity', args: { activity: pick(activities.length ? activities : ['rest']) } },
    ];
  }
  return [{ name: 'continue', args: { thought: 'This is as good a use of the hour as any.' } }];
}

function respond(body) {
  const names = (body.tools ?? []).map((tool) => tool.function?.name);
  const user = body.messages?.[body.messages.length - 1]?.content ?? '';
  if (names.includes('conversation')) {
    const opener = /^(\w+) opens with:/m.exec(user)?.[1] ?? 'A';
    const other = /starting with (\w+)/.exec(user)?.[1] ?? 'B';
    const count = 3 + Math.floor(Math.random() * 2);
    const lines = Array.from({ length: count }, (_, index) => ({ speaker: index % 2 === 0 ? other : opener, text: pick(REPLIES) }));
    return [{ name: 'conversation', args: { lines, topic: pick(['the night attacks', 'village gossip', 'work and weather', 'an old quarrel']), initiator_feels: pick(['better', 'same', 'worse', 'much_better']), other_feels: pick(['better', 'same', 'worse']) } }];
  }
  if (names.includes('reply')) {
    const said = (between(user, 'The conversation so far:', '\n\n').match(/^(You|\w+): /gm) ?? []).length;
    return [{ name: 'reply', args: { text: pick(REPLIES), feeling: pick(['better', 'same', 'worse']), end: said >= 3 + Math.floor(Math.random() * 2) } }];
  }
  if (names.includes('set_plan')) {
    return [{ name: 'set_plan', args: { thought: 'Keep busy, eat well, and see a friend this evening.', blocks: [
      { hour: 6.5, activity: 'eat', note: 'breakfast' }, { hour: 7.5, activity: pick(['forage', 'fish', 'train', 'trade', 'smith']) },
      { hour: 12.5, activity: 'eat' }, { hour: 13.5, activity: pick(['stroll', 'pray', 'shop', 'chop_wood']) },
      { hour: 18.5, activity: 'eat' }, { hour: 19.5, activity: 'drink', note: 'with friends' }, { hour: 22, activity: 'sleep' },
    ] } }];
  }
  if (names.includes('fight')) return [Math.random() < 0.8 ? { name: 'fight', args: { say: 'Hold the line!', thought: 'If I run, who stands?' } } : { name: 'flee', args: { say: 'Too many! Fall back!' } }];
  if (names.includes('spare')) return [{ name: 'spare', args: { say: 'Get up. And never cross me again.', thought: 'I am no murderer.' } }];
  if (names.includes('react_to_billboard')) return [{ name: 'react_to_billboard', args: pick([
    { reaction: 'laugh', say: 'Ha! Someone has a sense of humour.' },
    { reaction: 'scoff', say: 'Who pays for these things?' },
    { reaction: 'love', say: 'Well, that is rather nice.', thought: 'I will hold on to that one.' },
  ]) }];
  if (names.includes('reflection')) return [{ name: 'reflection', args: { summary: 'I am holding together, though the nights wear on me. I have more friends here than I thought.', insights: ['I should say what I feel before it is too late.'], relationships: [] } }];
  if (names.includes('go_to') || names.includes('do_activity')) return decide(names, user);
  return [{ name: names[0] ?? 'continue', args: {} }];
}

const CORS = { 'Access-Control-Allow-Origin': '*', 'Access-Control-Allow-Headers': '*', 'Access-Control-Allow-Methods': 'GET, POST, OPTIONS' };

http.createServer((request, response) => {
  if (request.method === 'OPTIONS') {
    response.writeHead(204, CORS).end();
    return;
  }
  if (request.url?.endsWith('/models')) {
    response.writeHead(200, { ...CORS, 'Content-Type': 'application/json' }).end(JSON.stringify({ data: [{ id: 'mock-villager' }] }));
    return;
  }
  let raw = '';
  request.on('data', (chunk) => (raw += chunk));
  request.on('end', () => {
    let body = {};
    try {
      body = JSON.parse(raw || '{}');
    } catch {
      response.writeHead(400, CORS).end('bad json');
      return;
    }
    const calls = respond(body);
    const message = textMode
      ? { role: 'assistant', content: JSON.stringify({ calls: calls.map((call) => ({ tool: call.name, args: call.args })) }) }
      : { role: 'assistant', content: null, tool_calls: calls.map((call, index) => ({ id: `call_${index}`, type: 'function', function: { name: call.name, arguments: JSON.stringify(call.args) } })) };
    const payload = { id: 'mock', object: 'chat.completion', model: body.model ?? 'mock-villager', choices: [{ index: 0, message, finish_reason: textMode ? 'stop' : 'tool_calls' }], usage: { prompt_tokens: Math.round(raw.length / 4), completion_tokens: 40 } };
    setTimeout(() => response.writeHead(200, { ...CORS, 'Content-Type': 'application/json' }).end(JSON.stringify(payload)), delay * (0.6 + Math.random() * 0.8));
    console.log(`${new Date().toISOString().slice(11, 19)} ${calls.map((call) => call.name).join('+')}`);
  });
}).listen(port, () => console.log(`Mock LLM on http://localhost:${port}/v1 (delay ~${delay} ms${textMode ? ', text mode' : ''})`));
