//! Ground heights sampled from the hex tile models by the asset build, so the
//! world layer knows where water, sand and road are without any 3D geometry.
//! All values here are in model units (multiply by `HEX_SCALE` for world units).

use std::collections::HashMap;
use std::sync::LazyLock;

use base64::Engine;
use serde::Deserialize;

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
struct Grid {
    nx: usize,
    nz: usize,
    min_x: f64,
    min_z: f64,
    max_x: f64,
    max_z: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
struct TileHeightsFile {
    tile_grid: Grid,
    bridge_grid: Grid,
    no_ground: i8,
    bridge_heights: String,
    tile_heights: HashMap<String, String>,
}

struct TileHeights {
    tile_grid: Grid,
    bridge_grid: Grid,
    no_ground: i8,
    bridge: Vec<i8>,
    tiles: HashMap<String, Vec<i8>>,
}

fn decode(base64: &str) -> Vec<i8> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(base64).expect("tile heights are base64");
    bytes.into_iter().map(|b| b as i8).collect()
}

static HEIGHTS: LazyLock<TileHeights> = LazyLock::new(|| {
    let file: TileHeightsFile =
        serde_json::from_str(include_str!("../../../data/tile-heights.json")).expect("server/data/tile-heights.json");
    TileHeights {
        tile_grid: file.tile_grid,
        bridge_grid: file.bridge_grid,
        no_ground: file.no_ground,
        bridge: decode(&file.bridge_heights),
        tiles: file.tile_heights.iter().map(|(model, data)| (model.clone(), decode(data))).collect(),
    }
});

fn sample(data: &[i8], grid: &Grid, x: f64, z: f64) -> Option<f64> {
    let ix = (((x - grid.min_x) / (grid.max_x - grid.min_x)) * grid.nx as f64).floor();
    let iz = (((z - grid.min_z) / (grid.max_z - grid.min_z)) * grid.nz as f64).floor();
    // NaN fails both comparisons and is off the tile too.
    if !(ix >= 0.0 && iz >= 0.0 && ix < grid.nx as f64 && iz < grid.nz as f64) {
        return None;
    }
    let value = data[iz as usize * grid.nx + ix as usize];
    if value == HEIGHTS.no_ground { None } else { Some(f64::from(value) / 100.0) }
}

/// Height of a tile's surface at a point in the tile's own (unrotated) frame; `None` off the tile.
pub fn tile_height(model: &str, x: f64, z: f64) -> Option<f64> {
    let heights = &*HEIGHTS;
    sample(heights.tiles.get(model)?, &heights.tile_grid, x, z)
}

/// Height of the bridge deck at a point in the bridge model's own frame; `None` off the bridge.
pub fn bridge_height(x: f64, z: f64) -> Option<f64> {
    let heights = &*HEIGHTS;
    sample(&heights.bridge, &heights.bridge_grid, x, z)
}

/// Half the walkable width of the bridge deck, in model units.
pub const BRIDGE_HALF_WIDTH: f64 = 0.42;
