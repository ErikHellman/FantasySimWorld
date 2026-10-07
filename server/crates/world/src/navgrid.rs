//! Square navigation grid over the whole map: 0 = blocked, otherwise the cost
//! of entering the cell.
//!
//! The grid built here is the static one: terrain, roads and every obstacle of
//! the map as authored. The sim keeps a copy of its costs with the changes of
//! a living world applied (a felled tree, a new fence) and asks the `*_on`
//! functions, which take any cost slice of the grid's shape; the plain
//! functions answer for the static grid, and [`world_hash`] describes only it.

use std::sync::LazyLock;

use fsw_core::jsmath::js_hypot;
use fsw_core::{Vec2, vec2};

use crate::map::{Ground, ground_at, is_on_road, world_map};
use crate::places::{Obstacle, obstacle_contains, places};
use crate::scatter::scatter;

pub const CELL: f64 = 1.0;
/// Obstacles are grown by this so a character's body does not clip them.
pub const AGENT_RADIUS: f64 = 0.55;

const ROAD_COST: f32 = 1.0;
const GRASS_COST: f32 = 1.45;

#[derive(Debug)]
pub struct NavGrid {
    pub origin_x: f64,
    pub origin_z: f64,
    pub width: usize,
    pub height: usize,
    /// 0 = blocked, otherwise the cost of entering the cell (f32, as the TypeScript `Float32Array`)
    pub cost: Vec<f32>,
}

/// The grid's placement over the map, known before any cell is filled in.
#[derive(Clone, Copy, Debug)]
struct Frame {
    origin_x: f64,
    origin_z: f64,
    width: usize,
    height: usize,
}

impl Frame {
    fn of_map() -> Frame {
        let map = world_map();
        let origin_x = map.min_x.floor();
        let origin_z = map.min_z.floor();
        Frame {
            origin_x,
            origin_z,
            width: ((map.max_x - origin_x) / CELL).ceil() as usize,
            height: ((map.max_z - origin_z) / CELL).ceil() as usize,
        }
    }

    fn of_grid() -> Frame {
        let grid = nav_grid();
        Frame { origin_x: grid.origin_x, origin_z: grid.origin_z, width: grid.width, height: grid.height }
    }

    fn centre(&self, ix: usize, iz: usize) -> (f64, f64) {
        (self.origin_x + (ix as f64 + 0.5) * CELL, self.origin_z + (iz as f64 + 0.5) * CELL)
    }

    /// The cells an obstacle, grown by the agent radius, could reach: a bounding rectangle of indices.
    fn reach_of(&self, obstacle: &Obstacle) -> Option<(usize, usize, usize, usize)> {
        let size = match *obstacle {
            Obstacle::Circle { radius, .. } => radius,
            Obstacle::Box { half_x, half_z, .. } => js_hypot(half_x, half_z),
        };
        let reach = size + AGENT_RADIUS + CELL;
        let (ox, oz) = (obstacle.x(), obstacle.z());
        let x0 = ((ox - reach - self.origin_x) / CELL).floor().max(0.0);
        let x1 = ((ox + reach - self.origin_x) / CELL).ceil().min(self.width as f64 - 1.0);
        let z0 = ((oz - reach - self.origin_z) / CELL).floor().max(0.0);
        let z1 = ((oz + reach - self.origin_z) / CELL).ceil().min(self.height as f64 - 1.0);
        if x1 < x0 || z1 < z0 {
            return None;
        }
        Some((x0 as usize, x1 as usize, z0 as usize, z1 as usize))
    }
}

/// What a cell costs from the ground alone: 0 off the open ground or too close to water, cliffs and
/// the map edge for a body to pass, else the road or grass cost. Obstacles are applied on top.
pub fn terrain_cost(x: f64, z: f64) -> f32 {
    if ground_at(x, z) != Ground::Open {
        return 0.0;
    }
    // Keep a body-width away from water, cliffs and the map edge too.
    if ground_at(x + AGENT_RADIUS, z) != Ground::Open
        || ground_at(x - AGENT_RADIUS, z) != Ground::Open
        || ground_at(x, z + AGENT_RADIUS) != Ground::Open
        || ground_at(x, z - AGENT_RADIUS) != Ground::Open
    {
        return 0.0;
    }
    if is_on_road(x, z) { ROAD_COST } else { GRASS_COST }
}

fn block_in(frame: &Frame, cost: &mut [f32], obstacle: &Obstacle) {
    let Some((x0, x1, z0, z1)) = frame.reach_of(obstacle) else {
        return;
    };
    for iz in z0..=z1 {
        for ix in x0..=x1 {
            let (x, z) = frame.centre(ix, iz);
            if obstacle_contains(obstacle, x, z, AGENT_RADIUS) {
                cost[iz * frame.width + ix] = 0.0;
            }
        }
    }
}

/// Zeroes every cell of `cost` (a slice of the grid's shape) whose centre lies within the agent radius
/// of an obstacle. This is how the static grid is built; the sim uses it for things built later.
pub fn block(cost: &mut [f32], obstacle: &Obstacle) {
    block_in(&Frame::of_grid(), cost, obstacle);
}

/// Indices of the cells an obstacle may block (its bounding rectangle grown by the agent radius), in
/// row order. A superset of what [`block`] zeroes; the sim recomputes exactly these when an obstacle goes.
pub fn cells_touched_by(obstacle: &Obstacle) -> Vec<usize> {
    let frame = Frame::of_grid();
    let Some((x0, x1, z0, z1)) = frame.reach_of(obstacle) else {
        return Vec::new();
    };
    (z0..=z1).flat_map(|iz| (x0..=x1).map(move |ix| iz * frame.width + ix)).collect()
}

fn build_nav_grid() -> NavGrid {
    let frame = Frame::of_map();
    let mut cost = vec![0.0f32; frame.width * frame.height];

    for iz in 0..frame.height {
        for ix in 0..frame.width {
            let (x, z) = frame.centre(ix, iz);
            cost[iz * frame.width + ix] = terrain_cost(x, z);
        }
    }

    for place in places() {
        place.obstacles.iter().for_each(|obstacle| block_in(&frame, &mut cost, obstacle));
    }
    scatter().obstacles.iter().for_each(|obstacle| block_in(&frame, &mut cost, obstacle));

    NavGrid { origin_x: frame.origin_x, origin_z: frame.origin_z, width: frame.width, height: frame.height, cost }
}

static NAV_GRID: LazyLock<NavGrid> = LazyLock::new(build_nav_grid);

pub fn nav_grid() -> &'static NavGrid {
    &NAV_GRID
}

/// Index of the cell under a point; `None` off the grid.
pub fn cell_index(x: f64, z: f64) -> Option<usize> {
    let grid = nav_grid();
    let ix = ((x - grid.origin_x) / CELL).floor();
    let iz = ((z - grid.origin_z) / CELL).floor();
    if !(ix >= 0.0 && iz >= 0.0 && ix < grid.width as f64 && iz < grid.height as f64) {
        return None;
    }
    Some(iz as usize * grid.width + ix as usize)
}

pub fn cell_center(index: usize) -> Vec2 {
    let grid = nav_grid();
    let ix = index % grid.width;
    let iz = index / grid.width;
    vec2(grid.origin_x + (ix as f64 + 0.5) * CELL, grid.origin_z + (iz as f64 + 0.5) * CELL)
}

pub fn is_walkable(x: f64, z: f64) -> bool {
    is_walkable_on(&nav_grid().cost, x, z)
}

/// [`is_walkable`] against any cost slice of the grid's shape.
pub fn is_walkable_on(cost: &[f32], x: f64, z: f64) -> bool {
    cell_index(x, z).is_some_and(|index| cost[index] > 0.0)
}

/// Nearest walkable cell centre to a point, searching outwards in rings; `None` if none within
/// `max_radius` cells (6 in the TypeScript default). A walkable point is returned as it is.
pub fn nearest_walkable(pos: Vec2, max_radius: i64) -> Option<Vec2> {
    nearest_walkable_on(&nav_grid().cost, pos, max_radius)
}

/// [`nearest_walkable`] against any cost slice of the grid's shape.
pub fn nearest_walkable_on(cost: &[f32], pos: Vec2, max_radius: i64) -> Option<Vec2> {
    if is_walkable_on(cost, pos.x, pos.z) {
        return Some(pos);
    }
    let grid = nav_grid();
    let cx = ((pos.x - grid.origin_x) / CELL).floor() as i64;
    let cz = ((pos.z - grid.origin_z) / CELL).floor() as i64;
    let (width, height) = (grid.width as i64, grid.height as i64);
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    for r in 1..=max_radius {
        for dz in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dz.abs()) != r {
                    continue;
                }
                let ix = cx + dx;
                let iz = cz + dz;
                if ix < 0 || iz < 0 || ix >= width || iz >= height {
                    continue;
                }
                if cost[(iz * width + ix) as usize] <= 0.0 {
                    continue;
                }
                let centre = vec2(grid.origin_x + (ix as f64 + 0.5) * CELL, grid.origin_z + (iz as f64 + 0.5) * CELL);
                let (ddx, ddz) = (centre.x - pos.x, centre.z - pos.z);
                let d = ddx * ddx + ddz * ddz;
                if d < best_distance {
                    best_distance = d;
                    best = Some(centre);
                }
            }
        }
        if best.is_some() {
            return best;
        }
    }
    None
}

/// True when the straight line between two points only crosses walkable cells.
pub fn line_of_walk(a: Vec2, b: Vec2) -> bool {
    line_of_walk_on(&nav_grid().cost, a, b)
}

/// [`line_of_walk`] against any cost slice of the grid's shape.
pub fn line_of_walk_on(cost: &[f32], a: Vec2, b: Vec2) -> bool {
    let distance = js_hypot(b.x - a.x, b.z - a.z);
    let steps = (distance / (CELL * 0.4)).ceil().max(1.0) as u64;
    (0..=steps).all(|i| {
        let t = i as f64 / steps as f64;
        is_walkable_on(cost, a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t)
    })
}

/// FNV-1a over the nav costs' little-endian bytes, as 8 hex digits. The nav grid
/// depends on the map, every place and the solid scatter, so two builds of the
/// world that agree on this agree on the layout. Same as `worldHash()` in
/// `src/world/navgrid.ts`; the server sends it in `hello`. It describes the
/// static world only: what the sim changes later is not in it.
pub fn world_hash() -> String {
    let mut h: u32 = 2_166_136_261;
    for cost in &nav_grid().cost {
        for byte in cost.to_le_bytes() {
            h ^= u32::from(byte);
            h = h.wrapping_mul(16_777_619);
        }
    }
    format!("{h:08x}")
}
