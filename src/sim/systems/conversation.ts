import { dist, facingTo } from '../../core/math2d';
import { minutesToTicks, secondsToTicks } from '../../core/time';
import { chance } from '../../core/rng';
import { ANIMS } from '../../render/anims';
import { WALK_SPEED } from '../data/classes';
import { genericContinuation, localScript } from '../data/smalltalk';
import type { Character, Conversation, ConversationLine, Feeling, Id, TalkTone } from '../state';
import { newId } from '../state';
import {
  bumpEpoch, chronicle, firstName, getCharacter, isFighting, nearbyCharacters, placeNameAt, rel, remember, say, setAnim, stimulate, their, type Ctx,
} from '../util';
import { face, routeTo, advance } from './movement';
import { adjust, applyConversation, becomeLovers } from './relationships';

export const MAX_LINES = 8;
const TALK_DISTANCE = 2.2;
const HAIL_TIMEOUT = secondsToTicks(14);
const GESTURES = ['interact', 'idle_alt', 'wave'] as const;

function seated(c: Character): boolean {
  return c.anim === 'sit_chair' || c.anim === 'sit_floor';
}

/** Starts a conversation: `a` has reached `b` and hails them. */
export function startConversation(ctx: Ctx, a: Character, b: Character, tone: TalkTone, opener: string): Conversation {
  const { state } = ctx;
  const script = opener ? genericContinuation(state, a, b, tone) : localScript(state, a, b, tone);
  // With no opening line given and a model to wait for, the whole conversation is left for it to
  // write; the locally written one (opener included) is kept as the fallback.
  const awaitOpener = !opener && state.tuning.scriptWaitTicks > 0;
  const localOpener: ConversationLine = { speakerId: a.id, text: opener || script.opener };
  const convo: Conversation = {
    id: newId(state, 'v'),
    a: a.id,
    b: b.id,
    tone,
    phase: 'hail',
    lines: awaitOpener ? [] : [localOpener],
    index: 0,
    lineEndTick: 0,
    startedTick: state.tick,
    awaitUntilTick: 0,
    closed: false,
    author: 'local',
    topic: script.topic,
    feelings: {},
    fallback: {
      lines: awaitOpener ? [localOpener, ...script.lines] : script.lines,
      topic: script.topic,
      feelings: { [a.id]: script.feelings[0], [b.id]: script.feelings[1] },
    },
  };
  state.conversations[convo.id] = convo;
  a.convoId = convo.id;
  b.convoId = convo.id;
  bumpEpoch(a);
  bumpEpoch(b);
  // Being hailed is a reflex: stop, turn, wave. No decision needed.
  if (!seated(b)) {
    b.act.path = [];
    setAnim(b, tone === 'hostile' ? 'idle' : 'wave', true);
  }
  ctx.events.push({ type: 'conversation_started', convoId: convo.id, a: a.id, b: b.id, tone });
  return convo;
}

function speak(ctx: Ctx, convo: Conversation): void {
  const line = convo.lines[convo.index] as ConversationLine;
  const speaker = getCharacter(ctx.state, line.speakerId);
  const listenerId = line.speakerId === convo.a ? convo.b : convo.a;
  if (!speaker) return;
  convo.lineEndTick = ctx.state.tick + say(ctx, speaker, line.text, listenerId, convo.tone === 'hostile') + secondsToTicks(0.35);
  if (!seated(speaker)) setAnim(speaker, GESTURES[convo.index % GESTURES.length] as keyof typeof ANIMS, true);
}

function useFallback(convo: Conversation): void {
  const room = Math.max(0, MAX_LINES - convo.lines.length);
  if (convo.lines.length === 0) {
    // Nothing was said yet: the local conversation is used whole, opener and all.
    convo.lines.push(...convo.fallback.lines.slice(0, room));
    convo.closed = true;
    convo.topic = convo.fallback.topic;
    return;
  }
  // The local lines were written to follow the opener; keep speakers alternating whatever came since.
  const last = convo.lines[convo.lines.length - 1]?.speakerId ?? convo.a;
  const first = last === convo.a ? convo.b : convo.a;
  const count = convo.lines.length > 1 ? Math.min(2, convo.fallback.lines.length) : convo.fallback.lines.length;
  const lines = convo.fallback.lines.slice(-count).map((line, index) => ({ speakerId: index % 2 === 0 ? first : last, text: line.text }));
  convo.lines.push(...lines.slice(0, room));
  convo.closed = true;
  if (!convo.topic) convo.topic = convo.fallback.topic;
}

/** Lines written by the LLM (a whole script, or one reply at a time). */
export function addLines(ctx: Ctx, convoId: Id, lines: ConversationLine[], closed: boolean, topic?: string, feelings?: Record<Id, Feeling>): void {
  const convo = ctx.state.conversations[convoId];
  if (!convo || convo.closed) return;
  const valid = lines.filter((line) => (line.speakerId === convo.a || line.speakerId === convo.b) && line.text.trim().length > 0);
  const room = Math.max(0, MAX_LINES - convo.lines.length);
  convo.lines.push(...valid.slice(0, room).map((line) => ({ speakerId: line.speakerId, text: line.text.trim().slice(0, 240) })));
  convo.author = 'llm';
  if (topic) convo.topic = topic.slice(0, 80);
  if (feelings) Object.assign(convo.feelings, feelings);
  if (closed || convo.lines.length >= MAX_LINES) convo.closed = true;
}

export function forceLocal(ctx: Ctx, convoId: Id): void {
  const convo = ctx.state.conversations[convoId];
  if (convo && !convo.closed) useFallback(convo);
}

function release(ctx: Ctx, convo: Conversation): [Character | null, Character | null] {
  const a = getCharacter(ctx.state, convo.a);
  const b = getCharacter(ctx.state, convo.b);
  for (const c of [a, b]) {
    if (c && c.convoId === convo.id) {
      c.convoId = null;
      c.nextSocialTick = ctx.state.tick + minutesToTicks(35 + (1 - c.profile.sociability) * 50);
    }
  }
  delete ctx.state.conversations[convo.id];
  return [a, b];
}

/** Ends a conversation that was cut short (someone was attacked, died or walked into a fight). */
export function abortConversation(ctx: Ctx, convoId: Id): void {
  const convo = ctx.state.conversations[convoId];
  if (!convo) return;
  const [a, b] = release(ctx, convo);
  if (a?.intent?.kind === 'talk') {
    a.outcome = `Your talk with ${b ? firstName(b) : 'them'} was cut short.`;
    a.act.phase = 'done';
  }
  ctx.events.push({ type: 'conversation_ended', convoId, a: convo.a, b: convo.b, feelings: {} });
}

function summaryOf(convo: Conversation, selfId: Id, otherName: string): string {
  const said = convo.lines.filter((line) => line.speakerId !== selfId).map((line) => line.text);
  const heard = said.length > 0 ? ` ${otherName} said: "${(said[said.length - 1] as string).slice(0, 110)}"` : '';
  const how = convo.tone === 'hostile' ? 'Had words with' : convo.tone === 'romantic' ? 'Had a tender talk with' : convo.tone === 'apology' ? 'Talked about making amends with' : 'Talked with';
  return `${how} ${otherName} about ${convo.topic || 'this and that'}.${heard}`;
}

const FEELING_NOTE: Record<Feeling, string> = {
  much_worse: 'I came away furious.',
  worse: 'It left a sour taste.',
  same: '',
  better: 'I feel better for it.',
  much_better: 'It lifted my heart.',
};

function finish(ctx: Ctx, convo: Conversation): void {
  const [a, b] = release(ctx, convo);
  if (!a || !b) return;
  const feelings: Record<Id, Feeling> = { ...convo.fallback.feelings, ...convo.feelings };
  const feelA = feelings[a.id] ?? 'same';
  const feelB = feelings[b.id] ?? 'same';

  applyConversation(ctx, a, b, feelA, convo.tone, true);
  applyConversation(ctx, b, a, feelB, convo.tone, false);
  a.needs.social = Math.min(100, a.needs.social + 15);
  b.needs.social = Math.min(100, b.needs.social + 15);

  const weight = convo.tone === 'friendly' ? 3 : 6;
  remember(ctx, a, `${summaryOf(convo, a.id, firstName(b))} ${FEELING_NOTE[feelA]}`.trim(), weight + (feelA === 'much_worse' || feelA === 'much_better' ? 2 : 0), 'talk', [b.id]);
  remember(ctx, b, `${summaryOf(convo, b.id, firstName(a))} ${FEELING_NOTE[feelB]}`.trim(), weight + (feelB === 'much_worse' || feelB === 'much_better' ? 2 : 0), 'talk', [a.id]);

  if (convo.tone === 'hostile') {
    const heated = feelA === 'much_worse' || feelB === 'much_worse';
    ctx.events.push({ type: 'emote', id: a.id, emote: 'anger' });
    if (feelB === 'worse' || feelB === 'much_worse') ctx.events.push({ type: 'emote', id: b.id, emote: 'anger' });
    chronicle(ctx, 'conflict', heated
      ? `${firstName(a)} and ${firstName(b)} had a furious row near ${placeLabel(a)}.`
      : feelA === 'better' && feelB === 'better'
        ? `${firstName(a)} confronted ${firstName(b)}, and they cleared the air.`
        : `${firstName(a)} confronted ${firstName(b)} near ${placeLabel(a)}.`, [a.id, b.id]);
    for (const witness of nearbyCharacters(ctx.state, a.pos, 11)) {
      if (witness.id === a.id || witness.id === b.id || witness.asleep) continue;
      remember(ctx, witness, `Overheard ${firstName(a)} and ${firstName(b)} quarrelling about ${convo.topic}.`, 4, 'event', [a.id, b.id]);
      adjust(ctx, witness, a.id, { affinity: -2 });
    }
  } else if (convo.tone === 'romantic') {
    const accepted = (feelA === 'better' || feelA === 'much_better') && (feelB === 'better' || feelB === 'much_better');
    const mutual = rel(a, b.id).attraction >= 45 && rel(b, a.id).attraction >= 45;
    if (accepted && mutual && !a.partnerId && !b.partnerId && !rel(a, b.id).kin) {
      becomeLovers(ctx, a, b);
    } else if (accepted) {
      ctx.events.push({ type: 'emote', id: a.id, emote: 'heart' });
      chronicle(ctx, 'romance', `${firstName(a)} and ${firstName(b)} were seen talking very close together.`, [a.id, b.id]);
    } else {
      ctx.events.push({ type: 'emote', id: a.id, emote: 'broken_heart' });
      adjust(ctx, a, b.id, { attraction: -8, grievance: 5 });
      chronicle(ctx, 'romance', `${firstName(a)} opened ${their(a)} heart to ${firstName(b)}, and was gently turned away.`, [a.id, b.id]);
    }
    // A partner who is close enough to see takes flirting badly.
    for (const [flirt, other] of [[a, b], [b, a]] as const) {
      const partner = getCharacter(ctx.state, flirt.partnerId);
      if (partner && partner.id !== other.id && !partner.inside && dist(partner.pos, flirt.pos) < 14) {
        adjust(ctx, partner, other.id, { grievance: 20, affinity: -15 });
        adjust(ctx, partner, flirt.id, { grievance: 14, affinity: -12, trust: -15 });
        remember(ctx, partner, `Saw ${firstName(flirt)} making eyes at ${firstName(other)}. It cut deep.`, 8, 'event', [flirt.id, other.id]);
        stimulate(ctx, partner, 'romance', `You saw your partner ${firstName(flirt)} in a tender conversation with ${firstName(other)}.`, 2, other.id);
        ctx.events.push({ type: 'emote', id: partner.id, emote: 'anger' });
      }
    }
  } else if (convo.tone === 'apology') {
    chronicle(ctx, 'social', feelB === 'better' || feelB === 'much_better'
      ? `${firstName(a)} apologised to ${firstName(b)}, and was forgiven.`
      : `${firstName(a)} apologised to ${firstName(b)}, who was not ready to hear it.`, [a.id, b.id]);
  } else if (feelA === 'much_better' && feelB === 'much_better') {
    ctx.events.push({ type: 'emote', id: a.id, emote: 'laugh' }, { type: 'emote', id: b.id, emote: 'laugh' });
  }

  if (a.intent?.kind === 'talk') {
    a.outcome = `You talked with ${firstName(b)} about ${convo.topic || 'this and that'}.`;
    a.act.phase = 'done';
  }
  stimulate(ctx, b, 'talked', `${firstName(a)} came to talk with you about ${convo.topic || 'this and that'}.`, 0, a.id);
  ctx.events.push({ type: 'conversation_ended', convoId: convo.id, a: a.id, b: b.id, feelings: { [a.id]: feelA, [b.id]: feelB } });
}

function placeLabel(c: Character): string {
  return placeNameAt(c.pos);
}

export function updateConversations(ctx: Ctx): void {
  const { state } = ctx;
  for (const id of Object.keys(state.conversations)) {
    const convo = state.conversations[id] as Conversation;
    const a = getCharacter(state, convo.a);
    const b = getCharacter(state, convo.b);
    if (!a || !b || isFighting(a) || isFighting(b) || a.asleep || b.asleep) {
      abortConversation(ctx, id);
      continue;
    }

    if (!seated(b)) face(b, a.pos);

    if (convo.phase === 'hail') {
      const d = dist(a.pos, b.pos);
      const timedOut = state.tick - convo.startedTick > HAIL_TIMEOUT;
      if (d > TALK_DISTANCE && !timedOut) {
        if (a.act.path.length === 0 || state.tick >= a.act.repathTick) {
          a.act.path = routeTo(a.pos, b.pos) ?? [];
          a.act.repathTick = state.tick + secondsToTicks(1);
        }
        // Stop a comfortable distance short rather than standing on their toes.
        if (d > TALK_DISTANCE + 0.3) {
          advance(a, a.act.path, WALK_SPEED);
          setAnim(a, 'walk');
        }
        continue;
      }
      a.act.path = [];
      a.facing = facingTo(a.pos, b.pos);
      if (!seated(b)) b.facing = facingTo(b.pos, a.pos);
      if (state.tuning.scriptWaitTicks === 0 && !convo.closed) useFallback(convo);
      if (convo.lines.length === 0) {
        // Face to face, but the opening line has not been written yet.
        convo.phase = 'await';
        convo.index = -1;
        convo.awaitUntilTick = state.tick + state.tuning.scriptWaitTicks;
        ctx.events.push({ type: 'conversation_waiting', convoId: convo.id }, { type: 'emote', id: a.id, emote: 'thinking' });
        continue;
      }
      convo.phase = 'play';
      convo.index = 0;
      speak(ctx, convo);
      continue;
    }

    if (!seated(a)) face(a, b.pos);

    if (convo.phase === 'play') {
      if (state.tick < convo.lineEndTick) {
        // Return to a neutral stance once the gesture has played.
        for (const c of [a, b]) {
          if (!seated(c) && !ANIMS[c.anim].loop && state.tick - convo.lineEndTick > -secondsToTicks(0.4)) setAnim(c, 'idle');
        }
        continue;
      }
      if (convo.index + 1 < convo.lines.length) {
        convo.index++;
        speak(ctx, convo);
      } else if (convo.closed) {
        finish(ctx, convo);
      } else {
        convo.phase = 'await';
        convo.awaitUntilTick = state.tick + state.tuning.scriptWaitTicks;
        for (const c of [a, b]) if (!seated(c)) setAnim(c, 'idle');
        ctx.events.push({ type: 'conversation_waiting', convoId: convo.id });
        const next = convo.lines[convo.index]?.speakerId === convo.a ? b : a;
        ctx.events.push({ type: 'emote', id: next.id, emote: 'thinking' });
      }
      continue;
    }

    // Awaiting more lines.
    if (convo.index + 1 < convo.lines.length) {
      convo.phase = 'play';
      convo.index++;
      speak(ctx, convo);
    } else if (convo.closed) {
      finish(ctx, convo);
    } else if (state.tick >= convo.awaitUntilTick) {
      useFallback(convo);
      if (convo.index + 1 >= convo.lines.length) finish(ctx, convo);
    }
  }
}

/** Whether two characters have spoken too recently to start again. */
export function talkedRecently(ctx: Ctx, a: Character, b: Character, minutes = 180): boolean {
  return ctx.state.tick - rel(a, b.id).lastTalkTick < minutesToTicks(minutes);
}

/** Random chance scaled by how talkative the sim is currently allowed to be. */
export function chatterRoll(ctx: Ctx, c: Character): boolean {
  const need = (100 - c.needs.social) / 100;
  return chance(ctx.state, ctx.state.tuning.chatter * (0.25 + c.profile.sociability * 0.5 + need * 0.4));
}
