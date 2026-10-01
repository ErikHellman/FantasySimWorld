import { dist } from '../core/math2d';
import { formatClock, gameTime, partOfDay, TICKS_PER_DAY } from '../core/time';
import { ACTIVITIES, ACTIVITY_IDS } from '../sim/data/activities';
import { CLASSES } from '../sim/data/classes';
import { MONSTERS } from '../sim/data/monsters';
import { describeActivity } from '../sim/describe';
import type { Character, Conversation, Id, Memory, Relationship, SimState } from '../sim/state';
import { RUNG_FEUD, lethalUnlocked, rungOf } from '../sim/systems/relationships';
import { blockAt } from '../sim/systems/routine';
import { firstName, living, makeCtx, monsterList, nameOf, placeNameAt, rel } from '../sim/util';
import { places } from '../world/places';
import type { ChatMessage, ToolDef } from './llm/types';
import { retrieve, since } from './memory/retrieval';
import { ATTACK, BRAWL, CONTINUE, CONVERSATION, DO_ACTIVITY, FIGHT, FINISH, FLEE, GIVE, GO_TO, REFLECTION, REPLY, SET_PLAN, SPARE, TALK_TO } from './tools/definitions';

/** Roughly four characters per token; good enough for budgeting. */
const CHARS_PER_TOKEN = 4;
const NEARBY = 22;

export interface Prompt {
  messages: ChatMessage[];
  tools: ToolDef[];
  maxCalls: number;
}

interface Section {
  text: string;
  /** lower numbers are dropped last */
  priority: number;
}

/** Joins sections, dropping the least important ones until the text fits the budget. */
export function assemble(sections: Section[], maxTokens: number): string {
  const kept = sections.filter((section) => section.text.trim().length > 0);
  const size = (): number => kept.reduce((sum, section) => sum + section.text.length + 2, 0);
  while (size() > maxTokens * CHARS_PER_TOKEN && kept.length > 1) {
    let worst = 0;
    for (let i = 1; i < kept.length; i++) if ((kept[i] as Section).priority >= (kept[worst] as Section).priority) worst = i;
    if ((kept[worst] as Section).priority === 0) break;
    kept.splice(worst, 1);
  }
  return kept.map((section) => section.text.trim()).join('\n\n');
}

const WORLD = `You are the mind of one villager in a small fantasy village in a valley. You decide what this one person says and intends to do; the world carries it out (walking, working and fighting happen on their own).

The valley: a village with a tavern (The Gilded Tankard), a market, a smithy, a chapel, a village green with a well, a training yard and cottages; around it a farm, a lumber camp, an old mine, the lake shore, a forest glade with a shrine, and a watchtower by the bridge. East of the river lie an old graveyard and ruins. On some nights the dead rise from the graveyard and attack; the watch raises the alarm and those who can fight defend the village. Death is permanent.

How to play:
- Stay in character. Act from your personality, needs, relationships and memories, not from what would be nice. People here hold grudges, flirt, gossip, sulk, forgive, lie and get things wrong.
- Answer only by calling the tools you are given. No narration.
- Anything said aloud is one or two short sentences in your own voice, with no stage directions and no quotation marks.
- A "thought" is one private sentence in the first person.
- Only name people and places that appear in the message. The village is small: nobody lives here but the people named, and the only monsters are the walking dead.`;

function level(value: number, words: [string, string, string]): string {
  return value < 0.34 ? words[0] : value < 0.67 ? words[1] : words[2];
}

/**
 * Who this character is. It opens the user message; the system message holds
 * only the world rules, which never change, so a server can cache them (and
 * the tool definitions) across all villagers.
 */
export function persona(c: Character): string {
  const p = c.profile;
  return [
    `You are ${p.name}, ${p.age}, ${p.occupation} (${p.pronouns}). You are a ${CLASSES[c.classId].label}.`,
    `Background: ${p.backstory}`,
    `Personality: ${p.traits.join(', ')}. You are ${level(p.temper, ['slow to anger', 'even-tempered', 'quick-tempered'])}, ${level(p.bravery, ['timid', 'steady enough in danger', 'fearless'])} and ${level(p.sociability, ['solitary', 'sociable enough', 'very sociable'])}.`,
    `How you speak: ${p.speechStyle}`,
    `What you want: ${p.goals.join('; ')}.`,
    c.summary ? `How you see yourself lately: ${c.summary}` : '',
  ].filter(Boolean).join('\n');
}

function asCharacter(c: Character, situation: string): ChatMessage[] {
  return [
    { role: 'system', content: WORLD },
    { role: 'user', content: `WHO YOU ARE\n${persona(c)}\n\nNOW\n${situation}` },
  ];
}

function needWords(c: Character): string {
  const parts: string[] = [];
  const { hunger, energy, social } = c.needs;
  parts.push(hunger < 20 ? 'very hungry' : hunger < 45 ? 'getting hungry' : 'fed');
  parts.push(energy < 15 ? 'exhausted' : energy < 40 ? 'tired' : 'rested');
  parts.push(social < 20 ? 'lonely' : social < 45 ? 'wanting company' : 'not lonely');
  const max = CLASSES[c.classId].maxHp;
  const hp = c.hp / max;
  parts.push(hp < 0.35 ? `badly hurt (${Math.round(c.hp)}/${max} health)` : hp < 0.75 ? `hurt (${Math.round(c.hp)}/${max} health)` : 'unhurt');
  return parts.join(', ');
}

function feelingWords(r: Relationship): string {
  const parts: string[] = [r.label];
  if (r.affinity >= 60) parts.push('you care for them deeply');
  else if (r.affinity >= 25) parts.push('you like them');
  else if (r.affinity <= -50) parts.push('you loathe them');
  else if (r.affinity <= -20) parts.push('you dislike them');
  if (r.trust <= -30) parts.push('you do not trust them');
  if (r.attraction >= 60) parts.push('you are in love with them');
  else if (r.attraction >= 35) parts.push('you are drawn to them');
  const rung = rungOf(r);
  if (rung === 'tension') parts.push('there is tension between you');
  else if (rung === 'grudge') parts.push('you hold a grudge against them');
  else if (rung === 'feud') parts.push('you have a feud with them and your patience is nearly gone');
  else if (rung === 'blood_feud') parts.push('you hate them enough to want them dead');
  return parts.join('; ');
}

function personLine(state: SimState, c: Character, other: Character, withActivity: boolean): string {
  const r = rel(c, other.id);
  const partner = c.partnerId === other.id ? ' (your partner)' : '';
  const doing = withActivity ? ` — ${other.inside ? `indoors at ${placeNameAt(other.pos)}` : describeActivity(state, other)}` : '';
  const view = r.summary ? ` You think: "${r.summary}"` : '';
  return `- ${firstName(other)}, ${other.profile.occupation}${partner}${doing}. ${capital(feelingWords(r))}.${view}`;
}

function capital(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function memoryLines(memories: readonly Memory[], nowDay: number): string {
  return memories
    .map((memory) => {
      const clock = formatClock(gameTime(memory.tick).minuteOfDay);
      const when = memory.day === nowDay ? `Earlier today (${clock})` : memory.day === nowDay - 1 ? `Yesterday (${clock})` : `Day ${memory.day}`;
      return `- ${when}: ${memory.text}`;
    })
    .join('\n');
}

function planLine(c: Character, hour: number): string {
  if (c.plan.length === 0) return '';
  const current = blockAt(c.plan, hour);
  return `Your plan for today: ${c.plan
    .map((block, index) => `${index === current ? '[now] ' : ''}${formatClock(block.hour * 60)} ${block.activity}${block.note ? ` (${block.note})` : ''}`)
    .join(' · ')}`;
}

function placeList(): string {
  return `Places: ${places.filter((place) => place.kind !== 'home').map((place) => place.name.replace(/^the /, '')).join(', ')}, home.`;
}

function activityList(): string {
  return `Activities: ${ACTIVITY_IDS.map((id) => `${id} (${ACTIVITIES[id].label})`).join('; ')}.`;
}

function header(state: SimState, c: Character): string {
  const time = gameTime(state.tick);
  const inventory = `${c.inventory.food} food, ${c.inventory.potion} healing potion${c.inventory.potion === 1 ? '' : 's'}, ${c.inventory.gift} small gift${c.inventory.gift === 1 ? '' : 's'}`;
  const monsters = monsterList(state).filter((m) => m.phase !== 'dying');
  const danger = monsters.length === 0
    ? 'All is quiet: there are no monsters about.'
    : `The dead are abroad: ${monsters.length} of them, the nearest ${Math.round(Math.min(...monsters.map((m) => dist(m.pos, c.pos))))} paces from you.`;
  return [
    `It is day ${time.day}, ${formatClock(time.minuteOfDay)} (${partOfDay(time.hour)}). You are at ${placeNameAt(c.pos)}, ${describeActivity(state, c)}.`,
    `You are ${needWords(c)}. You carry ${inventory}.`,
    danger,
  ].join('\n');
}

/** Everyone else, split into those within sight and those elsewhere who matter to this character. */
function people(state: SimState, c: Character): { near: Character[]; far: Character[] } {
  const near: Character[] = [];
  const far: Character[] = [];
  for (const other of living(state)) {
    if (other.id === c.id) continue;
    if (!other.inside && !c.inside && dist(other.pos, c.pos) <= NEARBY) near.push(other);
    else far.push(other);
  }
  const weight = (o: Character): number => {
    const r = rel(c, o.id);
    return Math.abs(r.affinity) + r.grievance + r.attraction + (c.partnerId === o.id ? 100 : 0);
  };
  far.sort((a, b) => weight(b) - weight(a));
  return { near, far };
}

/** Recent stimuli, without repeats, each with how long ago it was; newest last. */
function recentEvents(state: SimState, c: Character): string[] {
  const seen = new Set<string>();
  const lines: string[] = [];
  for (const stimulus of c.stimuli) {
    if (seen.has(stimulus.text)) continue;
    seen.add(stimulus.text);
    const minutes = Math.round((state.tick - stimulus.tick) / 20);
    lines.push(`${stimulus.text} (${minutes <= 2 ? 'just now' : `${minutes} minutes ago`})`);
  }
  return lines;
}

/** "What do you do next?" */
export function decidePrompt(state: SimState, c: Character, maxTokens: number): Prompt {
  const time = gameTime(state.tick);
  const { near, far } = people(state, c);
  const events = recentEvents(state, c);
  const cue = { subjects: [...near.map((o) => o.id), ...c.stimuli.flatMap((s) => (s.subjectId ? [s.subjectId] : []))], text: `${events.join(' ')} ${placeNameAt(c.pos)}` };
  const memories = retrieve(c, state.tick, cue, 7);

  const ctx = makeCtx(state, []);
  const feuds = living(state).filter((o) => o.id !== c.id && rel(c, o.id).grievance >= RUNG_FEUD);
  const killable = feuds.filter((o) => lethalUnlocked(ctx, c, o.id));
  const tools = [GO_TO, DO_ACTIVITY, TALK_TO, GIVE, CONTINUE];
  if (feuds.length > 0) tools.push(BRAWL);
  if (killable.length > 0) tools.push(ATTACK);

  const user = assemble([
    { priority: 0, text: header(state, c) },
    { priority: 0, text: events.length > 0 ? `What has just happened:\n${events.map((line) => `- ${line}`).join('\n')}` : '' },
    { priority: 1, text: near.length > 0 ? `People near you:\n${near.map((o) => personLine(state, c, o, true)).join('\n')}` : 'Nobody is near you.' },
    { priority: 4, text: far.length > 0 ? `Elsewhere:\n${far.slice(0, 5).map((o) => personLine(state, c, o, true)).join('\n')}` : '' },
    { priority: 3, text: memories.length > 0 ? `Things you remember:\n${memoryLines(memories, time.day)}` : '' },
    { priority: 2, text: planLine(c, time.hour) },
    { priority: 5, text: far.length > 5 ? `Also in the village: ${far.slice(5).map(firstName).join(', ')}.` : '' },
    { priority: 0, text: `${placeList()}\n${activityList()}` },
    {
      priority: 0,
      text: `Decide what you do next. Call one to three tools, in the order you will do them. If what you are already doing is still right, call continue.${
        feuds.length > 0 ? ` Your feud with ${feuds.map(firstName).join(' and ')} has gone far enough that you could come to blows.` : ''
      }`,
    },
  ], maxTokens);
  return { messages: asCharacter(c, user), tools, maxCalls: 3 };
}

/** "Danger: fight or flee?" */
export function reactPrompt(state: SimState, c: Character): Prompt {
  const monsters = monsterList(state).filter((m) => m.phase !== 'dying' && dist(m.pos, c.pos) < 40);
  const allies = living(state).filter((o) => o.id !== c.id && !o.inside && o.intent?.kind === 'fight' && dist(o.pos, c.pos) < 30);
  const attacker = living(state).find((o) => o.intent?.kind === 'fight' && o.intent.targetId === c.id);
  const kinds = new Map<string, number>();
  for (const monster of monsters) kinds.set(MONSTERS[monster.kind].label, (kinds.get(MONSTERS[monster.kind].label) ?? 0) + 1);
  const weapon = CLASSES[c.classId].weapon;
  const lines = [
    header(state, c),
    recentEvents(state, c).map((line) => `- ${line}`).join('\n'),
    monsters.length > 0 ? `Close by: ${[...kinds].map(([label, count]) => `${count} ${label}${count > 1 ? 's' : ''}`).join(', ')}.` : '',
    attacker ? `${firstName(attacker)} is attacking you. ${capital(feelingWords(rel(c, attacker.id)))}.` : '',
    allies.length > 0 ? `Fighting beside you: ${allies.map(firstName).join(', ')}.` : 'Nobody is fighting beside you.',
    `You fight ${weapon.style === 'melee' ? 'hand to hand' : `at range with ${weapon.style === 'magic' ? 'magic' : 'a bow'}`}.`,
    'Decide now: fight, or flee to your home. One tool call. If you shout something, keep it to a few words.',
  ];
  return { messages: asCharacter(c, lines.filter(Boolean).join('\n')), tools: [FIGHT, FLEE], maxCalls: 1 };
}

/** "Plan your day." */
export function planPrompt(state: SimState, c: Character, maxTokens: number): Prompt {
  const time = gameTime(state.tick);
  const { far, near } = people(state, c);
  const notable = [...near, ...far].filter((o) => {
    const r = rel(c, o.id);
    return Math.abs(r.affinity) >= 25 || r.grievance >= 20 || r.attraction >= 30 || c.partnerId === o.id;
  }).slice(0, 6);
  const memories = retrieve(c, state.tick, { subjects: notable.map((o) => o.id), text: '' }, 8);
  const watch = state.tick < c.watchUntilTick ? 'You have the night watch at the watchtower tonight (from about 20:30); rest in the afternoon.' : '';
  const user = assemble([
    { priority: 0, text: `${header(state, c)}\nIt is morning and the day is ahead of you.` },
    { priority: 1, text: `Your usual work: ${c.jobs.join(', ')}. What you enjoy: ${c.hobbies.join(', ')}. ${watch}` },
    { priority: 2, text: notable.length > 0 ? `People on your mind:\n${notable.map((o) => personLine(state, c, o, false)).join('\n')}` : '' },
    { priority: 3, text: memories.length > 0 ? `Things you remember:\n${memoryLines(memories, time.day)}` : '' },
    { priority: 0, text: activityList() },
    { priority: 0, text: 'Plan your day with set_plan: four to seven blocks from now until bedtime, each with the hour it starts. Include meals (eat) and end with sleep. Let the plan reflect what is on your mind, not only your duties.' },
  ], maxTokens);
  return { messages: asCharacter(c, user), tools: [SET_PLAN], maxCalls: 1 };
}

function brief(c: Character): string {
  const p = c.profile;
  return `${p.name}, ${p.age}, ${p.occupation} (${p.pronouns}). ${capital(p.traits.join(', '))}. Speaks: ${p.speechStyle}`;
}

function toneWords(tone: Conversation['tone'], a: Character, b: Character): string {
  switch (tone) {
    case 'hostile': return `${firstName(a)} has come to confront ${firstName(b)} over a grievance.`;
    case 'romantic': return `${firstName(a)} is making a romantic approach to ${firstName(b)}.`;
    case 'apology': return `${firstName(a)} has come to make amends with ${firstName(b)}.`;
    default: return `${firstName(a)} has stopped to talk with ${firstName(b)}.`;
  }
}

const SCENE = `You write short, natural conversations between two villagers in a small fantasy village, as a playwright would. Each speaks in their own voice and acts from their own personality, mood, memories and feelings about the other. Conversations may go badly: people can be cold, evasive, jealous, angry or hurt. Not every talk ends warmly, and a romantic approach is only welcomed if the other person feels the same.

Rules: each line is one or two short sentences, with no stage directions and no quotation marks. Neither knows the other's private thoughts or memories. The village is small: nobody lives here but the people named in the message, and the only monsters are the walking dead that rise from the graveyard at night. Answer only by calling the conversation tool.`;

/** One call that writes the rest of a conversation for both speakers. */
export function conversationPrompt(state: SimState, convo: Conversation, a: Character, b: Character, maxTokens: number): Prompt {
  const time = gameTime(state.tick);
  const side = (self: Character, other: Character): string => {
    const memories = retrieve(self, state.tick, { subjects: [other.id], text: '' }, 4).filter((memory) => memory.subjects.includes(other.id) || memory.importance >= 7);
    const r = rel(self, other.id);
    return [
      `${brief(self)}`,
      `Right now: ${needWords(self)}.`,
      `Towards ${firstName(other)}: ${feelingWords(r)}.${r.summary ? ` Thinks: "${r.summary}"` : ''}${self.partnerId === other.id ? ' They are a couple.' : self.partnerId ? ` Already has a partner (${nameOf(state, self.partnerId)}).` : ''}`,
      memories.length > 0 ? `Remembers:\n${memoryLines(memories, time.day)}` : '',
    ].filter(Boolean).join('\n');
  };
  const opening = convo.lines[0]?.text ?? '';
  const user = assemble([
    { priority: 0, text: `Day ${time.day}, ${formatClock(time.minuteOfDay)} (${partOfDay(time.hour)}), at ${placeNameAt(a.pos)}. ${toneWords(convo.tone, a, b)}` },
    { priority: 0, text: `THE ONE WHO STARTED IT\n${side(a, b)}` },
    { priority: 0, text: `THE OTHER\n${side(b, a)}` },
    { priority: 2, text: `Others in the village, whom they may mention: ${living(state).filter((o) => o.id !== a.id && o.id !== b.id).map((o) => `${firstName(o)} (${o.profile.occupation})`).join(', ')}.` },
    { priority: 0, text: opening ? `${firstName(a)} opens with: ${opening}` : '' },
    {
      priority: 0,
      text: opening
        ? `Write the three to five lines that follow, alternating speakers and starting with ${firstName(b)}, and bring the conversation to a natural end. Then say what it was about and how each now feels about the other.`
        : `Write the whole conversation: four to six lines, alternating speakers and starting with ${firstName(a)}, about whatever is on ${firstName(a)}'s mind, brought to a natural end. Then say what it was about and how each now feels about the other.`,
    },
  ], maxTokens);
  return { messages: [{ role: 'system', content: SCENE }, { role: 'user', content: user }], tools: [CONVERSATION], maxCalls: 1 };
}

/** One speaker's next line, from their own point of view. */
export function replyPrompt(state: SimState, convo: Conversation, lines: ReadonlyArray<{ speakerId: Id; text: string }>, self: Character, other: Character, maxTokens: number): Prompt {
  const time = gameTime(state.tick);
  const memories = retrieve(self, state.tick, { subjects: [other.id], text: lines.map((line) => line.text).join(' ') }, 5);
  const transcript = lines.length > 0
    ? lines.map((line) => `${line.speakerId === self.id ? 'You' : firstName(other)}: ${line.text}`).join('\n')
    : '(nothing has been said yet; you speak first)';
  const remaining = 6 - lines.length;
  const user = assemble([
    { priority: 0, text: header(state, self) },
    { priority: 0, text: `You are talking with ${firstName(other)}. ${convo.a === self.id ? 'You started the conversation.' : toneWords(convo.tone, other, self)}\n${personLine(state, self, other, false)}` },
    { priority: 2, text: memories.length > 0 ? `Things you remember:\n${memoryLines(memories, time.day)}` : '' },
    { priority: 0, text: `The conversation so far:\n${transcript}` },
    { priority: 0, text: `Say your next line with reply.${remaining <= 2 ? ' The conversation should end now: set end to true.' : ' Set end to true when the conversation has run its course.'}` },
  ], maxTokens);
  return { messages: asCharacter(self, user), tools: [REPLY], maxCalls: 1 };
}

/** The beaten opponent has yielded: spare or finish? */
export function mercyPrompt(state: SimState, attacker: Character, victim: Character): Prompt {
  const time = gameTime(state.tick);
  const memories = retrieve(attacker, state.tick, { subjects: [victim.id], text: '' }, 5);
  const watching = living(state).filter((o) => o.id !== attacker.id && o.id !== victim.id && !o.inside && dist(o.pos, attacker.pos) < 22).map(firstName);
  const user = [
    header(state, attacker),
    `You drew your weapon on ${firstName(victim)} and have beaten them. They are on the ground, wounded, and have cried out that they yield.`,
    personLine(state, attacker, victim, false),
    memories.length > 0 ? `Things you remember:\n${memoryLines(memories, time.day)}` : '',
    watching.length > 0 ? `Watching: ${watching.join(', ')}.` : 'Nobody else is watching.',
    'If you kill them, it is murder: the village will never forgive it. Decide: spare or finish. One tool call.',
  ].filter(Boolean).join('\n\n');
  return { messages: asCharacter(attacker, user), tools: [SPARE, FINISH], maxCalls: 1 };
}

/** End-of-day reflection: what does this character take from today? */
export function reflectPrompt(state: SimState, c: Character, maxTokens: number): Prompt {
  const time = gameTime(state.tick);
  const today = since(c, state.tick - TICKS_PER_DAY, 18);
  const involved = new Set(today.flatMap((memory) => memory.subjects));
  const others = living(state).filter((o) => involved.has(o.id)).slice(0, 6);
  const user = assemble([
    { priority: 0, text: `It is the night of day ${time.hour < 12 ? time.day - 1 : time.day} and you are falling asleep, thinking back over the day.` },
    { priority: 0, text: today.length > 0 ? `What happened:\n${memoryLines(today, time.day)}` : 'Nothing much happened.' },
    { priority: 1, text: others.length > 0 ? `The people involved:\n${others.map((o) => personLine(state, c, o, false)).join('\n')}` : '' },
    { priority: 0, text: 'Record what you take from the day with reflection.' },
  ], maxTokens);
  return { messages: asCharacter(c, user), tools: [REFLECTION], maxCalls: 1 };
}

/** The whole prompt as readable text, for the decision log. */
export function promptText(prompt: Prompt): string {
  return prompt.messages.map((message) => `[${message.role}]\n${message.content}`).join('\n\n');
}
