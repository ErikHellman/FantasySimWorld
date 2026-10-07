//! Places resolved into world space: models, obstacles, activity slots, doors.

use std::collections::HashMap;
use std::sync::LazyLock;

use fsw_core::jsmath::{cos, js_hypot, sin};
use fsw_core::math2d::rotate;
use fsw_core::{Vec2, vec2};
use serde::{Deserialize, Serialize};

use crate::assets::model_bounds;
use crate::hex::{HEX_SCALE, hex, hex_center};
pub use crate::place_defs::{
    ActivityId, BILLBOARD_DEPTH, BILLBOARD_HEIGHT, BILLBOARD_LIFT, BILLBOARD_READ_AT, BILLBOARD_WIDTH, PadKind,
    PlaceKind,
};
use crate::place_defs::{PlaceDef, place_defs};

const DEG: f64 = std::f64::consts::PI / 180.0;

#[derive(Clone, Debug, Serialize)]
pub struct PlacedModel {
    pub bundle: String,
    pub model: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// rotation about y, radians
    pub rotation: f64,
    pub scale: f64,
}

/// A solid thing characters walk around: a rotated box or a circle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum Obstacle {
    Box { x: f64, z: f64, half_x: f64, half_z: f64, rotation: f64 },
    Circle { x: f64, z: f64, radius: f64 },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub id: String,
    pub place_id: String,
    pub activities: Vec<ActivityId>,
    pub pos: Vec2,
    pub facing: f64,
    pub anim: Option<&'static str>,
    pub ranged: bool,
}

/// A spot where the village may build something small, in world space.
#[derive(Clone, Debug)]
pub struct Pad {
    /// `"{place}/{index}"`
    pub id: String,
    pub pos: Vec2,
    /// radians, as a placed model's rotation
    pub rotation: f64,
    pub kinds: Vec<PadKind>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct GravePlot {
    pub pos: Vec2,
    pub facing: f64,
}

/// A billboard in world space, with the two slots people stand in to read it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Billboard {
    pub id: String,
    pub place_id: String,
    /// the middle of the board, on the ground
    pub pos: Vec2,
    /// the direction its face looks, radians
    pub facing: f64,
    pub slot_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub id: String,
    pub name: &'static str,
    pub kind: PlaceKind,
    pub description: &'static str,
    pub origin: Vec2,
    pub facing: f64,
    /// where characters enter and leave, and where "go to this place" ends
    pub door: Vec2,
    pub beds: u32,
    pub clear: f64,
    pub models: Vec<PlacedModel>,
    pub obstacles: Vec<Obstacle>,
    pub slots: Vec<Slot>,
    pub grave_plots: Vec<GravePlot>,
    pub spawn_points: Vec<Vec2>,
    pub billboards: Vec<Billboard>,
    /// For each model, the index of its obstacle in `obstacles`, or `None` for one that blocks nothing
    /// (billboards have obstacles but no model here). Not part of the golden dump.
    #[serde(skip)]
    pub obstacle_of: Vec<Option<usize>>,
    /// Whether `models[0]` is the place's building (the rest are props). Not part of the golden dump.
    #[serde(skip)]
    pub has_building: bool,
    /// where the village may build small things. Not part of the golden dump.
    #[serde(skip)]
    pub pads: Vec<Pad>,
}

impl Place {
    /// The obstacle one of this place's models casts, if it is solid.
    pub fn obstacle_of_model(&self, index: usize) -> Option<&Obstacle> {
        self.obstacle_of.get(index).copied().flatten().map(|i| &self.obstacles[i])
    }
}

fn bounds_of(reference: &str) -> (String, String, [f64; 6]) {
    let (bundle, model) = reference.split_once('/').unwrap_or((reference, ""));
    let bounds = model_bounds(bundle, model).unwrap_or_else(|| panic!("Place refers to unknown model {reference}"));
    (bundle.to_owned(), model.to_owned(), *bounds)
}

fn resolve(def: &PlaceDef) -> Place {
    let centre = hex_center(hex(def.hex[0], def.hex[1]));
    let offset = def.offset.unwrap_or([0.0, 0.0]);
    let origin = vec2(centre.x + offset[0], centre.z + offset[1]);
    let facing = def.facing * DEG;
    let to_world = |local: [f64; 2]| {
        let r = rotate(vec2(local[0], local[1]), facing);
        vec2(origin.x + r.x, origin.z + r.z)
    };

    let mut models = Vec::new();
    let mut obstacles = Vec::new();
    let mut obstacle_of = Vec::new();

    if let Some(building) = &def.building {
        let (bundle, model, b) = bounds_of(&building.model);
        let scale = building.scale.unwrap_or(HEX_SCALE);
        let at = building.at.unwrap_or([0.0, 0.0]);
        let rotation = facing + building.rot.unwrap_or(0.0) * DEG;
        // The footprint box is centred on the model's bounds, which are not always centred on its origin.
        let cx = ((b[0] + b[3]) / 2.0) * scale;
        let cz = ((b[2] + b[5]) / 2.0) * scale;
        let pos = to_world(at);
        let centre_offset = rotate(vec2(cx, cz), rotation);
        models.push(PlacedModel { bundle, model, x: pos.x, y: 0.0, z: pos.z, rotation, scale });
        obstacle_of.push(Some(obstacles.len()));
        obstacles.push(Obstacle::Box {
            x: pos.x + centre_offset.x,
            z: pos.z + centre_offset.z,
            half_x: ((b[3] - b[0]) / 2.0) * scale * 0.94,
            half_z: ((b[5] - b[2]) / 2.0) * scale * 0.94,
            rotation,
        });
    }

    for prop in &def.props {
        let (bundle, model, b) = bounds_of(prop.model);
        let scale = prop.scale.unwrap_or(1.0);
        let pos = to_world(prop.at);
        let rotation = facing + prop.rot.unwrap_or(0.0) * DEG;
        models.push(PlacedModel { bundle, model, x: pos.x, y: prop.y.unwrap_or(0.0), z: pos.z, rotation, scale });
        let width = (b[3] - b[0]) * scale;
        let depth = (b[5] - b[2]) * scale;
        let height = (b[4] - b[1]) * scale;
        let before = obstacles.len();
        if let Some(block) = prop.block {
            if block > 0.0 {
                obstacles.push(Obstacle::Circle { x: pos.x, z: pos.z, radius: block });
            }
        } else if height >= 0.6 && width.max(depth) >= 0.9 {
            if width.max(depth) > 2.4 {
                let c = rotate(vec2(((b[0] + b[3]) / 2.0) * scale, ((b[2] + b[5]) / 2.0) * scale), rotation);
                obstacles.push(Obstacle::Box {
                    x: pos.x + c.x,
                    z: pos.z + c.z,
                    half_x: width / 2.0,
                    half_z: depth / 2.0,
                    rotation,
                });
            } else {
                obstacles.push(Obstacle::Circle { x: pos.x, z: pos.z, radius: width.max(depth) * 0.45 });
            }
        }
        obstacle_of.push((obstacles.len() > before).then_some(before));
    }

    for board in &def.billboards {
        let pos = to_world(board.at);
        obstacles.push(Obstacle::Box {
            x: pos.x,
            z: pos.z,
            half_x: BILLBOARD_WIDTH / 2.0,
            half_z: BILLBOARD_DEPTH / 2.0,
            rotation: facing + board.rot * DEG,
        });
    }

    let mut slots: Vec<Slot> = def
        .slots
        .iter()
        .enumerate()
        .map(|(index, slot)| Slot {
            id: format!("{}#{index}", def.id),
            place_id: def.id.clone(),
            activities: slot.activities.clone(),
            pos: to_world(slot.at),
            facing: facing + slot.face * DEG,
            anim: slot.anim,
            ranged: slot.ranged,
        })
        .collect();

    // Reading spots come after the authored slots, so the ids saves refer to stay the same.
    let mut billboards = Vec::new();
    for board in &def.billboards {
        let turn = board.rot * DEG;
        let mut slot_ids = Vec::new();
        for side in [-1.0, 1.0] {
            let spot = rotate(vec2(side * BILLBOARD_READ_AT[0], BILLBOARD_READ_AT[1]), turn);
            let id = format!("{}#{}", def.id, slots.len());
            slot_ids.push(id.clone());
            slots.push(Slot {
                id,
                place_id: def.id.clone(),
                activities: vec![ActivityId::ReadBoard],
                pos: to_world([board.at[0] + spot.x, board.at[1] + spot.z]),
                facing: facing + (board.rot + 180.0) * DEG,
                anim: None,
                ranged: false,
            });
        }
        billboards.push(Billboard {
            id: board.id.to_owned(),
            place_id: def.id.clone(),
            pos: to_world(board.at),
            facing: facing + turn,
            slot_ids,
        });
    }

    Place {
        id: def.id.clone(),
        name: def.name,
        kind: def.kind,
        description: def.description,
        origin,
        facing,
        door: to_world(def.door.unwrap_or([0.0, 0.0])),
        beds: def.beds.unwrap_or(0),
        clear: def.clear.unwrap_or(8.0),
        models,
        obstacles,
        slots,
        grave_plots: def.grave_plots.iter().map(|&p| GravePlot { pos: to_world(p), facing }).collect(),
        spawn_points: def.spawn_points.iter().map(|&p| to_world(p)).collect(),
        billboards,
        obstacle_of,
        has_building: def.building.is_some(),
        pads: def
            .pads
            .iter()
            .enumerate()
            .map(|(index, pad)| Pad {
                id: format!("{}/{index}", def.id),
                pos: to_world(pad.at),
                rotation: facing + pad.rot * DEG,
                kinds: pad.kinds.to_vec(),
            })
            .collect(),
    }
}

/// Every pad in the village, in place order.
pub fn pads() -> Vec<&'static Pad> {
    places().iter().flat_map(|place| place.pads.iter()).collect()
}

pub fn find_pad(id: &str) -> Option<&'static Pad> {
    pads().into_iter().find(|pad| pad.id == id)
}

struct Index {
    by_id: HashMap<String, usize>,
    slots: HashMap<String, (usize, usize)>,
    homes: Vec<&'static Place>,
    billboards: Vec<&'static Billboard>,
}

static PLACES: LazyLock<Vec<Place>> = LazyLock::new(|| place_defs().iter().map(resolve).collect());

static INDEX: LazyLock<Index> = LazyLock::new(|| {
    let list: &'static [Place] = &PLACES;
    let by_id = list.iter().enumerate().map(|(i, place)| (place.id.clone(), i)).collect();
    let slots = list
        .iter()
        .enumerate()
        .flat_map(|(p, place)| place.slots.iter().enumerate().map(move |(s, slot)| (slot.id.clone(), (p, s))))
        .collect();
    let homes = list.iter().filter(|place| place.kind == PlaceKind::Home).collect();
    let billboards = list.iter().flat_map(|place| place.billboards.iter()).collect();
    Index { by_id, slots, homes, billboards }
});

/// Every place, in definition order.
pub fn places() -> &'static [Place] {
    &PLACES
}

pub fn find_place(id: &str) -> Option<&'static Place> {
    INDEX.by_id.get(id).map(|&i| &PLACES[i])
}

pub fn get_place(id: &str) -> &'static Place {
    find_place(id).unwrap_or_else(|| panic!("Unknown place {id}"))
}

pub fn find_slot(id: &str) -> Option<&'static Slot> {
    INDEX.slots.get(id).map(|&(p, s)| &PLACES[p].slots[s])
}

pub fn get_slot(id: &str) -> &'static Slot {
    find_slot(id).unwrap_or_else(|| panic!("Unknown slot {id}"))
}

/// Every slot that offers `activity`, in place order.
pub fn slots_for(activity: ActivityId) -> Vec<&'static Slot> {
    places().iter().flat_map(|place| place.slots.iter().filter(|slot| slot.activities.contains(&activity))).collect()
}

pub fn homes() -> &'static [&'static Place] {
    &INDEX.homes
}

/// Every billboard, in place order. A board's index here is fixed by the map.
pub fn billboards() -> &'static [&'static Billboard] {
    &INDEX.billboards
}

pub fn find_billboard(id: &str) -> Option<&'static Billboard> {
    billboards().iter().copied().find(|board| board.id == id)
}

/// The place whose origin is nearest to a point, within `max_distance` (14 in the TypeScript default).
pub fn nearest_place(pos: Vec2, max_distance: f64) -> Option<&'static Place> {
    let mut best = None;
    let mut best_distance = max_distance;
    for place in places() {
        let d = js_hypot(place.origin.x - pos.x, place.origin.z - pos.z);
        if d < best_distance {
            best_distance = d;
            best = Some(place);
        }
    }
    best
}

pub fn obstacle_contains(obstacle: &Obstacle, x: f64, z: f64, margin: f64) -> bool {
    match *obstacle {
        Obstacle::Circle { x: ox, z: oz, radius } => {
            let r = radius + margin;
            let dx = x - ox;
            let dz = z - oz;
            dx * dx + dz * dz <= r * r
        }
        Obstacle::Box { x: ox, z: oz, half_x, half_z, rotation } => {
            // Into the box frame: the inverse of rotate().
            let dx = x - ox;
            let dz = z - oz;
            let cos = cos(rotation);
            let sin = sin(rotation);
            let lx = dx * cos - dz * sin;
            let lz = dx * sin + dz * cos;
            lx.abs() <= half_x + margin && lz.abs() <= half_z + margin
        }
    }
}

impl Obstacle {
    pub fn x(&self) -> f64 {
        match *self {
            Obstacle::Box { x, .. } | Obstacle::Circle { x, .. } => x,
        }
    }

    pub fn z(&self) -> f64 {
        match *self {
            Obstacle::Box { z, .. } | Obstacle::Circle { z, .. } => z,
        }
    }
}
