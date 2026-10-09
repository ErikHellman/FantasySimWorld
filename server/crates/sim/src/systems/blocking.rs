//! Someone standing in a villager's way. Walkers are only nudged around those who stand still
//! (`movement::separate`), so a villager standing squarely on a walker's route holds them up. Once
//! held up for a moment, the walker stops and says so, politely, curtly or in a temper as their
//! nature and their feelings for the other decide. The one in the way steps aside if they are free
//! to; if not, the walker brushes past them. A sharp word leaves a small grievance on both sides,
//! which the hostility ladder takes from there.

use fsw_core::jsmath::js_hypot;
use fsw_core::math2d::{dist, facing_to};
use fsw_core::time::{minutes_to_ticks, seconds_to_ticks};
use fsw_core::{Vec2, vec2};

use crate::data::anims::AnimId;
use crate::data::blocking::{ANGRY, CURT, POLITE, RETORT};
use crate::data::classes::WALK_SPEED;
use crate::events::{Emote, SimEvent};
use crate::nav::NavOverlay;
use crate::state::{Character, Id, IntentAction, MemoryKind, Passing, RelationLabel, Sidestep, SimState};
use crate::systems::movement::step_toward;
use crate::systems::relationships::{RelationDelta, adjust};
use crate::util::{Ctx, first_name, living_ids, place_name_at, rel, rel_or_default, set_anim};

/// Bodies this close are touching (twice `movement::BODY_RADIUS`, and a little).
const CONTACT: f64 = 1.35;
/// How squarely ahead someone must stand to be in the way: the cosine of the widest angle.
const AHEAD: f64 = 0.6;
/// Held up this long before saying anything; a walker only brushing someone slides past sooner.
const HELD_UP_SECONDS: f64 = 1.5;
/// How long a walker may brush past the one in their way.
const PASSING_SECONDS: f64 = 3.0;
/// How far the one in the way steps aside.
const SIDESTEP: f64 = 1.4;
const SIDESTEP_SECONDS: f64 = 3.0;
/// Held up by the same person again this soon, the walker brushes past without a word.
const QUIET_MINUTES: f64 = 20.0;
/// Held up by the same person again this soon, the walker is crosser about it.
const AGAIN_MINUTES: f64 = 180.0;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Temper {
    Polite,
    Curt,
    Angry,
}

/// Moves those stepping aside, then finds the walkers who have been held up and has them speak up.
/// Runs after everyone has moved and before bodies are pushed apart.
pub fn update_blocking(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let nav = ctx.state.nav.clone();
    let ids = living_ids(ctx.state);
    for id in &ids {
        let c = ctx.c(id);
        if c.passing.as_ref().is_some_and(|p| tick >= p.until_tick) {
            c.passing = None;
        }
        if c.sidestep.is_some() {
            step_aside(tick, c);
        }
    }
    for id in &ids {
        // Fights keep their own count of being stuck in `act.stuck`.
        if ctx.cr(id).intent.as_ref().is_some_and(|i| i.is_fight()) {
            continue;
        }
        let Some(blocker) = in_the_way(ctx.state, id) else {
            ctx.c(id).act.stuck = 0;
            continue;
        };
        let c = ctx.c(id);
        c.act.stuck += 1;
        if c.act.stuck >= seconds_to_ticks(HELD_UP_SECONDS) {
            c.act.stuck = 0;
            speak_up(ctx, &nav, id, &blocker);
        }
    }
}

/// Whether two bodies are brushing past each other, and so are not pushed apart.
pub fn brushing_past(state: &SimState, a: &Character, b: &Character) -> bool {
    let passes = |c: &Character, other: &Character| {
        c.passing.as_ref().is_some_and(|p| p.id == other.id && state.tick < p.until_tick)
    };
    passes(a, b) || passes(b, a)
}

/// One more step out of the way; done once there, or once something else needs them.
fn step_aside(tick: i64, c: &mut Character) {
    let Some(sidestep) = c.sidestep.clone() else { return };
    let taken_up = c.inside.is_some() || c.asleep || c.convo_id.is_some() || !c.act.path.is_empty();
    if taken_up || c.combat.target_id.is_some() || c.combat.yielded {
        c.sidestep = None;
        return;
    }
    let speed = WALK_SPEED * c.stride();
    if tick > sidestep.until_tick || step_toward(c, sidestep.to, speed) {
        set_anim(c, sidestep.anim, false);
        c.sidestep = None;
    } else {
        set_anim(c, AnimId::Walk, false);
    }
}

/// The direction a character is walking in, if they are following a route.
fn heading(c: &Character) -> Option<Vec2> {
    let next = c.act.path.iter().find(|p| dist(c.pos, **p) > 0.05)?;
    let d = dist(c.pos, *next);
    Some(vec2((next.x - c.pos.x) / d, (next.z - c.pos.z) / d))
}

/// Whether this character is on their way somewhere that leaves time for a word.
fn walking(c: &Character, tick: i64) -> bool {
    let busy = c.inside.is_some() || c.asleep || c.convo_id.is_some() || c.combat.yielded || c.sidestep.is_some();
    let urgent = c.intent.as_ref().is_none_or(|i| {
        matches!(
            i.action,
            IntentAction::Fight { .. }
                | IntentAction::Flee
                | IntentAction::Stalk { .. }
                | IntentAction::FightFire { .. }
                | IntentAction::Fetch { .. }
        )
    });
    let brushing = c.passing.as_ref().is_some_and(|p| tick < p.until_tick);
    c.moving && !c.act.path.is_empty() && !busy && !urgent && !brushing && c.combat.target_id.is_none()
}

/// The villager standing squarely in this walker's way, if any. Someone walking the same way is
/// not in it, and nor is the person the walker is on their way to.
fn in_the_way(state: &SimState, id: &str) -> Option<Id> {
    let c = &state.characters[id];
    if !walking(c, state.tick) {
        return None;
    }
    let dir = heading(c)?;
    let going_to = c.intent.as_ref().and_then(|i| i.action.target_id());
    for other_id in &state.order {
        if other_id == id || going_to == Some(other_id) {
            continue;
        }
        let Some(other) = state.characters.get(other_id) else { continue };
        if other.inside.is_some() {
            continue;
        }
        let (dx, dz) = (other.pos.x - c.pos.x, other.pos.z - c.pos.z);
        let d = js_hypot(dx, dz);
        if !(1e-4..CONTACT).contains(&d) || (dx * dir.x + dz * dir.z) / d < AHEAD {
            continue;
        }
        if other.moving
            && let Some(theirs) = heading(other)
            && theirs.x * dir.x + theirs.z * dir.z > 0.3
        {
            continue;
        }
        return Some(other_id.clone());
    }
    None
}

fn within(tick: i64, since: Option<i64>, minutes: f64) -> bool {
    since.is_some_and(|since| tick - since < minutes_to_ticks(minutes))
}

/// How cross the walker is at being held up, by their nature and their feelings for the other.
fn temper_of(ctx: &mut Ctx, walker: &str, blocker: &str, again: bool) -> Temper {
    let tick = ctx.tick();
    let c = ctx.cr(walker);
    let r = rel_or_default(c, blocker);
    let mut heat = c.profile.temper * 0.45 + c.profile.spite * 0.25 + r.grievance / 100.0 * 0.6;
    if c.drunk(tick) {
        heat += 0.3;
    }
    if matches!(r.label, RelationLabel::Rival | RelationLabel::Enemy) {
        heat += 0.3;
    }
    if r.affinity > 0.0 {
        heat -= r.affinity / 100.0 * 0.4;
    }
    if again {
        heat += 0.35;
    }
    let child = c.is_child();
    heat += ctx.range(0.0, 0.25);
    let temper = if heat < 0.45 {
        Temper::Polite
    } else if heat < 0.8 {
        Temper::Curt
    } else {
        Temper::Angry
    };
    // A child may sulk, but does not shout their elders down.
    if child { temper.min(Temper::Curt) } else { temper }
}

/// Whether the one in the way is standing idle and could step aside.
fn free_to_move(c: &Character) -> bool {
    c.inside.is_none()
        && !c.asleep
        && c.slot_id.is_none()
        && c.convo_id.is_none()
        && c.act.path.is_empty()
        && c.sidestep.is_none()
        && c.combat.target_id.is_none()
        && !c.combat.yielded
}

/// A spot to one side of the walker's line, on the side the one in the way already leans to.
fn aside_spot(nav: &NavOverlay, walker: &Character, blocker: &Character) -> Option<Vec2> {
    let dir = heading(walker)?;
    let across = vec2(-dir.z, dir.x);
    let lean = (blocker.pos.x - walker.pos.x) * across.x + (blocker.pos.z - walker.pos.z) * across.z;
    let first = if lean < 0.0 { -1.0 } else { 1.0 };
    [first, -first].into_iter().find_map(|side| {
        let to = vec2(blocker.pos.x + across.x * SIDESTEP * side, blocker.pos.z + across.z * SIDESTEP * side);
        let mid = vec2((blocker.pos.x + to.x) / 2.0, (blocker.pos.z + to.z) / 2.0);
        (nav.is_walkable(to.x, to.z) && nav.is_walkable(mid.x, mid.z)).then_some(to)
    })
}

/// Lets the walker by: the one in the way steps aside if they can, or the walker brushes past.
fn make_way(ctx: &mut Ctx, nav: &NavOverlay, walker: &str, blocker: &str) {
    let tick = ctx.tick();
    let (w, b) = (ctx.cr(walker), ctx.cr(blocker));
    let spot = if free_to_move(b) { aside_spot(nav, w, b) } else { None };
    match spot {
        Some(to) => {
            let b = ctx.c(blocker);
            let anim = b.anim;
            b.sidestep = Some(Sidestep { to, anim, until_tick: tick + seconds_to_ticks(SIDESTEP_SECONDS) });
        }
        None => {
            let until_tick = tick + seconds_to_ticks(PASSING_SECONDS);
            ctx.c(walker).passing = Some(Passing { id: blocker.to_owned(), until_tick });
        }
    }
}

fn line(ctx: &mut Ctx, lines: &[&str], walker: &str, blocker: &str) -> String {
    let template = ctx.pick(lines);
    let (a, b) = (first_name(ctx.cr(walker)).to_owned(), first_name(ctx.cr(blocker)).to_owned());
    template.replace("{a}", &a).replace("{b}", &b)
}

/// The walker stops, faces the one in their way and says something, then is let by.
fn speak_up(ctx: &mut Ctx, nav: &NavOverlay, walker: &str, blocker: &str) {
    let tick = ctx.tick();
    let last = rel_or_default(ctx.cr(walker), blocker).last_blocked_tick;
    let theirs = rel_or_default(ctx.cr(blocker), walker).last_blocked_tick;
    rel(ctx.c(walker), blocker).last_blocked_tick = Some(tick);
    let quiet = within(tick, last, QUIET_MINUTES) || within(tick, theirs, QUIET_MINUTES);
    if quiet || ctx.cr(blocker).asleep {
        make_way(ctx, nav, walker, blocker);
        return;
    }

    let blocker_pos = ctx.cr(blocker).pos;
    let w = ctx.c(walker);
    w.facing = facing_to(w.pos, blocker_pos);
    let temper = temper_of(ctx, walker, blocker, within(tick, last, AGAIN_MINUTES));
    let lines: &[&str] = match temper {
        Temper::Polite => &POLITE,
        Temper::Curt => &CURT,
        Temper::Angry => &ANGRY,
    };
    let text = line(ctx, lines, walker, blocker);
    ctx.say(walker, text, Some(blocker), temper == Temper::Angry);
    if temper == Temper::Angry {
        ctx.emit(SimEvent::Emote { id: walker.to_owned(), emote: Emote::Anger });
    }
    make_way(ctx, nav, walker, blocker);
    if temper > Temper::Polite {
        sharp_word(ctx, walker, blocker, temper == Temper::Angry, within(tick, last, AGAIN_MINUTES));
    }
}

/// What a sharp word leaves behind: a little grievance both ways, a memory each, and now and then
/// a word back from a hot-tempered one in the way.
fn sharp_word(ctx: &mut Ctx, walker: &str, blocker: &str, angry: bool, again: bool) {
    let (w, b) = (ctx.cr(walker), ctx.cr(blocker));
    let (w_temper, w_spite) = (w.profile.temper, w.profile.spite);
    let (b_temper, b_spite) = (b.profile.temper, b.profile.spite);
    let b_grievance = rel_or_default(b, walker).grievance;
    let (w_name, b_name) = (first_name(w).to_owned(), first_name(b).to_owned());
    let where_ = place_name_at(b.pos);

    let (base, scale) = if angry { (4.0, 1.5) } else { (2.0, 1.0) };
    let grievance = base + (3.0 * w_temper + 2.0 * w_spite + if again { 2.0 } else { 0.0 }) * scale;
    let affinity = if angry { -2.0 } else { 0.0 };
    adjust(ctx, walker, blocker, RelationDelta { grievance, affinity, ..Default::default() });
    let stung = (base - 1.0) + 3.0 * b_temper * scale;
    adjust(ctx, blocker, walker, RelationDelta { grievance: stung, affinity, ..Default::default() });

    let weight = if angry { 3.0 } else { 2.0 };
    let subject = vec![blocker.to_owned()];
    ctx.remember(walker, format!("{b_name} stood in my way at {where_}."), weight, MemoryKind::Event, subject);
    let memory = format!("{w_name} snapped at me for standing in the way at {where_}.");
    ctx.remember(blocker, memory, weight, MemoryKind::Event, vec![walker.to_owned()]);

    let heat = b_temper * 0.6 + b_spite * 0.3 + b_grievance / 100.0 * 0.4 + ctx.range(0.0, 0.3);
    if heat > 0.7 && !ctx.cr(blocker).is_child() {
        let text = line(ctx, &RETORT, walker, blocker);
        ctx.say(blocker, text, Some(walker), angry);
        if angry {
            ctx.emit(SimEvent::Emote { id: blocker.to_owned(), emote: Emote::Anger });
        }
    }
}
