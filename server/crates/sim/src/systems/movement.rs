//! Walking along routes, turning, and keeping bodies from overlapping.

use fsw_core::jsmath::js_hypot;
use fsw_core::math2d::{dist, facing_to, turn_toward};
use fsw_core::time::TICK_SECONDS;
use fsw_core::{Vec2, vec2};

use crate::nav::NavOverlay;
use crate::state::{MonsterPhase, SimState};
use crate::util::Body;

/// radians per second
const TURN_RATE: f64 = 9.0;
const BODY_RADIUS: f64 = 0.55;

/// Route to a point; an empty path means "already there", `None` means unreachable.
pub fn route_to(nav: &NavOverlay, from: Vec2, to: Vec2) -> Option<Vec<Vec2>> {
    if dist(from, to) < 0.05 {
        return Some(Vec::new());
    }
    nav.find_path(from, to)
}

pub fn face(mover: &mut impl Body, target: Vec2) {
    let pos = mover.pos();
    if dist(pos, target) < 0.01 {
        return;
    }
    let facing = mover.facing_mut();
    *facing = turn_toward(*facing, facing_to(pos, target), TURN_RATE * TICK_SECONDS);
}

/// Moves along a path at `speed` units per second, consuming waypoints.
/// Returns true once the path is finished.
pub fn advance(mover: &mut impl Body, path: &mut Vec<Vec2>, speed: f64) -> bool {
    let mut budget = speed * TICK_SECONDS;
    while budget > 0.0 && !path.is_empty() {
        let next = path[0];
        let pos = mover.pos();
        let d = dist(pos, next);
        if d <= budget {
            mover.set_pos(next);
            path.remove(0);
            budget -= d;
        } else {
            face(mover, next);
            mover.set_pos(vec2(pos.x + ((next.x - pos.x) / d) * budget, pos.z + ((next.z - pos.z) / d) * budget));
            budget = 0.0;
        }
        mover.set_moving(true);
    }
    path.is_empty()
}

/// Steps straight towards a point (used for the last stretch onto a seat or up to a person).
pub fn step_toward(mover: &mut impl Body, target: Vec2, speed: f64) -> bool {
    let pos = mover.pos();
    let d = dist(pos, target);
    let step = speed * TICK_SECONDS;
    if d <= step {
        mover.set_pos(target);
        mover.set_moving(d > 0.001);
        return true;
    }
    face(mover, target);
    mover.set_pos(vec2(pos.x + ((target.x - pos.x) / d) * step, pos.z + ((target.z - pos.z) / d) * step));
    mover.set_moving(true);
    false
}

#[derive(Clone, Copy)]
enum BodyRef {
    Character(usize),
    Monster(usize),
}

/// Nudges moving bodies apart so walkers do not pass through each other.
/// Bodies are pushed in order, each seeing the others' positions as already updated.
pub fn separate(state: &mut SimState) {
    let nav = state.nav.clone();
    let mut bodies = Vec::new();
    for id in &state.order {
        if let Some((index, _, c)) = state.characters.get_full(id)
            && c.inside.is_none()
        {
            bodies.push(BodyRef::Character(index));
        }
    }
    // The dragon flies over everyone's heads.
    for (index, m) in state.monsters.values().enumerate() {
        if m.phase != MonsterPhase::Dying && !m.kind.def().flies {
            bodies.push(BodyRef::Monster(index));
        }
    }
    let read = |state: &SimState, body: BodyRef| match body {
        BodyRef::Character(i) => (state.characters[i].pos, state.characters[i].moving),
        BodyRef::Monster(i) => (state.monsters[i].pos, state.monsters[i].moving),
    };
    for (mi, &mover) in bodies.iter().enumerate() {
        let (pos, moving) = read(state, mover);
        if !moving {
            continue;
        }
        let mut push_x = 0.0;
        let mut push_z = 0.0;
        for (oi, &other) in bodies.iter().enumerate() {
            if oi == mi {
                continue;
            }
            let (other_pos, _) = read(state, other);
            let dx = pos.x - other_pos.x;
            let dz = pos.z - other_pos.z;
            let d = js_hypot(dx, dz);
            if !(1e-4..BODY_RADIUS * 2.0).contains(&d) {
                continue;
            }
            let overlap = BODY_RADIUS * 2.0 - d;
            push_x += (dx / d) * overlap * 0.35;
            push_z += (dz / d) * overlap * 0.35;
        }
        if push_x == 0.0 && push_z == 0.0 {
            continue;
        }
        let x = pos.x + push_x;
        let z = pos.z + push_z;
        if nav.is_walkable(x, z) {
            match mover {
                BodyRef::Character(i) => state.characters[i].pos = vec2(x, z),
                BodyRef::Monster(i) => state.monsters[i].pos = vec2(x, z),
            }
        }
    }
}
