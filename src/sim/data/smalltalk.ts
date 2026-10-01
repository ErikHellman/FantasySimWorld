import { partOfDay, TICKS_PER_DAY } from '../../core/time';
import { chance, nextFloat, pick } from '../../core/rng';
import type { Character, ConversationLine, Feeling, SimState, TalkTone } from '../state';
import { firstName, living, rel } from '../util';

/**
 * Locally written conversations: used when no LLM is configured, when it is
 * too slow, or when the budget says a chat is not worth a request. Lines use
 * {a} for the initiator's first name and {b} for the other's.
 */
interface Exchange {
  topic: string;
  opener: string;
  /** remaining lines, alternating starting with the other speaker */
  lines: string[];
  when?: (state: SimState, a: Character, b: Character) => boolean;
}

const recentDeath = (state: SimState): string | null => {
  let latest: string | null = null;
  let latestDay = -1;
  const today = Math.floor(state.tick / TICKS_PER_DAY) + 1;
  for (const id in state.dead) {
    const record = state.dead[id]!;
    if (today - record.day <= 2 && record.day > latestDay) {
      latest = record.name.split(' ')[0] as string;
      latestDay = record.day;
    }
  }
  return latest;
};

const nightWasLoud = (state: SimState): boolean => state.tick - state.director.alarmUntilTick < TICKS_PER_DAY * 0.6 && state.director.alarmUntilTick > 0;

const FRIENDLY: Exchange[] = [
  { topic: 'the day', opener: 'Fine {part} for it, {b}.', lines: ['It is. Let us hope the night is as quiet.', 'We can hope.'] },
  { topic: 'work', opener: 'How goes the work, {b}?', lines: ['Slow, but honest. And yours?', 'Same as ever. I should not complain.'] },
  { topic: 'the tavern', opener: 'Will you be at the Tankard tonight, {b}?', lines: ['I would not miss it. First round is yours.', 'It always is, somehow.'] },
  { topic: 'the harvest', opener: 'The fields look good this year.', lines: ['They do. A full store makes for an easier winter.', 'If the dead leave us in peace to bring it in.'] },
  { topic: 'the lake', opener: 'They say the pike in the lake are biting.', lines: ['Kael says that every week.', 'One of these weeks he will be right.'] },
  { topic: 'an aching back', opener: '{b}, you look worn out.', lines: ['A long day. Nothing a seat and a bowl of stew will not mend.', 'Then go and sit. The work will keep.'] },
  { topic: 'the road', opener: 'No traders on the west road in days.', lines: ['Word has got round about our nights, I expect.', 'Then we shall have to make do with our own company.', 'There are worse things.'] },
  { topic: 'the market', opener: 'Anything good at the market today?', lines: ['Fresh bread, and cheese that has seen better weeks.', 'I will take the bread.'] },
  { topic: 'old stories', opener: 'My grandmother swore the old ruins were a lord\'s hall once.', lines: ['Mine said worse things were buried there than lords.', 'Cheerful folk, our grandmothers.'] },
  {
    topic: 'last night\'s attack', opener: 'Did you hear them last night, out by the graveyard?',
    lines: ['I did. I barely slept after.', 'The watch held. That is what matters.', 'This time.'],
    when: (state) => nightWasLoud(state),
  },
  {
    topic: 'a loss', opener: 'I still cannot believe {dead} is gone.',
    lines: ['Nor I. The village feels smaller.', 'We should visit the grave. Together, perhaps.', 'I would like that.'],
    when: (state) => recentDeath(state) !== null,
  },
  {
    topic: 'a newcomer', opener: 'Have you spoken to {new} yet?',
    lines: ['Only a word. Seems decent enough.', 'We could use decent. And another pair of hands.'],
    when: (state) => living(state).some((c) => state.tick - c.arrivedTick < TICKS_PER_DAY * 2 && c.arrivedTick > 0),
  },
];

const WARM: Exchange[] = [
  { topic: 'friendship', opener: '{b}! I was hoping to run into you.', lines: ['And here I am. What is on your mind?', 'Nothing at all. It is just good to see a friendly face.', 'Then sit a while.'] },
  { topic: 'a favour', opener: 'I owe you for the other day, {b}.', lines: ['You owe me nothing. That is what friends are for.', 'All the same. Thank you.'] },
  { topic: 'plans', opener: 'When all this is quieter, what will you do?', lines: ['Sleep for a week. Then, who knows? You?', 'The same, I think. But it is good to wonder.'] },
];

const COOL: Exchange[] = [
  { topic: 'nothing much', opener: '{b}.', lines: ['{a}.', 'Busy day.', 'Aren\'t they all.'] },
  { topic: 'an awkward silence', opener: 'I did not expect to see you here.', lines: ['It is a small village.', 'So it is.'] },
];

const HOSTILE: Exchange[] = [
  { topic: 'an old grudge', opener: 'We need to have this out, {b}.', lines: ['Do we? I have nothing to say to you.', 'You never do. That is the trouble.', 'Then stop asking.'] },
  { topic: 'blame', opener: 'You know what you did, {b}. Do not pretend otherwise.', lines: ['I know what you think I did.', 'Everyone knows it.', 'Then everyone is as wrong as you are.'] },
  { topic: 'a warning', opener: 'Stay out of my way, {b}. I will not say it twice.', lines: ['Or what? You will glare at me some more?', 'Keep pushing and you will find out.'] },
  { topic: 'an insult', opener: 'I have had enough of your sneering, {b}.', lines: ['Then stop giving me cause.', 'One day that mouth will cost you.'] },
];

const HOSTILE_RESOLVED: Exchange[] = [
  { topic: 'clearing the air', opener: 'We need to have this out, {b}.', lines: ['...Aye. Perhaps we do.', 'I am tired of carrying it.', 'So am I. Say your piece, and I will say mine.'] },
];

const ROMANTIC_YES: Exchange[] = [
  { topic: 'feelings', opener: '{b}, there is something I have wanted to tell you for a long time.', lines: ['I think I know. I have been hoping you would.', 'Then you do not mind?', 'Mind? I have been waiting.'] },
  { topic: 'a walk together', opener: 'Walk with me a while, {b}? Just the two of us.', lines: ['I would like nothing better.', 'I hoped you would say that.'] },
];

const ROMANTIC_NO: Exchange[] = [
  { topic: 'feelings unreturned', opener: '{b}, there is something I have wanted to tell you for a long time.', lines: ['Oh. {a}, I... I am fond of you. But not like that.', 'I see. Forget I spoke.', 'I am sorry.'] },
];

const APOLOGY_YES: Exchange[] = [
  { topic: 'making amends', opener: '{b}, I was wrong, and I am sorry for it.', lines: ['That cannot have been easy to say.', 'It was not.', 'Then let us put it behind us.'] },
];

const APOLOGY_NO: Exchange[] = [
  { topic: 'an apology refused', opener: '{b}, I was wrong, and I am sorry for it.', lines: ['Sorry does not mend it.', 'I know. I wanted you to hear it anyway.', 'I have heard it.'] },
];

function fill(text: string, state: SimState, a: Character, b: Character): string {
  const hour = ((6 * 60 + state.tick / 20) % 1440) / 60;
  const newcomer = living(state).find((c) => state.tick - c.arrivedTick < TICKS_PER_DAY * 2 && c.arrivedTick > 0 && c.id !== a.id && c.id !== b.id);
  return text
    .replaceAll('{a}', firstName(a))
    .replaceAll('{b}', firstName(b))
    .replaceAll('{part}', partOfDay(hour))
    .replaceAll('{dead}', recentDeath(state) ?? 'the old days')
    .replaceAll('{new}', newcomer ? firstName(newcomer) : 'the newcomer');
}

export interface LocalScript {
  topic: string;
  opener: string;
  lines: ConversationLine[];
  feelings: [Feeling, Feeling];
}

function rollFeeling(state: SimState, warmth: number): Feeling {
  const roll = nextFloat(state) + warmth;
  if (roll > 1.2) return 'much_better';
  if (roll > 0.6) return 'better';
  if (roll > 0.22) return 'same';
  if (roll > 0.04) return 'worse';
  return 'much_worse';
}

/**
 * Picks an exchange that fits how the two feel about each other, and decides
 * how each comes away feeling.
 */
export function localScript(state: SimState, a: Character, b: Character, tone: TalkTone): LocalScript {
  const ab = rel(a, b.id);
  const ba = rel(b, a.id);
  let pool: Exchange[];
  let feelings: [Feeling, Feeling];

  if (tone === 'hostile') {
    const cools = chance(state, 0.22 + (1 - b.profile.temper) * 0.15);
    pool = cools ? HOSTILE_RESOLVED : HOSTILE;
    feelings = cools ? ['better', 'better'] : [chance(state, 0.5) ? 'worse' : 'much_worse', chance(state, 0.4) ? 'much_worse' : 'worse'];
  } else if (tone === 'romantic') {
    const returned = ba.attraction >= 42 && ba.affinity >= 25 && !b.partnerId && !ba.kin;
    pool = returned ? ROMANTIC_YES : ROMANTIC_NO;
    feelings = returned ? ['much_better', 'much_better'] : ['worse', 'same'];
  } else if (tone === 'apology') {
    const forgiven = chance(state, 0.7 - b.profile.temper * 0.35);
    pool = forgiven ? APOLOGY_YES : APOLOGY_NO;
    feelings = forgiven ? ['better', 'much_better'] : ['same', 'same'];
  } else {
    const mutual = (ab.affinity + ba.affinity) / 2;
    pool = mutual >= 35 && chance(state, 0.5) ? WARM : mutual <= -15 ? COOL : FRIENDLY;
    // Hot tempers and old grudges make an ordinary chat more likely to go wrong.
    const warmth = (c: Character, r: typeof ab): number => r.affinity / 170 + 0.1 - c.profile.temper * 0.12 - r.grievance / 220;
    feelings = [rollFeeling(state, warmth(a, ab)), rollFeeling(state, warmth(b, ba))];
  }

  const eligible = pool.filter((exchange) => !exchange.when || exchange.when(state, a, b));
  const exchange = pick(state, eligible.length > 0 ? eligible : pool);
  const lines = exchange.lines.map((text, index) => ({
    speakerId: index % 2 === 0 ? b.id : a.id,
    text: fill(text, state, a, b),
  }));
  return { topic: exchange.topic, opener: fill(exchange.opener, state, a, b), lines, feelings };
}

const GENERIC: Record<TalkTone, string[][]> = {
  friendly: [
    ['Hm. I had not thought of it like that.', 'Well, it is something to chew on.'],
    ['You may be right.', 'I usually am. Now and then.'],
    ['Ha. Fair enough.', 'We should talk again soon.'],
  ],
  romantic: [
    ['You have a way of catching me off guard.', 'I hope that is no bad thing.'],
    ['I... need a moment to think about that.', 'Take all the time you need.'],
  ],
  hostile: [
    ['I have heard enough.', 'You have not heard the half of it.'],
    ['Say what you like. It changes nothing.', 'We shall see.'],
  ],
  apology: [
    ['I hear you. Give me time.', 'That is all I ask.'],
    ['It will take more than words.', 'Then I will find more than words.'],
  ],
};

/** A neutral way to wind up a conversation whose opening line was written elsewhere. */
export function genericContinuation(state: SimState, a: Character, b: Character, tone: TalkTone): LocalScript {
  const script = localScript(state, a, b, tone);
  const replies = pick(state, GENERIC[tone]);
  return {
    topic: tone === 'friendly' ? 'this and that' : script.topic,
    opener: '',
    lines: replies.map((text, index) => ({ speakerId: index % 2 === 0 ? b.id : a.id, text })),
    feelings: script.feelings,
  };
}
