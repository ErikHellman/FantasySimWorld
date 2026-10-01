import type { ActivityId } from '../../world/places';
import { places, type Place } from '../../world/places';
import { ACTIVITIES, ACTIVITY_IDS } from '../../sim/data/activities';
import type { Character, ConversationLine, Feeling, Id, Intent, ItemId, PlanBlock, SimState, TalkTone } from '../../sim/state';
import { RUNG_FEUD } from '../../sim/systems/relationships';
import { firstName, living, monsterList, rel } from '../../sim/util';
import type { ToolCall } from '../llm/types';

const FEELINGS: Feeling[] = ['much_worse', 'worse', 'same', 'better', 'much_better'];
const TONES: TalkTone[] = ['friendly', 'romantic', 'hostile', 'apology'];
const ITEMS: ItemId[] = ['food', 'potion', 'gift'];
const MAX_SPOKEN = 220;

const PLACE_ALIASES: Record<string, string> = {
  tavern: 'tavern', tankard: 'tavern', inn: 'tavern', pub: 'tavern',
  green: 'green', well: 'green', square: 'green', crossroads: 'green',
  market: 'market', stalls: 'market',
  smithy: 'smithy', forge: 'smithy', blacksmith: 'smithy',
  chapel: 'chapel', church: 'chapel', temple: 'chapel',
  yard: 'training_yard', barracks: 'training_yard', training: 'training_yard',
  farm: 'farm', windmill: 'farm', fields: 'farm',
  lumber: 'lumber_camp', sawmill: 'lumber_camp',
  glade: 'glade', shrine: 'glade', forest: 'glade', campfire: 'glade', woods: 'glade',
  mine: 'mine',
  lake: 'docks', shore: 'docks', docks: 'docks', beach: 'docks',
  watchtower: 'watchtower', tower: 'watchtower', bridge: 'watchtower',
  graveyard: 'graveyard', cemetery: 'graveyard', graves: 'graveyard',
  ruins: 'ruins',
  road: 'west_road',
};

const ACTIVITY_ALIASES: Record<string, ActivityId> = {
  work: 'smith', forge: 'smith', smithing: 'smith', hammer: 'smith',
  wood: 'chop_wood', chop: 'chop_wood', lumber: 'chop_wood',
  fishing: 'fish', farming: 'farm', mining: 'mine', dig: 'mine',
  meal: 'eat', breakfast: 'eat', lunch: 'eat', dinner: 'eat', supper: 'eat', food: 'eat',
  ale: 'drink', beer: 'drink', drinking: 'drink',
  nap: 'sleep', bed: 'sleep', sleeping: 'sleep',
  sit: 'rest', bench: 'rest',
  market: 'trade', stall: 'trade', sell: 'trade', trading: 'trade',
  buy: 'shop', browse: 'shop', shopping: 'shop',
  meditate: 'pray', prayer: 'pray', praying: 'pray',
  training: 'train', practise: 'train', practice: 'train', spar: 'train',
  herbs: 'forage', gather: 'forage', foraging: 'forage',
  guard: 'watch', patrol: 'watch',
  campfire: 'relax', fire: 'relax',
  potions: 'brew', brewing: 'brew', potion: 'brew',
  grave: 'mourn', graves: 'mourn',
  walk: 'stroll', wander: 'stroll',
};

function norm(value: unknown): string {
  return typeof value === 'string' ? value.trim().toLowerCase().replace(/^the\s+/, '').replace(/[^a-z0-9_ '-]/g, '') : '';
}

function editDistance(a: string, b: string): number {
  const row = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    let previous = row[0] as number;
    row[0] = i;
    for (let j = 1; j <= b.length; j++) {
      const current = row[j] as number;
      row[j] = Math.min((row[j] as number) + 1, (row[j - 1] as number) + 1, previous + (a[i - 1] === b[j - 1] ? 0 : 1));
      previous = current;
    }
  }
  return row[b.length] as number;
}

/** Finds a living person by the name a model gave: exact, first name, prefix, or a near miss. */
export function resolvePerson(state: SimState, name: unknown, exceptId?: Id): Character | null {
  const wanted = norm(name);
  if (!wanted) return null;
  const people = living(state).filter((c) => c.id !== exceptId);
  const first = (c: Character): string => firstName(c).toLowerCase();
  return (
    people.find((c) => c.profile.name.toLowerCase() === wanted || first(c) === wanted) ??
    people.find((c) => wanted.split(' ')[0] === first(c) || first(c).startsWith(wanted) || c.profile.name.toLowerCase().startsWith(wanted)) ??
    people.find((c) => editDistance(first(c), wanted.split(' ')[0] as string) <= 1) ??
    null
  );
}

export function resolvePlace(c: Character, name: unknown): Place | null {
  const wanted = norm(name);
  if (!wanted) return null;
  if (wanted === 'home' || wanted === 'my home' || wanted === 'my house') return places.find((p) => p.id === c.homeId) ?? null;
  const exact = places.find((p) => p.id === wanted || p.id === wanted.replace(/ /g, '_') || norm(p.name) === wanted);
  if (exact) return exact;
  for (const word of wanted.split(/[\s_]+/)) {
    const alias = PLACE_ALIASES[word];
    if (alias) return places.find((p) => p.id === alias) ?? null;
  }
  return places.find((p) => norm(p.name).includes(wanted) || wanted.includes(norm(p.name))) ?? null;
}

export function resolveActivity(value: unknown): ActivityId | null {
  const wanted = norm(value).replace(/ /g, '_');
  if (!wanted) return null;
  if ((ACTIVITY_IDS as string[]).includes(wanted)) return wanted as ActivityId;
  for (const word of wanted.split('_')) {
    if ((ACTIVITY_IDS as string[]).includes(word)) return word as ActivityId;
    const alias = ACTIVITY_ALIASES[word];
    if (alias) return alias;
  }
  return ACTIVITY_IDS.find((id) => ACTIVITIES[id].label.toLowerCase().includes(wanted.replace(/_/g, ' '))) ?? null;
}

/** Tidies a spoken line: no quotes, no stage directions, not too long. */
export function spoken(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined;
  let text = value.replace(/\*[^*]*\*/g, ' ').replace(/\([^)]*\)/g, ' ').replace(/\s+/g, ' ').trim();
  text = text.replace(/^["“'‘]+|["”'’]+$/g, '').trim();
  if (!text) return undefined;
  if (text.length > MAX_SPOKEN) {
    const cut = text.slice(0, MAX_SPOKEN);
    const stop = Math.max(cut.lastIndexOf('. '), cut.lastIndexOf('! '), cut.lastIndexOf('? '));
    text = stop > 60 ? cut.slice(0, stop + 1) : `${cut.trimEnd()}…`;
  }
  return text;
}

function thought(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim().slice(0, 240) : undefined;
}

export interface Decision {
  intents: Intent[];
  /** the model chose to carry on */
  keep: boolean;
  thought?: string;
  /** what was wrong with calls that were dropped, for the decision log */
  problems: string[];
}

/** Turns the tool calls of a "decide" or "react" reply into intents, repairing what can be repaired. */
export function interpretDecision(state: SimState, c: Character, calls: readonly ToolCall[]): Decision {
  const decision: Decision = { intents: [], keep: false, problems: [] };
  for (const call of calls) {
    const args = call.args;
    const meta = { source: 'llm' as const, say: spoken(args.say), thought: thought(args.thought) };
    decision.thought ??= meta.thought;
    switch (call.name) {
      case 'continue':
        decision.keep = true;
        break;
      case 'go_to': {
        const place = resolvePlace(c, args.place);
        if (place) decision.intents.push({ kind: 'go_to', placeId: place.id, ...meta });
        else decision.problems.push(`unknown place "${String(args.place)}"`);
        break;
      }
      case 'do_activity': {
        const activity = resolveActivity(args.activity);
        if (activity) decision.intents.push({ kind: 'activity', activity, ...meta });
        else decision.problems.push(`unknown activity "${String(args.activity)}"`);
        break;
      }
      case 'talk_to': {
        const person = resolvePerson(state, args.person, c.id);
        const opener = spoken(args.message);
        const tone = TONES.includes(args.tone as TalkTone) ? (args.tone as TalkTone) : 'friendly';
        if (person && opener) decision.intents.push({ kind: 'talk', targetId: person.id, opener, tone, source: 'llm', thought: meta.thought });
        else decision.problems.push(person ? 'talk_to without a message' : `unknown person "${String(args.person)}"`);
        break;
      }
      case 'give': {
        const person = resolvePerson(state, args.person, c.id);
        const item = ITEMS.includes(args.item as ItemId) ? (args.item as ItemId) : null;
        if (!person) decision.problems.push(`unknown person "${String(args.person)}"`);
        else if (!item || c.inventory[item] <= 0) decision.problems.push(`nothing to give (${String(args.item)})`);
        else decision.intents.push({ kind: 'give', targetId: person.id, item, ...meta });
        break;
      }
      case 'brawl':
      case 'attack': {
        const person = resolvePerson(state, args.person, c.id);
        if (!person) {
          decision.problems.push(`unknown person "${String(args.person)}"`);
          break;
        }
        const lethal = call.name === 'attack';
        if (!lethal && rel(c, person.id).grievance < RUNG_FEUD) {
          decision.problems.push(`not enough of a feud with ${firstName(person)} to brawl`);
          break;
        }
        decision.intents.push({ kind: 'fight', targetId: person.id, mode: lethal ? 'lethal' : 'brawl', ...meta });
        break;
      }
      case 'fight': {
        // The nearest danger: a monster if there is one, otherwise whoever is attacking.
        let targetId: Id | null = null;
        let best = Infinity;
        for (const monster of monsterList(state)) {
          if (monster.phase === 'dying') continue;
          const d = Math.hypot(monster.pos.x - c.pos.x, monster.pos.z - c.pos.z);
          if (d < best) {
            best = d;
            targetId = monster.id;
          }
        }
        if (targetId && best < 160) {
          decision.intents.push({ kind: 'fight', targetId, mode: 'monster', ...meta });
        } else {
          const attacker = living(state).find((o) => o.intent?.kind === 'fight' && o.intent.targetId === c.id);
          if (attacker && attacker.intent?.kind === 'fight') decision.intents.push({ kind: 'fight', targetId: attacker.id, mode: attacker.intent.mode, ...meta });
          else decision.problems.push('nothing to fight');
        }
        break;
      }
      case 'flee':
        decision.intents.push({ kind: 'flee', ...meta });
        break;
      default:
        decision.problems.push(`unexpected tool ${call.name}`);
    }
  }
  return decision;
}

export function interpretPlan(calls: readonly ToolCall[]): { blocks: PlanBlock[]; thought?: string } | null {
  const call = calls.find((entry) => entry.name === 'set_plan');
  const raw = call?.args.blocks;
  if (!Array.isArray(raw)) return null;
  const blocks: PlanBlock[] = [];
  for (const entry of raw) {
    if (!entry || typeof entry !== 'object') continue;
    const item = entry as Record<string, unknown>;
    const activity = resolveActivity(item.activity);
    let hour = typeof item.hour === 'string' ? Number.parseFloat(item.hour) : (item.hour as number);
    if (typeof item.hour === 'string' && item.hour.includes(':')) {
      const [h, m] = item.hour.split(':').map(Number);
      hour = (h ?? 0) + (m ?? 0) / 60;
    }
    if (!activity || !Number.isFinite(hour) || hour < 0 || hour >= 24) continue;
    blocks.push({ hour, activity, note: typeof item.note === 'string' ? item.note.trim().slice(0, 100) || undefined : undefined });
  }
  return blocks.length >= 2 ? { blocks, thought: thought(call?.args.thought) } : null;
}

export function feeling(value: unknown): Feeling | undefined {
  const text = norm(value).replace(/ /g, '_');
  return FEELINGS.find((f) => f === text);
}

/** Lines of a whole scripted conversation; speakers are matched by name and must be the two participants. */
export function interpretConversation(calls: readonly ToolCall[], a: Character, b: Character, startsWithInitiator = false): { lines: ConversationLine[]; topic: string; feelings: Record<Id, Feeling> } | null {
  const call = calls.find((entry) => entry.name === 'conversation');
  const raw = call?.args.lines;
  if (!call || !Array.isArray(raw)) return null;
  const lines: ConversationLine[] = [];
  const nameA = firstName(a).toLowerCase();
  const nameB = firstName(b).toLowerCase();
  // Normally the opening line is already spoken, so the other person answers first.
  let expected = startsWithInitiator ? a.id : b.id;
  for (const entry of raw) {
    if (!entry || typeof entry !== 'object') continue;
    const item = entry as Record<string, unknown>;
    const text = spoken(item.text);
    if (!text) continue;
    const speaker = norm(item.speaker).split(' ')[0] ?? '';
    const speakerId = speaker === nameA ? a.id : speaker === nameB ? b.id : expected;
    lines.push({ speakerId, text });
    expected = speakerId === a.id ? b.id : a.id;
  }
  if (lines.length === 0) return null;
  const feelings: Record<Id, Feeling> = {};
  const fa = feeling(call.args.initiator_feels);
  const fb = feeling(call.args.other_feels);
  if (fa) feelings[a.id] = fa;
  if (fb) feelings[b.id] = fb;
  const topic = typeof call.args.topic === 'string' ? call.args.topic.trim().slice(0, 80) : '';
  return { lines: lines.slice(0, 6), topic, feelings };
}
