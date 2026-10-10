//! Believers and sceptics. Now and then a billboard hints that someone beyond the sky is watching the village
//! (`billboards::hints_at_watchers`). After a couple of such hints a villager settles on what they make of it, for
//! good: a believer, glad or awed to be watched; defiant, sure of it and furious; or a sceptic, sure it is nonsense.
//! Believers and the defiant now and then speak to the sky, and a sceptic within earshot answers them. The server
//! tells the sim roughly how many are watching (`Command::Audience`), and a believer can feel the crowd come and go.
//!
//! Who speaks when is worked out from hashes, not the RNG, so a village without believers runs exactly as before.

use fsw_core::math2d::dist;
use fsw_core::rng::hash_string;
use fsw_core::time::TICKS_PER_GAME_MINUTE;

use crate::events::{Emote, SimEvent};
use crate::state::{BoardReaction, Character, ChronicleKind, MemoryKind, SimState, Stance};
use crate::util::{Ctx, first_name, is_approachable};

/// Hints it takes to settle on a stance.
pub const STANCE_AFTER: u8 = 2;
/// On average, someone with a stance speaks to the sky once in this many game minutes of daytime.
const SPEAK_EVERY: i64 = 180;
/// How near a sceptic must be to answer.
const EARSHOT: f64 = 10.0;

const BELIEVER: &[&str] = &[
    "I know you're up there.",
    "Hello up there! Can you see me?",
    "Whoever you are, be kind to us.",
    "Smile, everyone. They're watching.",
];
const DEFIANT: &[&str] = &[
    "Stop staring at us!",
    "Go and watch someone else!",
    "We're not here for your amusement!",
    "I can feel your eyes. I hate it.",
];
const SCEPTIC: &[&str] = &[
    "There's nobody up there, you fool.",
    "Talking to the clouds again?",
    "It's paint on a board. Nobody's watching.",
];

/// The server's viewer count, in the few steps the village can feel: nobody, one or two, a few, a crowd.
pub fn crowd_of(viewers: u32) -> u8 {
    match viewers {
        0 => 0,
        1..=2 => 1,
        3..=5 => 2,
        _ => 3,
    }
}

/// Someone has read a board that hints at the watchers and taken it as `reaction`: it counts toward their stance,
/// and once they have read enough of them they settle on one.
pub fn took_hint(ctx: &mut Ctx, id: &str, reaction: BoardReaction) {
    let c = ctx.c(id);
    if c.sky.stance.is_some() {
        return;
    }
    let sky = &mut c.sky;
    match reaction {
        BoardReaction::Intrigued | BoardReaction::Shocked => sky.awe += 1,
        BoardReaction::Angry => sky.anger += 1,
        _ => sky.doubt += 1,
    }
    if sky.awe + sky.anger + sky.doubt < STANCE_AFTER {
        return;
    }
    let stance = settle(ctx.cr(id));
    ctx.c(id).sky.stance = Some(stance);
    let name = first_name(ctx.cr(id)).to_owned();
    let (chronicle, memory) = match stance {
        Stance::Believer => (
            format!("{name} has come to believe that someone beyond the sky is watching the village."),
            "I am sure now: someone beyond the sky is watching us. I am not afraid of them.",
        ),
        Stance::Defiant => (
            format!("{name} is sure that someone beyond the sky is spying on the village, and is furious about it."),
            "I am sure now: someone beyond the sky is spying on us. I will not stand for it.",
        ),
        Stance::Sceptic => (
            format!("{name} has decided that the talk of watchers beyond the sky is nonsense."),
            "All this talk of someone beyond the sky watching us is nonsense, and I will say so.",
        ),
    };
    ctx.chronicle(ChronicleKind::World, chronicle, vec![id.to_owned()]);
    ctx.remember(id, memory, 5.0, MemoryKind::Event, Vec::new());
}

/// The stance someone settles on: what the hints mostly did to them, though the bold and level-headed laugh them
/// off whatever they felt at first.
fn settle(c: &Character) -> Stance {
    let sky = &c.sky;
    if sky.doubt >= sky.awe + sky.anger || (c.profile.bravery > 0.75 && c.profile.temper <= 0.6) {
        Stance::Sceptic
    } else if sky.anger > sky.awe {
        Stance::Defiant
    } else {
        Stance::Believer
    }
}

/// What someone with a stance might say to the sky, given how many are watching.
fn sky_line(stance: Stance, crowd: u8, pick: u32) -> Option<&'static str> {
    let felt = match (stance, crowd) {
        (Stance::Believer, 3) => Some("The sky feels crowded today."),
        (Stance::Believer, 0) => Some("They've gone quiet up there."),
        (Stance::Defiant, 3) => Some("Look at them all, gawping down at us!"),
        (Stance::Defiant, 0) => Some("Good. Stay away."),
        _ => None,
    };
    let lines = match stance {
        Stance::Believer => BELIEVER,
        Stance::Defiant => DEFIANT,
        Stance::Sceptic => return None,
    };
    // Half the time, what they feel of the crowd, when there is something to feel.
    match felt {
        Some(line) if pick.is_multiple_of(2) => Some(line),
        _ => Some(lines[(pick / 2) as usize % lines.len()]),
    }
}

/// Free to say something to the sky: out and about, awake, and not caught up in anything.
fn at_leisure(c: &Character) -> bool {
    is_approachable(c) && c.queue.is_empty()
}

/// Speaks `line` to the sky as `id`, and lets the nearest sceptic within earshot answer.
fn speak_to_the_sky(ctx: &mut Ctx, id: &str, line: &str, stance: Stance) {
    ctx.say(id, line, None, false);
    let emote = if stance == Stance::Defiant { Emote::Anger } else { Emote::Thinking };
    ctx.emit(SimEvent::Emote { id: id.to_owned(), emote });
    let pos = ctx.cr(id).pos;
    let sceptic = ctx
        .state
        .order
        .iter()
        .filter_map(|other| ctx.state.characters.get(other))
        .filter(|o| o.id != id && o.sky.stance == Some(Stance::Sceptic) && at_leisure(o))
        .map(|o| (dist(o.pos, pos), o.id.clone()))
        .filter(|&(d, _)| d <= EARSHOT)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, sceptic)) = sceptic {
        let answer = SCEPTIC[hash_string(&format!("retort:{sceptic}:{}", ctx.tick())) as usize % SCEPTIC.len()];
        ctx.say(&sceptic, answer, Some(id), false);
        ctx.emit(SimEvent::Emote { id: sceptic, emote: Emote::Laugh });
    }
}

/// The server says how many are watching. When a crowd gathers or everyone goes, the first believer (or defiant
/// villager) at leisure says so, once a day at most, and the chronicle notes it.
pub fn set_audience(ctx: &mut Ctx, crowd: u8) {
    let crowd = crowd.min(3);
    let before = ctx.state.audience.crowd;
    ctx.state.audience.crowd = crowd;
    let felt = (crowd == 3 && before < 3) || (crowd == 0 && before > 0);
    let day = ctx.time.day;
    if !felt || ctx.state.audience.remarked_day == day {
        return;
    }
    let Some((id, stance)) = ctx.state.order.iter().find_map(|id| {
        let c = ctx.state.characters.get(id)?;
        let stance = c.sky.stance.filter(|s| *s != Stance::Sceptic)?;
        at_leisure(c).then(|| (id.clone(), stance))
    }) else {
        return;
    };
    let Some(line) = sky_line(stance, crowd, 0) else { return };
    ctx.state.audience.remarked_day = day;
    speak_to_the_sky(ctx, &id, line, stance);
    let name = first_name(ctx.cr(&id)).to_owned();
    ctx.chronicle(ChronicleKind::World, format!("{name} looks up at the sky: \u{201c}{line}\u{201d}"), vec![id]);
}

/// Once a game minute, in the daytime: those who believe in the watchers or rage at them now and then say so aloud,
/// each at a moment of their own in every few hours.
pub fn update_sky(ctx: &mut Ctx) {
    if ctx.tick() % TICKS_PER_GAME_MINUTE != 0 || !(7.0..20.0).contains(&ctx.time.hour) {
        return;
    }
    let minute = ctx.time.total_minutes.floor() as i64;
    let (stretch, at) = (minute.div_euclid(SPEAK_EVERY), minute.rem_euclid(SPEAK_EVERY));
    let speakers: Vec<(String, Stance, u32)> = ctx
        .state
        .order
        .iter()
        .filter_map(|id| ctx.state.characters.get(id))
        .filter_map(|c| {
            let stance = c.sky.stance.filter(|s| *s != Stance::Sceptic)?;
            let hash = hash_string(&format!("sky:{}:{}:{stretch}", ctx.state.seed, c.id));
            (i64::from(hash) % SPEAK_EVERY == at && at_leisure(c)).then(|| (c.id.clone(), stance, hash))
        })
        .collect();
    let crowd = ctx.state.audience.crowd;
    for (id, stance, hash) in speakers {
        if let Some(line) = sky_line(stance, crowd, hash / SPEAK_EVERY as u32) {
            speak_to_the_sky(ctx, &id, line, stance);
        }
    }
}

/// How the prompts put someone's stance to the model: a line for their header, or nothing.
pub fn stance_line(state: &SimState, c: &Character) -> Option<String> {
    let crowd = state.audience.crowd;
    let felt = match crowd {
        0 => " Lately it feels as if they have gone quiet.",
        3 => " Today the sky feels crowded with them.",
        _ => "",
    };
    Some(match c.sky.stance? {
        Stance::Believer => {
            format!("You are sure someone beyond the sky is watching the village, and you do not mind.{felt}")
        }
        Stance::Defiant => {
            format!("You are sure someone beyond the sky is spying on the village, and you resent it.{felt}")
        }
        Stance::Sceptic => "You think the talk of watchers beyond the sky is nonsense.".to_owned(),
    })
}
