//! Ambitions: every grown villager sets their heart on one long goal (a fortune, a marriage, a child, glory
//! against the village's enemies, or close friends), in their own words. The casting model writes it with the
//! rest of a newcomer; anyone without one is given one from their temperament. The sim watches for it to come
//! true and the chronicle says so; after a month without it, they give it up.
//!
//! Nothing here draws from the RNG: a stock ambition is picked by a hash of the seed and the name.

use fsw_core::rng::hash_string;
use fsw_core::time::{TICKS_PER_GAME_MINUTE, game_time};

use crate::events::{Emote, SimEvent};
use crate::state::{
    Ambition, AmbitionDraft, AmbitionKind, AmbitionOutcome, Character, ChronicleKind, MemoryKind, Profile,
    RelationLabel, SimState,
};
use crate::util::{Ctx, first_name, name_of};

/// Gold to put by on top of what they had when they set out.
pub const WEALTH_MORE: i64 = 60;
/// The dead or raiders to fell.
pub const GLORY_KILLS: i64 = 3;
/// Close friends to make.
pub const FRIENDS_WANTED: i64 = 2;
/// Days before an ambition not yet won is given up.
pub const GIVE_UP_DAYS: i64 = 30;

/// How someone with no words from the model puts each kind.
fn stock_words(kind: AmbitionKind) -> &'static str {
    match kind {
        AmbitionKind::Wealth => "I mean to put by a proper fortune before I'm old.",
        AmbitionKind::Love => "I want to marry someone who'll stand by me.",
        AmbitionKind::Family => "I want a child of my own to raise.",
        AmbitionKind::Glory => "I want to be the one the dead learn to fear.",
        AmbitionKind::Friends => "I want true friends in this valley, the kind you'd die for.",
    }
}

/// The ambition the sim gives someone the model gave none: from their temperament where it is plain (the bold
/// want glory, the gregarious friends), otherwise by a hash of the seed and their name.
pub fn stock(seed: u32, profile: &Profile) -> AmbitionDraft {
    let kind = if profile.bravery > 0.75 {
        AmbitionKind::Glory
    } else if profile.sociability > 0.75 {
        AmbitionKind::Friends
    } else {
        let quieter = [AmbitionKind::Wealth, AmbitionKind::Love, AmbitionKind::Family];
        quieter[hash_string(&format!("ambition:{seed}:{}", profile.name)) as usize % quieter.len()]
    };
    AmbitionDraft { kind, words: stock_words(kind).to_owned() }
}

/// How many children someone is raising.
fn children_of(state: &SimState, id: &str) -> i64 {
    state
        .characters
        .values()
        .filter(|o| o.child.as_ref().is_some_and(|child| child.parents.iter().any(|p| p == id)))
        .count() as i64
}

fn close_friends(c: &Character) -> i64 {
    c.relationships.values().filter(|r| r.label == RelationLabel::CloseFriend).count() as i64
}

/// Where someone stands on a kind of ambition right now: their gold, kills, children or close friends, or
/// whether they are married.
fn standing(state: &SimState, c: &Character, kind: AmbitionKind) -> i64 {
    match kind {
        AmbitionKind::Wealth => c.gold,
        AmbitionKind::Love => i64::from(c.spouse_id.is_some()),
        AmbitionKind::Family => children_of(state, &c.id),
        AmbitionKind::Glory => c.kills,
        AmbitionKind::Friends => close_friends(c),
    }
}

/// Sets out on an ambition today, from where they stand.
pub fn begin(state: &SimState, c: &Character, draft: AmbitionDraft) -> Ambition {
    let baseline = match draft.kind {
        // Friends and a marriage count from nothing: one made before counts too.
        AmbitionKind::Friends | AmbitionKind::Love => 0,
        kind => standing(state, c, kind),
    };
    Ambition {
        kind: draft.kind,
        words: draft.words,
        since_day: game_time(state.tick).day,
        baseline,
        outcome: None,
        outcome_day: 0,
    }
}

/// How far along someone is, as (done, wanted).
pub fn progress(state: &SimState, c: &Character, ambition: &Ambition) -> (i64, i64) {
    let now = standing(state, c, ambition.kind);
    match ambition.kind {
        AmbitionKind::Wealth => ((now - ambition.baseline).clamp(0, WEALTH_MORE), WEALTH_MORE),
        AmbitionKind::Love => (now.min(1), 1),
        AmbitionKind::Family => ((now - ambition.baseline).clamp(0, 1), 1),
        AmbitionKind::Glory => ((now - ambition.baseline).clamp(0, GLORY_KILLS), GLORY_KILLS),
        AmbitionKind::Friends => (now.min(FRIENDS_WANTED), FRIENDS_WANTED),
    }
}

/// Where they stand on it, in a sentence for their page and their prompts.
pub fn progress_words(state: &SimState, c: &Character, ambition: &Ambition) -> String {
    let (done, wanted) = progress(state, c, ambition);
    match ambition.outcome {
        Some(AmbitionOutcome::Won) => return format!("Achieved on day {}.", ambition.outcome_day),
        Some(AmbitionOutcome::Abandoned) => return format!("Given up on day {}.", ambition.outcome_day),
        None => {}
    }
    match ambition.kind {
        AmbitionKind::Wealth => format!("{done} of {wanted} gold put by."),
        AmbitionKind::Love => match c.partner_id.as_deref().filter(|p| state.characters.contains_key(*p)) {
            Some(partner) => format!("Courting {}; not married yet.", name_of(state, partner)),
            None => "Not married yet.".to_owned(),
        },
        AmbitionKind::Family => "No child yet.".to_owned(),
        AmbitionKind::Glory => format!("{done} of {wanted} of the village's enemies felled."),
        AmbitionKind::Friends => format!("{done} of {wanted} close friends."),
    }
}

/// How the prompts put someone's ambition to the model: a line for their header, or nothing for a child.
pub fn ambition_line(state: &SimState, c: &Character) -> Option<String> {
    let ambition = c.ambition.as_ref()?;
    let quoted = format!("\u{201c}{}\u{201d}", ambition.words);
    Some(match ambition.outcome {
        None => format!("What you want most from life here: {quoted} So far: {}", progress_words(state, c, ambition)),
        Some(AmbitionOutcome::Won) => format!("You have achieved what you wanted most: {quoted}"),
        Some(AmbitionOutcome::Abandoned) => format!("You have given up on what you wanted most: {quoted}"),
    })
}

/// What the chronicle files a won ambition under.
fn chronicle_kind(kind: AmbitionKind) -> ChronicleKind {
    match kind {
        AmbitionKind::Wealth => ChronicleKind::Trade,
        AmbitionKind::Love | AmbitionKind::Family => ChronicleKind::Romance,
        AmbitionKind::Glory => ChronicleKind::Combat,
        AmbitionKind::Friends => ChronicleKind::Social,
    }
}

/// Once a game hour: grown villagers without an ambition get one; one that has come true is won, and one a month
/// old is given up.
pub fn update_ambitions(ctx: &mut Ctx) {
    if ctx.tick() % (60 * TICKS_PER_GAME_MINUTE) != 0 {
        return;
    }
    let day = ctx.time.day;
    let ids: Vec<String> = ctx.state.order.clone();
    for id in ids {
        let Some(c) = ctx.state.characters.get(&id) else { continue };
        if c.is_child() || c.exiled_since.is_some() {
            continue;
        }
        let Some(ambition) = c.ambition.clone() else {
            // Someone come of age, or from a world saved before ambitions.
            let ambition = begin(ctx.state, c, stock(ctx.state.seed, &c.profile));
            ctx.c(&id).ambition = Some(ambition);
            continue;
        };
        if ambition.outcome.is_some() {
            continue;
        }
        let (done, wanted) = progress(ctx.state, c, &ambition);
        if done >= wanted && ambition.since_day == day {
            // Theirs already the day they set out (a founder married by a bond): they want a fortune instead.
            let draft = AmbitionDraft { kind: AmbitionKind::Wealth, words: stock_words(AmbitionKind::Wealth).into() };
            let ambition = begin(ctx.state, c, draft);
            ctx.c(&id).ambition = Some(ambition);
        } else if done >= wanted {
            settle(ctx, &id, AmbitionOutcome::Won);
        } else if day - ambition.since_day >= GIVE_UP_DAYS {
            settle(ctx, &id, AmbitionOutcome::Abandoned);
        }
    }
}

/// An ambition won or given up: marked so, told in the chronicle, and remembered.
fn settle(ctx: &mut Ctx, id: &str, outcome: AmbitionOutcome) {
    let day = ctx.time.day;
    let Some(ambition) = ctx.c(id).ambition.as_mut() else { return };
    ambition.outcome = Some(outcome);
    ambition.outcome_day = day;
    let (kind, words) = (ambition.kind, ambition.words.clone());
    let name = first_name(ctx.cr(id)).to_owned();
    let quoted = format!("\u{201c}{words}\u{201d}");
    match outcome {
        AmbitionOutcome::Won => {
            ctx.chronicle(
                chronicle_kind(kind),
                format!("{name} has done what they set out to do: {quoted}"),
                vec![id.to_owned()],
            );
            ctx.remember(
                id,
                format!("I did it. I set out to do it, and I did: {quoted}"),
                7.0,
                MemoryKind::Event,
                Vec::new(),
            );
            ctx.emit(SimEvent::Emote { id: id.to_owned(), emote: Emote::Heart });
        }
        AmbitionOutcome::Abandoned => {
            ctx.chronicle(ChronicleKind::Social, format!("{name} has given up on it: {quoted}"), vec![id.to_owned()]);
            ctx.remember(
                id,
                format!("I have given up on what I wanted most: {quoted}"),
                6.0,
                MemoryKind::Event,
                Vec::new(),
            );
        }
    }
}
