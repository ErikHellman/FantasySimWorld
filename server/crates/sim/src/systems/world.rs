//! The world that changes: what villagers have done to the trees, rocks, props,
//! buildings and ground of the map, and how it grows back. `SimState.objects`
//! holds only what differs from the static map (see [`WorldObject`]); this
//! module owns every change to it, so the walkability overlay (`nav`) is
//! always refreshed with it, and runs the clock that turns a stump into a
//! sapling and a sapling into a tree.
//!
//! Nothing here draws from the RNG, or changes anything at all, while the map
//! is as it was made, so a world nobody has touched runs exactly as before.

use fsw_core::time::{TICKS_PER_DAY, TICKS_PER_GAME_MINUTE};
use fsw_world::places::{Obstacle, find_place};
use fsw_world::scatter::{is_rock, is_tree, scatter};

use crate::nav;
use crate::state::{BuildingStage, PropStage, RockStage, SimState, TreeStage, WorldObject};
use crate::util::Ctx;

/// How long a felled tree takes to come down.
pub const FALL_TICKS: i64 = 30;
/// A stump sends up a sapling after this long, ...
pub const STUMP_DAYS: i64 = 2;
/// ... which is a young tree after this long more, ...
pub const SAPLING_DAYS: i64 = 5;
/// ... and the map's own tree again after this.
pub const YOUNG_DAYS: i64 = 6;
/// What is left of a trunk, for walking round; the stump of a young tree blocks the same.
pub const STUMP_RADIUS: f64 = 0.35;
/// What a pit someone dug blocks.
pub const PIT_RADIUS: f64 = 1.0;
/// The rubble of a building taken down.
pub const RUBBLE_RADIUS: f64 = 1.2;

/// The id of a scatter model (a tree, rock or bush of the map).
pub fn scatter_id(index: usize) -> String {
    format!("s:{index}")
}

/// The scatter index an `s:` id names.
pub fn scatter_index(id: &str) -> Option<usize> {
    id.strip_prefix("s:")?.parse().ok()
}

/// The id of a place's building.
pub fn building_id(place_id: &str) -> String {
    format!("b:{place_id}")
}

/// The id of one of a place's props (`models[index]`).
pub fn prop_id(place_id: &str, index: usize) -> String {
    format!("p:{place_id}:{index}")
}

/// The place and model index a `p:` or `b:` id names.
pub fn place_model(id: &str) -> Option<(&str, usize)> {
    if let Some(place) = id.strip_prefix("b:") {
        return find_place(place).filter(|p| p.has_building).map(|_| (place, 0));
    }
    let rest = id.strip_prefix("p:")?;
    let (place, index) = rest.rsplit_once(':')?;
    let index: usize = index.parse().ok()?;
    let p = find_place(place)?;
    (index < p.models.len() && !(index == 0 && p.has_building)).then_some((place, index))
}

/// The static obstacle an object cast when the map was made, if any.
pub fn static_obstacle(id: &str) -> Option<&'static Obstacle> {
    if let Some(index) = scatter_index(id) {
        return scatter().obstacle_of_model(index);
    }
    let (place, index) = place_model(id)?;
    find_place(place)?.obstacle_of_model(index)
}

/// Where an object stands.
pub fn position_of(id: &str, object: &WorldObject) -> Option<(f64, f64)> {
    if let WorldObject::Built { spec, .. } = object {
        return Some((spec.x, spec.z));
    }
    if let Some(index) = scatter_index(id) {
        let model = scatter().models.get(index)?;
        return Some((model.x, model.z));
    }
    let (place, index) = place_model(id)?;
    let model = &find_place(place)?.models[index];
    Some((model.x, model.z))
}

/// How an object bears on walking: the static obstacle it no longer casts, and what it casts instead.
pub struct Footprint {
    pub removed: Option<&'static Obstacle>,
    pub added: Option<Obstacle>,
    /// names the footprint, so a refresh can tell whether anything changed
    pub signature: &'static str,
}

impl Footprint {
    const NONE: Footprint = Footprint { removed: None, added: None, signature: "" };
}

/// What an object does to walkability.
pub fn footprint(id: &str, object: &WorldObject) -> Footprint {
    let at = |radius: f64| position_of(id, object).map(|(x, z)| Obstacle::Circle { x, z, radius });
    match object {
        WorldObject::Tree { stage: TreeStage::Standing | TreeStage::Falling, .. } => Footprint::NONE,
        WorldObject::Tree { .. } => {
            Footprint { removed: static_obstacle(id), added: at(STUMP_RADIUS), signature: "stump" }
        }
        WorldObject::Rock { stage: RockStage::Whole, .. } => Footprint::NONE,
        WorldObject::Rock { .. } => Footprint { removed: static_obstacle(id), added: None, signature: "gone" },
        WorldObject::Prop { stage: PropStage::Whole | PropStage::Burning, .. } => Footprint::NONE,
        WorldObject::Prop { .. } => Footprint { removed: static_obstacle(id), added: None, signature: "gone" },
        WorldObject::Building { stage: BuildingStage::Dismantled, .. } => {
            Footprint { removed: static_obstacle(id), added: at(RUBBLE_RADIUS), signature: "rubble" }
        }
        WorldObject::Building { .. } => Footprint::NONE,
        WorldObject::Ground { stage: crate::state::GroundStage::Dug, .. } => Footprint::NONE,
        WorldObject::Ground { .. } => Footprint::NONE,
        WorldObject::Built { stage: PropStage::Whole | PropStage::Burning, spec, .. } => {
            Footprint { removed: None, added: spec.footprint, signature: "built" }
        }
        WorldObject::Built { .. } => Footprint::NONE,
    }
}

/// Records what became of an object, and keeps the walkability overlay in step.
pub fn put_object(state: &mut SimState, id: &str, object: WorldObject) {
    let before = state.objects.get(id).map(|old| footprint(id, old).signature);
    let after = footprint(id, &object).signature;
    state.objects.insert(id.to_owned(), object);
    if before != Some(after) {
        nav::refresh(state);
    }
}

/// Forgets an object: it is as the map has it again.
pub fn drop_object(state: &mut SimState, id: &str) {
    let Some(old) = state.objects.shift_remove(id) else { return };
    if !footprint(id, &old).signature.is_empty() {
        nav::refresh(state);
    }
}

/// Whether a scatter model is a tree of the map that stands as it was made (or is being felled).
pub fn tree_standing(state: &SimState, index: usize) -> bool {
    let is_map_tree =
        scatter().models.get(index).is_some_and(|m| is_tree(&m.model)) && scatter().obstacle_of_model(index).is_some();
    is_map_tree
        && match state.objects.get(&scatter_id(index)) {
            None => true,
            Some(WorldObject::Tree { stage: TreeStage::Standing, .. }) => true,
            Some(_) => false,
        }
}

/// Whether a scatter model is a solid rock of the map, not yet mined out.
pub fn rock_whole(state: &SimState, index: usize) -> bool {
    let is_map_rock =
        scatter().models.get(index).is_some_and(|m| is_rock(&m.model)) && scatter().obstacle_of_model(index).is_some();
    is_map_rock
        && match state.objects.get(&scatter_id(index)) {
            None => true,
            Some(WorldObject::Rock { stage: RockStage::Whole, .. }) => true,
            Some(_) => false,
        }
}

/// A tree comes down: it leans over for [`FALL_TICKS`], then is a stump. False if it is not a standing tree.
pub fn fell_tree(ctx: &mut Ctx, index: usize) -> bool {
    if !tree_standing(ctx.state, index) {
        return false;
    }
    let tick = ctx.tick();
    put_object(
        ctx.state,
        &scatter_id(index),
        WorldObject::Tree { stage: TreeStage::Falling, since_tick: tick, work: 0.0 },
    );
    true
}

/// The stage clock: once a game minute, whatever has stood long enough moves on.
pub fn update_world(ctx: &mut Ctx) {
    if ctx.state.objects.is_empty() {
        return;
    }
    let tick = ctx.tick();
    let falling_done = ctx.state.objects.values().any(|o| matches!(o, WorldObject::Tree { stage: TreeStage::Falling, since_tick, .. } if tick >= since_tick + FALL_TICKS));
    if tick % TICKS_PER_GAME_MINUTE != 0 && !falling_done {
        return;
    }
    let ids: Vec<String> = ctx.state.objects.keys().cloned().collect();
    for id in ids {
        let Some(object) = ctx.state.objects.get(&id) else { continue };
        let age = tick - object.since_tick();
        let next = match object {
            WorldObject::Tree { stage, .. } => match stage {
                TreeStage::Falling if age >= FALL_TICKS => Some(Some(TreeStage::Stump)),
                TreeStage::Stump if age >= STUMP_DAYS * TICKS_PER_DAY => Some(Some(TreeStage::Sapling)),
                TreeStage::Sapling if age >= SAPLING_DAYS * TICKS_PER_DAY => Some(Some(TreeStage::Young)),
                TreeStage::Young if age >= YOUNG_DAYS * TICKS_PER_DAY => Some(None),
                _ => None,
            },
            _ => None,
        };
        match next {
            Some(Some(stage)) => put_object(ctx.state, &id, WorldObject::Tree { stage, since_tick: tick, work: 0.0 }),
            Some(None) => drop_object(ctx.state, &id),
            None => {}
        }
    }
}
