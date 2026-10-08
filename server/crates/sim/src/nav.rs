//! Walking in a world that changes. The static nav grid in `fsw_world` is the
//! map as authored; the sim lays its own changes over it (a felled tree frees
//! the ground it stood on, a new fence blocks some), and every path, nearest
//! walkable cell and walkability check in the sim goes through [`NavOverlay`].
//!
//! The overlay is a cache, never saved: it is derived from `SimState.objects`
//! on load and after every change to them (`refresh`), and two states that are
//! equal as saves compare equal whatever their overlays hold. A world nothing
//! has changed in holds no copy at all and answers from the static grid, so it
//! runs exactly as it did before there was an overlay.

use std::sync::Arc;

use fsw_core::Vec2;
use fsw_world::navgrid::{
    AGENT_RADIUS, block, cell_center, cells_touched_by, is_walkable_on, line_of_walk_on, nav_grid, nearest_walkable_on,
    terrain_cost,
};
use fsw_world::pathfind::find_path_on;
use fsw_world::places::{Obstacle, obstacle_contains, places};
use fsw_world::scatter::scatter;

use crate::state::SimState;
use crate::systems::world::footprint;

/// The sim's view of the nav grid: the static costs with the living world's changes applied.
#[derive(Clone, Debug, Default)]
pub struct NavOverlay {
    /// `None` while nothing in the world changes walkability
    costs: Option<Arc<[f32]>>,
    /// what the costs were built from, so a refresh that changes nothing is free
    key: u64,
}

/// The overlay is derived state: it never makes two saves differ.
impl PartialEq for NavOverlay {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl NavOverlay {
    /// The cost of every cell, in the static grid's shape.
    pub fn costs(&self) -> &[f32] {
        match &self.costs {
            Some(costs) => costs,
            None => &nav_grid().cost,
        }
    }

    /// Whether anything in the world has changed where one can walk.
    pub fn is_live(&self) -> bool {
        self.costs.is_some()
    }

    pub fn is_walkable(&self, x: f64, z: f64) -> bool {
        is_walkable_on(self.costs(), x, z)
    }

    pub fn nearest_walkable(&self, pos: Vec2, max_radius: i64) -> Option<Vec2> {
        nearest_walkable_on(self.costs(), pos, max_radius)
    }

    pub fn line_of_walk(&self, a: Vec2, b: Vec2) -> bool {
        line_of_walk_on(self.costs(), a, b)
    }

    pub fn find_path(&self, from: Vec2, to: Vec2) -> Option<Vec<Vec2>> {
        find_path_on(self.costs(), from, to)
    }
}

/// What the overlay is built from: the static obstacles that no longer stand and the new ones.
struct Changes {
    removed: Vec<&'static Obstacle>,
    added: Vec<Obstacle>,
    key: u64,
}

fn changes_of(state: &SimState) -> Changes {
    let mut changes = Changes { removed: Vec::new(), added: Vec::new(), key: 2_166_136_261 };
    let mut hash = |text: &str| {
        for byte in text.bytes() {
            changes.key ^= u64::from(byte);
            changes.key = changes.key.wrapping_mul(1_099_511_628_211);
        }
    };
    for (id, object) in &state.objects {
        let print = footprint(id, object);
        if print.removed.is_none() && print.added.is_none() {
            continue;
        }
        hash(id);
        hash(print.signature);
        hash(";");
        changes.removed.extend(print.removed);
        changes.added.extend(print.added);
    }
    changes
}

/// Brings the overlay in line with `state.objects`. Cheap when nothing relevant changed.
pub fn refresh(state: &mut SimState) {
    let changes = changes_of(state);
    if changes.removed.is_empty() && changes.added.is_empty() {
        state.nav = NavOverlay::default();
        return;
    }
    if state.nav.costs.is_some() && state.nav.key == changes.key {
        return;
    }
    state.nav = NavOverlay { costs: Some(rebuild(&changes)), key: changes.key };
}

/// The static grid with the changes applied. A pure function of the two sets, whatever their order.
fn rebuild(changes: &Changes) -> Arc<[f32]> {
    let grid = nav_grid();
    let mut costs = grid.cost.clone();

    // Where an obstacle went, the ground is as the terrain says, unless something else still stands there.
    let mut touched: Vec<usize> = changes.removed.iter().flat_map(|obstacle| cells_touched_by(obstacle)).collect();
    touched.sort_unstable();
    touched.dedup();
    let standing = |x: f64, z: f64| {
        let still_there = |obstacle: &'static Obstacle| {
            !changes.removed.iter().any(|gone| std::ptr::eq(*gone, obstacle))
                && obstacle_contains(obstacle, x, z, AGENT_RADIUS)
        };
        places().iter().flat_map(|place| place.obstacles.iter()).any(still_there)
            || scatter().obstacles.iter().any(still_there)
    };
    for &index in &touched {
        let centre = cell_center(index);
        costs[index] = if standing(centre.x, centre.z) { 0.0 } else { terrain_cost(centre.x, centre.z) };
    }
    for obstacle in &changes.added {
        block(&mut costs, obstacle);
    }
    Arc::from(costs.into_boxed_slice())
}
