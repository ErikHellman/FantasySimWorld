//! The billboards: what each one shows today, who goes to read one, how they take it, and who goes and does
//! what it urges.

use fsw_core::jsmath::utf16_truncate;
use fsw_core::math2d::dist;
use fsw_core::rng::hash_string;
use fsw_core::time::{game_time, minutes_to_ticks};
use fsw_world::places::{ActivityId, Billboard, billboards, get_place};

use crate::data::activities::{ActivityKind, activity};
use crate::data::billboards::{Advice, DECK, WATCHER_DECK, advice_in, lines};
use crate::events::{Emote, SimEvent};
use crate::state::{
    ActPhase, BoardReaction, BoardTone, Character, ChronicleKind, Intent, IntentAction, IntentSource, MemoryKind,
    Profile, SimState, TalkTone,
};
use crate::util::{Ctx, first_name, is_approachable, living};

/// The most a billboard's text may hold, in UTF-16 code units.
pub const TEXT_CAP: usize = 90;

/// How far someone looks for a board to read: further when they have time on their hands.
const LEISURE_REACH: f64 = 60.0;
const PASSING_REACH: f64 = 20.0;
/// How far someone urged to be friendly looks for somebody to be friendly to.
const MINGLE_REACH: f64 = 20.0;
/// One board-day in this many, on average, hints that someone beyond the world is watching the village: with
/// seven boards, about one hint every other day.
const WATCHER_ODDS: u32 = 14;

/// What a board shows right now.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub text: String,
    pub tone: BoardTone,
    /// put up from outside (an admin) rather than the day's slogan
    pub custom: bool,
    /// what it urges its readers to do, if anything
    pub advice: Option<Advice>,
    /// it hints that someone beyond the world is watching the village (never an admin's notice)
    pub watchers: bool,
}

/// Whether the board at `index` hints at the watchers beyond the world on `day`. Like the slogans it is worked
/// out from the seed, the day and the board, so it draws nothing from the RNG.
pub fn hints_at_watchers(seed: u32, day: i64, index: usize) -> bool {
    hash_string(&format!("watchers:{seed}:{day}:{index}")).is_multiple_of(WATCHER_ODDS)
}

/// The stock slogan for a board on a day: from the deck, or on a day it hints at the watchers, from theirs.
fn stock(seed: u32, day: i64, index: usize) -> (&'static str, BoardTone, Option<Advice>) {
    let turn = i64::from(seed) + day * 13 + index as i64 * 7;
    if hints_at_watchers(seed, day, index) {
        return WATCHER_DECK[turn.rem_euclid(WATCHER_DECK.len() as i64) as usize];
    }
    // 13 and 7 share no factor with the deck's 40, so every board changes daily and no two match on a day.
    DECK[turn.rem_euclid(DECK.len() as i64) as usize]
}

/// The tone the board at `index` takes today, which the model writes in when it writes the day's words.
pub fn day_tone(state: &SimState, index: usize) -> BoardTone {
    stock(state.seed, game_time(state.tick).day, index).1
}

/// The day the board at `index` is waiting for the model's words for: today, unless an admin's notice is up or
/// the model has already written today's.
pub fn wants_words(state: &SimState, index: usize) -> Option<i64> {
    let day = game_time(state.tick).day;
    let board = billboards().get(index)?;
    match state.billboards.get(&board.id) {
        Some(notice) if notice.day.is_none_or(|written| written == day) => None,
        _ => Some(day),
    }
}

/// What the board at `index` in `billboards()` shows: an admin's notice; else the words the model wrote for
/// today; else the day's stock slogan, which depends only on the seed, the day and the board, so it takes nothing
/// from the RNG and nothing needs saving.
pub fn shown(state: &SimState, index: usize) -> Shown {
    let day = game_time(state.tick).day;
    if let Some(notice) = billboards().get(index).and_then(|board| state.billboards.get(&board.id)) {
        let advice = advice_in(&notice.text);
        match notice.day {
            None => {
                return Shown { text: notice.text.clone(), tone: notice.tone, custom: true, advice, watchers: false };
            }
            Some(written) if written == day => {
                let watchers = hints_at_watchers(state.seed, day, index);
                return Shown { text: notice.text.clone(), tone: notice.tone, custom: false, advice, watchers };
            }
            // Yesterday's words have come down.
            Some(_) => {}
        }
    }
    let (text, tone, advice) = stock(state.seed, day, index);
    Shown { text: text.to_owned(), tone, custom: false, advice, watchers: hints_at_watchers(state.seed, day, index) }
}

pub fn board_index(board_id: &str) -> Option<usize> {
    billboards().iter().position(|board| board.id == board_id)
}

/// Text fit to go up on a board: whitespace (newlines included) collapsed to single spaces, nothing else that
/// does not print. `None` when nothing is left or something unprintable was in it. Callers cap the length.
pub fn clean_text(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let clean = words.join(" ");
    if clean.is_empty() || clean.chars().any(char::is_control) {
        return None;
    }
    Some(clean)
}

/// The board someone is standing at and reading, as an index into `billboards()`.
pub fn reading(c: &Character) -> Option<usize> {
    let Some(Intent { action: IntentAction::Activity { activity: ActivityId::ReadBoard, .. }, .. }) = &c.intent else {
        return None;
    };
    if c.act.phase != ActPhase::Perform {
        return None;
    }
    let slot = c.slot_id.as_deref()?;
    billboards().iter().position(|board| board.slot_ids.iter().any(|id| id == slot))
}

/// The mark a reader keeps of a board's text. Someone pulled away from a board (a child to fetch, a fire,
/// a wedding) keeps the mark too, so they do not read the same thing twice that day.
pub fn read_mark(state: &SimState, index: usize) -> (String, u32) {
    let board: &Billboard = billboards()[index];
    (board.id.clone(), hash_string(&shown(state, index).text))
}

/// Whether someone has already read (and reacted to) what the board shows now.
pub fn has_read(state: &SimState, c: &Character, index: usize) -> bool {
    let board = billboards()[index];
    c.boards_read.get(&board.id) == Some(&hash_string(&shown(state, index).text))
}

/// How someone takes a slogan when no model says otherwise: from its tone and their temperament.
pub fn template_reaction(profile: &Profile, tone: BoardTone) -> BoardReaction {
    use BoardReaction::*;
    match tone {
        BoardTone::Absurd if profile.temper > 0.7 => Scoff,
        BoardTone::Absurd => Laugh,
        BoardTone::Uplifting if profile.temper > 0.6 || profile.sociability < 0.3 => Scoff,
        BoardTone::Uplifting => Love,
        BoardTone::Pushy if profile.temper > 0.5 => Annoyed,
        BoardTone::Pushy => Scoff,
        BoardTone::Ominous if profile.bravery < 0.4 => Uneasy,
        BoardTone::Ominous if profile.bravery > 0.75 => Laugh,
        BoardTone::Ominous => Scoff,
        BoardTone::Wholesome if profile.temper > 0.85 => Scoff,
        BoardTone::Wholesome => Love,
    }
}

/// How someone takes a board that hints at the watchers beyond the world: the hot-headed are furious at being
/// spied on, the timid frightened, everyone else curious.
pub fn watcher_reaction(profile: &Profile) -> BoardReaction {
    if profile.temper > 0.6 {
        BoardReaction::Angry
    } else if profile.bravery < 0.4 {
        BoardReaction::Shocked
    } else {
        BoardReaction::Intrigued
    }
}

/// How someone takes what a board shows when no model says otherwise.
pub fn stock_reaction(profile: &Profile, shown: &Shown) -> BoardReaction {
    if shown.watchers { watcher_reaction(profile) } else { template_reaction(profile, shown.tone) }
}

/// How likely someone is to do what a board urges, by how they took it, when no model decides for them.
pub fn follow_chance(reaction: BoardReaction) -> f64 {
    match reaction {
        BoardReaction::Love => 0.6,
        BoardReaction::Uneasy => 0.4,
        BoardReaction::Laugh => 0.25,
        BoardReaction::Scoff => 0.15,
        BoardReaction::Annoyed | BoardReaction::Angry => 0.0,
        BoardReaction::Shocked => 0.3,
        BoardReaction::Intrigued => 0.2,
    }
}

/// Whether advice is something this person could take now. After dark only going home to rest is; children
/// keep to being friendly, eating, praying and resting.
fn suits(c: &Character, advice: Advice, hour: f64) -> bool {
    match advice {
        Advice::Do(ActivityId::Rest) => true,
        _ if !(5.6..19.4).contains(&hour) => false,
        Advice::Mingle | Advice::Do(ActivityId::Eat | ActivityId::Pray) => true,
        _ => !c.is_child(),
    }
}

/// What the board at `index` urges that this person could take up now, if anything.
pub fn advice_for(state: &SimState, c: &Character, index: usize) -> Option<Advice> {
    let hour = game_time(state.tick).hour;
    shown(state, index).advice.filter(|&advice| suits(c, advice, hour))
}

/// Advice as the prompts and memories put it: "go home and rest".
pub fn advice_label(advice: Advice) -> &'static str {
    match advice {
        Advice::Do(ActivityId::Rest) => "go home and rest",
        Advice::Do(activity_id) => activity(activity_id).label,
        Advice::Work => "get on with your work",
        Advice::Mingle => "go and have a friendly word with someone",
    }
}

/// What taking the advice means for this person: their first trade for work, the nearest approachable soul for
/// a friendly word (another child, for a child), home for a rest. `None` when there is nothing to do it with.
fn advice_intent(state: &SimState, c: &Character, advice: Advice) -> Option<Intent> {
    let activity_id = match advice {
        Advice::Mingle => {
            let other = living(state)
                .filter(|o| o.id != c.id && is_approachable(o) && o.is_child() == c.is_child())
                .map(|o| (dist(o.pos, c.pos), o))
                .filter(|&(d, _)| d <= MINGLE_REACH)
                .min_by(|a, b| a.0.total_cmp(&b.0))?
                .1;
            let action =
                IntentAction::Talk { target_id: other.id.clone(), opener: String::new(), tone: TalkTone::Friendly };
            return Some(Intent::new(action, IntentSource::Routine));
        }
        Advice::Work => *c.jobs.iter().find(|&&job| activity(job).kind == ActivityKind::Work)?,
        Advice::Do(activity_id) => activity_id,
    };
    let def = activity(activity_id);
    let minutes = (def.minutes[0] + def.minutes[1]) as f64 / 2.0;
    let place_id = (activity_id == ActivityId::Rest).then(|| c.home_id.clone());
    let action = IntentAction::Activity { activity: activity_id, place_id, minutes: Some(minutes) };
    Some(Intent::new(action, IntentSource::Routine))
}

/// Reacts to the board at `index`: a line aloud (the given one, or a stock one), an emote, a small change of
/// mood, a memory of what it said, and perhaps going off to do what it urges (`follow`, as the model decided,
/// or a roll on how they took it). Returns how long the line stays up, in ticks.
pub fn react(
    ctx: &mut Ctx,
    id: &str,
    index: usize,
    reaction: BoardReaction,
    say: Option<String>,
    thought: Option<String>,
    follow: Option<bool>,
) -> i64 {
    let board: &Billboard = billboards()[index];
    let shown = shown(ctx.state, index);
    let line = match say.filter(|s| !s.is_empty()) {
        Some(line) => line,
        None => ctx.pick(lines(reaction)).to_owned(),
    };
    let ticks = ctx.say(id, line, None, false);
    let (emote, social, energy, feeling, importance) = match reaction {
        BoardReaction::Laugh => (Emote::Laugh, 4.0, 0.0, "It made me laugh.", 2.0),
        BoardReaction::Love => (Emote::Heart, 5.0, 3.0, "It cheered me up.", 2.0),
        BoardReaction::Scoff => (Emote::Thinking, 0.0, 0.0, "I didn't think much of it.", 2.0),
        BoardReaction::Annoyed => (Emote::Anger, -3.0, 0.0, "It annoyed me.", 3.0),
        BoardReaction::Uneasy => (Emote::Alert, -2.0, -2.0, "It left me uneasy.", 3.0),
        // Being watched from beyond the world is news: these memories weigh enough to come up in talk.
        BoardReaction::Shocked => {
            (Emote::Alert, -2.0, -3.0, "It hinted that someone beyond the world is watching us. It frightened me.", 5.0)
        }
        BoardReaction::Intrigued => {
            (Emote::Thinking, 3.0, 0.0, "It hinted that someone beyond the world is watching us. I wonder who.", 5.0)
        }
        BoardReaction::Angry => {
            (Emote::Anger, -3.0, 0.0, "It hinted that someone beyond the world is watching us. How dare they.", 5.0)
        }
    };
    ctx.emit(SimEvent::Emote { id: id.into(), emote });
    let c = ctx.c(id);
    c.needs.social = (c.needs.social + social).clamp(0.0, 100.0);
    c.needs.energy = (c.needs.energy + energy).clamp(0.0, 100.0);
    let stock_thought = match reaction {
        BoardReaction::Shocked => Some("Someone out there is watching us. Every moment. Every one of us."),
        BoardReaction::Intrigued => Some("Who is watching us, and what do they want to see?"),
        BoardReaction::Angry => Some("Nobody has the right to spy on this village."),
        _ => None,
    };
    if let Some(thought) = thought.filter(|t| !t.is_empty()).or_else(|| stock_thought.map(str::to_owned)) {
        c.thought = thought;
    }
    if shown.watchers && matches!(reaction, BoardReaction::Shocked | BoardReaction::Intrigued | BoardReaction::Angry) {
        let name = first_name(ctx.cr(id)).to_owned();
        let place = get_place(&board.place_id).name;
        let took = match reaction {
            BoardReaction::Shocked => "is shaken",
            BoardReaction::Intrigued => "is intrigued",
            _ => "is furious",
        };
        let text =
            format!("{name} read the billboard at {place} and {took}: it says someone beyond the world is watching.");
        ctx.chronicle(ChronicleKind::World, text, vec![id.to_owned()]);
    }
    if shown.watchers {
        crate::systems::sky::took_hint(ctx, id, reaction);
    }
    ctx.c(id).boards_read.insert(board.id.clone(), hash_string(&shown.text));
    // The RNG is only drawn from when there is advice this person could take and a chance they would.
    let heeded = advice_for(ctx.state, ctx.cr(id), index).filter(|_| match follow {
        Some(follow) => follow,
        None => {
            let chance = follow_chance(reaction);
            chance > 0.0 && ctx.chance(chance)
        }
    });
    let plan = heeded.and_then(|advice| Some((advice, advice_intent(ctx.state, ctx.cr(id), advice)?)));
    let resolve = match plan {
        Some((advice, intent)) => {
            let tick = ctx.tick();
            let c = ctx.c(id);
            if matches!(intent.action, IntentAction::Talk { .. }) {
                c.next_social_tick = tick + minutes_to_ticks(40.0);
            }
            c.queue.push(intent);
            format!(" I decided to {}.", advice_label(advice).replacen("your", "my", 1))
        }
        None => String::new(),
    };
    let place = get_place(&board.place_id).name;
    let text = utf16_truncate(&shown.text, TEXT_CAP);
    ctx.remember(
        id,
        format!("Read the billboard at {place}: \u{201c}{text}\u{201d} {feeling}{resolve}"),
        importance,
        MemoryKind::Event,
        Vec::new(),
    );
    ticks
}

/// Whether someone with nothing else pressing wanders over to a board they have not read: sometimes, more often
/// with time on their hands. Returns the board's place. The RNG is only drawn from when there is a board in reach.
pub fn wants_to_read(ctx: &mut Ctx, id: &str, leisure: bool) -> Option<String> {
    let c = ctx.cr(id);
    let reach = if leisure { LEISURE_REACH } else { PASSING_REACH };
    let mut best: Option<(f64, &Billboard)> = None;
    for (index, board) in billboards().iter().enumerate() {
        let d = dist(c.pos, board.pos);
        if d > reach || has_read(ctx.state, c, index) {
            continue;
        }
        if board.slot_ids.iter().all(|slot| ctx.state.slots.contains_key(slot)) {
            continue;
        }
        if best.is_none_or(|(best_d, _)| d < best_d) {
            best = Some((d, board));
        }
    }
    let (_, board) = best?;
    let place = board.place_id.clone();
    ctx.chance(if leisure { 0.1 } else { 0.03 }).then_some(place)
}
