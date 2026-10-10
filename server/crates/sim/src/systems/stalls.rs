//! Nothing stays stuck. Each system walks its own movers and handles the snags it knows of (a villager in the way,
//! an opponent out of reach, an animal cutting a corner); this is the net under all of them. Anyone with a path to
//! walk (a villager, an animal, one of the dead, a raider or a ghost) who has got nowhere for a while is helped on:
//! close to where they were going, they are there; otherwise they find their way afresh, or step round whatever
//! holds them; and if that fails too, they give up on going there and their own system picks what to do next.
//!
//! Nothing here draws from the RNG, and a world in which nothing is stuck runs exactly as it did without it.

use fsw_core::math2d::dist;
use fsw_core::rng::hash_string;
use fsw_core::time::{TICKS_PER_GAME_MINUTE, seconds_to_ticks};
use fsw_core::{Vec2, vec2};

use crate::nav::NavOverlay;
use crate::state::{ActPhase, Id, MonsterPhase, SimState, Stall};
use crate::systems::movement::route_to;
use crate::util::Ctx;

/// How long one may walk without getting anywhere before being helped on.
pub const STALL_SECONDS: f64 = 8.0;
/// Getting this far counts as getting somewhere.
const PROGRESS: f64 = 0.5;
/// Held up this close to where they were going, they are as good as there.
const NEAR_ENOUGH: f64 = 1.5;
/// How far to one side a step round goes.
const STEP_ROUND: f64 = 2.0;
/// After this many helps without getting anywhere in between, they give up on going there.
const GIVE_UP_AFTER: u32 = 3;

/// Once a game minute: who has a path and has got nowhere, and help for those stalled too long.
pub fn update_stalls(ctx: &mut Ctx) {
    let tick = ctx.tick();
    if tick % TICKS_PER_GAME_MINUTE != 0 {
        return;
    }
    let walking = walkers(ctx.state);
    ctx.state.stalls.retain(|id, _| walking.iter().any(|(w, _)| w == id));
    let nav = ctx.state.nav.clone();
    for (id, pos) in walking {
        let stall = ctx.state.stalls.entry(id.clone()).or_insert(Stall { at: pos, since: tick, helped: 0 });
        if dist(stall.at, pos) > PROGRESS {
            *stall = Stall { at: pos, since: tick, helped: 0 };
            continue;
        }
        if tick - stall.since < seconds_to_ticks(STALL_SECONDS) {
            continue;
        }
        stall.since = tick;
        stall.helped += 1;
        let give_up = stall.helped >= GIVE_UP_AFTER;
        help_on(ctx.state, &nav, &id, give_up);
        // Being set down on open ground is not getting anywhere.
        if let Some(pos) = body_mut(ctx.state, &id).map(|(pos, _)| *pos)
            && let Some(stall) = ctx.state.stalls.get_mut(&id)
        {
            stall.at = pos;
        }
    }
}

/// Everything with a path to walk right now, and where it stands, in a stable order.
fn walkers(state: &SimState) -> Vec<(Id, Vec2)> {
    let mut out = Vec::new();
    for id in &state.order {
        let Some(c) = state.characters.get(id) else { continue };
        // A fight has its own way of giving up on an opponent out of reach.
        let fighting = c.combat.target_id.is_some() || c.intent.as_ref().is_some_and(|i| i.is_fight());
        if c.inside.is_none() && c.act.phase == ActPhase::Travel && !c.act.path.is_empty() && !fighting {
            out.push((c.id.clone(), c.pos));
        }
    }
    for m in state.monsters.values() {
        if !m.path.is_empty() && m.phase != MonsterPhase::Dying && !m.kind.def().flies {
            out.push((m.id.clone(), m.pos));
        }
    }
    for g in state.ghosts.values().filter(|g| !g.path.is_empty()) {
        out.push((g.id.clone(), g.pos));
    }
    for a in state.animals.values().filter(|a| !a.path.is_empty() && a.rider.is_none() && a.flight.is_none()) {
        out.push((a.id.clone(), a.pos));
    }
    out
}

/// The path and position of whoever goes by this id.
fn body_mut<'a>(state: &'a mut SimState, id: &str) -> Option<(&'a mut Vec2, &'a mut Vec<Vec2>)> {
    if let Some(c) = state.characters.get_mut(id) {
        return Some((&mut c.pos, &mut c.act.path));
    }
    if let Some(m) = state.monsters.get_mut(id) {
        return Some((&mut m.pos, &mut m.path));
    }
    if let Some(g) = state.ghosts.get_mut(id) {
        return Some((&mut g.pos, &mut g.path));
    }
    state.animals.get_mut(id).map(|a| (&mut a.pos, &mut a.path))
}

/// Helps one stalled walker on, as the module describes.
fn help_on(state: &mut SimState, nav: &NavOverlay, id: &str, give_up: bool) {
    let Some((pos, path)) = body_mut(state, id) else { return };
    let Some(&goal) = path.last() else { return };
    if give_up || dist(*pos, goal) < NEAR_ENOUGH {
        // An empty path is an arrival to every system that walks one.
        path.clear();
        return;
    }
    if !nav.is_walkable(pos.x, pos.z)
        && let Some(open) = nav.nearest_walkable(*pos, 3)
    {
        *pos = open;
    }
    let fresh = route_to(nav, *pos, goal).unwrap_or_default();
    if !fresh.is_empty() && fresh != *path {
        *path = fresh;
        return;
    }
    // The same way again would meet the same snag: step round it first, to one side or the other.
    let next = path.iter().copied().find(|p| dist(*pos, *p) > 0.05).unwrap_or(goal);
    let d = dist(*pos, next).max(1e-6);
    let (ax, az) = ((next.x - pos.x) / d, (next.z - pos.z) / d);
    let side = if hash_string(id).is_multiple_of(2) { 1.0 } else { -1.0 };
    let aside = vec2(pos.x - az * side * STEP_ROUND, pos.z + ax * side * STEP_ROUND);
    let Some(aside) = nav.nearest_walkable(aside, 2).filter(|a| nav.line_of_walk(*pos, *a)) else {
        path.clear();
        return;
    };
    let rest = route_to(nav, aside, goal).unwrap_or_default();
    *path = std::iter::once(aside).chain(rest).collect();
}
