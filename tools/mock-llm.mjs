// A stand-in for an OpenAI-compatible chat completions server, for trying the
// LLM path end to end without a model. It answers every request with plausible
// tool calls picked from the tools and options in the prompt.
//
//   node tools/mock-llm.mjs [port]          (default 8787; base URL http://localhost:8787/v1)
//   MOCK_DELAY=1500 node tools/mock-llm.mjs  simulated latency in ms (default 700)
//   MOCK_MODE=text node tools/mock-llm.mjs   answer with JSON in the text instead of native tool calls
//   POST /v1/embeddings answers at once with vectors from hashed words (fire, flames, smoke… count as one word)
//   create_villager (the casting of a new world and of newcomers) answers with a numbered, never-repeating person
import http from 'node:http';

const port = Number(process.argv[2] ?? 8787);
const delay = Number(process.env.MOCK_DELAY ?? 700);
const textMode = process.env.MOCK_MODE === 'text';

const pick = (list) => list[Math.floor(Math.random() * list.length)];
const OPENERS = ['Have you a moment? I have been meaning to ask you something.', 'You look troubled. What is it?', 'Strange days. Did you sleep at all last night?', 'I heard something at the market you will want to know.'];
const REPLIES = ['I was thinking the very same thing.', 'Hm. I am not sure I agree, but go on.', 'Ha! You always say that.', 'That is kind of you. Truly.', 'Mind your own affairs, for once.', 'We should speak of this again, somewhere quieter.'];
// People for `create_villager`: a first name is never given twice, and never one the prompt says is taken.
const FIRSTS = ['Edda', 'Bram', 'Wren', 'Osric', 'Tamsin', 'Hale', 'Ysolt', 'Garnet', 'Perrin', 'Linnea', 'Cade', 'Marta', 'Jory', 'Senna', 'Alder', 'Hild', 'Maud', 'Anselm', 'Berrick', 'Thea', 'Ivo', 'Rook', 'Sorrel', 'Fen', 'Dagny', 'Godric', 'Ottilie', 'Dunstan', 'Cerys', 'Merrick'];
const LASTS = ['Underhill', 'Blackwater', 'Fairweather', 'Stonebridge', 'Larkspur', 'Holt', 'Greenmantle', 'Ashby', 'Wolfe', 'Tanner', 'Thistlewood', 'Mossglen'];
const TRAITS = ['cheerful', 'wary', 'stubborn', 'generous', 'sharp-tongued', 'patient', 'restless', 'superstitious', 'ambitious', 'soft-hearted', 'boastful', 'thoughtful'];
const QUIRKS = ['coward', 'lazy', 'diligent', 'fighter', 'rumourmonger', 'drunkard'];
let invented = 0;
function invent(body, user) {
  const tool = (body.tools ?? []).find((t) => t.function?.name === 'create_villager');
  const classes = tool?.function?.parameters?.properties?.class?.enum ?? ['farmer', 'knight', 'ranger', 'druid', 'mage', 'rogue'];
  const taken = between(user, 'First names already taken, which the new person must not have: ', '.').split(',').map((s) => s.trim().toLowerCase());
  let first = FIRSTS[invented++ % FIRSTS.length];
  for (let tries = 0; taken.includes(first.toLowerCase()) && tries < FIRSTS.length; tries++) first = FIRSTS[invented++ % FIRSTS.length];
  const cls = pick(classes);
  const traits = [...new Set([pick(TRAITS), pick(TRAITS), pick(TRAITS)])];
  return {
    firstName: first, lastName: pick(LASTS), pronouns: pick(['she/her', 'he/him', 'they/them']), age: 19 + Math.floor(Math.random() * 40),
    class: cls, occupation: `travelling ${cls}`,
    backstory: `${first} came up the road with little more than a trade and a grudge against the cold. Settled here because the tavern was warm and nobody asked questions.`,
    speechStyle: pick(['Plain and direct.', 'Dry, with a joke in most sentences.', 'Slow, as if weighing every word.']), timbre: pick(['low and even', 'bright and quick', 'husky']),
    traits, goals: ['find a place in the village', 'stay alive through the nights'],
    ambition: pick([{ kind: 'wealth', words: 'I mean to die richer than I was born.' }, { kind: 'friends', words: 'I want friends who would walk through fire for me.' }, { kind: 'glory', words: 'Let the dead learn my name.' }]),
    temper: Math.round(Math.random() * 100) / 100, bravery: Math.round(Math.random() * 100) / 100, sociability: Math.round(Math.random() * 100) / 100,
    ...(Math.random() < 0.2 ? { quirk: pick(QUIRKS) } : {}),
    ...(user.includes('west road') ? { reason: pick(['looking for work', 'because the road ran out', 'hoping for a quieter life']) } : {}),
  };
}
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
  // The economy's tools, when the message offers them: now and then, so the real paths get exercised.
  if (names.includes('pick_up') && roll < 0.5) {
    const loot = /\(pick_up\): ([^;.]+)/.exec(user)?.[1] ?? 'the coins';
    return [{ name: 'pick_up', args: { what: loot, thought: 'Waste not.' } }];
  }
  const deal = /\(trade\): (buy|sell) (?:a |an )?(\w+) (?:from|to) (\w+)/.exec(user);
  if (names.includes('trade') && deal && roll < 0.3) {
    return [{ name: 'trade', args: { person: deal[3], item: deal[2], deal: deal[1], say: 'A fair price, I think.' } }];
  }
  if (names.includes('commission') && roll < 0.4) {
    const what = /\(commission\): ([^;.(]+)/.exec(user)?.[1]?.trim() ?? 'a new room';
    return [{ name: 'commission', args: { what, thought: 'Time I had something to show for the coin.' } }];
  }
  if (names.includes('pet') && roll < 0.2) {
    const animal = /\(pet\): ([^,(.]+)/.exec(user)?.[1]?.trim() ?? 'the dog';
    return [{ name: 'pet', args: { animal, say: 'Who is a good one, then?' } }];
  }
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
  if (names.includes('speech')) {
    const vow = /Write the vow you will speak to (\w+)/.exec(user)?.[1];
    const text = vow
      ? `${vow}, I have no fine words, only this: whatever comes up the road, it finds us standing together. That is my promise, before everyone here.`
      : 'We lay a neighbour in the earth today, and the valley is smaller for it. Remember the work of their hands and the nights they stood with us. Go gently, and let the living hold each other a little closer.';
    return [{ name: 'speech', args: { text } }];
  }
  if (names.includes('write_herald')) {
    // The headline is the first piece of yesterday's news, so the page can be seen to follow the chronicle.
    const news = /in the order it happened:\n- \d\d:\d\d (.+)/.exec(user)?.[1] ?? 'A quiet day in the valley';
    return [{ name: 'write_herald', args: {
      headline: news.length > 70 ? `${news.slice(0, 67)}...` : news,
      story1: 'The valley went about its business, and the Herald was there to see it.',
      gossip: pick(['Someone was seen whistling past the graveyard.', 'The miller swears the river talked back.']),
    } }];
  }
  if (names.includes('write_billboard')) return [{ name: 'write_billboard', args: { text: pick(['Eat more turnips.', 'Fear nothing. Except the dark.', 'Smile: someone may be watching.']) } }];
  if (names.includes('name_pet')) return [{ name: 'name_pet', args: { name: pick(['Bramblepaw', 'Hob', 'Turnip', 'Clementine', 'Sixpence', 'Mossy']) } }];
  if (names.includes('reflection')) return [{ name: 'reflection', args: { summary: 'I am holding together, though the nights wear on me. I have more friends here than I thought.', insights: ['I should say what I feel before it is too late.'], relationships: [] } }];
  if (names.includes('create_villager')) return [{ name: 'create_villager', args: invent(body, user) }];
  if (names.includes('go_to') || names.includes('do_activity')) return decide(names, user);
  return [{ name: names[0] ?? 'continue', args: {} }];
}

const CORS = { 'Access-Control-Allow-Origin': '*', 'Access-Control-Allow-Headers': '*', 'Access-Control-Allow-Methods': 'GET, POST, OPTIONS' };

// Words that should mean the same to the mock embedding model, so recall by meaning can be seen working.
const SAME = { flames: 'fire', blaze: 'fire', burning: 'fire', burnt: 'fire', smoke: 'fire', skeleton: 'dead', skeletons: 'dead', undead: 'dead' };

function embed(text) {
  const vector = new Array(64).fill(0);
  for (const raw of text.toLowerCase().match(/[a-z]{4,}/g) ?? []) {
    const word = SAME[raw] ?? raw;
    let hash = 2166136261;
    for (const ch of word) hash = Math.imul(hash ^ ch.charCodeAt(0), 16777619) >>> 0;
    vector[hash % 64] += 1;
  }
  if (vector.every((x) => x === 0)) vector[0] = 1;
  return vector;
}

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
    if (request.url?.endsWith('/embeddings')) {
      const inputs = Array.isArray(body.input) ? body.input : [body.input ?? ''];
      const data = inputs.map((text, index) => ({ object: 'embedding', index, embedding: embed(String(text)) }));
      const tokens = Math.round(raw.length / 4);
      response.writeHead(200, { ...CORS, 'Content-Type': 'application/json' }).end(JSON.stringify({ object: 'list', data, model: body.model ?? 'mock-embed', usage: { prompt_tokens: tokens, total_tokens: tokens } }));
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
