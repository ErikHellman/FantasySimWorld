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
use fsw_world::places::{Obstacle, find_place, get_place};
use fsw_world::scatter::{is_rock, is_tree, scatter};

use crate::data::activities::{ActivityKind, activity};
use crate::data::anims::AnimId;
use crate::data::jobs::job as job_def;
use crate::nav;
use crate::state::{
    ActPhase, ActivityId, BuildingStage, CARRY_CAP, Character, Intent, IntentAction, IntentSource, PropStage, Quirk,
    Resource, RockStage, SimState, Store, TreeStage, WorldJob, WorldObject,
};
use crate::systems::intents::{finish, travel, travel_to};
use crate::util::{Ctx, place_name_at, set_anim};

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
    if ctx.state.objects.is_empty() {
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
    let ids: Vec<String> = ctx.state.objects.keys().cloned().collect();
    for id in ids {
        let Some(object) = ctx.state.objects.get(&id) else { continue };
        let age = tick - object.since_tick();
        let next = match object {
            WorldObject::Tree { stage: TreeStage::Standing, .. }
            | WorldObject::Rock { stage: RockStage::Whole, .. }
                if age >= ABANDONED_MINUTES * TICKS_PER_GAME_MINUTE && claims_on(ctx.state, &id) == 0 =>
            {
                Some(None)
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
        match next {
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
        WorldJob::Deliver => is_stockpile(target),
        // These come later; nothing accepts them yet.
        WorldJob::Dig | WorldJob::Mow | WorldJob::Dismantle | WorldJob::Burn | WorldJob::Build => false,
    }
}

/// Where a job's target stands, and how far its footprint reaches.
fn target_spot(state: &SimState, job: WorldJob, target: &str) -> Option<(Vec2, f64)> {
    if job == WorldJob::Deliver {
        return find_place(target).map(|place| (place.door, 0.0));
    }
    if let Some(index) = scatter_index(target) {
        let model = scatter().models.get(index)?;
        let radius = match scatter().obstacle_of_model(index) {
            Some(Obstacle::Circle { radius, .. }) => *radius,
            _ => 0.5,
        };
        return Some((vec2(model.x, model.z), radius));
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
pub fn job_for_block(state: &SimState, c: &Character, block: ActivityId, minutes: f64) -> Option<Intent> {
    if activity(block).kind != ActivityKind::Work {
        return None;
    }
    let work = |job: WorldJob, target: String, minutes: Option<f64>| {
        Some(Intent::new(IntentAction::WorldWork { job, target, minutes }, IntentSource::Routine))
    };
    if let Some((resource, _)) = c.carried.iter().next() {
        return work(WorldJob::Deliver, stockpile_for(resource).to_owned(), None);
    }
    let job = match block {
        ActivityId::ChopWood => WorldJob::Fell,
        ActivityId::Mine => WorldJob::Mine,
        _ => return None,
    };
    let target = gather_target(state, c.pos, job)?;
    work(job, target, Some(minutes))
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
        WorldJob::Dig | WorldJob::Mow | WorldJob::Dismantle | WorldJob::Burn | WorldJob::Build => {
            "Nothing came of it.".into()
        }
    }
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
            if job == WorldJob::Deliver && ctx.cr(id).carried.is_empty() {
                finish(ctx, id, "You had nothing to carry.");
                return;
            }
            if !claim(ctx.state, id, target, def.helpers) {
                finish(ctx, id, "Someone was already at it.");
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
        }
        ActPhase::Perform => {
            if !job_accepts(ctx.state, job, target) {
                set_anim(ctx.c(id), AnimId::Idle, false);
                finish(ctx, id, "Someone else finished it.");
                return;
            }
            let c = ctx.cr(id);
            let elapsed = tick - c.act.since_tick;
            if job == WorldJob::Deliver {
                if tick >= c.act.until_tick {
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
            let done = add_work(ctx.state, target, effort) >= def.work;
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

/// A minute's work on a target, kept on the object itself so helpers share it; returns the total.
fn add_work(state: &mut SimState, target: &str, effort: f64) -> f64 {
    let tick = state.tick;
    if let Some(object) = state.objects.get_mut(target) {
        if let Some(work) = object.work_mut() {
            *work += effort;
            let total = *work;
            // The last minute anyone worked at it, for the clock and for the swaying on the screen.
            if let WorldObject::Tree { since_tick, .. } | WorldObject::Rock { since_tick, .. } = object {
                *since_tick = tick;
            }
            return total;
        }
        return 0.0;
    }
    // The first swing at a thing of the map starts its record.
    let Some(index) = scatter_index(target) else { return 0.0 };
    let model = &scatter().models[index];
    let object = if is_tree(&model.model) {
        WorldObject::Tree { stage: TreeStage::Standing, since_tick: tick, work: effort }
    } else if is_rock(&model.model) {
        WorldObject::Rock { stage: RockStage::Whole, since_tick: tick, work: effort }
    } else {
        return 0.0;
    };
    put_object(state, target, object);
    effort
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
        _ => return 0.0,
    };
    (object.work() / whole).clamp(0.0, 1.0)
}

/// "a tree near the lumber camp": what someone is working on, for status lines.
pub fn describe_target(state: &SimState, job: WorldJob, target: &str) -> String {
    let what = match job {
        WorldJob::Fell => "a tree",
        WorldJob::Mine => "a rock",
        WorldJob::Dig => "the ground",
        WorldJob::Mow => "the grass",
        WorldJob::Dismantle | WorldJob::Burn | WorldJob::Build => "something",
        WorldJob::Deliver => return find_place(target).map_or("the stockpile", |p| p.name).to_owned(),
    };
    match target_spot(state, job, target) {
        Some((pos, _)) => format!("{what} near {}", place_name_at(pos)),
        None => what.to_owned(),
    }
}
