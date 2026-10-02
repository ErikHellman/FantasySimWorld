//! Square navigation grid over the whole map: 0 = blocked, otherwise the cost
//! of entering the cell.

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

fn build_nav_grid() -> NavGrid {
    let map = world_map();
    let origin_x = map.min_x.floor();
    let origin_z = map.min_z.floor();
    let width = ((map.max_x - origin_x) / CELL).ceil() as usize;
    let height = ((map.max_z - origin_z) / CELL).ceil() as usize;
    let mut cost = vec![0.0f32; width * height];

    for iz in 0..height {
        for ix in 0..width {
            let x = origin_x + (ix as f64 + 0.5) * CELL;
            let z = origin_z + (iz as f64 + 0.5) * CELL;
            if ground_at(x, z) != Ground::Open {
                continue;
            }
            // Keep a body-width away from water, cliffs and the map edge too.
            if ground_at(x + AGENT_RADIUS, z) != Ground::Open
                || ground_at(x - AGENT_RADIUS, z) != Ground::Open
                || ground_at(x, z + AGENT_RADIUS) != Ground::Open
                || ground_at(x, z - AGENT_RADIUS) != Ground::Open
            {
                continue;
            }
            cost[iz * width + ix] = if is_on_road(x, z) { ROAD_COST } else { GRASS_COST };
        }
    }

    let mut block = |obstacle: &Obstacle| {
        let size = match *obstacle {
            Obstacle::Circle { radius, .. } => radius,
            Obstacle::Box { half_x, half_z, .. } => js_hypot(half_x, half_z),
        };
        let reach = size + AGENT_RADIUS + CELL;
        let (ox, oz) = (obstacle.x(), obstacle.z());
        let x0 = ((ox - reach - origin_x) / CELL).floor().max(0.0);
        let x1 = ((ox + reach - origin_x) / CELL).ceil().min(width as f64 - 1.0);
        let z0 = ((oz - reach - origin_z) / CELL).floor().max(0.0);
        let z1 = ((oz + reach - origin_z) / CELL).ceil().min(height as f64 - 1.0);
        if x1 < x0 || z1 < z0 {
            return;
        }
        for iz in z0 as usize..=z1 as usize {
            for ix in x0 as usize..=x1 as usize {
                let x = origin_x + (ix as f64 + 0.5) * CELL;
                let z = origin_z + (iz as f64 + 0.5) * CELL;
                if obstacle_contains(obstacle, x, z, AGENT_RADIUS) {
                    cost[iz * width + ix] = 0.0;
                }
            }
        }
    };
    for place in places() {
        place.obstacles.iter().for_each(&mut block);
    }
    scatter().obstacles.iter().for_each(&mut block);

    NavGrid { origin_x, origin_z, width, height, cost }
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
    cell_index(x, z).is_some_and(|index| nav_grid().cost[index] > 0.0)
}

/// Nearest walkable cell centre to a point, searching outwards in rings; `None` if none within
/// `max_radius` cells (6 in the TypeScript default). A walkable point is returned as it is.
pub fn nearest_walkable(pos: Vec2, max_radius: i64) -> Option<Vec2> {
    if is_walkable(pos.x, pos.z) {
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
                if grid.cost[(iz * width + ix) as usize] <= 0.0 {
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
    let distance = js_hypot(b.x - a.x, b.z - a.z);
    let steps = (distance / (CELL * 0.4)).ceil().max(1.0) as u64;
    (0..=steps).all(|i| {
        let t = i as f64 / steps as f64;
        is_walkable(a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t)
    })
}

/// FNV-1a over the nav costs' little-endian bytes, as 8 hex digits. The nav grid
/// depends on the map, every place and the solid scatter, so two builds of the
/// world that agree on this agree on the layout. Same as `worldHash()` in
/// `src/world/navgrid.ts`; the server sends it in `hello`.
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
