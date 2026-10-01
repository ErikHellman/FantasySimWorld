import { ACTIVITY_IDS } from '../../sim/data/activities';
import type { ToolDef } from '../llm/types';

/**
 * The tools a character's mind can call. Schemas are static (valid people,
 * places and activities are listed in the prompt and checked afterwards), so
 * local servers can keep the tool block in their prompt cache.
 */
const SAY = { type: 'string', description: 'Optional. What you say aloud as you begin: one short sentence in your own voice, no quotation marks.' };
const THOUGHT = { type: 'string', description: 'Optional. One private sentence, in the first person, on why.' };
const PERSON = { type: 'string', description: 'First name of a living person named in the message.' };
const FEELING = {
  type: 'string',
  enum: ['much_worse', 'worse', 'same', 'better', 'much_better'],
};

const tool = (name: string, description: string, properties: Record<string, unknown>, required: string[] = []): ToolDef => ({
  name,
  description,
  parameters: { type: 'object', properties, required, additionalProperties: false },
});

export const GO_TO = tool('go_to', 'Walk to a place.', {
  place: { type: 'string', description: 'A place named in the message.' },
  say: SAY,
  thought: THOUGHT,
}, ['place']);

export const DO_ACTIVITY = tool('do_activity', 'Go and do one of the listed activities for a while.', {
  activity: { type: 'string', enum: ACTIVITY_IDS, description: 'The activity id.' },
  say: SAY,
  thought: THOUGHT,
}, ['activity']);

export const TALK_TO = tool('talk_to', 'Walk up to someone and start a conversation.', {
  person: PERSON,
  message: { type: 'string', description: 'Your opening line: one or two short sentences in your own voice.' },
  tone: {
    type: 'string',
    enum: ['friendly', 'romantic', 'hostile', 'apology'],
    description: 'friendly: ordinary talk. romantic: flirting or confessing feelings. hostile: confronting them over a grievance. apology: trying to make amends.',
  },
  thought: THOUGHT,
}, ['person', 'message', 'tone']);

export const GIVE = tool('give', 'Walk up to someone and give them something you are carrying.', {
  person: PERSON,
  item: { type: 'string', enum: ['food', 'potion', 'gift'] },
  say: SAY,
  thought: THOUGHT,
}, ['person', 'item']);

export const CONTINUE = tool('continue', 'Carry on with what you are already doing.', { thought: THOUGHT });

export const BRAWL = tool('brawl', 'Start a fist fight with someone you have a feud with. Nobody dies, but it will be remembered.', {
  person: PERSON,
  say: SAY,
  thought: THOUGHT,
}, ['person']);

export const ATTACK = tool('attack', 'Draw your weapon on another villager, meaning to kill. Only when hatred has gone as far as it can go; the whole village will know.', {
  person: PERSON,
  say: SAY,
  thought: THOUGHT,
}, ['person']);

export const FIGHT = tool('fight', 'Stand and fight the danger in front of you.', { say: SAY, thought: THOUGHT });
export const FLEE = tool('flee', 'Run for home and hide until it is safe.', { say: SAY, thought: THOUGHT });

export const SET_PLAN = tool('set_plan', 'Set your plan for today.', {
  blocks: {
    type: 'array',
    minItems: 3,
    maxItems: 8,
    description: 'The day in order, from waking to bedtime. Include meals and end with sleep.',
    items: {
      type: 'object',
      properties: {
        hour: { type: 'number', description: 'Hour of day the block starts, 0-24 (for example 13.5 for half past one).' },
        activity: { type: 'string', enum: ACTIVITY_IDS },
        note: { type: 'string', description: 'Optional. A few words on why or with whom.' },
      },
      required: ['hour', 'activity'],
      additionalProperties: false,
    },
  },
  thought: { type: 'string', description: 'One sentence, in the first person, on what you want from today.' },
}, ['blocks']);

export const CONVERSATION = tool('conversation', 'Write the rest of the conversation.', {
  lines: {
    type: 'array',
    minItems: 2,
    maxItems: 6,
    description: 'The lines that follow the opening line, alternating speakers, starting with the person who was spoken to.',
    items: {
      type: 'object',
      properties: {
        speaker: { type: 'string', description: 'First name of the speaker.' },
        text: { type: 'string', description: 'What they say: one or two short sentences, no stage directions.' },
      },
      required: ['speaker', 'text'],
      additionalProperties: false,
    },
  },
  topic: { type: 'string', description: 'What the conversation was about, in a few words.' },
  initiator_feels: { ...FEELING, description: 'How the one who started the conversation now feels about the other, compared with before.' },
  other_feels: { ...FEELING, description: 'How the other now feels about the one who started it, compared with before.' },
}, ['lines', 'topic', 'initiator_feels', 'other_feels']);

export const REPLY = tool('reply', 'Say your next line in the conversation.', {
  text: { type: 'string', description: 'What you say: one or two short sentences in your own voice, no stage directions.' },
  feeling: { ...FEELING, description: 'How you now feel about the person you are talking to, compared with before this conversation.' },
  end: { type: 'boolean', description: 'True if this line ends the conversation.' },
}, ['text', 'feeling', 'end']);

export const SPARE = tool('spare', 'Let your beaten opponent live.', { say: SAY, thought: THOUGHT });
export const FINISH = tool('finish', 'Kill your beaten opponent. This cannot be undone.', { say: SAY, thought: THOUGHT });

export const REFLECTION = tool('reflection', 'Record what you take from the day.', {
  summary: { type: 'string', description: 'Two or three sentences, first person: who you are and where things stand for you now.' },
  insights: {
    type: 'array',
    minItems: 1,
    maxItems: 3,
    items: { type: 'string' },
    description: 'One to three conclusions you have drawn (about yourself, others, or what to do), each one sentence in the first person.',
  },
  relationships: {
    type: 'array',
    maxItems: 4,
    description: 'For up to four people who mattered today: how you now see them.',
    items: {
      type: 'object',
      properties: { person: PERSON, opinion: { type: 'string', description: 'One sentence, first person.' } },
      required: ['person', 'opinion'],
      additionalProperties: false,
    },
  },
}, ['summary', 'insights']);
