import { clamp } from '../../core/math2d';
import { TICKS_PER_DAY } from '../../core/time';
import type { Character, Feeling, Id, RelationLabel, Relationship, TalkTone } from '../state';
import { chemistry, chronicle, firstName, getCharacter, living, rel, remember, stimulate, type Ctx } from '../util';

export interface RelationDelta {
  affinity?: number;
  trust?: number;
  attraction?: number;
  grievance?: number;
  familiarity?: number;
}

/** Hostility ladder rungs, by grievance. */
export type Rung = 'calm' | 'tension' | 'grudge' | 'feud' | 'blood_feud';

export const RUNG_TENSION = 20;
export const RUNG_GRUDGE = 40;
export const RUNG_FEUD = 65;
export const RUNG_BLOOD = 85;

export function rungOf(r: Relationship): Rung {
  if (r.grievance >= RUNG_BLOOD) return 'blood_feud';
  if (r.grievance >= RUNG_FEUD) return 'feud';
  if (r.grievance >= RUNG_GRUDGE) return 'grudge';
  if (r.grievance >= RUNG_TENSION) return 'tension';
  return 'calm';
}

/** Label with hysteresis: a label is harder to lose than to gain, so it does not flicker. */
export function labelFor(r: Relationship, isPartner: boolean): RelationLabel {
  if (isPartner) return 'lover';
  const was = r.label;
  if (r.grievance >= 60 || r.affinity <= -55 || (was === 'enemy' && (r.grievance >= 40 || r.affinity <= -35))) return 'enemy';
  if (r.grievance >= 35 || r.affinity <= -25 || (was === 'rival' && (r.grievance >= 25 || r.affinity <= -10))) return 'rival';
  if (r.kin) return 'family';
  if ((r.affinity >= 60 && r.familiarity >= 40) || (was === 'close friend' && r.affinity >= 45)) return 'close friend';
  if (r.affinity >= 30 || ((was === 'friend' || was === 'close friend') && r.affinity >= 18)) return 'friend';
  if (r.familiarity >= 8) return 'acquaintance';
  return 'stranger';
}

const LABEL_NOTES: Partial<Record<RelationLabel, (name: string) => string>> = {
  friend: (name) => `I have come to think of ${name} as a friend.`,
  'close friend': (name) => `${name} has become one of my closest friends.`,
  rival: (name) => `There is bad blood between me and ${name} now.`,
  enemy: (name) => `${name} is my enemy.`,
  acquaintance: (name) => `${name} and I are on speaking terms, nothing more.`,
};

/** Applies a change to how `from` feels about `toId` and updates the label. */
export function adjust(ctx: Ctx, from: Character, toId: Id, delta: RelationDelta): void {
  const r = rel(from, toId);
  if (delta.affinity) r.affinity = clamp(r.affinity + delta.affinity, -100, 100);
  if (delta.trust) r.trust = clamp(r.trust + delta.trust, -100, 100);
  if (delta.attraction) r.attraction = clamp(r.attraction + delta.attraction, 0, 100);
  if (delta.familiarity) r.familiarity = clamp(r.familiarity + delta.familiarity, 0, 100);
  if (delta.grievance) {
    r.grievance = clamp(r.grievance + delta.grievance, 0, 100);
    if (delta.grievance > 0) r.lastOffenceTick = ctx.state.tick;
  }
  relabel(ctx, from, toId);
}

export function relabel(ctx: Ctx, from: Character, toId: Id): void {
  const r = rel(from, toId);
  const next = labelFor(r, from.partnerId === toId);
  if (next === r.label) return;
  const previous = r.label;
  r.label = next;
  const other = getCharacter(ctx.state, toId);
  if (!other) return;
  const note = LABEL_NOTES[next];
  const worsened = next === 'rival' || next === 'enemy';
  const improved = (next === 'friend' && previous !== 'close friend') || next === 'close friend';
  if (note && (worsened || improved)) {
    remember(ctx, from, note(firstName(other)), worsened ? 6 : 5, 'thought', [toId]);
    chronicle(
      ctx,
      worsened ? 'conflict' : 'social',
      next === 'enemy'
        ? `${firstName(from)} now counts ${firstName(other)} as an enemy.`
        : next === 'rival'
          ? `${firstName(from)} has fallen out with ${firstName(other)}.`
          : next === 'close friend'
            ? `${firstName(from)} and ${firstName(other)} have grown close.`
            : `${firstName(from)} has warmed to ${firstName(other)}.`,
      [from.id, toId],
    );
  }
}

const FEELING_AFFINITY: Record<Feeling, number> = { much_worse: -11, worse: -5, same: 0, better: 3, much_better: 6 };

/** How each side's reported feeling after a conversation changes the relationship. */
export function applyConversation(ctx: Ctx, self: Character, other: Character, feeling: Feeling, tone: TalkTone, initiator: boolean): void {
  const base = FEELING_AFFINITY[feeling];
  const delta: RelationDelta = { affinity: base, trust: Math.round(base * 0.5), familiarity: 3 };
  if (tone === 'hostile') {
    // Being confronted stings whatever is said; a good outcome clears the air.
    delta.grievance = base < 0 ? -base * 1.5 : -Math.max(8, base * 3);
    if (!initiator && base <= 0) delta.grievance = (delta.grievance ?? 0) + 6;
  } else if (tone === 'apology') {
    delta.grievance = base > 0 ? -(12 + base * 3) : 4;
  } else if (base < 0) {
    delta.grievance = -base * 1.1;
  } else if (base > 0) {
    delta.grievance = -base * 0.6;
  }
  if (tone === 'romantic') {
    delta.attraction = base > 0 ? base * 3 : base;
  } else if (base >= 3 && rel(self, other.id).affinity >= 30 && !self.partnerId && !rel(self, other.id).kin && chemistry(self, other)) {
    delta.attraction = 2;
  }
  adjust(ctx, self, other.id, delta);
  rel(self, other.id).lastTalkTick = ctx.state.tick;
}

export function becomeLovers(ctx: Ctx, a: Character, b: Character): void {
  a.partnerId = b.id;
  b.partnerId = a.id;
  adjust(ctx, a, b.id, { affinity: 15, trust: 10, attraction: 10 });
  adjust(ctx, b, a.id, { affinity: 15, trust: 10, attraction: 10 });
  remember(ctx, a, `${firstName(b)} and I are together now. My heart is full.`, 9, 'event', [b.id]);
  remember(ctx, b, `${firstName(a)} and I are together now. My heart is full.`, 9, 'event', [a.id]);
  chronicle(ctx, 'romance', `${firstName(a)} and ${firstName(b)} have fallen in love.`, [a.id, b.id]);
  ctx.events.push({ type: 'emote', id: a.id, emote: 'heart' }, { type: 'emote', id: b.id, emote: 'heart' });
  // Anyone who was sweet on either of them takes it hard.
  for (const other of living(ctx.state)) {
    if (other.id === a.id || other.id === b.id) continue;
    for (const [loved, rival] of [[a, b], [b, a]] as const) {
      if (rel(other, loved.id).attraction >= 40) {
        adjust(ctx, other, rival.id, { grievance: 18, affinity: -12 });
        remember(ctx, other, `${firstName(loved)} chose ${firstName(rival)}. It hurts more than I expected.`, 8, 'event', [loved.id, rival.id]);
        stimulate(ctx, other, 'romance', `You learned that ${firstName(loved)}, whom you care for, is now with ${firstName(rival)}.`, 1, loved.id);
        ctx.events.push({ type: 'emote', id: other.id, emote: 'broken_heart' });
      }
    }
  }
}

export function breakUp(ctx: Ctx, a: Character, b: Character, reason: string): void {
  a.partnerId = null;
  b.partnerId = null;
  adjust(ctx, a, b.id, { affinity: -15, attraction: -30, grievance: 12 });
  adjust(ctx, b, a.id, { affinity: -15, attraction: -30, grievance: 12 });
  remember(ctx, a, `${firstName(b)} and I have parted (${reason}).`, 8, 'event', [b.id]);
  remember(ctx, b, `${firstName(a)} and I have parted (${reason}).`, 8, 'event', [a.id]);
  chronicle(ctx, 'romance', `${firstName(a)} and ${firstName(b)} have parted ways.`, [a.id, b.id]);
  ctx.events.push({ type: 'emote', id: a.id, emote: 'broken_heart' }, { type: 'emote', id: b.id, emote: 'broken_heart' });
}

/** Once a day: tempers cool, and couples who have turned on each other split. */
export function dailyRelationships(ctx: Ctx): void {
  for (const c of living(ctx.state)) {
    for (const otherId of Object.keys(c.relationships)) {
      const r = c.relationships[otherId] as Relationship;
      if (r.grievance > 0) {
        const recent = ctx.state.tick - r.lastOffenceTick < TICKS_PER_DAY;
        r.grievance = Math.max(0, r.grievance - (recent ? 0.5 : 1.8) * (1 - c.profile.temper * 0.6));
      }
      if (ctx.state.characters[otherId]) relabel(ctx, c, otherId);
    }
    const partner = getCharacter(ctx.state, c.partnerId);
    if (partner && c.id < partner.id) {
      const mine = rel(c, partner.id);
      const theirs = rel(partner, c.id);
      if (mine.affinity < 12 || theirs.affinity < 12 || mine.grievance >= 50 || theirs.grievance >= 50) {
        breakUp(ctx, c, partner, 'too many quarrels');
      }
    }
  }
}

/** Whether `from` may take up arms against `to`: the top of the hostility ladder, with extra gates. */
export function lethalUnlocked(ctx: Ctx, from: Character, toId: Id): boolean {
  const r = rel(from, toId);
  if (r.grievance < RUNG_BLOOD) return false;
  if (ctx.state.tick - r.lastOffenceTick > TICKS_PER_DAY * 2) return false;
  if (ctx.state.tick - ctx.state.director.lastLethalTick < TICKS_PER_DAY * 3) return false;
  return ctx.state.order.length >= 8;
}
