//! House fires. When the whole village is asleep the dead put a torch to a
//! home; everyone is woken to carry water, and a house they cannot save burns
//! down, leaving those who lived there to sleep rough until it is rebuilt.
//!
//! Nothing here draws from the RNG, or changes anything at all, until a fire
//! is possible, so a world that never has one runs exactly as it did before.

use fsw_core::jsmath::{cos, sin};
use fsw_core::math2d::{dist, facing_to};
use fsw_core::rng::hash_string;
use fsw_core::time::{TICKS_PER_DAY, TICKS_PER_GAME_MINUTE, minutes_to_ticks, seconds_to_ticks};
use fsw_core::{Vec2, vec2};
use fsw_world::places::{Place, find_place, get_place, homes};

use crate::data::anims::AnimId;
use crate::data::classes::RUN_SPEED;
use crate::events::{Emote, SimEvent};
use crate::nav::NavOverlay;
use crate::state::{
    ActPhase, Arson, Character, ChronicleKind, House, HousePhase, Id, Intent, IntentAction, IntentSource, MemoryKind,
    MonsterPhase, SimState, StimulusKind,
};
use crate::systems::combat::max_hp;
use crate::systems::death::{detach, kill_character};
use crate::systems::intents::{finish, fresh_act, leave_building, travel, travel_to};
use crate::systems::movement::{advance, face, route_to};
use crate::systems::relationships::{RelationDelta, adjust};
use crate::systems::world::building_whole;
use crate::util::{
    Ctx, bump_epoch, first_name, has_recent_stimulus, is_fighting, join_names, living, living_ids, set_anim,
};

/// After a fire the dead leave the houses alone this long.
const COOLDOWN_TICKS: i64 = 6 * TICKS_PER_DAY;
/// How long a monster works at the door before the house catches.
const LIGHT_SECONDS: f64 = 4.0;
/// How far the fire has taken hold when it is discovered.
const START_FIRE: f64 = 20.0;
/// How much the fire grows each game minute, whatever anyone does: a steady part, and a part that rises
/// with the fire itself, so a blaze that is not being beaten back soon gets out of hand.
const GROWTH_PER_MINUTE: f64 = 1.0;
const GROWTH_PER_FIRE: f64 = 0.02;
/// How much one bucket of water knocks it back.
const BUCKET: f64 = 3.0;
const FILL_SECONDS: f64 = 2.0;
const THROW_SECONDS: f64 = 1.2;
/// Damage to those thrown out of a house as it catches.
const ESCAPE_BURN: [i64; 2] = [6, 14];
/// The chance that throwing a bucket costs a scorched arm, and what it costs.
const SCORCH_CHANCE: f64 = 0.05;
const SCORCH: [i64; 2] = [2, 5];
/// Too badly hurt to carry water.
const UNFIT: f64 = 0.25;
/// Villager-minutes of work that raise a house again.
pub const REBUILD_WORK: f64 = 6000.0;
/// What water is carried in.
const BUCKET_TOOL: &str = "tools/bucket_metal";
/// Where water is drawn: the well on the green and the lake shore.
const WATER: [&str; 2] = ["green", "docks"];
/// Where those without a roof sleep.
const ROUGH_PLACE: &str = "glade";
/// Where those without a home hide from danger.
const SHELTER: &str = "tavern";

/// Whether this character's home can be slept in: it is neither burning nor in ruins, and nobody has
/// taken it down.
pub fn home_usable(state: &SimState, c: &Character) -> bool {
    !state.houses.contains_key(&c.home_id) && building_whole(state, &c.home_id)
}

/// Whether this character's home has burned down or been taken apart, and is not yet raised again.
pub fn is_homeless(state: &SimState, c: &Character) -> bool {
    state.houses.get(&c.home_id).is_some_and(|house| house.phase == HousePhase::Ruined)
        || !building_whole(state, &c.home_id)
}

/// The house on fire right now, if there is one.
pub fn burning(state: &SimState) -> Option<&str> {
    state.houses.iter().find(|(_, house)| house.phase == HousePhase::Burning).map(|(id, _)| id.as_str())
}

/// The first house lying in ruins, if there is one.
pub fn ruin(state: &SimState) -> Option<&str> {
    state.houses.iter().find(|(_, house)| house.phase == HousePhase::Ruined).map(|(id, _)| id.as_str())
}

/// The ruin this character's working hours go to today, if any: every day for
/// those who lived there, every other day for everyone else, so the fields are
/// still tended.
pub fn ruin_to_rebuild(state: &SimState, c: &Character, day: i64) -> Option<String> {
    let place = ruin(state)?;
    let turn = (i64::from(hash_string(&c.id)) + day) % 2 == 0;
    (c.home_id == place || turn).then(|| place.to_owned())
}

/// Where a character runs to hide: home, or the tavern for those without one.
pub fn shelter_of(state: &SimState, c: &Character) -> &'static Place {
    if home_usable(state, c) { get_place(&c.home_id) } else { get_place(SHELTER) }
}

/// A spot on the ground near the campfire for someone with no bed, fixed for each person.
pub fn rough_spot(nav: &NavOverlay, c: &Character) -> Vec2 {
    let place = get_place(ROUGH_PLACE);
    ring(nav, place.door, &c.id, 5.0).unwrap_or(place.door)
}

/// A walkable point `radius` out from `centre`, in a direction fixed for each person, so a crowd spreads out.
fn ring(nav: &NavOverlay, centre: Vec2, id: &str, radius: f64) -> Option<Vec2> {
    let angle = f64::from(hash_string(id) % 12) * std::f64::consts::TAU / 12.0;
    nav.nearest_walkable(vec2(centre.x + cos(angle) * radius, centre.z + sin(angle) * radius), 5)
}

/// The water nearest a house.
fn water_for(house: &Place) -> &'static Place {
    let mut best = get_place(WATER[0]);
    for id in WATER {
        let place = get_place(id);
        if dist(place.origin, house.door) < dist(best.origin, house.door) {
            best = place;
        }
    }
    best
}

fn residents(state: &SimState, place_id: &str) -> Vec<Id> {
    living(state).filter(|c| c.home_id == place_id).map(|c| c.id.clone()).collect()
}

fn names(state: &SimState, ids: &[Id]) -> String {
    let names: Vec<&str> = ids.iter().filter_map(|id| state.characters.get(id)).map(first_name).collect();
    join_names(&names)
}

/// Whether the dead may set a fire now: it is dark, nothing is burning, the last
/// fire was a while ago, and not one villager is awake.
fn may_torch(ctx: &Ctx) -> bool {
    let hour = ctx.time.hour;
    let director = &ctx.state.director;
    !(5.6..19.4).contains(&hour)
        && burning(ctx.state).is_none()
        && (director.last_fire_tick == 0 || ctx.tick() - director.last_fire_tick >= COOLDOWN_TICKS)
        && !ctx.state.order.is_empty()
        && living(ctx.state).all(|c| c.asleep)
}

/// The nearest home that still stands and has someone living in it.
fn house_to_torch(state: &SimState, from: Vec2) -> Option<String> {
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    for home in homes() {
        if state.houses.contains_key(&home.id) || !living(state).any(|c| c.home_id == home.id) {
            continue;
        }
        let d = dist(home.door, from);
        if d < best_distance {
            best_distance = d;
            best = Some(home.id.clone());
        }
    }
    best
}

/// A roaming monster's turn at arson. With the whole village asleep it picks a
/// house, walks to the door and sets it alight; the plan is dropped the moment
/// anyone wakes. True when the monster spent this tick on it.
pub fn arson(ctx: &mut Ctx, id: &str, check: bool) -> bool {
    let tick = ctx.tick();
    let monster = &ctx.state.monsters[id];
    if monster.phase != MonsterPhase::Roam {
        if monster.arson.is_some() {
            ctx.m(id).arson = None;
        }
        return false;
    }
    if monster.arson.is_none() {
        if !check || !may_torch(ctx) {
            return false;
        }
        let Some(place_id) = house_to_torch(ctx.state, monster.pos) else { return false };
        let monster = ctx.m(id);
        monster.arson = Some(Arson { place_id, light_tick: 0 });
        monster.path.clear();
        monster.repath_tick = 0;
    }
    let plan = ctx.state.monsters[id].arson.clone().expect("a plan was just made");
    if !may_torch(ctx) || ctx.state.houses.contains_key(&plan.place_id) {
        let monster = ctx.m(id);
        monster.arson = None;
        monster.path.clear();
        return false;
    }

    let house = get_place(&plan.place_id);
    let nav = ctx.state.nav.clone();
    let door = nav.nearest_walkable(house.door, 4).unwrap_or(house.door);
    let speed = ctx.state.monsters[id].kind.def().speed;
    let monster = ctx.m(id);
    if plan.light_tick == 0 {
        if dist(monster.pos, door) > 1.6 {
            if monster.path.is_empty() || tick >= monster.repath_tick {
                monster.path = route_to(&nav, monster.pos, door).unwrap_or_default();
                monster.repath_tick = tick + seconds_to_ticks(2.0);
            }
            if monster.path.is_empty() {
                // No way there: give the house up.
                monster.arson = None;
                return false;
            }
            let mut path = std::mem::take(&mut monster.path);
            advance(monster, &mut path, speed);
            monster.path = path;
            set_anim(monster, AnimId::SkeletonWalk, false);
            return true;
        }
        monster.path.clear();
        face(monster, house.origin);
        set_anim(monster, AnimId::Interact, true);
        monster.arson = Some(Arson { light_tick: tick + seconds_to_ticks(LIGHT_SECONDS), ..plan });
        return true;
    }
    if tick < plan.light_tick {
        return true;
    }
    monster.arson = None;
    set_anim(monster, AnimId::SkeletonIdle, false);
    ignite(ctx, &plan.place_id);
    true
}

/// Hurts someone with fire; it can kill.
fn burn(ctx: &mut Ctx, id: &str, amount: i64, place: &Place) {
    let c = ctx.c(id);
    c.hp -= amount as f64;
    if c.hp <= 0.0 {
        kill_character(ctx, id, &format!("burned in the fire at {}", place.name), None);
        return;
    }
    set_anim(c, AnimId::Hit, true);
    ctx.stimulate(id, StimulusKind::Attacked, "The fire burned you.", 2, None);
}

/// Sets a house on fire. Whoever is inside is thrown out of bed, burnt, onto the street.
pub fn ignite(ctx: &mut Ctx, place_id: &str) {
    let tick = ctx.tick();
    let Some(place) = find_place(place_id) else { return };
    if ctx.state.houses.contains_key(place_id) {
        return;
    }
    ctx.state.houses.insert(
        place_id.to_owned(),
        House { phase: HousePhase::Burning, fire: START_FIRE, built: 0.0, since_tick: tick, helpers: Vec::new() },
    );
    ctx.state.director.last_fire_tick = tick;

    let inside: Vec<Id> =
        living(ctx.state).filter(|c| c.inside.as_deref() == Some(place_id)).map(|c| c.id.clone()).collect();
    let mut crier = None;
    let nav = ctx.state.nav.clone();
    for id in &inside {
        let c = ctx.c(id);
        leave_building(&nav, c);
        // Whatever they were doing in there (sleeping, hiding) is over.
        c.intent = None;
        c.queue.clear();
        c.act = fresh_act(tick);
        bump_epoch(c);
        let amount = ctx.int_range(ESCAPE_BURN[0], ESCAPE_BURN[1]);
        burn(ctx, id, amount, place);
        if crier.is_none() && ctx.alive(id) {
            crier = Some(id.clone());
        }
    }
    if let Some(crier) = &crier {
        ctx.say(crier, format!("Fire! Fire! {} is burning!", capitalised(place.name)), None, true);
        ctx.emit(SimEvent::Emote { id: crier.clone(), emote: Emote::Alert });
    }
    ctx.chronicle(
        ChronicleKind::Fire,
        format!("While the village slept, the dead set fire to {}.", place.name),
        residents(ctx.state, place_id),
    );
    ctx.emit(SimEvent::SavePoint { reason: "fire".into() });
}

fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Whether this character is carrying water to a fire.
pub fn fighting_fire(c: &Character) -> bool {
    matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::FightFire { .. }))
}

/// Calls everyone who can carry a bucket to the fire, asleep or not, brave or not.
fn muster(ctx: &mut Ctx, place_id: &str) {
    let tick = ctx.tick();
    let place = get_place(place_id);
    let water = water_for(place);
    let nav = ctx.state.nav.clone();
    for (i, id) in living_ids(ctx.state).iter().enumerate() {
        let Some(c) = ctx.state.characters.get(id) else { continue };
        if (tick + i as i64 * 3) % 10 != 0
            || c.combat.yielded
            || is_fighting(c)
            || fighting_fire(c)
            || c.hp < max_hp(c) * UNFIT
        {
            continue;
        }
        detach(ctx, id);
        let c = ctx.c(id);
        let was_asleep = c.asleep;
        if c.asleep || c.inside.is_some() {
            leave_building(&nav, c);
        }
        c.tool = None;
        c.queue.clear();
        c.intent = Some(Intent::new(IntentAction::FightFire { place_id: place_id.to_owned() }, IntentSource::Reflex));
        c.act = fresh_act(tick);
        bump_epoch(c);
        if !has_recent_stimulus(c, StimulusKind::Alarm, tick - minutes_to_ticks(30.0), None) {
            let text = format!(
                "{} is on fire! Everyone is carrying water from {} to put it out.",
                capitalised(place.name),
                water.name
            );
            ctx.stimulate(id, StimulusKind::Alarm, text, 3, None);
        }
        if was_asleep {
            ctx.emit(SimEvent::Emote { id: id.clone(), emote: Emote::Alert });
        }
    }
}

/// One character's part in the bucket chain: to the water, fill, back to the house, throw, and again.
pub fn run_fight_fire(ctx: &mut Ctx, id: &str, place_id: &str) {
    let tick = ctx.tick();
    if burning(ctx.state) != Some(place_id) {
        let saved = !ctx.state.houses.contains_key(place_id);
        let c = ctx.c(id);
        set_anim(c, AnimId::Idle, false);
        finish(ctx, id, if saved { "The fire is out." } else { "The house burned down in spite of everything." });
        return;
    }
    let place = get_place(place_id);
    let water = water_for(place);
    let nav = ctx.state.nav.clone();
    let c = ctx.c(id);
    let carrying = c.tool.is_some();
    match c.act.phase {
        ActPhase::Start => {
            let goal = if carrying { ring(&nav, place.door, id, 2.5) } else { ring(&nav, water.origin, id, 3.2) };
            let goal = goal.unwrap_or(if carrying { place.door } else { water.door });
            if !travel_to(tick, &nav, c, goal) {
                finish(ctx, id, "You could not get to the fire.");
            }
        }
        ActPhase::Travel => {
            if !travel(tick, c, Some(RUN_SPEED)) {
                return;
            }
            c.act.phase = ActPhase::Perform;
            c.act.since_tick = tick;
            if carrying {
                c.facing = facing_to(c.pos, place.origin);
                c.act.until_tick = tick + seconds_to_ticks(THROW_SECONDS);
                set_anim(c, AnimId::Interact, true);
            } else {
                c.facing = facing_to(c.pos, water.origin);
                c.act.until_tick = tick + seconds_to_ticks(FILL_SECONDS);
                set_anim(c, AnimId::PickUp, true);
            }
        }
        _ => {
            if tick < c.act.until_tick {
                return;
            }
            c.act = fresh_act(tick);
            if carrying {
                c.tool = None;
                douse(ctx, id, place_id);
            } else {
                c.tool = Some(BUCKET_TOOL.into());
            }
        }
    }
}

/// A bucket of water lands on the fire.
fn douse(ctx: &mut Ctx, id: &str, place_id: &str) {
    let Some(house) = ctx.state.houses.get_mut(place_id) else { return };
    house.fire -= BUCKET;
    if !house.helpers.iter().any(|helper| helper == id) {
        house.helpers.push(id.to_owned());
    }
    let out = house.fire <= 0.0;
    if ctx.chance(SCORCH_CHANCE) {
        let amount = ctx.int_range(SCORCH[0], SCORCH[1]);
        burn(ctx, id, amount, get_place(place_id));
    }
    if out {
        put_out(ctx, place_id);
    }
}

/// The fire is out and the house still stands.
fn put_out(ctx: &mut Ctx, place_id: &str) {
    let Some(house) = ctx.state.houses.shift_remove(place_id) else { return };
    let place = get_place(place_id);
    let helpers: Vec<Id> = house.helpers.into_iter().filter(|id| ctx.alive(id)).collect();
    ctx.chronicle(
        ChronicleKind::Fire,
        format!("The fire at {} is out. {} carried the water that saved it.", place.name, names(ctx.state, &helpers)),
        helpers.clone(),
    );
    for id in &helpers {
        let others: Vec<Id> = helpers.iter().filter(|other| *other != id).cloned().collect();
        for other in &others {
            adjust(ctx, id, other, RelationDelta { affinity: 2.0, trust: 3.0, familiarity: 2.0, ..Default::default() });
        }
        let text = if others.is_empty() {
            format!("Put out the fire at {} alone.", place.name)
        } else {
            format!("Carried water with the whole village and saved {} from the fire.", place.name)
        };
        ctx.remember(id, text, 7.0, MemoryKind::Event, others);
        ctx.stimulate(
            id,
            StimulusKind::Victory,
            format!("The fire at {} is out; the house is saved.", place.name),
            1,
            None,
        );
    }
    ctx.emit(SimEvent::SavePoint { reason: "fire out".into() });
}

/// The fire has won: the house is a ruin until it is built again.
fn burn_down(ctx: &mut Ctx, place_id: &str) {
    let tick = ctx.tick();
    let Some(house) = ctx.state.houses.get_mut(place_id) else { return };
    house.phase = HousePhase::Ruined;
    house.fire = 0.0;
    house.built = 0.0;
    house.since_tick = tick;
    house.helpers.clear();
    let place = get_place(place_id);
    let homeless = residents(ctx.state, place_id);
    let lost = match homeless.len() {
        0 => String::new(),
        1 => format!(" {} has lost a home.", names(ctx.state, &homeless)),
        _ => format!(" {} have lost their home.", names(ctx.state, &homeless)),
    };
    ctx.chronicle(
        ChronicleKind::Fire,
        format!("{} burned to the ground.{lost}", capitalised(place.name)),
        homeless.clone(),
    );
    for id in living_ids(ctx.state) {
        if homeless.contains(&id) {
            let text =
                format!("My home, {}, burned to the ground. I have nowhere to sleep until it is rebuilt.", place.name);
            ctx.remember(&id, text, 9.0, MemoryKind::Event, Vec::new());
            let text = "Your home has burned to the ground. Until it is rebuilt you must sleep rough by the campfire.";
            ctx.stimulate(&id, StimulusKind::Witness, text, 2, None);
        } else {
            let text = format!("The village could not save {} from the fire; it burned to the ground.", place.name);
            ctx.remember(&id, text, 6.0, MemoryKind::Event, homeless.clone());
            let text = format!("{} has burned to the ground and must be rebuilt.", capitalised(place.name));
            ctx.stimulate(&id, StimulusKind::Witness, text, 1, None);
        }
    }
    ctx.emit(SimEvent::SavePoint { reason: "house burned down".into() });
}

/// A minute of someone's work on a ruin. True when that finished the house.
pub fn add_work(ctx: &mut Ctx, id: &str, place_id: &str) -> bool {
    let Some(house) = ctx.state.houses.get_mut(place_id).filter(|house| house.phase == HousePhase::Ruined) else {
        return true;
    };
    house.built += 1.0;
    if !house.helpers.iter().any(|helper| helper == id) {
        house.helpers.push(id.to_owned());
    }
    if house.built < REBUILD_WORK {
        return false;
    }
    let house = ctx.state.houses.shift_remove(place_id).expect("the house was just there");
    let place = get_place(place_id);
    let housed = residents(ctx.state, place_id);
    let again = match housed.len() {
        0 => String::new(),
        _ => format!(" {} can sleep under a roof again.", names(ctx.state, &housed)),
    };
    ctx.chronicle(
        ChronicleKind::Fire,
        format!("{} stands again, rebuilt by the village.{again}", capitalised(place.name)),
        housed.clone(),
    );
    for id in living_ids(ctx.state) {
        if housed.contains(&id) {
            ctx.remember(
                &id,
                format!("The village rebuilt my home, {}. I have a bed again.", place.name),
                8.0,
                MemoryKind::Event,
                house.helpers.clone(),
            );
            ctx.stimulate(&id, StimulusKind::Victory, "Your home has been rebuilt: you have a bed again.", 1, None);
        } else if house.helpers.contains(&id) {
            ctx.remember(
                &id,
                format!("Helped rebuild {} after the fire.", place.name),
                5.0,
                MemoryKind::Event,
                housed.clone(),
            );
        }
    }
    ctx.emit(SimEvent::SavePoint { reason: "house rebuilt".into() });
    true
}

/// Where someone stands to work on a ruin.
pub fn build_spot(nav: &NavOverlay, place: &Place, id: &str) -> Vec2 {
    ring(nav, place.door, id, 2.8).unwrap_or(place.door)
}

/// How far a ruin has been rebuilt, 0..1.
pub fn built_fraction(house: &House) -> f64 {
    (house.built / REBUILD_WORK).clamp(0.0, 1.0)
}

/// Fires spread, and everyone who can carry a bucket is called to them.
pub fn update_fire(ctx: &mut Ctx) {
    let Some(place_id) = burning(ctx.state).map(str::to_owned) else { return };
    let tick = ctx.tick();
    if tick % TICKS_PER_GAME_MINUTE == 0 {
        let house = ctx.state.houses.get_mut(&place_id).expect("the burning house");
        house.fire += GROWTH_PER_MINUTE + house.fire * GROWTH_PER_FIRE;
        if house.fire >= 100.0 {
            burn_down(ctx, &place_id);
            return;
        }
    }
    muster(ctx, &place_id);
}
