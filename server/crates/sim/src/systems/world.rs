//! The world that changes: what villagers have done to the trees, rocks, props,
//! buildings and ground of the map, and how it grows back. `SimState.objects`
//! holds only what differs from the static map (see [`WorldObject`]); this
//! module owns every change to it, so the walkability overlay (`nav`) is
//! always refreshed with it, and runs the clock that turns a stump into a
//! sapling and a sapling into a tree.
//!
//! Nothing here draws from the RNG, or changes anything at all, while the map
//! is as it was made, so a world nobody has touched runs exactly as before.

use fsw_core::jsmath::{cos, sin};
use fsw_core::math2d::{dist, facing_to};
use fsw_core::rng::hash_string;
use fsw_core::time::{TICKS_PER_DAY, TICKS_PER_GAME_MINUTE, minutes_to_ticks, seconds_to_ticks};
use fsw_core::{Vec2, vec2};
use fsw_world::hex::{Hex, hex_at};
use fsw_world::map::{HexCell, Terrain, cell_at};
use fsw_world::places::{Obstacle, Pad, PadKind, PlaceKind, find_pad, find_place, get_place, pads};
use fsw_world::scatter::{is_rock, is_tree, scatter};

use crate::data::activities::{ActivityKind, activity};
use crate::data::anims::AnimId;
use crate::data::jobs::job as job_def;
use crate::nav;
use crate::state::{
    ActPhase, ActivityId, BuildTarget, BuildingStage, BuiltSpec, CARRY_CAP, Character, ChronicleKind, GroundStage,
    Intent, IntentAction, IntentSource, MemoryKind, Project, ProjectKind, PropStage, Quirk, Resource, RockStage,
    SimState, StimulusKind, Store, TreeStage, WorldJob, WorldObject,
};
use crate::systems::fire;
use crate::systems::intents::{finish, leave_building, travel, travel_to};
use crate::util::{Ctx, first_name, join_names, living, living_ids, place_name_at, set_anim};

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
/// How far from the lumber camp a woodcutter looks for a tree to fell.
pub const FELLING_REACH: f64 = 40.0;
/// How far from the mine a miner looks for a rock.
pub const MINING_REACH: f64 = 45.0;
/// Logs from a tree; one more from a big one.
const WOOD_PER_TREE: i64 = 4;
/// Stone from a rock big enough to walk round (one from a small one), and the ore in every third.
const STONE_PER_ROCK: i64 = 3;
/// How long handing materials over takes.
const DELIVER_SECONDS: f64 = 2.0;
/// If no block is left to finish a job in, this long is allowed.
const DEFAULT_JOB_MINUTES: f64 = 240.0;
/// Work on a tree or rock nobody has touched for this long is lost: the cuts close over.
const ABANDONED_MINUTES: i64 = 30;
/// Grass grows back this long after it is cut, and a pit fills in after this.
pub const MOWN_DAYS: i64 = 3;
pub const DUG_DAYS: i64 = 4;
/// Pieces of a thing taken apart are carried off, and ash blows away, after this.
pub const PIECES_DAYS: i64 = 10;
pub const ASH_DAYS: i64 = 6;
/// Villager-minutes to take a building down to its footings.
pub const BUILDING_WORK: f64 = 600.0;
/// What taking a building apart gives back, and how far a farmer looks for ground to work.
const BUILDING_WOOD: i64 = 5;
const BUILDING_STONE: i64 = 2;
pub const FARMING_REACH: f64 = 30.0;
/// How many things the village builds at once, and what each kind of work takes.
pub const MAX_PROJECTS: usize = 2;
pub const RAISE_WORK: f64 = 1500.0;
pub const RESTORE_WORK: f64 = 60.0;
pub const EXPAND_WORK: f64 = 2400.0;
pub const CONSTRUCT_WORK: f64 = 180.0;
/// Something new is put up when the camp holds at least this much wood.
const CONSTRUCT_WHEN_WOOD: i64 = 30;
/// The village looks over what wants building at this hour each day.
const PROJECT_HOUR: f64 = 6.5;

/// Places whose things are left alone: the dead's, the ruins', and the signpost at the road's end.
const UNTOUCHABLE_PLACES: [&str; 3] = ["graveyard", "ruins", "west_road"];
/// Props nobody takes apart: what gives light, the well, the campfire, and what grows.
const UNTOUCHABLE_MODELS: [&str; 9] = [
    "post_lantern",
    "lantern_standing",
    "torch_lit",
    "Campfire_Base",
    "Campfire_Logs",
    "building_well_blue",
    "dirt_plot",
    "lettuce",
    "carrot",
];

/// Where the village keeps its materials: the place, what it takes, and how much it holds. The market
/// takes anything the others cannot.
pub const STOCKPILES: [(&str, &[Resource], i64); 3] = [
    ("lumber_camp", &[Resource::Wood], 60),
    ("mine", &[Resource::Stone, Resource::Ore], 60),
    ("market", &[Resource::Wood, Resource::Stone, Resource::Ore], 120),
];

/// Where a material is taken to.
pub fn stockpile_for(resource: Resource) -> &'static str {
    STOCKPILES.iter().find(|(_, takes, _)| takes.contains(&resource)).map_or("market", |(place, _, _)| place)
}

/// Whether a place keeps materials.
pub fn is_stockpile(place_id: &str) -> bool {
    STOCKPILES.iter().any(|(place, _, _)| *place == place_id)
}

/// How much of everything the village has put by, over every stockpile.
pub fn village_store(state: &SimState) -> Store {
    let mut all = Store::default();
    for store in state.stockpiles.values() {
        for (resource, count) in store.iter() {
            all.add(resource, count, i64::MAX);
        }
    }
    all
}

/// Puts what someone carries into a stockpile, the overflow into the market, and leaves the rest
/// in their arms. Returns what was put down.
pub fn deposit(state: &mut SimState, place_id: &str, carried: &mut Store) -> Store {
    let mut put = Store::default();
    for target in [place_id, "market"] {
        let Some((_, takes, cap)) = STOCKPILES.iter().find(|(place, _, _)| *place == target) else { continue };
        for resource in Resource::ALL {
            if !takes.contains(&resource) || carried.get(resource) == 0 {
                continue;
            }
            let store = state.stockpiles.entry(target.to_owned()).or_default();
            let room = cap - store.total();
            let moved = store.add(resource, carried.get(resource), room);
            carried.take(resource, moved);
            put.add(resource, moved, i64::MAX);
        }
        if state.stockpiles.get(target).is_some_and(Store::is_empty) {
            state.stockpiles.shift_remove(target);
        }
    }
    put
}

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

/// The id of a hex's ground.
pub fn ground_id(hex: Hex) -> String {
    format!("h:{},{}", hex.col, hex.row)
}

/// The hex an `h:` id names, if it is on the map.
pub fn ground_cell(id: &str) -> Option<&'static HexCell> {
    let rest = id.strip_prefix("h:")?;
    let (col, row) = rest.split_once(',')?;
    cell_at(col.parse().ok()?, row.parse().ok()?)
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
    if let Some(cell) = ground_cell(id) {
        return Some((cell.center.x, cell.center.z));
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
        WorldObject::Ground { stage: GroundStage::Dug, .. } => {
            Footprint { removed: None, added: at(PIT_RADIUS), signature: "pit" }
        }
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

/// Whether a thing of the map may be felled, mined, taken apart or set alight at all.
pub fn destroyable(id: &str) -> bool {
    if let Some(index) = scatter_index(id) {
        return scatter().models.get(index).is_some_and(|m| is_tree(&m.model) || is_rock(&m.model));
    }
    if ground_cell(id).is_some() || id.starts_with("n:") {
        return true;
    }
    let Some((place_id, index)) = place_model(id) else { return false };
    if UNTOUCHABLE_PLACES.contains(&place_id) {
        return false;
    }
    let model = &get_place(place_id).models[index].model;
    // Flowers and mushrooms are dressing, not things.
    !UNTOUCHABLE_MODELS.contains(&model.as_str()) && !model.starts_with("flowers") && !model.starts_with("mushrooms")
}

/// Whether a place's building stands to be used: not taken down, not being raised again. A home that
/// burns or lies in ruins is `houses`' business, not this.
pub fn building_whole(state: &SimState, place_id: &str) -> bool {
    match state.objects.get(&building_id(place_id)) {
        None => true,
        Some(WorldObject::Building { stage: BuildingStage::Whole | BuildingStage::Expanded, .. }) => true,
        Some(_) => false,
    }
}

/// Whether a prop of the map is still there to be worked on.
pub fn prop_whole(state: &SimState, id: &str) -> bool {
    match state.objects.get(id) {
        None => true,
        Some(WorldObject::Prop { stage: PropStage::Whole, .. }) => true,
        Some(WorldObject::Built { stage: PropStage::Whole, .. }) => true,
        Some(_) => false,
    }
}

/// How much grass the map put on a hex.
fn grass_on(cell: &HexCell) -> usize {
    scatter()
        .models
        .iter()
        .filter(|m| m.model.starts_with("Grass"))
        .filter(|m| {
            let hex = hex_at(m.x, m.z);
            hex.col == cell.col && hex.row == cell.row
        })
        .count()
}

/// Ground a spade or a scythe can be taken to: open meadow (or a grain field), no road or river over
/// it, and nothing done to it already.
fn ground_workable(state: &SimState, id: &str, job: WorldJob) -> bool {
    let Some(cell) = ground_cell(id) else { return false };
    if !matches!(cell.terrain, Terrain::Meadow | Terrain::Grain) || cell.road_mask != 0 || cell.river_mask != 0 {
        return false;
    }
    if state.objects.contains_key(id) {
        return false;
    }
    match job {
        WorldJob::Mow => grass_on(cell) > 0,
        _ => state.nav.nearest_walkable(cell.center, 2).is_some(),
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

/// Whether a scatter model is a rock of the map, not yet mined out. Small rocks count too: there is
/// stone in them, if less.
pub fn rock_whole(state: &SimState, index: usize) -> bool {
    let is_map_rock = scatter().models.get(index).is_some_and(|m| is_rock(&m.model));
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
    if ctx.state.objects.is_empty() && ctx.state.projects.is_empty() && ctx.state.stockpiles.is_empty() {
        return;
    }
    let tick = ctx.tick();
    let falling_done = ctx.state.objects.values().any(|o| matches!(o, WorldObject::Tree { stage: TreeStage::Falling, since_tick, .. } if tick >= since_tick + FALL_TICKS));
    if tick % TICKS_PER_GAME_MINUTE != 0 && !falling_done {
        return;
    }
    // A claim on a target outlives its holder only until here.
    let stale: Vec<String> = ctx
        .state
        .slots
        .iter()
        .filter(|(key, holder)| key.contains('@') && !ctx.state.characters.contains_key(*holder))
        .map(|(key, _)| key.clone())
        .collect();
    for key in stale {
        ctx.state.slots.shift_remove(&key);
    }
    if tick >= ctx.state.director.next_project_tick && ctx.time.hour >= PROJECT_HOUR {
        queue_projects(ctx);
        ctx.state.director.next_project_tick = crate::systems::director::tick_at(ctx.time.day + 1, PROJECT_HOUR);
    }
    let ids: Vec<String> = ctx.state.objects.keys().cloned().collect();
    for id in ids {
        let Some(object) = ctx.state.objects.get(&id) else { continue };
        let age = tick - object.since_tick();
        let next = match object {
            WorldObject::Tree { stage: TreeStage::Standing, .. }
            | WorldObject::Rock { stage: RockStage::Whole, .. }
            | WorldObject::Prop { stage: PropStage::Whole, .. }
            | WorldObject::Building { stage: BuildingStage::Whole, .. }
                if age >= ABANDONED_MINUTES * TICKS_PER_GAME_MINUTE && claims_on(ctx.state, &id) == 0 =>
            {
                Some(None)
            }
            WorldObject::Ground { stage: GroundStage::Mown, .. } if age >= MOWN_DAYS * TICKS_PER_DAY => Some(None),
            WorldObject::Ground { stage: GroundStage::Dug, .. } if age >= DUG_DAYS * TICKS_PER_DAY => Some(None),
            WorldObject::Prop { stage: PropStage::Pieces, .. } if age >= PIECES_DAYS * TICKS_PER_DAY => {
                Some(Some(TreeStage::Standing))
            }
            WorldObject::Prop { stage: PropStage::Ash, .. } if age >= ASH_DAYS * TICKS_PER_DAY => {
                Some(Some(TreeStage::Standing))
            }
            WorldObject::Tree { stage, .. } => match stage {
                TreeStage::Falling if age >= FALL_TICKS => Some(Some(TreeStage::Stump)),
                TreeStage::Stump if age >= STUMP_DAYS * TICKS_PER_DAY => Some(Some(TreeStage::Sapling)),
                TreeStage::Sapling if age >= SAPLING_DAYS * TICKS_PER_DAY => Some(Some(TreeStage::Young)),
                TreeStage::Young if age >= YOUNG_DAYS * TICKS_PER_DAY => Some(None),
                _ => None,
            },
            _ => None,
        };
        let prop = matches!(object, WorldObject::Prop { .. });
        match next {
            // Pieces and ash are cleared away: the prop is gone until the village puts up another.
            Some(Some(_)) if prop => {
                let gone = WorldObject::Prop { stage: PropStage::Gone, since_tick: tick, work: 0.0, fire: 0.0 };
                put_object(ctx.state, &id, gone);
            }
            Some(Some(stage)) => put_object(ctx.state, &id, WorldObject::Tree { stage, since_tick: tick, work: 0.0 }),
            Some(None) => drop_object(ctx.state, &id),
            None => {}
        }
    }
}

/// A material gained; what does not fit in the arms is left on the ground and lost.
fn gain(c: &mut Character, resource: Resource, count: i64) -> i64 {
    let room = CARRY_CAP - c.carried.total();
    c.carried.add(resource, count, room)
}

/// Whether a job can be done to its target right now.
pub fn job_accepts(state: &SimState, job: WorldJob, target: &str) -> bool {
    match job {
        WorldJob::Fell => scatter_index(target).is_some_and(|i| tree_standing(state, i)),
        WorldJob::Mine => scatter_index(target).is_some_and(|i| rock_whole(state, i)),
        WorldJob::Deliver => {
            is_stockpile(target) || project(state, target).is_some_and(|p| !p.delivered.has_all(&p.cost))
        }
        WorldJob::Build => project(state, target).is_some_and(|p| p.delivered.has_all(&p.cost)),
        WorldJob::Dismantle => {
            destroyable(target)
                && match place_model(target) {
                    Some((place, 0)) if get_place(place).has_building => {
                        building_whole(state, place) && !state.houses.contains_key(place)
                    }
                    Some(_) => prop_whole(state, target),
                    None => target.starts_with("n:") && prop_whole(state, target),
                }
        }
        WorldJob::Dig | WorldJob::Mow => ground_workable(state, target, job),
        WorldJob::Burn => {
            let building = place_model(target).is_some_and(|(place, i)| i == 0 && get_place(place).has_building);
            let fires = fire::burning_sites(state).len();
            destroyable(target)
                && fire::fire_of(state, target).is_none()
                && fires < fire::MAX_FIRES
                && (!building || fire::alarm_sites(state).len() < fire::MAX_ALARM_FIRES)
                && match place_model(target) {
                    Some((place, 0)) if get_place(place).has_building => {
                        building_whole(state, place) && !state.houses.contains_key(place)
                    }
                    Some(_) => prop_whole(state, target),
                    None => target.starts_with("n:") && prop_whole(state, target),
                }
        }
    }
}

/// The project an id names.
pub fn project<'a>(state: &'a SimState, id: &str) -> Option<&'a Project> {
    state.projects.iter().find(|p| p.id == id)
}

fn project_mut<'a>(state: &'a mut SimState, id: &str) -> Option<&'a mut Project> {
    state.projects.iter_mut().find(|p| p.id == id)
}

/// How many beds a home has: the map's, and one more once it has been made bigger.
pub fn beds_of(state: &SimState, place_id: &str) -> u32 {
    let base = find_place(place_id).map_or(0, |p| p.beds);
    match state.objects.get(&building_id(place_id)) {
        Some(WorldObject::Building { stage: BuildingStage::Expanded, .. }) => base + 1,
        _ => base,
    }
}

/// Every bed in the village.
pub fn beds_total(state: &SimState) -> u32 {
    fsw_world::places::homes().iter().map(|home| beds_of(state, &home.id)).sum()
}

/// "raising the smithy", "putting the bench at the village green back", "making the red cottage bigger",
/// "a fence by the red cottage": what a project is.
pub fn project_label(project: &Project) -> String {
    match project.kind {
        ProjectKind::Raise => format!("raising {}", fire::site_name(&project.target)),
        ProjectKind::Restore => format!("putting {} back", fire::site_name(&project.target)),
        ProjectKind::Expand => format!("making {} bigger", fire::site_name(&project.target)),
        ProjectKind::Construct => match find_pad(&project.target) {
            Some(pad) => format!("{} by {}", pad_kind_name(pad_kind_for(project, pad)), place_name_at(pad.pos)),
            None => "something new".to_owned(),
        },
    }
}

/// The site a project's chronicle lines belong to: the building of the pad's place for something new,
/// else the thing worked on. A finished thing of its own (`n:<n>`) is added when it is put up.
fn project_site(project: &Project) -> String {
    match project.kind {
        ProjectKind::Construct => building_id(project.target.split('/').next().unwrap_or_default()),
        _ => fire::site_id(&project.target),
    }
}

/// Which of a pad's kinds a project puts up: fixed by the project's id, so every machine agrees.
fn pad_kind_for(project: &Project, pad: &Pad) -> PadKind {
    pad.kinds[hash_string(&project.id) as usize % pad.kinds.len()]
}

fn pad_kind_name(kind: PadKind) -> &'static str {
    match kind {
        PadKind::Fence => "a fence",
        PadKind::LogStack => "a stack of logs",
        PadKind::Bench => "a bench",
        PadKind::Crate => "a crate of goods",
        PadKind::Planter => "a bed of flowers",
    }
}

/// What a pad kind is built from and looks like: "bundle/model", scale, footprint radius (0 = walk-through).
fn pad_kind_model(kind: PadKind, salt: u32) -> (&'static str, &'static str, f64, f64) {
    match kind {
        PadKind::Fence => ("custom", "fence_wood", 1.0, 0.0),
        PadKind::LogStack => ("resource", "Wood_Log_Stack", 1.0, 0.7),
        PadKind::Bench => ("halloween", "bench", 1.0, 0.0),
        PadKind::Crate => {
            const CRATES: [&str; 4] = ["crate_carrots", "crate_tomatoes", "crate_cheese", "crate_buns"];
            ("props", CRATES[salt as usize % CRATES.len()], 1.0, 0.5)
        }
        PadKind::Planter => ("custom", "flowers_mixed", 1.5, 0.0),
    }
}

fn pad_kind_cost(kind: PadKind) -> Store {
    let mut cost = Store::default();
    let wood = match kind {
        PadKind::Fence => 4,
        PadKind::LogStack => 6,
        PadKind::Bench => 5,
        PadKind::Crate => 4,
        PadKind::Planter => 2,
    };
    cost.add(Resource::Wood, wood, i64::MAX);
    cost
}

fn cost_of(wood: i64, stone: i64) -> Store {
    let mut cost = Store::default();
    cost.add(Resource::Wood, wood, i64::MAX);
    cost.add(Resource::Stone, stone, i64::MAX);
    cost
}

/// Whether a pad has something standing on it, or going up.
fn pad_taken(state: &SimState, pad_id: &str) -> bool {
    state.objects.values().any(|o| matches!(o, WorldObject::Built { spec, .. } if spec.pad.as_deref() == Some(pad_id)))
        || state.projects.iter().any(|p| p.kind == ProjectKind::Construct && p.target == pad_id)
}

fn new_project(state: &mut SimState, kind: ProjectKind, target: String, cost: Store, need: f64) -> Project {
    let id = state.new_id("j:");
    Project {
        id,
        kind,
        target,
        cost,
        delivered: Store::default(),
        need,
        work: 0.0,
        since_tick: state.tick,
        helpers: Vec::new(),
    }
}

/// Once a day the village looks over what wants building and takes on what it can: a building to
/// raise again first, then a prop to put back, a home to make bigger when beds are short, and
/// something new when the camp is full of timber.
fn queue_projects(ctx: &mut Ctx) {
    let mut wanted: Vec<Project> = Vec::new();
    let taken = |state: &SimState, target: &str| state.projects.iter().any(|p| p.target == target);
    let room = |state: &SimState, wanted: &[Project]| state.projects.len() + wanted.len() < MAX_PROJECTS;
    let ids: Vec<String> = ctx.state.objects.keys().cloned().collect();
    for id in &ids {
        if !room(ctx.state, &wanted) {
            break;
        }
        if taken(ctx.state, id) {
            continue;
        }
        if let Some(WorldObject::Building { stage: BuildingStage::Ruin | BuildingStage::Dismantled, .. }) =
            ctx.state.objects.get(id)
        {
            let project = new_project(ctx.state, ProjectKind::Raise, id.clone(), cost_of(12, 6), RAISE_WORK);
            wanted.push(project);
        }
    }
    for id in &ids {
        if !room(ctx.state, &wanted) {
            break;
        }
        if taken(ctx.state, id) {
            continue;
        }
        if let Some(WorldObject::Prop { stage: PropStage::Gone, .. }) = ctx.state.objects.get(id) {
            let (place, index) = place_model(id).expect("a prop id");
            let stone = is_stone_model(&get_place(place).models[index].model);
            let cost = if stone { cost_of(0, 2) } else { cost_of(2, 0) };
            let project = new_project(ctx.state, ProjectKind::Restore, id.clone(), cost, RESTORE_WORK);
            wanted.push(project);
        }
    }
    // Beds are short: the fullest small cottage is made bigger.
    if room(ctx.state, &wanted)
        && ctx.state.order.len() as u32 + 1 > beds_total(ctx.state)
        && !ctx.state.projects.iter().any(|p| p.kind == ProjectKind::Expand)
    {
        let mut best: Option<(usize, String)> = None;
        for home in fsw_world::places::homes() {
            let small = home.models[0].model.contains("home_A");
            if !small
                || ctx.state.objects.contains_key(&building_id(&home.id))
                || ctx.state.houses.contains_key(&home.id)
            {
                continue;
            }
            let residents = living(ctx.state).filter(|c| c.home_id == home.id).count();
            if best.as_ref().is_none_or(|(n, _)| residents > *n) {
                best = Some((residents, home.id.clone()));
            }
        }
        if let Some((_, home)) = best {
            let project = new_project(ctx.state, ProjectKind::Expand, building_id(&home), cost_of(20, 10), EXPAND_WORK);
            wanted.push(project);
        }
    }
    // Timber to spare: something new goes up on the first free pad.
    if room(ctx.state, &wanted)
        && village_store(ctx.state).get(Resource::Wood) >= CONSTRUCT_WHEN_WOOD
        && !ctx.state.projects.iter().any(|p| p.kind == ProjectKind::Construct)
        && let Some(pad) = pads().into_iter().find(|pad| !pad_taken(ctx.state, &pad.id))
    {
        let mut project =
            new_project(ctx.state, ProjectKind::Construct, pad.id.clone(), Store::default(), CONSTRUCT_WORK);
        project.cost = pad_kind_cost(pad_kind_for(&project, pad));
        wanted.push(project);
    }
    for project in wanted {
        // A building being raised is shown as such from the first day.
        if project.kind == ProjectKind::Raise
            && let Some(WorldObject::Building { since_tick, fire, helpers, .. }) =
                ctx.state.objects.get(&project.target).cloned()
        {
            let raising = WorldObject::Building {
                stage: BuildingStage::Raising { target: BuildTarget::Same },
                since_tick,
                work: 0.0,
                fire,
                helpers,
            };
            put_object(ctx.state, &project.target, raising);
        }
        let line = format!("The village takes on {}.", project_label(&project));
        ctx.chronicle(ChronicleKind::World, line, vec![project_site(&project)]);
        ctx.state.projects.push(project);
    }
}

/// A project is done: the building stands, the prop is back, the home is bigger, the new thing is up.
fn complete_project(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    let Some(index) = ctx.state.projects.iter().position(|p| p.id == id) else { return };
    let project = ctx.state.projects.remove(index);
    let label = project_label(&project);
    let mut sites = vec![project_site(&project)];
    match project.kind {
        ProjectKind::Raise | ProjectKind::Restore => drop_object(ctx.state, &project.target),
        ProjectKind::Expand => {
            let bigger = WorldObject::Building {
                stage: BuildingStage::Expanded,
                since_tick: tick,
                work: 0.0,
                fire: 0.0,
                helpers: project.helpers.clone(),
            };
            put_object(ctx.state, &project.target, bigger);
        }
        ProjectKind::Construct => {
            if let Some(pad) = find_pad(&project.target) {
                let kind = pad_kind_for(&project, pad);
                let (bundle, model, scale, radius) = pad_kind_model(kind, hash_string(&project.id));
                let footprint = (radius > 0.0).then_some(Obstacle::Circle { x: pad.pos.x, z: pad.pos.z, radius });
                let spec = BuiltSpec {
                    bundle: bundle.to_owned(),
                    model: model.to_owned(),
                    x: pad.pos.x,
                    z: pad.pos.z,
                    rotation: pad.rotation,
                    scale,
                    footprint,
                    pad: Some(pad.id.clone()),
                };
                let id_new = ctx.state.new_id("n:");
                let built =
                    WorldObject::Built { stage: PropStage::Whole, since_tick: tick, work: 0.0, fire: 0.0, spec };
                put_object(ctx.state, &id_new, built);
                sites.push(id_new);
            }
        }
    }
    let helpers: Vec<String> = project.helpers.iter().filter(|h| ctx.alive(h)).cloned().collect();
    let names = join_names(&helpers.iter().map(|h| first_name(ctx.cr(h)).to_owned()).collect::<Vec<_>>());
    let names = if names.is_empty() { "the village".to_owned() } else { names };
    let site = capitalised(&fire::site_name(&project.target));
    let line = match project.kind {
        ProjectKind::Raise => format!("{site} stands again, raised by {names}."),
        ProjectKind::Restore => format!("{site} is back where it was, thanks to {names}."),
        ProjectKind::Expand => format!("{site} has a new room; {names} built it."),
        ProjectKind::Construct => format!("{} put up {label}.", capitalised(&names)),
    };
    ctx.chronicle(ChronicleKind::World, line, [helpers.clone(), sites].concat());
    for h in &helpers {
        ctx.remember(h, format!("Helped with {label}."), 5.0, MemoryKind::Event, Vec::new());
    }
    if matches!(project.kind, ProjectKind::Raise | ProjectKind::Expand) {
        for id in living_ids(ctx.state) {
            if !helpers.contains(&id) {
                ctx.stimulate(&id, StimulusKind::Victory, format!("The village finished {label}."), 1, None);
            }
        }
    }
    ctx.emit(crate::events::SimEvent::SavePoint { reason: "built".into() });
}

fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or(String::new(), |first| first.to_uppercase().chain(chars).collect())
}

/// A fire site id for a `Burn` target: a home is known to the fire system by its place id.
fn fire_site(target: &str) -> String {
    match place_model(target) {
        Some((place, 0)) if get_place(place).kind == PlaceKind::Home => place.to_owned(),
        _ => target.to_owned(),
    }
}

/// How long a torch is held to something before it catches.
const LIGHT_SECONDS: f64 = 8.0;

/// What someone standing at `pos` could work on, for the brain to choose from: trees and rocks within
/// reach (the nearest of each), the ground underfoot, and the buildings and props round about, nearest
/// first, at most `limit` entries.
pub struct WorkTarget {
    pub id: String,
    pub jobs: Vec<WorldJob>,
    /// "a tree (3 within reach)", "the bench at the village green"
    pub label: String,
}

pub const TARGET_REACH: f64 = 30.0;

pub fn nearby_targets(state: &SimState, pos: Vec2, limit: usize) -> Vec<WorkTarget> {
    let mut targets = Vec::new();
    for (job, what) in [(WorldJob::Fell, "a tree"), (WorldJob::Mine, "a rock")] {
        let within: Vec<(f64, usize)> = scatter()
            .models
            .iter()
            .enumerate()
            .filter(|(i, _)| match job {
                WorldJob::Fell => tree_standing(state, *i),
                _ => rock_whole(state, *i),
            })
            .map(|(i, m)| (dist(pos, vec2(m.x, m.z)), i))
            .filter(|(d, _)| *d <= TARGET_REACH)
            .collect();
        if let Some((_, nearest)) = within.iter().min_by(|a, b| a.0.total_cmp(&b.0)) {
            let label =
                if within.len() > 1 { format!("{what} ({} within reach)", within.len()) } else { what.to_owned() };
            targets.push(WorkTarget { id: scatter_id(*nearest), jobs: vec![job], label });
        }
    }
    for project in &state.projects {
        if job_accepts(state, WorldJob::Build, &project.id) {
            targets.push(WorkTarget {
                id: project.id.clone(),
                jobs: vec![WorldJob::Build],
                label: project_label(project),
            });
        }
    }
    let here = ground_id(hex_at(pos.x, pos.z));
    let ground: Vec<WorldJob> =
        [WorldJob::Dig, WorldJob::Mow].into_iter().filter(|job| ground_workable(state, &here, *job)).collect();
    if !ground.is_empty() {
        targets.push(WorkTarget { id: here, jobs: ground, label: "the ground here".to_owned() });
    }
    let mut things: Vec<(f64, WorkTarget)> = Vec::new();
    for place in fsw_world::places::places() {
        for (index, model) in place.models.iter().enumerate() {
            let id = if index == 0 && place.has_building { building_id(&place.id) } else { prop_id(&place.id, index) };
            let d = dist(pos, vec2(model.x, model.z));
            if d > TARGET_REACH || !destroyable(&id) {
                continue;
            }
            let jobs: Vec<WorldJob> =
                [WorldJob::Dismantle, WorldJob::Burn].into_iter().filter(|job| job_accepts(state, *job, &id)).collect();
            if jobs.is_empty() {
                continue;
            }
            let label = if index == 0 && place.has_building {
                place.name.to_owned()
            } else {
                format!("{} at {}", prop_name(&model.model), place.name)
            };
            things.push((d, WorkTarget { id, jobs, label }));
        }
    }
    // Buildings before props, nearest first, and one of a kind: three stools are one entry.
    things.sort_by(|a, b| {
        let building = |t: &WorkTarget| !t.id.starts_with("b:");
        building(&a.1).cmp(&building(&b.1)).then_with(|| a.0.total_cmp(&b.0)).then_with(|| a.1.id.cmp(&b.1.id))
    });
    for (_, thing) in things {
        if targets.len() >= limit {
            break;
        }
        if !targets.iter().any(|t| t.label == thing.label) {
            targets.push(thing);
        }
    }
    targets.truncate(limit);
    targets
}

/// The nearest tree or rock to `pos` that the job could be done to, for a villager who asked for one.
pub fn nearest_target(state: &SimState, pos: Vec2, job: WorldJob) -> Option<String> {
    nearby_targets(state, pos, 8).into_iter().find(|t| t.jobs.contains(&job)).map(|t| t.id)
}

/// The ground underfoot if a spade or scythe can be taken to it, else the nearest hex within reach that can.
pub fn ground_near(state: &SimState, pos: Vec2, job: WorldJob) -> Option<String> {
    let here = ground_id(hex_at(pos.x, pos.z));
    if ground_workable(state, &here, job) {
        return Some(here);
    }
    let mut best: Option<(f64, String)> = None;
    for cell in &fsw_world::map::world_map().cells {
        let d = dist(cell.center, pos);
        if d > TARGET_REACH || best.as_ref().is_some_and(|(bd, _)| d >= *bd) {
            continue;
        }
        let id = ground_id(Hex { col: cell.col, row: cell.row });
        if ground_workable(state, &id, job) && claims_on(state, &id) == 0 {
            best = Some((d, id));
        }
    }
    best.map(|(_, id)| id)
}

/// Where a job's target stands, and how far its footprint reaches.
fn target_spot(state: &SimState, job: WorldJob, target: &str) -> Option<(Vec2, f64)> {
    if job == WorldJob::Deliver && is_stockpile(target) {
        return find_place(target).map(|place| (place.door, 0.0));
    }
    if let Some(project) = project(state, target) {
        return match project.kind {
            ProjectKind::Construct => find_pad(&project.target).map(|pad| (pad.pos, 0.6)),
            _ => match place_model(&project.target) {
                Some((place, 0)) if get_place(place).has_building => Some((get_place(place).door, 0.0)),
                _ => target_spot(state, WorldJob::Dismantle, &project.target),
            },
        };
    }
    if let Some(index) = scatter_index(target) {
        let model = scatter().models.get(index)?;
        let radius = match scatter().obstacle_of_model(index) {
            Some(Obstacle::Circle { radius, .. }) => *radius,
            _ => 0.5,
        };
        return Some((vec2(model.x, model.z), radius));
    }
    if let Some(cell) = ground_cell(target) {
        return Some((cell.center, 0.0));
    }
    let object = state.objects.get(target);
    let (x, z) = object.and_then(|o| position_of(target, o)).or_else(|| {
        let (place, index) = place_model(target)?;
        let model = &find_place(place)?.models[index];
        Some((model.x, model.z))
    })?;
    let radius = match static_obstacle(target) {
        Some(Obstacle::Circle { radius, .. }) => *radius,
        Some(Obstacle::Box { half_x, half_z, .. }) => half_x.max(*half_z),
        None => 0.6,
    };
    Some((vec2(x, z), radius))
}

/// Where someone stands to work on a target: round it, at a direction fixed for each person so a
/// pair does not stand on each other's toes, on the nearest walkable ground.
pub fn work_spot(state: &SimState, id: &str, job: WorldJob, target: &str) -> Option<Vec2> {
    let (centre, footprint) = target_spot(state, job, target)?;
    let radius = footprint + job_def(job).reach;
    let first = hash_string(id) % 12;
    for step in 0..12 {
        let angle = f64::from((first + step) % 12) * std::f64::consts::TAU / 12.0;
        let ideal = vec2(centre.x + cos(angle) * radius, centre.z + sin(angle) * radius);
        if let Some(spot) = state.nav.nearest_walkable(ideal, 2)
            && dist(spot, centre) <= radius + 1.6
        {
            return Some(spot);
        }
    }
    None
}

/// How many are at work on a target.
pub fn claims_on(state: &SimState, target: &str) -> usize {
    let prefix = format!("{target}@");
    state.slots.keys().filter(|key| key.starts_with(&prefix)).count()
}

/// Takes a place at a target, in `state.slots` beside the activity slots (`"{target}@{n}"`); false when full.
fn claim(state: &mut SimState, id: &str, target: &str, helpers: usize) -> bool {
    for n in 0..helpers {
        let key = format!("{target}@{n}");
        match state.slots.get(&key) {
            Some(holder) if holder != id => continue,
            _ => {
                state.slots.insert(key.clone(), id.to_owned());
                if let Some(c) = state.characters.get_mut(id) {
                    c.slot_id = Some(key);
                }
                return true;
            }
        }
    }
    false
}

/// The nearest thing a gathering job could be done to, for someone standing at `from`: a standing
/// tree within reach of the lumber camp for `Fell`, a whole rock within reach of the mine for `Mine`.
pub fn gather_target(state: &SimState, from: Vec2, job: WorldJob) -> Option<String> {
    let (place, reach) = match job {
        WorldJob::Fell => ("lumber_camp", FELLING_REACH),
        WorldJob::Mine => ("mine", MINING_REACH),
        _ => return None,
    };
    let origin = get_place(place).origin;
    let helpers = job_def(job).helpers;
    let mut best: Option<(f64, usize)> = None;
    for (index, model) in scatter().models.iter().enumerate() {
        let here = vec2(model.x, model.z);
        if dist(here, origin) > reach {
            continue;
        }
        let fit = match job {
            WorldJob::Fell => tree_standing(state, index),
            _ => rock_whole(state, index),
        };
        if !fit || claims_on(state, &scatter_id(index)) >= helpers {
            continue;
        }
        let d = dist(from, here);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, index));
        }
    }
    best.map(|(_, index)| scatter_id(index))
}

/// The world work a routine work block turns into, if any: woodcutters fell real trees, miners mine
/// real rocks, and anyone with their arms full puts it down first. `None` leaves the block as it is.
pub fn job_for_block(state: &SimState, c: &Character, block: ActivityId, minutes: f64, day: i64) -> Option<Intent> {
    if activity(block).kind != ActivityKind::Work {
        return None;
    }
    let work = |job: WorldJob, target: String, minutes: Option<f64>| {
        Some(Intent::new(IntentAction::WorldWork { job, target, minutes }, IntentSource::Routine))
    };
    // Every other working day a villager's hours go to what the village is building, if it can be worked:
    // materials carried to it from the stores, or hands put to it once they are there.
    if (i64::from(hash_string(&c.id)) + day).rem_euclid(2) == 0 {
        for project in &state.projects {
            if job_accepts(state, WorldJob::Build, &project.id)
                && claims_on(state, &project.id) < job_def(WorldJob::Build).helpers
            {
                return work(WorldJob::Build, project.id.clone(), Some(minutes));
            }
            if job_accepts(state, WorldJob::Deliver, &project.id)
                && (carries_for(&c.carried, project) || stockpile_with(state, project).is_some())
                && claims_on(state, &project.id) < job_def(WorldJob::Deliver).helpers
            {
                return work(WorldJob::Deliver, project.id.clone(), None);
            }
        }
    }
    if let Some((resource, _)) = c.carried.iter().next() {
        return work(WorldJob::Deliver, stockpile_for(resource).to_owned(), None);
    }
    // Every fourth working day a farmer takes a scythe or a spade to the meadows round the farm.
    let turn = i64::from(hash_string(&c.id)) + day;
    if c.jobs.contains(&ActivityId::Farm) && turn.rem_euclid(4) == 0 {
        let job = if turn.rem_euclid(8) == 0 { WorldJob::Dig } else { WorldJob::Mow };
        if let Some(target) = ground_target(state, job) {
            return work(job, target, Some(minutes));
        }
    }
    let job = match block {
        ActivityId::ChopWood => WorldJob::Fell,
        ActivityId::Mine => WorldJob::Mine,
        _ => return None,
    };
    let target = gather_target(state, c.pos, job)?;
    work(job, target, Some(minutes))
}

/// The meadow hex nearest the farm that a spade or scythe could be taken to, in a fixed order.
pub fn ground_target(state: &SimState, job: WorldJob) -> Option<String> {
    let farm = get_place("farm").origin;
    let mut best: Option<(f64, String)> = None;
    for cell in &fsw_world::map::world_map().cells {
        let d = dist(cell.center, farm);
        if d > FARMING_REACH || best.as_ref().is_some_and(|(bd, _)| d >= *bd) {
            continue;
        }
        let id = ground_id(Hex { col: cell.col, row: cell.row });
        if ground_workable(state, &id, job) && claims_on(state, &id) == 0 {
            best = Some((d, id));
        }
    }
    best.map(|(_, id)| id)
}

/// What the work has come to: a sentence for the chronicle of one person's day, and the materials gained.
fn complete_job(ctx: &mut Ctx, id: &str, job: WorldJob, target: &str) -> String {
    let tick = ctx.tick();
    match job {
        WorldJob::Fell => {
            let Some(index) = scatter_index(target) else { return "There was no tree there.".into() };
            let big = scatter().models[index].scale >= 1.1;
            put_object(ctx.state, target, WorldObject::Tree { stage: TreeStage::Falling, since_tick: tick, work: 0.0 });
            let logs = gain(ctx.c(id), Resource::Wood, WOOD_PER_TREE + i64::from(big));
            format!("You felled a tree and have {}.", Resource::Wood.label(logs))
        }
        WorldJob::Mine => {
            let big = static_obstacle(target).is_some();
            put_object(
                ctx.state,
                target,
                WorldObject::Rock { stage: RockStage::MinedOut, since_tick: tick, work: 0.0 },
            );
            let stone = gain(ctx.c(id), Resource::Stone, if big { STONE_PER_ROCK } else { 1 });
            let ore = if hash_string(target).is_multiple_of(3) { gain(ctx.c(id), Resource::Ore, 1) } else { 0 };
            if ore > 0 {
                format!(
                    "You broke up a rock for {} and found {}.",
                    Resource::Stone.label(stone),
                    Resource::Ore.label(ore)
                )
            } else {
                format!("You broke up a rock for {}.", Resource::Stone.label(stone))
            }
        }
        WorldJob::Deliver if project(ctx.state, target).is_some() => {
            let mut carried = std::mem::take(&mut ctx.c(id).carried);
            let mut put = Store::default();
            if let Some(project) = project_mut(ctx.state, target) {
                for (resource, have) in carried.clone().iter() {
                    let lacking = project.cost.get(resource) - project.delivered.get(resource);
                    let moved = carried.take(resource, have.min(lacking.max(0)));
                    project.delivered.add(resource, moved, i64::MAX);
                    put.add(resource, moved, i64::MAX);
                }
                if !project.helpers.iter().any(|h| h == id) {
                    project.helpers.push(id.to_owned());
                }
            }
            ctx.c(id).carried = carried;
            let label = project(ctx.state, target).map(project_label).unwrap_or_default();
            if put.is_empty() {
                format!("The work on {label} needed nothing you carried.")
            } else {
                format!("You brought {} for {label}.", put.describe())
            }
        }
        WorldJob::Deliver => {
            let mut carried = std::mem::take(&mut ctx.c(id).carried);
            let put = deposit(ctx.state, target, &mut carried);
            ctx.c(id).carried = carried;
            let place = get_place(target).name;
            if put.is_empty() {
                format!("There was no room left at {place} for what you carried.")
            } else {
                format!("You put {} down at {place}.", put.describe())
            }
        }
        WorldJob::Build => {
            let label = project(ctx.state, target).map(project_label).unwrap_or_default();
            complete_project(ctx, target);
            format!("You finished {label}.")
        }
        WorldJob::Dismantle => dismantle(ctx, id, target),
        WorldJob::Dig => {
            put_object(ctx.state, target, WorldObject::Ground { stage: GroundStage::Dug, since_tick: tick });
            let stone = if hash_string(target).is_multiple_of(4) { gain(ctx.c(id), Resource::Stone, 1) } else { 0 };
            if stone > 0 {
                format!("You dug a pit and turned up {}.", Resource::Stone.label(stone))
            } else {
                "You dug a pit; nothing but earth.".into()
            }
        }
        WorldJob::Mow => {
            put_object(ctx.state, target, WorldObject::Ground { stage: GroundStage::Mown, since_tick: tick });
            "You cut the grass short.".into()
        }
        WorldJob::Burn => {
            let site = fire_site(target);
            fire::ignite_site(ctx, &site, Some(id));
            format!("You set fire to {}.", fire::site_name(&site))
        }
    }
}

/// Whether a prop is stone rather than wood, by what it is called; what comes of taking it apart.
fn is_stone_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    ["rock", "stone", "anvil", "statue", "font", "pillar", "wall", "grindstone", "iron"]
        .iter()
        .any(|w| lower.contains(w))
}

/// A thing of the map comes apart: a prop into pieces, a building down to its footings. Whoever lived or
/// worked in a building is put out, and the village hears of it.
fn dismantle(ctx: &mut Ctx, id: &str, target: &str) -> String {
    let tick = ctx.tick();
    let Some((place_id, index)) = place_model(target) else {
        // Something the village built: it comes apart into pieces.
        if let Some(WorldObject::Built { spec, .. }) = ctx.state.objects.get(target).cloned() {
            let pieces = WorldObject::Built { stage: PropStage::Pieces, since_tick: tick, work: 0.0, fire: 0.0, spec };
            put_object(ctx.state, target, pieces);
            let wood = gain(ctx.c(id), Resource::Wood, 1);
            return format!("You took it apart and kept {}.", Resource::Wood.label(wood));
        }
        return "There was nothing to take apart.".into();
    };
    let place = get_place(place_id);
    if index == 0 && place.has_building {
        let helpers = match ctx.state.objects.get(target) {
            Some(WorldObject::Building { helpers, .. }) => helpers.clone(),
            _ => vec![id.to_owned()],
        };
        let down = WorldObject::Building {
            stage: BuildingStage::Dismantled,
            since_tick: tick,
            work: 0.0,
            fire: 0.0,
            helpers: helpers.clone(),
        };
        put_object(ctx.state, target, down);
        let wood = gain(ctx.c(id), Resource::Wood, BUILDING_WOOD);
        let stone = gain(ctx.c(id), Resource::Stone, BUILDING_STONE);
        // Anyone inside is put out on the street.
        let inside: Vec<String> =
            living(ctx.state).filter(|c| c.inside.as_deref() == Some(place_id)).map(|c| c.id.clone()).collect();
        let nav = ctx.state.nav.clone();
        for other in &inside {
            let c = ctx.c(other);
            leave_building(&nav, c);
            c.intent = None;
            c.queue.clear();
        }
        let names = join_names(&helpers.iter().map(|h| first_name(ctx.cr(h)).to_owned()).collect::<Vec<_>>());
        let what = if place.kind == PlaceKind::Home { "home" } else { "building" };
        ctx.chronicle(
            ChronicleKind::World,
            format!("{names} took {} apart, down to its footings.", place.name),
            [helpers.clone(), vec![building_id(place_id)]].concat(),
        );
        for other in living_ids(ctx.state) {
            if helpers.contains(&other) {
                continue;
            }
            let mine = ctx.cr(&other).home_id == place_id;
            let text = if mine {
                format!(
                    "{names} took my home, {}, apart. I have nowhere to sleep until it is raised again.",
                    place.name
                )
            } else {
                format!("{names} took {} apart; the {what} is gone until the village raises it again.", place.name)
            };
            ctx.remember(&other, text, if mine { 9.0 } else { 5.0 }, MemoryKind::Event, helpers.clone());
            let urgency = if mine { 2 } else { 1 };
            let text = format!("{names} took {} apart.", place.name);
            ctx.stimulate(&other, StimulusKind::Witness, text, urgency, Some(id));
        }
        ctx.emit(crate::events::SimEvent::SavePoint { reason: "building taken apart".into() });
        return format!(
            "You took {} apart and kept {} and {}.",
            place.name,
            Resource::Wood.label(wood),
            Resource::Stone.label(stone)
        );
    }
    let model = &place.models[index].model;
    let pieces = WorldObject::Prop { stage: PropStage::Pieces, since_tick: tick, work: 0.0, fire: 0.0 };
    put_object(ctx.state, target, pieces);
    let resource = if is_stone_model(model) { Resource::Stone } else { Resource::Wood };
    let got = gain(ctx.c(id), resource, 1);
    format!("You took {} apart and kept {}.", prop_name(model), resource.label(got))
}

/// "the bench", "the small barrel": a prop's model name as words.
pub fn prop_name(model: &str) -> String {
    let words: Vec<String> =
        model.split('_').filter(|w| w.len() > 1 && !["decorated"].contains(w)).map(|w| w.to_lowercase()).collect();
    let words = match words.as_slice() {
        [noun, size] if ["small", "large", "medium", "stacked", "broken"].contains(&size.as_str()) => {
            format!("{size} {noun}")
        }
        _ => words.join(" "),
    };
    format!("the {words}")
}

/// After gathering, what is in the arms goes to a stockpile before anything else.
pub fn deliver_next(c: &mut Character) {
    if let Some((resource, _)) = c.carried.iter().next() {
        let deliver = Intent::new(
            IntentAction::WorldWork {
                job: WorldJob::Deliver,
                target: stockpile_for(resource).to_owned(),
                minutes: None,
            },
            IntentSource::Routine,
        );
        c.queue.insert(0, deliver);
    }
}

/// Carries out a `WorldWork` intent, one tick at a time: walk up to the target, work at it a minute
/// at a time with the right tool and swing, and change the world when the work is done.
pub fn run_world_work(ctx: &mut Ctx, id: &str, job: WorldJob, target: &str, minutes: Option<f64>) {
    let tick = ctx.tick();
    let def = job_def(job);
    match ctx.cr(id).act.phase {
        ActPhase::Start => {
            if !job_accepts(ctx.state, job, target) {
                finish(ctx, id, "There was nothing to be done there.");
                return;
            }
            if job == WorldJob::Deliver && is_stockpile(target) && ctx.cr(id).carried.is_empty() {
                finish(ctx, id, "You had nothing to carry.");
                return;
            }
            if !claim(ctx.state, id, target, def.helpers) {
                finish(ctx, id, "Someone was already at it.");
                return;
            }
            // Carrying to a building site starts at the stockpile that has what it lacks, arms empty.
            if job == WorldJob::Deliver
                && let Some(project) = project(ctx.state, target).cloned()
                && !carries_for(&ctx.cr(id).carried, &project)
            {
                let Some(from) = stockpile_with(ctx.state, &project) else {
                    finish(ctx, id, "The stores have nothing the work needs.");
                    return;
                };
                let Some(spot) = work_spot(ctx.state, id, WorldJob::Deliver, from) else {
                    finish(ctx, id, "You could not get at the stores.");
                    return;
                };
                let nav = ctx.state.nav.clone();
                if !travel_to(tick, &nav, ctx.c(id), spot) {
                    finish(ctx, id, "You could not find a way to the stores.");
                }
                return;
            }
            let Some(spot) = work_spot(ctx.state, id, job, target) else {
                finish(ctx, id, "You could not get at it.");
                return;
            };
            let nav = ctx.state.nav.clone();
            if !travel_to(tick, &nav, ctx.c(id), spot) {
                finish(ctx, id, "You could not find a way there.");
            }
        }
        ActPhase::Travel => {
            if !travel(tick, ctx.c(id), None) {
                return;
            }
            if !job_accepts(ctx.state, job, target) {
                finish(ctx, id, "Someone had finished it before you got there.");
                return;
            }
            let facing = target_spot(ctx.state, job, target).map(|(centre, _)| centre);
            let c = ctx.c(id);
            if let Some(centre) = facing {
                c.facing = facing_to(c.pos, centre);
            }
            c.act.phase = ActPhase::Perform;
            c.act.since_tick = tick;
            c.act.until_tick = tick + minutes_to_ticks(minutes.unwrap_or(DEFAULT_JOB_MINUTES));
            c.tool = def.tool.map(str::to_owned);
            set_anim(c, def.anim, true);
            if job == WorldJob::Deliver {
                c.act.until_tick = tick + seconds_to_ticks(DELIVER_SECONDS);
            }
            if job == WorldJob::Burn {
                c.act.until_tick = tick + seconds_to_ticks(LIGHT_SECONDS);
            }
        }
        ActPhase::Perform => {
            if !job_accepts(ctx.state, job, target) {
                set_anim(ctx.c(id), AnimId::Idle, false);
                finish(ctx, id, "Someone else finished it.");
                return;
            }
            let c = ctx.cr(id);
            let elapsed = tick - c.act.since_tick;
            if job == WorldJob::Deliver || job == WorldJob::Burn {
                if tick >= c.act.until_tick {
                    // At the stores on the way to a building site: fill the arms and set off again.
                    if job == WorldJob::Deliver
                        && let Some(project) = project(ctx.state, target).cloned()
                        && !carries_for(&ctx.cr(id).carried, &project)
                    {
                        take_for(ctx.state, id, &project);
                        let c = ctx.c(id);
                        set_anim(c, AnimId::Idle, false);
                        c.tool = None;
                        if c.carried.is_empty() {
                            finish(ctx, id, "The stores had nothing the work needs.");
                            return;
                        }
                        // The claim is kept; the walk starts over, towards the site this time.
                        c.act = crate::systems::intents::fresh_act(tick);
                        return;
                    }
                    let outcome = complete_job(ctx, id, job, target);
                    set_anim(ctx.c(id), AnimId::Idle, false);
                    finish(ctx, id, outcome);
                }
                return;
            }
            if tick >= c.act.until_tick {
                set_anim(ctx.c(id), AnimId::Idle, false);
                finish(ctx, id, format!("You stopped {} for now.", def.verb));
                deliver_next(ctx.c(id));
                return;
            }
            if elapsed <= 0 || elapsed % TICKS_PER_GAME_MINUTE != 0 {
                return;
            }
            let effort = if c.has(Quirk::Lazy) {
                0.5
            } else if c.has(Quirk::Diligent) {
                1.25
            } else {
                1.0
            };
            // Work on a thing is kept on the thing, shared by all hands; work on the ground is each worker's own.
            let total = match add_work(ctx.state, id, target, effort) {
                Some(total) => total,
                None => (elapsed / TICKS_PER_GAME_MINUTE) as f64 * effort,
            };
            let done = total >= work_needed(ctx.state, job, target);
            if done {
                let outcome = complete_job(ctx, id, job, target);
                set_anim(ctx.c(id), AnimId::Idle, false);
                finish(ctx, id, outcome);
                deliver_next(ctx.c(id));
            }
        }
        ActPhase::Done => {}
    }
}

/// Villager-minutes a job takes on this target: a building takes far more taking apart than a stool.
pub fn work_needed(state: &SimState, job: WorldJob, target: &str) -> f64 {
    if job == WorldJob::Build {
        return project(state, target).map_or(f64::INFINITY, |p| p.need);
    }
    match (job, place_model(target)) {
        (WorldJob::Dismantle, Some((place, 0))) if get_place(place).has_building => BUILDING_WORK,
        _ => job_def(job).work,
    }
}

/// Whether someone's arms hold anything a project still lacks.
fn carries_for(carried: &Store, project: &Project) -> bool {
    carried.iter().any(|(r, _)| project.delivered.get(r) < project.cost.get(r))
}

/// The stockpile holding something a project still lacks, if any.
fn stockpile_with(state: &SimState, project: &Project) -> Option<&'static str> {
    for (place, _, _) in STOCKPILES.iter() {
        let Some(store) = state.stockpiles.get(*place) else { continue };
        if project.cost.iter().any(|(r, n)| project.delivered.get(r) < n && store.get(r) > 0) {
            return Some(place);
        }
    }
    None
}

/// Fills someone's arms from the stockpile they stand at with what a project lacks.
fn take_for(state: &mut SimState, id: &str, project: &Project) {
    let Some(from) = stockpile_with(state, project) else { return };
    for (resource, need) in project.cost.iter() {
        let lacking = need - project.delivered.get(resource);
        if lacking <= 0 {
            continue;
        }
        let room = CARRY_CAP - state.characters[id].carried.total();
        if room <= 0 {
            break;
        }
        let Some(store) = state.stockpiles.get_mut(from) else { break };
        let taken = store.take(resource, lacking.min(room));
        state.characters.get_mut(id).expect("the carrier").carried.add(resource, taken, i64::MAX);
    }
    if state.stockpiles.get(from).is_some_and(Store::is_empty) {
        state.stockpiles.shift_remove(from);
    }
}

/// A minute's work on a target, kept on the object itself so helpers share it; returns the total, or
/// `None` for a target that keeps no record (the ground).
fn add_work(state: &mut SimState, id: &str, target: &str, effort: f64) -> Option<f64> {
    let tick = state.tick;
    if let Some(project) = project_mut(state, target) {
        project.work += effort;
        if !project.helpers.iter().any(|h| h == id) {
            project.helpers.push(id.to_owned());
        }
        let total = project.work;
        // A building being raised shows the work on it.
        let site = project.target.clone();
        if let Some(WorldObject::Building { work, .. }) = state.objects.get_mut(&site) {
            *work = total;
        }
        return Some(total);
    }
    if let Some(object) = state.objects.get_mut(target) {
        if let Some(work) = object.work_mut() {
            *work += effort;
            let total = *work;
            // The last minute anyone worked at it, for the clock and for the swaying on the screen.
            match object {
                WorldObject::Tree { since_tick, .. }
                | WorldObject::Rock { since_tick, .. }
                | WorldObject::Prop { since_tick, .. } => *since_tick = tick,
                WorldObject::Building { since_tick, helpers, .. } => {
                    *since_tick = tick;
                    if !helpers.iter().any(|h| h == id) {
                        helpers.push(id.to_owned());
                    }
                }
                _ => {}
            }
            return Some(total);
        }
        return None;
    }
    // The first swing at a thing of the map starts its record.
    let object = if let Some(index) = scatter_index(target) {
        let model = &scatter().models[index];
        if is_tree(&model.model) {
            WorldObject::Tree { stage: TreeStage::Standing, since_tick: tick, work: effort }
        } else if is_rock(&model.model) {
            WorldObject::Rock { stage: RockStage::Whole, since_tick: tick, work: effort }
        } else {
            return None;
        }
    } else if let Some((place, index)) = place_model(target) {
        if index == 0 && get_place(place).has_building {
            WorldObject::Building {
                stage: BuildingStage::Whole,
                since_tick: tick,
                work: effort,
                fire: 0.0,
                helpers: vec![id.to_owned()],
            }
        } else {
            WorldObject::Prop { stage: PropStage::Whole, since_tick: tick, work: effort, fire: 0.0 }
        }
    } else {
        return None;
    };
    put_object(state, target, object);
    Some(effort)
}

/// How far the work on an object has got, 0..1: the swings at a standing tree against what felling takes,
/// the blows at a rock against mining it. Zero for anything not being worked on.
pub fn progress_of(object: &WorldObject) -> f64 {
    let whole = match object {
        WorldObject::Tree { stage: TreeStage::Standing, .. } => job_def(WorldJob::Fell).work,
        WorldObject::Rock { stage: RockStage::Whole, .. } => job_def(WorldJob::Mine).work,
        WorldObject::Prop { stage: PropStage::Whole, .. } | WorldObject::Built { stage: PropStage::Whole, .. } => {
            job_def(WorldJob::Dismantle).work
        }
        WorldObject::Building { stage: BuildingStage::Whole | BuildingStage::Expanded, .. } => BUILDING_WORK,
        WorldObject::Building { stage: BuildingStage::Raising { target }, .. } => match target {
            BuildTarget::Same => RAISE_WORK,
            BuildTarget::Expand => EXPAND_WORK,
        },
        _ => return 0.0,
    };
    (object.work() / whole).clamp(0.0, 1.0)
}

/// What a thing of the world is called: "a tree near the lumber camp", "the bench at the village green",
/// "The Gilded Tankard", "the ground near the windmill farm".
pub fn object_name(id: &str, object: &WorldObject) -> String {
    match object {
        WorldObject::Tree { .. } | WorldObject::Rock { .. } | WorldObject::Ground { .. } => {
            let what = match object {
                WorldObject::Tree { .. } => "a tree",
                WorldObject::Rock { .. } => "a rock",
                _ => "the ground",
            };
            match position_of(id, object) {
                Some((x, z)) => format!("{what} near {}", place_name_at(vec2(x, z))),
                None => what.to_owned(),
            }
        }
        WorldObject::Prop { .. } | WorldObject::Building { .. } => fire::site_name(id),
        WorldObject::Built { spec, .. } => {
            format!("{} near {}", prop_name(&spec.model), place_name_at(vec2(spec.x, spec.z)))
        }
    }
}

/// "a tree near the lumber camp": what someone is working on, for status lines.
pub fn describe_target(state: &SimState, job: WorldJob, target: &str) -> String {
    let what = match job {
        WorldJob::Fell => "a tree",
        WorldJob::Mine => "a rock",
        WorldJob::Dig => "the ground",
        WorldJob::Mow => "the grass",
        WorldJob::Dismantle | WorldJob::Burn => match place_model(target) {
            Some((place, 0)) if get_place(place).has_building => return get_place(place).name.to_owned(),
            Some((place, index)) => {
                let place = get_place(place);
                return format!("{} at {}", prop_name(&place.models[index].model), place.name);
            }
            None => "something",
        },
        WorldJob::Build | WorldJob::Deliver if project(state, target).is_some() => {
            return project(state, target).map(project_label).unwrap_or_default();
        }
        WorldJob::Build => "something",
        WorldJob::Deliver => return find_place(target).map_or("the stockpile", |p| p.name).to_owned(),
    };
    match target_spot(state, job, target) {
        Some((pos, _)) => format!("{what} near {}", place_name_at(pos)),
        None => what.to_owned(),
    }
}
