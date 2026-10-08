//! The billboards: what each one shows today, who goes to read one, and how they take it.

use fsw_core::jsmath::utf16_truncate;
use fsw_core::math2d::dist;
use fsw_core::rng::hash_string;
use fsw_core::time::game_time;
use fsw_world::places::{ActivityId, Billboard, billboards, get_place};

use crate::data::billboards::{DECK, lines};
use crate::events::{Emote, SimEvent};
use crate::state::{
    ActPhase, BoardReaction, BoardTone, Character, Intent, IntentAction, MemoryKind, Profile, SimState,
};
use crate::util::Ctx;

/// The most a billboard's text may hold, in UTF-16 code units.
pub const TEXT_CAP: usize = 90;

/// How far someone looks for a board to read: further when they have time on their hands.
const LEISURE_REACH: f64 = 60.0;
const PASSING_REACH: f64 = 20.0;

/// What a board shows right now.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub text: String,
    pub tone: BoardTone,
    /// put up from outside (an admin) rather than the day's slogan
    pub custom: bool,
}

/// What the board at `index` in `billboards()` shows. Without a notice it is the day's slogan, which depends
/// only on the seed, the day and the board, so it takes nothing from the RNG and nothing needs saving.
pub fn shown(state: &SimState, index: usize) -> Shown {
    if let Some(notice) = billboards().get(index).and_then(|board| state.billboards.get(&board.id)) {
        return Shown { text: notice.text.clone(), tone: notice.tone, custom: true };
    }
    let day = game_time(state.tick).day;
    // 13 and 7 share no factor with the deck's 40, so every board changes daily and no two match on a day.
    let at = (i64::from(state.seed) + day * 13 + index as i64 * 7).rem_euclid(DECK.len() as i64) as usize;
    let (text, tone) = DECK[at];
    Shown { text: text.to_owned(), tone, custom: false }
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

/// Reacts to the board at `index`: a line aloud (the given one, or a stock one), an emote, a small change of
/// mood, a memory of what it said. Returns how long the line stays up, in ticks.
pub fn react(
    ctx: &mut Ctx,
    id: &str,
    index: usize,
    reaction: BoardReaction,
    say: Option<String>,
    thought: Option<String>,
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
    };
    ctx.emit(SimEvent::Emote { id: id.into(), emote });
    let c = ctx.c(id);
    c.needs.social = (c.needs.social + social).clamp(0.0, 100.0);
    c.needs.energy = (c.needs.energy + energy).clamp(0.0, 100.0);
    if let Some(thought) = thought.filter(|t| !t.is_empty()) {
        c.thought = thought;
    }
    c.boards_read.insert(board.id.clone(), hash_string(&shown.text));
    let place = get_place(&board.place_id).name;
    let text = utf16_truncate(&shown.text, TEXT_CAP);
    ctx.remember(
        id,
        format!("Read the billboard at {place}: \u{201c}{text}\u{201d} {feeling}"),
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
