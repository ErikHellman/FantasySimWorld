//! Deterministic decoration (trees, bushes, rocks, grass, flowers): the same
//! world every time, independent of the sim seed. Solid pieces become obstacles.

use std::sync::LazyLock;

use fsw_core::math2d::dist_sq;
use fsw_core::rng::{chance, int_range, next_float, pick, range};
use fsw_core::{Vec2, vec2};
use serde::Serialize;

use crate::assets::model_bounds;
use crate::hex::{HEX_APOTHEM, HEX_RADIUS, HEX_SCALE, hex_at};
use crate::layout::SCATTER_SEED;
use crate::map::{Ground, HexCell, Terrain, ground_at, road_distance, world_map};
use crate::places::{Obstacle, PlacedModel, places};

#[derive(Debug, Serialize)]
pub struct Scatter {
    pub models: Vec<PlacedModel>,
    pub obstacles: Vec<Obstacle>,
}

const TREES: &[&str] = &[
    "Tree_1_A", "Tree_1_B", "Tree_1_C", "Tree_2_A", "Tree_2_B", "Tree_2_C", "Tree_2_D", "Tree_3_A", "Tree_3_B",
    "Tree_4_A", "Tree_4_B", "Tree_5_A", "Tree_5_B", "Tree_5_C", "Tree_6_A", "Tree_6_B", "Tree_7_A", "Tree_7_B",
];
const MEADOW_TREES: &[&str] = &["Tree_1_A", "Tree_1_B", "Tree_2_A", "Tree_2_B", "Tree_5_A", "Tree_6_A", "Tree_7_A"];
const BUSHES: &[&str] = &[
    "Bush_1_A", "Bush_1_B", "Bush_1_C", "Bush_2_A", "Bush_2_B", "Bush_3_A", "Bush_3_B", "Bush_4_A", "Bush_4_B",
    "Bush_4_C",
];
const ROCKS: &[&str] = &[
    "Rock_1_A", "Rock_1_B", "Rock_1_C", "Rock_2_A", "Rock_2_B", "Rock_3_A", "Rock_3_B", "Rock_3_C", "Rock_4_A",
    "Rock_5_A", "Rock_6_A",
];
const GRASS: &[&str] =
    &["Grass_1_A", "Grass_1_B", "Grass_1_C", "Grass_1_D", "Grass_2_A", "Grass_2_B", "Grass_2_C", "Grass_2_D"];
const FLOWERS: &[&str] = &["flowers_red", "flowers_yellow", "flowers_violet", "flowers_mixed", "flowers_mixed"];
const MUSHROOMS: &[&str] = &["mushrooms_red", "mushrooms_brown"];
const WATER_PLANTS: &[&str] = &["waterlily_A", "waterlily_B", "waterplant_A", "waterplant_B", "waterplant_C"];

/// Largest footprint side and height of a model, in model units.
fn size_of(bundle: &str, model: &str) -> (f64, f64) {
    let b = model_bounds(bundle, model).unwrap_or_else(|| panic!("Scatter refers to unknown model {bundle}/{model}"));
    ((b[3] - b[0]).max(b[5] - b[2]), b[4] - b[1])
}

fn random_point_in_hex(rng: &mut u32, cell: &HexCell) -> Vec2 {
    loop {
        let x = cell.center.x + range(rng, -HEX_APOTHEM, HEX_APOTHEM);
        let z = cell.center.z + range(rng, -HEX_RADIUS, HEX_RADIUS);
        let hex = hex_at(x, z);
        if hex.col == cell.col && hex.row == cell.row {
            return vec2(x, z);
        }
    }
}

fn near_place(p: Vec2, extra: f64) -> bool {
    places().iter().any(|place| {
        let r = place.clear + extra;
        dist_sq(p, place.origin) < r * r
    })
}

struct Options {
    spacing: f64,
    road_gap: f64,
    solid: bool,
    scale: Option<(f64, f64)>,
    place_gap: f64,
}

const fn opts(spacing: f64, road_gap: f64, solid: bool) -> Options {
    Options { spacing, road_gap, solid, scale: None, place_gap: 0.0 }
}

impl Options {
    const fn scale(mut self, min: f64, max: f64) -> Self {
        self.scale = Some((min, max));
        self
    }
    const fn place_gap(mut self, gap: f64) -> Self {
        self.place_gap = gap;
        self
    }
}

struct Builder {
    rng: u32,
    models: Vec<PlacedModel>,
    obstacles: Vec<Obstacle>,
    solid: Vec<(Vec2, f64)>,
}

impl Builder {
    fn too_close(&self, p: Vec2, radius: f64) -> bool {
        self.solid.iter().any(|&(pos, other)| {
            let r = radius + other;
            dist_sq(p, pos) < r * r
        })
    }

    fn place(&mut self, cell: &HexCell, bundle: &str, list: &[&str], count: i64, options: Options) {
        for _ in 0..count {
            for _attempt in 0..8 {
                let p = random_point_in_hex(&mut self.rng, cell);
                if ground_at(p.x, p.z) != Ground::Open {
                    continue;
                }
                if road_distance(p.x, p.z) < options.road_gap {
                    continue;
                }
                if near_place(p, options.place_gap) {
                    continue;
                }
                if options.solid && self.too_close(p, options.spacing) {
                    continue;
                }
                let model = *pick(&mut self.rng, list);
                let scale = match options.scale {
                    Some((min, max)) => range(&mut self.rng, min, max),
                    None => 1.0,
                };
                let rotation = next_float(&mut self.rng) * std::f64::consts::PI * 2.0;
                self.models.push(PlacedModel {
                    bundle: bundle.into(),
                    model: model.into(),
                    x: p.x,
                    y: 0.0,
                    z: p.z,
                    rotation,
                    scale,
                });
                if options.solid {
                    let (width, height) = size_of(bundle, model);
                    self.solid.push((p, options.spacing));
                    if height * scale > 0.9 {
                        // Trunks are much thinner than the canopy; rocks and bushes block most of their width.
                        let radius = if model.starts_with("Tree") { 0.7 } else { 2.2f64.min(width * scale * 0.4) };
                        self.obstacles.push(Obstacle::Circle { x: p.x, z: p.z, radius });
                    }
                }
                break;
            }
        }
    }
}

fn build_scatter() -> Scatter {
    let mut b = Builder { rng: SCATTER_SEED, models: Vec::new(), obstacles: Vec::new(), solid: Vec::new() };

    for cell in &world_map().cells {
        match cell.terrain {
            Terrain::Forest => {
                let n = int_range(&mut b.rng, 9, 13);
                b.place(cell, "forest", TREES, n, opts(1.7, 3.4, true).scale(0.9, 1.2));
                let n = int_range(&mut b.rng, 3, 6);
                b.place(cell, "forest", BUSHES, n, opts(1.0, 2.6, true));
                let n = int_range(&mut b.rng, 0, 2);
                b.place(cell, "forest", ROCKS, n, opts(1.0, 2.6, true));
                let n = int_range(&mut b.rng, 6, 10);
                b.place(cell, "forest", GRASS, n, opts(0.0, 2.0, false));
                let n = int_range(&mut b.rng, 0, 3);
                b.place(cell, "custom", MUSHROOMS, n, opts(0.0, 2.0, false).scale(1.2, 1.9));
            }
            Terrain::Meadow => {
                if cell.water_mask == 0 {
                    let n = if chance(&mut b.rng, 0.45) { int_range(&mut b.rng, 1, 2) } else { 0 };
                    b.place(cell, "forest", MEADOW_TREES, n, opts(1.7, 4.0, true).place_gap(2.0));
                    let n = int_range(&mut b.rng, 0, 3);
                    b.place(cell, "forest", BUSHES, n, opts(1.0, 2.8, true).place_gap(1.0));
                    let n = int_range(&mut b.rng, 1, 5);
                    b.place(cell, "custom", FLOWERS, n, opts(0.0, 2.2, false).place_gap(-2.0).scale(1.2, 1.8));
                }
                let n = if chance(&mut b.rng, 0.35) { 1 } else { 0 };
                b.place(cell, "forest", ROCKS, n, opts(1.0, 2.8, true).place_gap(1.0));
                let n = int_range(&mut b.rng, 7, 13);
                b.place(cell, "forest", GRASS, n, opts(0.0, 2.0, false).place_gap(-3.0));
            }
            Terrain::Water => {
                let count = if chance(&mut b.rng, 0.5) { int_range(&mut b.rng, 1, 3) } else { 0 };
                for _ in 0..count {
                    let p = random_point_in_hex(&mut b.rng, cell);
                    let model = *pick(&mut b.rng, WATER_PLANTS);
                    let rotation = next_float(&mut b.rng) * std::f64::consts::PI * 2.0;
                    b.models.push(PlacedModel {
                        bundle: "hex".into(),
                        model: model.into(),
                        x: p.x,
                        y: -0.2 * HEX_SCALE,
                        z: p.z,
                        rotation,
                        scale: HEX_SCALE,
                    });
                }
            }
            _ => {}
        }
    }
    Scatter { models: b.models, obstacles: b.obstacles }
}

static SCATTER: LazyLock<Scatter> = LazyLock::new(build_scatter);

pub fn scatter() -> &'static Scatter {
    &SCATTER
}
