//! Ghosts: some of those the dead cut down come back for a few nights. A ghost rises at dusk by its grave,
//! drifts between the homes of its life, troubles the sleepers inside, and frightens and chills whoever it meets in
//! the dark. Nobody can fight one; after two or three nights it finds rest and is gone.
//!
//! Ghosts are kept apart from the monsters, so nothing that hunts, counts or fears the dead counts them. The roll
//! that decides whether someone returns is a hash, not a draw, and nothing here touches the RNG until a ghost walks,
//! so a world without one runs exactly as it did before ghosts.

use fsw_core::math2d::dist;
use fsw_core::rng;
use fsw_core::time::{minutes_to_ticks, seconds_to_ticks};
use fsw_core::{Vec2, vec2};
use fsw_world::places::{ActivityId, get_place};

use crate::data::anims::AnimId;
use crate::events::{Emote, SimEvent};
use crate::state::{ActPhase, ChronicleKind, Ghost, GhostPhase, Id, IntentSource, MemoryKind, Quirk};
use crate::systems::combat::{max_hp, start_flee};
use crate::systems::movement::{advance, route_to};
use crate::util::{Ctx, is_fighting, living, place_name_at, set_anim};

/// One in this many of those slain by the dead comes back.
const RETURN_ONE_IN: u32 = 3;
/// The dead must lie this long before they first walk.
const FIRST_RISE_HOURS: f64 = 8.0;
/// Ghosts walk from this hour until the monsters' dawn.
const RISE_HOUR: f64 = 20.5;
const DAWN_HOUR: f64 = 5.6;
const DUSK_HOUR: f64 = 19.4;
/// How many nights a ghost walks, at least and at most.
const NIGHTS: (i64, i64) = (2, 3);
const WANDER_SPEED: f64 = 1.2;
const DRIFT_SPEED: f64 = 2.0;
/// How far a ghost is seen in the dark, and how far it notices the living.
const SIGHT: f64 = 14.0;
const DRIFT_RANGE: f64 = 12.0;
const TOUCH: f64 = 1.4;
/// Someone who has met a ghost is not startled by the same one again this soon.
const MET_AGAIN_MINUTES: f64 = 120.0;
const CALM_MINUTES: f64 = 30.0;
const LINGER_MINUTES: (f64, f64) = (20.0, 40.0);
const HAUNT_EVERY_MINUTES: f64 = 10.0;
const HAUNT_CHANCE: f64 = 0.35;
const HAUNT_ENERGY: f64 = 15.0;
const DRAIN_ENERGY: f64 = 25.0;
const DRAIN_HP: (i64, i64) = (6, 10);
/// A ghost never leaves anyone below this share of their health.
const DRAIN_FLOOR: f64 = 0.25;
/// Homes a ghost haunts besides its own.
const MAX_LOVED_HOMES: usize = 3;
const FADE_SECONDS: f64 = 5.0;

/// Whether this villager, slain by the dead, comes back. A hash of the world and the villager, so the roll draws
/// nothing from the RNG.
pub fn returns(seed: u32, victim_id: &str) -> bool {
    rng::hash_string(&format!("{seed}:{victim_id}@ghost")).is_multiple_of(RETURN_ONE_IN)
}

/// What a ghost needs to know of the villager it was, taken while they still live.
pub struct Haunt {
    pub was_id: Id,
    name: String,
    look: crate::state::Look,
    pos: Vec2,
    facing: f64,
    home_id: String,
    close: Vec<Id>,
}

impl Haunt {
    /// Takes what a ghost of this living villager would need: who they were, and who they loved.
    pub fn of(ctx: &Ctx, id: &str) -> Haunt {
        let c = ctx.cr(id);
        let close = living(ctx.state)
            .filter(|o| o.id != id)
            .filter(|o| {
                c.spouse_id.as_deref() == Some(o.id.as_str())
                    || c.partner_id.as_deref() == Some(o.id.as_str())
                    || c.relationships.get(&o.id).is_some_and(|r| r.kin || r.affinity >= 25.0)
            })
            .map(|o| o.id.clone())
            .collect();
        Haunt {
            was_id: c.id.clone(),
            name: c.profile.name.clone(),
            look: c.look.clone(),
            pos: c.pos,
            facing: c.facing,
            home_id: c.home_id.clone(),
            close,
        }
    }
}

/// The slain villager will walk again: a ghost waits by their grave for the first dusk.
pub fn bind(ctx: &mut Ctx, haunt: Haunt) -> Id {
    let tick = ctx.tick();
    let graveyard = get_place("graveyard");
    let grave = ctx
        .state
        .graves
        .iter()
        .find(|g| g.character_id == haunt.was_id)
        .and_then(|g| graveyard.grave_plots.get(g.plot as usize))
        .map(|plot| plot.pos);
    let rest = grave.and_then(|pos| ctx.state.nav.nearest_walkable(pos, 4)).unwrap_or(haunt.pos);
    let mut haunts = vec![haunt.home_id.clone()];
    for other in &haunt.close {
        let home = &ctx.state.characters[other].home_id;
        if haunts.len() <= MAX_LOVED_HOMES && !haunts.contains(home) {
            haunts.push(home.clone());
        }
    }
    let id = ctx.state.new_id("g");
    let ghost = Ghost {
        id: id.clone(),
        was_id: haunt.was_id,
        name: haunt.name,
        look: crate::state::Look { main_hand: None, off_hand: None, ..haunt.look },
        pos: rest,
        facing: haunt.facing,
        moving: false,
        phase: GhostPhase::Waiting,
        since_tick: tick,
        rise_tick: tick + minutes_to_ticks(FIRST_RISE_HOURS * 60.0),
        nights_left: 0,
        rest,
        fell: haunt.pos,
        haunts,
        close: haunt.close,
        path: Vec::new(),
        repath_tick: 0,
        linger_until_tick: 0,
        lingering_at: None,
        next_haunt_tick: 0,
        target_id: None,
        calm_until_tick: 0,
        seen: Default::default(),
        sighted_night: 0,
        anim: AnimId::Idle,
        anim_seq: 0,
    };
    ctx.state.ghosts.insert(id.clone(), ghost);
    id
}

/// The night an hour belongs to: the small hours count with the evening before.
fn night_of(day: i64, hour: f64) -> i64 {
    if hour < 12.0 { day - 1 } else { day }
}

fn g<'s>(ctx: &'s mut Ctx, id: &str) -> &'s mut Ghost {
    ctx.state.ghosts.get_mut(id).unwrap_or_else(|| panic!("no ghost {id}"))
}

pub fn update_ghosts(ctx: &mut Ctx) {
    if ctx.state.ghosts.is_empty() {
        return;
    }
    let tick = ctx.tick();
    let hour = ctx.time.hour;
    let daylight = (DAWN_HOUR..DUSK_HOUR).contains(&hour);
    let walking_hours = !(5.0..RISE_HOUR).contains(&hour);
    let ids: Vec<Id> = ctx.state.ghosts.keys().cloned().collect();
    for (index, id) in ids.iter().enumerate() {
        let ghost = g(ctx, id);
        ghost.moving = false;
        match ghost.phase {
            GhostPhase::Fading => {
                if tick - ghost.since_tick > seconds_to_ticks(FADE_SECONDS) {
                    ctx.state.ghosts.shift_remove(id);
                }
            }
            GhostPhase::Waiting | GhostPhase::Resting => {
                if tick >= ghost.rise_tick && walking_hours {
                    rise(ctx, id);
                }
            }
            GhostPhase::Abroad => {
                if daylight {
                    dawn(ctx, id);
                } else {
                    walk(ctx, id, (tick + index as i64) % 10 == 0);
                }
            }
        }
    }
}

fn rise(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    let first = g(ctx, id).phase == GhostPhase::Waiting;
    if first {
        let nights = ctx.int_range(NIGHTS.0, NIGHTS.1);
        g(ctx, id).nights_left = nights;
    }
    let ghost = g(ctx, id);
    ghost.phase = GhostPhase::Abroad;
    ghost.since_tick = tick;
    ghost.pos = ghost.rest;
    ghost.path.clear();
    ghost.target_id = None;
    ghost.linger_until_tick = tick + minutes_to_ticks(5.0);
    ghost.lingering_at = None;
    set_anim(ghost, AnimId::Idle, true);
    if first {
        let text = format!("The ghost of {} rose from the grave.", ghost.name);
        ctx.chronicle(ChronicleKind::Death, text, Vec::new());
        ctx.emit(SimEvent::SavePoint { reason: "ghost".into() });
    }
}

fn dawn(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    let ghost = g(ctx, id);
    ghost.nights_left -= 1;
    ghost.since_tick = tick;
    ghost.path.clear();
    ghost.target_id = None;
    ghost.lingering_at = None;
    if ghost.nights_left > 0 {
        ghost.phase = GhostPhase::Resting;
        ghost.pos = ghost.rest;
        ghost.rise_tick = tick;
        return;
    }
    ghost.phase = GhostPhase::Fading;
    set_anim(ghost, AnimId::Idle, false);
    let ghost = &ctx.state.ghosts[id];
    let (name, was_id) = (ghost.name.clone(), ghost.was_id.clone());
    let first = first_name_of(ghost).to_owned();
    let close: Vec<Id> = ghost.close.iter().filter(|o| ctx.state.characters.contains_key(*o)).cloned().collect();
    let text = format!("The ghost of {name} found rest at last, and was seen no more.");
    ctx.chronicle(ChronicleKind::Death, text, close.clone());
    for other in close {
        ctx.remember(
            &other,
            format!("{first} is at peace now. The nights are quiet again."),
            7.0,
            MemoryKind::Event,
            vec![was_id.clone()],
        );
    }
    ctx.emit(SimEvent::SavePoint { reason: "ghost at rest".into() });
}

fn walk(ctx: &mut Ctx, id: &str, look_around: bool) {
    let tick = ctx.tick();
    if look_around {
        sightings(ctx, id);
        let ghost = &ctx.state.ghosts[id];
        if tick >= ghost.calm_until_tick {
            let target = prey(ctx, id);
            g(ctx, id).target_id = target;
        }
    }
    let nav = ctx.state.nav.clone();

    // Drifting after someone in the dark.
    let ghost = &ctx.state.ghosts[id];
    let target = ghost
        .target_id
        .as_deref()
        .and_then(|t| ctx.state.characters.get(t))
        .filter(|c| c.inside.is_none())
        .map(|c| (c.id.clone(), c.pos));
    if let Some((target_id, target_pos)) = target {
        let ghost = g(ctx, id);
        ghost.lingering_at = None;
        if dist(ghost.pos, target_pos) <= TOUCH {
            drain(ctx, id, &target_id);
            return;
        }
        if ghost.path.is_empty() || tick >= ghost.repath_tick {
            ghost.path = route_to(&nav, ghost.pos, target_pos).unwrap_or_default();
            ghost.repath_tick = tick + seconds_to_ticks(0.7);
        }
        let mut path = std::mem::take(&mut ghost.path);
        advance(ghost, &mut path, DRIFT_SPEED);
        ghost.path = path;
        set_anim(ghost, AnimId::Walk, false);
        return;
    }
    // Whoever it drifted after got away indoors: the way after them goes nowhere now.
    let ghost = g(ctx, id);
    if ghost.target_id.take().is_some() {
        ghost.path.clear();
    }

    // Lingering at one of the places of its life.
    let ghost = &ctx.state.ghosts[id];
    if ghost.path.is_empty() && tick < ghost.linger_until_tick {
        if let Some(home) = ghost.lingering_at.clone()
            && tick >= ghost.next_haunt_tick
        {
            g(ctx, id).next_haunt_tick = tick + minutes_to_ticks(HAUNT_EVERY_MINUTES);
            haunt_sleepers(ctx, id, &home);
        }
        set_anim(g(ctx, id), AnimId::Idle, false);
        return;
    }

    // On to the next of them.
    if ghost.path.is_empty() {
        let (home, goal) = next_haunt(ctx, id);
        let ghost = g(ctx, id);
        match goal.and_then(|goal| route_to(&nav, ghost.pos, goal)) {
            Some(path) if !path.is_empty() => {
                ghost.path = path;
                ghost.lingering_at = home;
            }
            _ => {
                // Nowhere to go from here, or already there: wait a while and look again.
                ghost.lingering_at = None;
                ghost.linger_until_tick = tick + minutes_to_ticks(5.0);
                return;
            }
        }
    }
    let ghost = g(ctx, id);
    let mut path = std::mem::take(&mut ghost.path);
    let arrived = advance(ghost, &mut path, WANDER_SPEED);
    ghost.path = path;
    set_anim(ghost, AnimId::Walk, false);
    if arrived {
        let minutes = ctx.range(LINGER_MINUTES.0, LINGER_MINUTES.1);
        let ghost = g(ctx, id);
        ghost.linger_until_tick = tick + minutes_to_ticks(minutes);
        ghost.next_haunt_tick = tick + minutes_to_ticks(HAUNT_EVERY_MINUTES * 0.5);
    }
}

/// Picks where the ghost goes next: one of the homes it haunts, its grave, or where it fell.
fn next_haunt(ctx: &mut Ctx, id: &str) -> (Option<String>, Option<Vec2>) {
    let ghost = &ctx.state.ghosts[id];
    let homes = ghost.haunts.len();
    let pick = ctx.pick_index(homes + 2);
    let ghost = &ctx.state.ghosts[id];
    let (home, near) = match pick {
        i if i < homes => (Some(ghost.haunts[i].clone()), get_place(&ghost.haunts[i]).door),
        i if i == homes => (None, ghost.rest),
        _ => (None, ghost.fell),
    };
    let x = near.x + ctx.range(-1.5, 1.5);
    let z = near.z + ctx.range(-1.5, 1.5);
    (home, ctx.state.nav.nearest_walkable(vec2(x, z), 4))
}

/// The nearest grown villager awake in the open, close enough for the ghost to go after.
fn prey(ctx: &Ctx, id: &str) -> Option<Id> {
    let ghost = &ctx.state.ghosts[id];
    let mut best = None;
    let mut best_distance = DRIFT_RANGE;
    for c in living(ctx.state) {
        if c.inside.is_some() || c.asleep || c.is_child() {
            continue;
        }
        let d = dist(c.pos, ghost.pos);
        if d < best_distance {
            best_distance = d;
            best = Some(c.id.clone());
        }
    }
    best
}

/// Everyone awake in the open who catches sight of the ghost reacts: those who loved them grieve, the rest run
/// or stand their ground.
fn sightings(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    let night = night_of(ctx.time.day, ctx.time.hour);
    let ghost = &ctx.state.ghosts[id];
    let pos = ghost.pos;
    let met_again = minutes_to_ticks(MET_AGAIN_MINUTES);
    let witnesses: Vec<Id> = living(ctx.state)
        .filter(|c| c.inside.is_none() && !c.asleep && !c.combat.yielded && dist(c.pos, pos) < SIGHT)
        .filter(|c| ghost.seen.get(&c.id).is_none_or(|&at| tick - at >= met_again))
        .map(|c| c.id.clone())
        .collect();
    if witnesses.is_empty() {
        return;
    }
    let name = ctx.state.ghosts[id].name.clone();
    let first = first_name_of(&ctx.state.ghosts[id]).to_owned();
    let where_ = place_name_at(pos);
    if ctx.state.ghosts[id].sighted_night != night {
        g(ctx, id).sighted_night = night;
        let text = format!("The ghost of {name} was seen near {where_}.");
        ctx.chronicle(ChronicleKind::Death, text, vec![witnesses[0].clone()]);
    }
    let was_id = ctx.state.ghosts[id].was_id.clone();
    for witness in witnesses {
        g(ctx, id).seen.insert(witness.clone(), tick);
        let subjects = vec![was_id.clone()];
        if ctx.state.ghosts[id].close.contains(&witness) {
            ctx.emit(SimEvent::Emote { id: witness.clone(), emote: Emote::Grief });
            let text = format!("I saw {first}'s ghost near {where_}. {first} looked straight through me.");
            ctx.remember(&witness, text, 8.0, MemoryKind::Event, subjects);
            continue;
        }
        let c = ctx.cr(&witness);
        if is_fighting(c) {
            continue;
        }
        let on_watch =
            c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Watch)) && c.act.phase == ActPhase::Perform;
        let courage = c.profile.bravery * 0.8 + if on_watch { 0.25 } else { 0.0 };
        let (coward, fighter) = (c.has(Quirk::Coward), c.has(Quirk::Fighter));
        let afraid = !fighter && (coward || ctx.next_float() > courage);
        if afraid {
            ctx.emit(SimEvent::Emote { id: witness.clone(), emote: Emote::Alert });
            ctx.say(&witness, format!("A ghost! It's {first}!"), None, true);
            start_flee(ctx, &witness, IntentSource::Reflex);
            let text = format!("Saw the ghost of {first} drifting near {where_}, and ran.");
            ctx.remember(&witness, text, 6.0, MemoryKind::Event, subjects);
        } else {
            let text = format!("Saw the ghost of {first} near {where_}. I held my ground, though my hands shook.");
            ctx.remember(&witness, text, 5.0, MemoryKind::Event, subjects);
        }
    }
}

/// The ghost passes through someone: a cold that takes the strength out of them, never their life.
fn drain(ctx: &mut Ctx, id: &str, victim: &str) {
    let tick = ctx.tick();
    let ghost = g(ctx, id);
    ghost.target_id = None;
    ghost.path.clear();
    ghost.calm_until_tick = tick + minutes_to_ticks(CALM_MINUTES);
    ghost.seen.insert(victim.to_owned(), tick);
    set_anim(ghost, AnimId::Idle, false);
    let was_id = ghost.was_id.clone();
    let first = first_name_of(ghost).to_owned();
    let chill = ctx.int_range(DRAIN_HP.0, DRAIN_HP.1) as f64;
    let c = ctx.c(victim);
    let floor = (max_hp(c) * DRAIN_FLOOR).min(c.hp);
    c.hp = (c.hp - chill).max(floor);
    c.needs.energy = (c.needs.energy - DRAIN_ENERGY).max(0.0);
    let where_ = place_name_at(c.pos);
    ctx.emit(SimEvent::Emote { id: victim.into(), emote: Emote::Alert });
    let text = format!("{first}'s ghost passed right through me near {where_}. I have never been so cold.");
    ctx.remember(victim, text, 7.0, MemoryKind::Event, vec![was_id]);
    if !is_fighting(ctx.cr(victim)) {
        start_flee(ctx, victim, IntentSource::Reflex);
    }
}

/// Lingering at a door, the ghost troubles the sleep of those inside.
fn haunt_sleepers(ctx: &mut Ctx, id: &str, home: &str) {
    let sleepers: Vec<Id> =
        living(ctx.state).filter(|c| c.asleep && c.inside.as_deref() == Some(home)).map(|c| c.id.clone()).collect();
    if sleepers.is_empty() {
        return;
    }
    let ghost = &ctx.state.ghosts[id];
    let (was_id, close) = (ghost.was_id.clone(), ghost.close.clone());
    let first = first_name_of(ghost).to_owned();
    for sleeper in sleepers {
        if !ctx.chance(HAUNT_CHANCE) {
            continue;
        }
        let c = ctx.c(&sleeper);
        c.needs.energy = (c.needs.energy - HAUNT_ENERGY).max(0.0);
        let importance = if close.contains(&sleeper) { 8.0 } else { 6.0 };
        let text = format!("Something moaned at the window in the night. I swear it was {first}.");
        ctx.remember(&sleeper, text, importance, MemoryKind::Event, vec![was_id.clone()]);
    }
}

/// A ghost is seen only by night.
pub fn visible(ghost: &Ghost) -> bool {
    matches!(ghost.phase, GhostPhase::Abroad | GhostPhase::Fading)
}

/// The first name of the villager a ghost was.
pub fn first_name_of(ghost: &Ghost) -> &str {
    ghost.name.split(' ').next().unwrap_or_default()
}
