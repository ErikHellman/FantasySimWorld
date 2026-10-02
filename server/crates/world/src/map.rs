//! The hex map: terrain per hex, road and river connections, the tile model on
//! each hex, and what the ground allows at any point.

use std::collections::HashMap;
use std::sync::LazyLock;

use fsw_core::jsmath::{cos, sin};
use fsw_core::math2d::dist_to_segment;
use fsw_core::rng::hash_string;
use fsw_core::{Vec2, vec2};
use serde::Serialize;

use crate::height::{BRIDGE_HALF_WIDTH, bridge_height, tile_height};
use crate::hex::{
    DIR_COUNT, HEX_APOTHEM, HEX_RADIUS, HEX_ROW_PITCH, HEX_SCALE, HEX_WIDTH, Hex, dir_vector, direction_to, hex_at,
    hex_center, mask_has, neighbor,
};
use crate::layout::{HexPath, LAYOUT_ROWS, RIVERS, ROADS};
use crate::tiles::{TileChoice, coast_tile, crossing_tile, river_tile, road_tile};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Terrain {
    Meadow,
    Forest,
    Water,
    Mountain,
    Hills,
    Trees,
    Grain,
}

impl Terrain {
    fn from_char(ch: char) -> Option<Self> {
        Some(match ch {
            '.' => Self::Meadow,
            'f' => Self::Forest,
            '~' => Self::Water,
            'M' => Self::Mountain,
            'H' => Self::Hills,
            'T' => Self::Trees,
            'g' => Self::Grain,
            _ => return None,
        })
    }

    pub fn impassable(self) -> bool {
        matches!(self, Self::Water | Self::Mountain | Self::Hills | Self::Trees)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HexCell {
    pub col: i32,
    pub row: i32,
    pub terrain: Terrain,
    pub center: Vec2,
    /// edges with a road / river connection
    pub road_mask: u8,
    pub river_mask: u8,
    /// edges that face open water (lake)
    pub water_mask: u8,
    pub tile: TileChoice,
    /// hex-pack model standing on the tile (mountain, hills, trees, grain), scaled like the tile
    pub decoration: Option<TileChoice>,
    /// true when a bridge model sits on this hex
    pub bridge: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldMap {
    pub cols: i32,
    pub rows: i32,
    pub cells: Vec<HexCell>,
    pub min_x: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_z: f64,
}

/// Half-width of the road strip, in world units.
pub const ROAD_HALF_WIDTH: f64 = 1.6;

fn parse_layout() -> Vec<Vec<Terrain>> {
    LAYOUT_ROWS
        .iter()
        .enumerate()
        .map(|(row, line)| {
            line.chars()
                .filter(|ch| !ch.is_whitespace())
                .enumerate()
                .map(|(col, ch)| {
                    Terrain::from_char(ch).unwrap_or_else(|| panic!("Unknown terrain '{ch}' at {col},{row}"))
                })
                .collect()
        })
        .collect()
}

fn pick_variant(models: &[&'static str], col: i32, row: i32) -> TileChoice {
    let h = hash_string(&format!("{col},{row}"));
    TileChoice { model: models[h as usize % models.len()], rot: ((h >> 8) % 6) as u8 }
}

fn build_map() -> WorldMap {
    let grid = parse_layout();
    let rows = grid.len() as i32;
    let cols = grid.first().map_or(0, Vec::len) as i32;
    for (row, line) in grid.iter().enumerate() {
        assert!(line.len() as i32 == cols, "Layout row {row} has {} hexes, expected {cols}", line.len());
    }
    let in_map = |h: Hex| h.col >= 0 && h.row >= 0 && h.col < cols && h.row < rows;
    let terrain_at = |h: Hex| in_map(h).then(|| grid[h.row as usize][h.col as usize]);

    let connect = |paths: &[HexPath], label: &str| {
        let mut masks: HashMap<Hex, u8> = HashMap::new();
        for path in paths {
            for pair in path.windows(2) {
                let a = Hex { col: pair[0].0, row: pair[0].1 };
                let b = Hex { col: pair[1].0, row: pair[1].1 };
                let dir =
                    direction_to(a, b).unwrap_or_else(|| panic!("{label} path: {a:?} and {b:?} are not neighbours"));
                if in_map(a) {
                    *masks.entry(a).or_default() |= 1 << dir;
                }
                if in_map(b) {
                    *masks.entry(b).or_default() |= 1 << ((dir + 3) % 6);
                }
            }
        }
        masks
    };
    let road_masks = connect(&ROADS, "Road");
    let river_masks = connect(&RIVERS, "River");

    let mut cells = Vec::with_capacity((rows * cols) as usize);
    for row in 0..rows {
        for col in 0..cols {
            let hex = Hex { col, row };
            let terrain = grid[row as usize][col as usize];
            let water = terrain == Terrain::Water;
            let road_mask = if water { 0 } else { road_masks.get(&hex).copied().unwrap_or(0) };
            let river_mask = if water { 0 } else { river_masks.get(&hex).copied().unwrap_or(0) };
            let mut water_mask = 0;
            if !water {
                for dir in 0..DIR_COUNT {
                    if terrain_at(neighbor(hex, dir)) == Some(Terrain::Water) {
                        water_mask |= 1 << dir;
                    }
                }
            }

            let mut decoration = None;
            let mut bridge = false;
            let grass = TileChoice { model: "hex_grass", rot: 0 };
            let tile = if water {
                TileChoice { model: "hex_water", rot: 0 }
            } else if river_mask != 0 && road_mask != 0 {
                bridge = true;
                crossing_tile(river_mask, road_mask).unwrap_or_else(|| panic!("No river crossing tile fits {hex:?}"))
            } else if river_mask != 0 {
                river_tile(river_mask).unwrap_or_else(|| panic!("No river tile fits {hex:?} (mask {river_mask:b})"))
            } else if road_mask != 0 {
                road_tile(road_mask).unwrap_or_else(|| panic!("No road tile fits {hex:?} (mask {road_mask:b})"))
            } else if water_mask != 0 {
                coast_tile(water_mask).unwrap_or(grass)
            } else {
                decoration = match terrain {
                    Terrain::Mountain => Some(pick_variant(
                        &[
                            "mountain_A",
                            "mountain_B",
                            "mountain_C",
                            "mountain_A_grass",
                            "mountain_B_grass",
                            "mountain_C_grass_trees",
                        ],
                        col,
                        row,
                    )),
                    Terrain::Hills => Some(pick_variant(
                        &["hills_A", "hills_B", "hills_C", "hills_A_trees", "hills_B_trees", "hills_C_trees"],
                        col,
                        row,
                    )),
                    Terrain::Trees => Some(pick_variant(
                        &["trees_A_large", "trees_B_large", "trees_A_medium", "trees_B_medium"],
                        col,
                        row,
                    )),
                    Terrain::Grain => Some(TileChoice { model: "building_grain", rot: 0 }),
                    _ => None,
                };
                grass
            };

            cells.push(HexCell {
                col,
                row,
                terrain,
                center: hex_center(hex),
                road_mask,
                river_mask,
                water_mask,
                tile,
                decoration,
                bridge,
            });
        }
    }

    WorldMap {
        cols,
        rows,
        cells,
        min_x: -HEX_APOTHEM,
        min_z: -HEX_RADIUS,
        max_x: HEX_WIDTH * f64::from(cols - 1) + HEX_APOTHEM * 2.0,
        max_z: HEX_ROW_PITCH * f64::from(rows - 1) + HEX_RADIUS,
    }
}

static WORLD_MAP: LazyLock<WorldMap> = LazyLock::new(build_map);

pub fn world_map() -> &'static WorldMap {
    &WORLD_MAP
}

pub fn cell_at(col: i32, row: i32) -> Option<&'static HexCell> {
    let map = world_map();
    if col < 0 || row < 0 || col >= map.cols || row >= map.rows {
        return None;
    }
    map.cells.get((row * map.cols + col) as usize)
}

pub fn cell_at_point(x: f64, z: f64) -> Option<&'static HexCell> {
    let hex = hex_at(x, z);
    cell_at(hex.col, hex.row)
}

/// Distance from a point to the centre line of the lines drawn on its hex for `mask`.
fn distance_to_spokes(cell: &HexCell, mask: u8, p: Vec2) -> f64 {
    let mut best = f64::INFINITY;
    for dir in 0..DIR_COUNT {
        if !mask_has(mask, dir) {
            continue;
        }
        let v = dir_vector(dir);
        let edge = vec2(cell.center.x + v.x * HEX_APOTHEM, cell.center.z + v.z * HEX_APOTHEM);
        best = best.min(dist_to_segment(p, cell.center, edge));
    }
    best
}

/// Distance to the middle of the road on this hex; infinite off any road hex.
pub fn road_distance(x: f64, z: f64) -> f64 {
    match cell_at_point(x, z) {
        Some(cell) if cell.road_mask != 0 => distance_to_spokes(cell, cell.road_mask, vec2(x, z)),
        _ => f64::INFINITY,
    }
}

pub fn is_on_road(x: f64, z: f64) -> bool {
    road_distance(x, z) <= ROAD_HALF_WIDTH
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Ground {
    Void,
    Blocked,
    Water,
    Open,
}

const TILE_ROTATION_STEP: f64 = -std::f64::consts::PI / 3.0;
/// Surfaces lower than this (model units) are under water. River beds are flat
/// at -0.1; a beach slopes all the way down to the lake at -0.2.
const RIVER_WATER_LEVEL: f64 = -0.075;
const LAKE_WATER_LEVEL: f64 = -0.16;

/// The bridge model's deck runs between two opposite hex edges, along direction 1
/// in the model's own frame (60° from +x), not along z.
const BRIDGE_MODEL_DIR: u8 = 1;

/// The edge direction the road (and so the bridge) on this hex runs along.
fn bridge_dir(cell: &HexCell) -> u8 {
    (0..DIR_COUNT).find(|&dir| mask_has(cell.road_mask, dir)).unwrap_or(BRIDGE_MODEL_DIR)
}

/// The rotation about y that lines the bridge model on this hex up with the road.
pub fn bridge_rotation(cell: &HexCell) -> f64 {
    f64::from(i32::from(bridge_dir(cell)) - i32::from(BRIDGE_MODEL_DIR)) * TILE_ROTATION_STEP
}

/// A world offset from a hex centre, expressed in the frame of a model rotated by `rotation` about y.
fn to_local(dx: f64, dz: f64, rotation: f64) -> Vec2 {
    let cos = cos(rotation);
    let sin = sin(rotation);
    vec2((dx * cos - dz * sin) / HEX_SCALE, (dx * sin + dz * cos) / HEX_SCALE)
}

fn tile_surface(cell: &HexCell, x: f64, z: f64) -> Option<f64> {
    let local = to_local(x - cell.center.x, z - cell.center.z, f64::from(cell.tile.rot) * TILE_ROTATION_STEP);
    // Right on a hex edge the sample can miss the tile; pull it slightly towards the centre.
    tile_height(cell.tile.model, local.x, local.z)
        .or_else(|| tile_height(cell.tile.model, local.x * 0.93, local.z * 0.93))
}

fn bridge_deck(cell: &HexCell, x: f64, z: f64) -> Option<f64> {
    if !cell.bridge {
        return None;
    }
    let dx = x - cell.center.x;
    let dz = z - cell.center.z;
    // The deck runs along the road, so its width is measured square to the road.
    let along = dir_vector(bridge_dir(cell));
    if (dx * along.z - dz * along.x).abs() / HEX_SCALE > BRIDGE_HALF_WIDTH {
        return None;
    }
    let local = to_local(dx, dz, bridge_rotation(cell));
    bridge_height(local.x, local.z)
}

/// What the terrain itself allows at a point (buildings and props are handled by the nav grid).
pub fn ground_at(x: f64, z: f64) -> Ground {
    let Some(cell) = cell_at_point(x, z) else { return Ground::Void };
    if cell.terrain == Terrain::Water {
        return Ground::Water;
    }
    if cell.terrain.impassable() {
        return Ground::Blocked;
    }
    if bridge_deck(cell, x, z).is_some() {
        return Ground::Open;
    }
    let level = if cell.river_mask != 0 { RIVER_WATER_LEVEL } else { LAKE_WATER_LEVEL };
    match tile_surface(cell, x, z) {
        Some(height) if height < level => Ground::Water,
        _ => Ground::Open,
    }
}

pub fn is_walkable_terrain(x: f64, z: f64) -> bool {
    ground_at(x, z) == Ground::Open
}

/// Height of the ground (or bridge deck) in world units; 0 on plain grass. Used only for drawing.
pub fn ground_height(x: f64, z: f64) -> f64 {
    let Some(cell) = cell_at_point(x, z) else { return 0.0 };
    let surface = tile_surface(cell, x, z).unwrap_or(0.0);
    let deck = bridge_deck(cell, x, z).unwrap_or(f64::NEG_INFINITY);
    surface.max(deck) * HEX_SCALE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_every_hex_with_a_tile() {
        let map = world_map();
        assert_eq!(map.cells.len() as i32, map.cols * map.rows);
        assert!(map.cells.iter().all(|cell| cell.tile.model.starts_with("hex_")));
    }

    #[test]
    fn has_the_bridge_on_the_main_road_and_blocks_the_river_elsewhere() {
        let crossing = cell_at(10, 7).unwrap();
        assert!(crossing.bridge);
        assert_eq!(ground_at(crossing.center.x, crossing.center.z), Ground::Open);
        let river = cell_at(11, 5).unwrap();
        assert_eq!(ground_at(river.center.x, river.center.z), Ground::Water);
        let lake = cell_at(12, 12).unwrap();
        assert_eq!(ground_at(lake.center.x, lake.center.z), Ground::Water);
        let corner = cell_at(0, 0).unwrap();
        assert_eq!(ground_at(corner.center.x, corner.center.z), Ground::Blocked);
        assert_eq!(ground_at(-50.0, -50.0), Ground::Void);
    }
}
