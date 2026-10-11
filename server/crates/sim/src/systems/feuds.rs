//! Feuds and love triangles: what bad blood and unwanted love look like out in the village. A grievance already
//! climbs a ladder of rungs (`relationships::rung_of`); here each rung shows. With tension, someone snubs the one
//! they resent; with a grudge, the two have a shouting match in front of others; with a feud, the cool-headed slip
//! into the other's house while they are out and take some of their coin, and the hot-headed challenge them to
//! settle it with their fists before onlookers. Someone still sweet on a villager who is with another tells them so
//! when the other is not about, and is turned away or, now and then, chosen instead.
//!
//! Who acts when is worked out from hashes, not the RNG: each pair gets one moment in every stretch of
//! [`WINDOW`] game minutes, and a scene between them leaves them be for a while after (`Relationship::scene_tick`).

use fsw_core::math2d::dist;
use fsw_core::rng::hash_string;
use fsw_core::time::{TICKS_PER_DAY, TICKS_PER_GAME_MINUTE};

use crate::events::{Emote, SimEvent};
use crate::state::{ChronicleKind, FightMode, Id, IntentSource, MemoryKind, SimState};
use crate::systems::ceremonies::cancel_wedding_of;
use crate::systems::combat::{max_hp, start_fight, witnesses};
use crate::systems::relationships::{
    RUNG_FEUD, RUNG_GRUDGE, RUNG_TENSION, RelationDelta, adjust, become_lovers, brawl_unlocked, break_up,
};
use fsw_world::places::find_place;

use crate::util::{
    Ctx, first_name, is_approachable, living_ids, nearby_characters, place_name_at, rel, rel_or_default,
};

/// Each pair has one moment in every stretch of this many game minutes.
pub const WINDOW: i64 = 20;
/// Between two scenes from one villager to another.
pub const SCENE_GAP: i64 = TICKS_PER_DAY / 2;
/// Between two declarations of love to the same villager.
pub const DECLARE_GAP: i64 = TICKS_PER_DAY * 2;
/// Attraction that makes someone pine for a villager who is with another.
pub const PINING: f64 = 40.0;
/// How near to snub, row or challenge someone.
const NEAR: f64 = 8.0;
/// How near the other's door a burglar must be ...
const BURGLE_NEAR: f64 = 6.0;
/// ... how far from it the other ...
const OWNER_AWAY: f64 = 30.0;
/// ... and how far off anyone awake to see it.
const UNSEEN: f64 = 12.0;
/// How far the beloved's partner must be for an admirer to speak up.
const PARTNER_AWAY: f64 = 15.0;
/// Onlookers a row or a challenge wants.
const AUDIENCE: usize = 2;
/// A spouse weighs this much more in the balance than a suitor.
const MARRIAGE_WEIGHT: f64 = 40.0;

const SNUBS: &[&str] = &[
    "Don't even look at me, {name}.",
    "Oh. It's you, {name}.",
    "I've nothing to say to you, {name}.",
    "Walk on, {name}.",
];
const ROW_OPEN: &[&str] = &[
    "Everyone should know what you are, {name}!",
    "I'm sick of you, {name}, and I don't care who hears it!",
    "You've some nerve showing your face here, {name}!",
];
const ROW_ANSWER: &[&str] = &[
    "Say that again, {name}, I dare you!",
    "Listen to yourself, {name}! You're a disgrace!",
    "Shout all you like, {name}. Nobody believes you.",
];
const CHALLENGES: &[&str] = &[
    "Enough talk, {name}. Settle it with your fists, here and now!",
    "Put your fists up, {name}! Let them all see who's right!",
];
const DECLARATIONS: &[&str] = &[
    "I can't keep it in any longer, {name}. It should have been me.",
    "You could still choose me, {name}.",
    "I think about you all the time, {name}. Does that mean nothing?",
];
const REFUSALS: &[&str] =
    &["I'm with {name}. Let it go.", "Please don't. I love {name}.", "No. {name} is the one for me."];
const ACCEPTANCES: &[&str] = &["Maybe it should have been you.", "I've felt it too. I can't pretend any more."];

fn line(lines: &[&str], pick: u32, name: &str) -> String {
    lines[pick as usize % lines.len()].replace("{name}", name)
}

/// A grown-up out and about with nothing else on: not talking, fighting, at a ceremony or busy with a plan.
fn free(ctx: &Ctx, id: &str) -> bool {
    ctx.state
        .characters
        .get(id)
        .is_some_and(|c| !c.is_child() && c.exiled_since.is_none() && is_approachable(c) && c.queue.is_empty())
}

/// Whether `from` has made a scene with `to` lately.
fn lately(ctx: &Ctx, from: &str, to: &str, gap: i64) -> bool {
    ctx.cr(from).relationships.get(to).and_then(|r| r.scene_tick).is_some_and(|at| ctx.tick() - at < gap)
}

fn mark_scene(ctx: &mut Ctx, from: &str, to: &str) {
    let tick = ctx.tick();
    rel(ctx.c(from), to).scene_tick = Some(tick);
}

/// Once a game minute: each villager with bad blood or an unwanted love may make a scene of it, at the moment their
/// hash gives them, if the people it needs are about.
pub fn update_feuds(ctx: &mut Ctx) {
    if ctx.tick() % TICKS_PER_GAME_MINUTE != 0 || ctx.state.director.founding {
        return;
    }
    let minute = ctx.time.total_minutes.floor() as i64;
    let (window, at) = (minute.div_euclid(WINDOW), minute.rem_euclid(WINDOW));
    let seed = ctx.state.seed;
    for id in living_ids(ctx.state) {
        if !free(ctx, &id) {
            continue;
        }
        let others: Vec<(Id, f64, f64)> = ctx
            .cr(&id)
            .relationships
            .iter()
            .filter(|(_, r)| r.grievance >= RUNG_TENSION || r.attraction >= PINING)
            .map(|(other, r)| (other.clone(), r.grievance, r.attraction))
            .collect();
        for (other, grievance, attraction) in others {
            let hash = hash_string(&format!("feud:{seed}:{id}:{other}:{window}"));
            if i64::from(hash) % WINDOW != at {
                continue;
            }
            let acted = (attraction >= PINING && declare(ctx, &id, &other, hash))
                || (grievance >= RUNG_TENSION && confront(ctx, &id, &other, grievance, hash));
            if acted {
                break;
            }
        }
    }
}

/// `id` takes their grievance against `other` out on them, as far as its rung and who is about allow.
fn confront(ctx: &mut Ctx, id: &str, other: &str, grievance: f64, hash: u32) -> bool {
    if lately(ctx, id, other, SCENE_GAP) {
        return false;
    }
    let (me, them) = (ctx.cr(id), ctx.cr(other));
    let hot = me.profile.temper > 0.5 || me.profile.bravery > 0.6;
    if grievance >= RUNG_FEUD && !hot && can_burgle(ctx, id, other) {
        burgle(ctx, id, other, hash);
        mark_scene(ctx, id, other);
        return true;
    }
    if dist(me.pos, them.pos) > NEAR || !free(ctx, other) {
        return false;
    }
    let onlookers = witnesses(ctx, me.pos, &[id, other]);
    let fit = me.hp >= max_hp(me) * 0.6 && them.hp >= max_hp(them) * 0.6;
    if grievance >= RUNG_FEUD && hot && fit && onlookers.len() >= AUDIENCE && brawl_unlocked(me, other) {
        challenge(ctx, id, other, onlookers.len(), hash);
    } else if grievance >= RUNG_GRUDGE && onlookers.len() >= AUDIENCE {
        row(ctx, id, other, &onlookers, hash);
    } else {
        snub(ctx, id, other, hash);
    }
    mark_scene(ctx, id, other);
    true
}

/// Whether `id` is at `other`'s door with `other` well away, nobody about to see, and coin in the house to take.
fn can_burgle(ctx: &Ctx, id: &str, other: &str) -> bool {
    let (me, them) = (ctx.cr(id), ctx.cr(other));
    let Some(door) = find_place(&them.home_id).map(|home| home.door) else { return false };
    me.home_id != them.home_id
        && them.gold >= 4
        && dist(me.pos, door) <= BURGLE_NEAR
        && them.inside.as_deref() != Some(them.home_id.as_str())
        && dist(them.pos, door) >= OWNER_AWAY
        && nearby_characters(ctx.state, me.pos, UNSEEN, Some(id)).iter().all(|o| ctx.cr(o).asleep)
}

/// The lowest rung: a cold word and a turned back.
fn snub(ctx: &mut Ctx, id: &str, other: &str, hash: u32) {
    let first = rel_or_default(ctx.cr(id), other).scene_tick.is_none();
    let (name, other_name) = (first_name(ctx.cr(id)).to_owned(), first_name(ctx.cr(other)).to_owned());
    ctx.say(id, line(SNUBS, hash / WINDOW as u32, &other_name), Some(other), false);
    ctx.emit(SimEvent::Emote { id: id.to_owned(), emote: Emote::Anger });
    // A scene vents a little of the anger that made it and passes a little on, so two who keep snubbing each other
    // do not climb the ladder on snubs alone.
    adjust(ctx, id, other, RelationDelta { grievance: -2.0, ..Default::default() });
    adjust(ctx, other, id, RelationDelta { grievance: 2.0, affinity: -3.0, ..Default::default() });
    ctx.remember(other, format!("{name} snubbed me in front of everyone."), 4.0, MemoryKind::Event, vec![id.into()]);
    // Only the first snub between them is news; after that it is just how they are.
    if first {
        let place = place_name_at(ctx.cr(id).pos);
        ctx.chronicle(
            ChronicleKind::Conflict,
            format!("{name} snubbed {other_name} at {place}."),
            vec![id.to_owned(), other.to_owned()],
        );
    }
}

/// A grudge aired in public: both shout, and everyone about remembers it.
fn row(ctx: &mut Ctx, id: &str, other: &str, onlookers: &[Id], hash: u32) {
    let (name, other_name) = (first_name(ctx.cr(id)).to_owned(), first_name(ctx.cr(other)).to_owned());
    let pick = hash / WINDOW as u32;
    ctx.say(id, line(ROW_OPEN, pick, &other_name), Some(other), true);
    ctx.say(other, line(ROW_ANSWER, pick / 3, &name), Some(id), true);
    for who in [id, other] {
        ctx.emit(SimEvent::Emote { id: who.to_owned(), emote: Emote::Anger });
    }
    // Airing it vents the one who started it and stings the other.
    adjust(ctx, id, other, RelationDelta { grievance: -6.0, affinity: -5.0, ..Default::default() });
    adjust(ctx, other, id, RelationDelta { grievance: 4.0, affinity: -5.0, ..Default::default() });
    let place = place_name_at(ctx.cr(id).pos);
    ctx.remember(
        id,
        format!("Told {other_name} what I think of them at {place}, in front of everyone."),
        5.0,
        MemoryKind::Event,
        vec![other.into()],
    );
    ctx.remember(
        other,
        format!("{name} shouted at me at {place}, in front of everyone."),
        6.0,
        MemoryKind::Event,
        vec![id.into()],
    );
    for onlooker in onlookers {
        ctx.remember(
            onlooker,
            format!("Saw {name} and {other_name} shouting at each other at {place}."),
            4.0,
            MemoryKind::Event,
            vec![id.into(), other.into()],
        );
    }
    let crowd =
        if onlookers.len() >= 6 { "half the village".to_owned() } else { format!("{} people", onlookers.len()) };
    ctx.chronicle(
        ChronicleKind::Conflict,
        format!("{name} and {other_name} had a shouting match at {place} in front of {crowd}."),
        vec![id.to_owned(), other.to_owned()],
    );
}

/// The cool-headed get even unseen: they slip into the other's house while they are out and take some of their
/// coin. The one robbed blames whoever they most resent, which is not always the thief.
fn burgle(ctx: &mut Ctx, id: &str, other: &str, hash: u32) {
    let amount = (3 + i64::from(hash / WINDOW as u32 % 8)).min(ctx.cr(other).gold);
    ctx.c(other).gold -= amount;
    ctx.c(id).gold += amount;
    let (name, other_name) = (first_name(ctx.cr(id)).to_owned(), first_name(ctx.cr(other)).to_owned());
    ctx.remember(
        id,
        format!("Crept into {other_name}'s house and took {amount} gold. Serves them right."),
        5.0,
        MemoryKind::Event,
        vec![other.into()],
    );
    let suspect = most_resented(ctx, other);
    let blame = match &suspect {
        Some(suspect) => {
            adjust(ctx, other, suspect, RelationDelta { grievance: 8.0, trust: -10.0, ..Default::default() });
            format!(" I'd wager it was {}.", first_name(ctx.cr(suspect)))
        }
        None => String::new(),
    };
    let ids = suspect.iter().cloned().collect();
    ctx.remember(
        other,
        format!("{amount} gold has gone from my house. Someone has robbed me.{blame}"),
        6.0,
        MemoryKind::Event,
        ids,
    );
    ctx.chronicle(
        ChronicleKind::Conflict,
        format!("{name} crept into {other_name}'s house while they were out and took {amount} gold."),
        vec![id.to_owned(), other.to_owned()],
    );
}

/// The living grown-up `id` resents most, if anyone at all.
fn most_resented(ctx: &Ctx, id: &str) -> Option<Id> {
    let mut best: Option<(&Id, f64)> = None;
    for (other, r) in &ctx.cr(id).relationships {
        let grown = ctx.state.characters.get(other).is_some_and(|o| !o.is_child());
        if grown && r.grievance >= RUNG_TENSION && best.is_none_or(|(_, g)| r.grievance > g) {
            best = Some((other, r.grievance));
        }
    }
    best.map(|(other, _)| other.clone())
}

/// The hot-headed call the other out before onlookers, and the fists do the rest (`combat::end_brawl`).
fn challenge(ctx: &mut Ctx, id: &str, other: &str, audience: usize, hash: u32) {
    let (name, other_name) = (first_name(ctx.cr(id)).to_owned(), first_name(ctx.cr(other)).to_owned());
    let place = place_name_at(ctx.cr(id).pos);
    let words = line(CHALLENGES, hash / WINDOW as u32, &other_name);
    start_fight(ctx, id, other, FightMode::Brawl, IntentSource::Routine, Some(words));
    ctx.chronicle(
        ChronicleKind::Conflict,
        format!(
            "{name} challenged {other_name} to settle their feud with fists at {place}, before {audience} onlookers."
        ),
        vec![id.to_owned(), other.to_owned()],
    );
}

/// `id` is sweet on `loved`, who is with someone else. With that someone out of the way, they say so; the
/// beloved weighs it, and nearly always turns them away.
fn declare(ctx: &mut Ctx, id: &str, loved: &str, hash: u32) -> bool {
    if !free(ctx, loved) {
        return false;
    }
    let me = ctx.cr(id);
    let them = ctx.cr(loved);
    let Some(partner) = them.partner_id.clone().filter(|p| p != id && ctx.state.characters.contains_key(p)) else {
        return false;
    };
    let kin = rel_or_default(me, loved).kin;
    if kin || me.partner_id.is_some() || them.is_child() || lately(ctx, id, loved, DECLARE_GAP) {
        return false;
    }
    if dist(me.pos, them.pos) > NEAR || dist(ctx.cr(&partner).pos, them.pos) <= PARTNER_AWAY {
        return false;
    }
    mark_scene(ctx, id, loved);
    let (name, loved_name, partner_name) = (
        first_name(ctx.cr(id)).to_owned(),
        first_name(ctx.cr(loved)).to_owned(),
        first_name(ctx.cr(&partner)).to_owned(),
    );
    let pick = hash / WINDOW as u32;
    ctx.say(id, line(DECLARATIONS, pick, &loved_name), Some(loved), false);
    let married = ctx.cr(loved).spouse_id.as_deref() == Some(partner.as_str());
    let feeling = |r: crate::state::Relationship| r.affinity + r.attraction;
    let for_suitor = feeling(rel_or_default(ctx.cr(loved), id));
    let for_partner = feeling(rel_or_default(ctx.cr(loved), &partner)) + if married { MARRIAGE_WEIGHT } else { 0.0 };
    if for_suitor > for_partner {
        ctx.say(loved, line(ACCEPTANCES, pick / 3, &name), Some(id), false);
        ctx.chronicle(
            ChronicleKind::Romance,
            format!("{name} told {loved_name} how they feel, and {loved_name} has left {partner_name} for them."),
            vec![id.to_owned(), loved.to_owned(), partner.clone()],
        );
        cancel_wedding_of(ctx, loved);
        break_up(ctx, loved, &partner, &format!("{name} came between us"));
        become_lovers(ctx, id, loved);
        adjust(ctx, &partner, id, RelationDelta { grievance: 25.0, affinity: -20.0, ..Default::default() });
        ctx.remember(
            &partner,
            format!("{name} stole {loved_name} from me."),
            8.0,
            MemoryKind::Event,
            vec![id.into(), loved.into()],
        );
    } else {
        ctx.say(loved, line(REFUSALS, pick / 3, &partner_name), Some(id), false);
        ctx.emit(SimEvent::Emote { id: id.to_owned(), emote: Emote::BrokenHeart });
        // Each refusal cools it a little, and turns a little more of it against the one who was chosen.
        adjust(ctx, id, loved, RelationDelta { attraction: -12.0, ..Default::default() });
        adjust(ctx, id, &partner, RelationDelta { grievance: 8.0, affinity: -5.0, ..Default::default() });
        ctx.remember(
            id,
            format!("Told {loved_name} how I feel. They chose {partner_name}."),
            7.0,
            MemoryKind::Event,
            vec![loved.into(), partner.clone()],
        );
        ctx.remember(
            loved,
            format!("{name} told me they love me. I am with {partner_name}."),
            6.0,
            MemoryKind::Event,
            vec![id.into(), partner.clone()],
        );
        ctx.chronicle(
            ChronicleKind::Romance,
            format!(
                "{name} told {loved_name}, who is with {partner_name}, how they feel; {loved_name} turned them away."
            ),
            vec![id.to_owned(), loved.to_owned()],
        );
    }
    true
}

/// Which of a couple being wed `guest` is still sweet on, if either.
pub fn heartbroken_at<'a>(state: &SimState, guest: &str, a: &'a str, b: &'a str) -> Option<&'a str> {
    let c = state.characters.get(guest).filter(|c| !c.is_child())?;
    [a, b].into_iter().find(|loved| {
        let r = rel_or_default(c, loved);
        !r.kin && r.attraction >= PINING
    })
}

/// At the pronouncement, a guest who wanted one of the couple for themselves cannot cheer.
pub fn watch_them_wed(ctx: &mut Ctx, guest: &str, a: &str, b: &str) {
    let Some(loved) = heartbroken_at(ctx.state, guest, a, b) else { return };
    let rival = if loved == a { b } else { a };
    let (name, loved_name, rival_name) = (
        first_name(ctx.cr(guest)).to_owned(),
        first_name(ctx.cr(loved)).to_owned(),
        first_name(ctx.cr(rival)).to_owned(),
    );
    ctx.say(guest, "I can't watch this.", None, false);
    ctx.emit(SimEvent::Emote { id: guest.to_owned(), emote: Emote::BrokenHeart });
    adjust(ctx, guest, rival, RelationDelta { grievance: 10.0, affinity: -8.0, ..Default::default() });
    ctx.remember(
        guest,
        format!("Watched {loved_name} marry {rival_name}. It should have been me."),
        8.0,
        MemoryKind::Event,
        vec![loved.into(), rival.into()],
    );
    ctx.chronicle(
        ChronicleKind::Romance,
        format!("{name} watched {loved_name} marry {rival_name} with a broken heart."),
        vec![guest.to_owned(), loved.to_owned()],
    );
}
