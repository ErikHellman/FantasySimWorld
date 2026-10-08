//! Fires. When the whole village is asleep the dead put a torch to a home;
//! everyone is woken to carry water, and a house they cannot save burns down,
//! leaving those who lived there to sleep rough until it is rebuilt. Villagers
//! can set fires too, to any building or prop: a burning building musters the
//! village the same way and is a ruin if it is lost; a burning prop is left to
//! burn to ash. Whoever lit it is remembered.
//!
//! A fire's "site" is a home's place id (its state lives in `houses`) or a
//! world object's id (`b:<place>`, `p:<place>:<index>`, `n:<n>`, in `objects`).
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
    ActPhase, Arson, BuildingStage, Character, ChronicleKind, House, HousePhase, Id, Intent, IntentAction,
    IntentSource, MemoryKind, MonsterPhase, PropStage, SimState, StimulusKind, WorldObject,
};
use crate::systems::combat::{max_hp, witnesses};
use crate::systems::death::{detach, kill_character};
use crate::systems::intents::{finish, fresh_act, leave_building, travel, travel_to};
use crate::systems::movement::{advance, face, route_to};
use crate::systems::relationships::{RelationDelta, adjust};
use crate::systems::world::{
    building_id, building_whole, drop_object, place_model, position_of, prop_name, put_object,
};
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
/// Too badly hurt to carry water, or to stand through a ceremony.
pub const UNFIT: f64 = 0.25;
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
/// How many buildings may burn at once (each musters the village), and how many fires in all.
pub const MAX_ALARM_FIRES: usize = 2;
pub const MAX_FIRES: usize = 3;
/// A prop catches faster than a house and is not fought; this is how far gone it is when it is seen alight.
const PROP_START_FIRE: f64 = 35.0;
/// What a villager who sets a fire earns from those who see it, and from those who lived there.
const ARSON_GRIEVANCE: f64 = 45.0;
const ARSON_HOME_GRIEVANCE: f64 = 60.0;

/// Whether a fire site is a home (its state is in `houses`) rather than a world object.
pub fn is_home_site(site: &str) -> bool {
    !site.contains(':')
}

/// The place a fire site belongs to: the home, the building's place, or the place whose prop it is.
pub fn site_place(site: &str) -> Option<&'static Place> {
    if is_home_site(site) {
        return find_place(site);
    }
    place_model(site).and_then(|(place, _)| find_place(place))
}

/// Where a fire site is.
pub fn site_pos(state: &SimState, site: &str) -> Vec2 {
    if let Some(object) = state.objects.get(site)
        && let Some((x, z)) = position_of(site, object)
    {
        return vec2(x, z);
    }
    site_place(site).map_or(vec2(0.0, 0.0), |place| place.origin)
}

/// The door a fire is fought from: a building's door, or the thing itself.
fn site_door(state: &SimState, site: &str) -> Vec2 {
    match place_model(site) {
        Some((place, 0)) if get_place(place).has_building => get_place(place).door,
        Some(_) => site_pos(state, site),
        None if is_home_site(site) => site_place(site).map_or(site_pos(state, site), |place| place.door),
        None => site_pos(state, site),
    }
}

/// "the red cottage", "the bench at the village green": a fire site as the village names it.
pub fn site_name(site: &str) -> String {
    match place_model(site) {
        Some((place, 0)) if get_place(place).has_building => get_place(place).name.to_owned(),
        Some((place, index)) => {
            let place = get_place(place);
            format!("{} at {}", prop_name(&place.models[index].model), place.name)
        }
        None if is_home_site(site) => find_place(site).map_or_else(|| site.to_owned(), |place| place.name.to_owned()),
        None => "something the village built".to_owned(),
    }
}

/// The id a site goes by on a viewer's page: a home by its building (`b:<home>`), anything else as it is.
/// Chronicle lines about a site carry it, after the people they name, so the page can find them.
pub fn site_id(site: &str) -> String {
    if is_home_site(site) { building_id(site) } else { site.to_owned() }
}

/// `ids` with the site's page id on the end, for a chronicle line about it.
fn ids_with(mut ids: Vec<Id>, site: &str) -> Vec<Id> {
    ids.push(site_id(site));
    ids
}

/// Whether a fire site is a building (which musters the village) rather than a prop.
pub fn is_building_site(site: &str) -> bool {
    is_home_site(site) || matches!(place_model(site), Some((place, 0)) if get_place(place).has_building)
}

/// How far a fire has taken hold of a site, if it is alight.
pub fn fire_of(state: &SimState, site: &str) -> Option<f64> {
    if is_home_site(site) {
        return state.houses.get(site).filter(|house| house.phase == HousePhase::Burning).map(|house| house.fire);
    }
    match state.objects.get(site) {
        Some(WorldObject::Building { stage: BuildingStage::Burning, fire, .. })
        | Some(WorldObject::Prop { stage: PropStage::Burning, fire, .. })
        | Some(WorldObject::Built { stage: PropStage::Burning, fire, .. }) => Some(*fire),
        _ => None,
    }
}

/// Everything alight right now: homes first, then the world's things, in a fixed order.
pub fn burning_sites(state: &SimState) -> Vec<String> {
    let homes = state.houses.iter().filter(|(_, house)| house.phase == HousePhase::Burning).map(|(id, _)| id.clone());
    let objects = state.objects.iter().filter(|(id, _)| fire_of(state, id).is_some()).map(|(id, _)| id.clone());
    homes.chain(objects).collect()
}

/// The burning buildings: the fires the village is called to.
pub fn alarm_sites(state: &SimState) -> Vec<String> {
    burning_sites(state).into_iter().filter(|site| is_building_site(site)).collect()
}

/// The first building on fire, if any: a house burning is what wakes courage and keeps people out of hiding.
pub fn burning(state: &SimState) -> Option<String> {
    alarm_sites(state).into_iter().next()
}

/// The ids of those who have carried water to a site.
fn helpers_of(state: &SimState, site: &str) -> Vec<Id> {
    if is_home_site(site) {
        return state.houses.get(site).map(|house| house.helpers.clone()).unwrap_or_default();
    }
    match state.objects.get(site) {
        Some(WorldObject::Building { helpers, .. }) => helpers.clone(),
        _ => Vec::new(),
    }
}

/// Lights a fire at a site, by a villager or (`None`) by the dead. Does nothing to a site that is
/// already alight or cannot burn.
pub fn ignite_site(ctx: &mut Ctx, site: &str, arsonist: Option<&str>) {
    let tick = ctx.tick();
    if fire_of(ctx.state, site).is_some() {
        return;
    }
    if is_home_site(site) {
        if !ctx.state.houses.contains_key(site) && building_whole(ctx.state, site) {
            ignite_home(ctx, site, arsonist);
        }
    } else {
        let Some((place_id, index)) = place_model(site) else {
            // Something the village built.
            if let Some(WorldObject::Built { stage: PropStage::Whole, spec, .. }) = ctx.state.objects.get(site).cloned()
            {
                let alight = WorldObject::Built {
                    stage: PropStage::Burning,
                    since_tick: tick,
                    work: 0.0,
                    fire: PROP_START_FIRE,
                    spec,
                };
                put_object(ctx.state, site, alight);
                arson_consequences(ctx, site, arsonist);
            }
            return;
        };
        let place = get_place(place_id);
        if index == 0 && place.has_building {
            if !building_whole(ctx.state, place_id) {
                return;
            }
            let alight = WorldObject::Building {
                stage: BuildingStage::Burning,
                since_tick: tick,
                work: 0.0,
                fire: START_FIRE,
                helpers: Vec::new(),
            };
            put_object(ctx.state, site, alight);
            ctx.state.director.last_fire_tick = tick;
            // Anyone sheltering inside is out on the street.
            let inside: Vec<Id> =
                living(ctx.state).filter(|c| c.inside.as_deref() == Some(place_id)).map(|c| c.id.clone()).collect();
            let nav = ctx.state.nav.clone();
            for id in &inside {
                let c = ctx.c(id);
                leave_building(&nav, c);
                c.intent = None;
                c.queue.clear();
                c.act = fresh_act(tick);
                bump_epoch(c);
            }
            if arsonist.is_none() {
                ctx.chronicle(
                    ChronicleKind::Fire,
                    format!("{} is on fire!", capitalised(place.name)),
                    vec![site.to_owned()],
                );
            }
            ctx.emit(SimEvent::SavePoint { reason: "fire".into() });
        } else {
            if !crate::systems::world::prop_whole(ctx.state, site) {
                return;
            }
            let alight =
                WorldObject::Prop { stage: PropStage::Burning, since_tick: tick, work: 0.0, fire: PROP_START_FIRE };
            put_object(ctx.state, site, alight);
        }
        arson_consequences(ctx, site, arsonist);
    }
}

/// What a villager who set a fire has coming: the village hears of it, those who saw it hold it
/// against them, and those whose home it was most of all.
fn arson_consequences(ctx: &mut Ctx, site: &str, arsonist: Option<&str>) {
    let Some(arsonist) = arsonist else { return };
    if !ctx.alive(arsonist) {
        return;
    }
    let tick = ctx.tick();
    let name = site_name(site);
    let who = first_name(ctx.cr(arsonist)).to_owned();
    ctx.c(arsonist).last_arson_tick = tick;
    ctx.chronicle(ChronicleKind::Fire, format!("{who} set fire to {name}!"), vec![arsonist.to_owned(), site_id(site)]);
    ctx.remember(arsonist, format!("Set fire to {name}."), 7.0, MemoryKind::Event, Vec::new());
    let pos = site_pos(ctx.state, site);
    let saw = witnesses(ctx, pos, &[arsonist]);
    let residents: Vec<Id> = match site_place(site) {
        Some(place) if is_building_site(site) => residents(ctx.state, &place.id),
        _ => Vec::new(),
    };
    for id in living_ids(ctx.state) {
        if id == arsonist {
            continue;
        }
        let mine = residents.contains(&id);
        let witness = saw.contains(&id);
        if !mine && !witness {
            continue;
        }
        let grievance = if mine { ARSON_HOME_GRIEVANCE } else { ARSON_GRIEVANCE };
        adjust(ctx, &id, arsonist, RelationDelta { affinity: -25.0, trust: -30.0, grievance, ..Default::default() });
        let text =
            if mine { format!("{who} set fire to my home, {name}!") } else { format!("Saw {who} set fire to {name}.") };
        ctx.remember(&id, text, 8.0, MemoryKind::Event, vec![arsonist.to_owned()]);
        ctx.stimulate(&id, StimulusKind::Hostility, format!("{who} has set fire to {name}!"), 3, Some(arsonist));
    }
}

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

/// The water nearest a fire.
fn water_near(pos: Vec2) -> &'static Place {
    let mut best = get_place(WATER[0]);
    for id in WATER {
        let place = get_place(id);
        if dist(place.origin, pos) < dist(best.origin, pos) {
            best = place;
        }
    }
    best
}

pub(crate) fn residents(state: &SimState, place_id: &str) -> Vec<Id> {
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

/// Sets a house on fire, the dead's way: whoever is inside is thrown out of bed, burnt, onto the street.
pub fn ignite(ctx: &mut Ctx, place_id: &str) {
    ignite_home(ctx, place_id, None);
}

/// A home catches, set alight by a villager or (`None`) by the dead.
pub fn ignite_home(ctx: &mut Ctx, place_id: &str, arsonist: Option<&str>) {
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
    if arsonist.is_none() {
        ctx.chronicle(
            ChronicleKind::Fire,
            format!("While the village slept, the dead set fire to {}.", place.name),
            ids_with(residents(ctx.state, place_id), place_id),
        );
    }
    arson_consequences(ctx, place_id, arsonist);
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

/// Calls everyone who can carry a bucket to the burning buildings, asleep or not, brave or not; each
/// goes to the nearest. Whoever lit a fire is not asked to put it out.
fn muster(ctx: &mut Ctx, sites: &[String]) {
    let tick = ctx.tick();
    let nav = ctx.state.nav.clone();
    let doors: Vec<(String, Vec2, i64)> =
        sites.iter().map(|site| (site.clone(), site_door(ctx.state, site), since_tick_of(ctx.state, site))).collect();
    for (i, id) in living_ids(ctx.state).iter().enumerate() {
        let Some(c) = ctx.state.characters.get(id) else { continue };
        if (tick + i as i64 * 3) % 10 != 0
            || c.is_child()
            || c.combat.yielded
            || is_fighting(c)
            || fighting_fire(c)
            || c.hp < max_hp(c) * UNFIT
        {
            continue;
        }
        let pos = c.pos;
        let arson_tick = c.last_arson_tick;
        let Some((site, _, _)) = doors
            .iter()
            .filter(|(_, _, since)| arson_tick < *since)
            .min_by(|a, b| dist(pos, a.1).total_cmp(&dist(pos, b.1)))
        else {
            continue;
        };
        let site = site.clone();
        let name = site_name(&site);
        let water = water_near(site_door(ctx.state, &site));
        detach(ctx, id);
        let c = ctx.c(id);
        let was_asleep = c.asleep;
        if c.asleep || c.inside.is_some() {
            leave_building(&nav, c);
        }
        c.tool = None;
        c.queue.clear();
        c.intent = Some(Intent::new(IntentAction::FightFire { place_id: site }, IntentSource::Reflex));
        c.act = fresh_act(tick);
        bump_epoch(c);
        if !has_recent_stimulus(c, StimulusKind::Alarm, tick - minutes_to_ticks(30.0), None) {
            let text = format!(
                "{} is on fire! Everyone is carrying water from {} to put it out.",
                capitalised(&name),
                water.name
            );
            ctx.stimulate(id, StimulusKind::Alarm, text, 3, None);
        }
        if was_asleep {
            ctx.emit(SimEvent::Emote { id: id.clone(), emote: Emote::Alert });
        }
    }
}

/// The tick a site caught fire.
fn since_tick_of(state: &SimState, site: &str) -> i64 {
    if is_home_site(site) {
        return state.houses.get(site).map_or(0, |house| house.since_tick);
    }
    state.objects.get(site).map_or(0, WorldObject::since_tick)
}

/// One character's part in the bucket chain: to the water, fill, back to the fire, throw, and again.
pub fn run_fight_fire(ctx: &mut Ctx, id: &str, site: &str) {
    let tick = ctx.tick();
    if fire_of(ctx.state, site).is_none() {
        let lost = if is_home_site(site) {
            ctx.state.houses.contains_key(site)
        } else {
            !building_whole(ctx.state, site.strip_prefix("b:").unwrap_or(site))
        };
        let c = ctx.c(id);
        set_anim(c, AnimId::Idle, false);
        finish(ctx, id, if lost { "It burned down in spite of everything." } else { "The fire is out." });
        return;
    }
    let door = site_door(ctx.state, site);
    let water = water_near(door);
    let nav = ctx.state.nav.clone();
    let c = ctx.c(id);
    let carrying = c.tool.is_some();
    match c.act.phase {
        ActPhase::Start => {
            let goal = if carrying { ring(&nav, door, id, 2.5) } else { ring(&nav, water.origin, id, 3.2) };
            let goal = goal.unwrap_or(if carrying { door } else { water.door });
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
                c.facing = facing_to(c.pos, door);
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
                douse(ctx, id, site);
            } else {
                c.tool = Some(BUCKET_TOOL.into());
            }
        }
    }
}

/// A bucket of water lands on the fire.
fn douse(ctx: &mut Ctx, id: &str, site: &str) {
    let out = if is_home_site(site) {
        let Some(house) = ctx.state.houses.get_mut(site) else { return };
        house.fire -= BUCKET;
        if !house.helpers.iter().any(|helper| helper == id) {
            house.helpers.push(id.to_owned());
        }
        house.fire <= 0.0
    } else {
        let Some(WorldObject::Building { fire, helpers, .. }) = ctx.state.objects.get_mut(site) else { return };
        *fire -= BUCKET;
        if !helpers.iter().any(|helper| helper == id) {
            helpers.push(id.to_owned());
        }
        *fire <= 0.0
    };
    if ctx.chance(SCORCH_CHANCE) {
        let amount = ctx.int_range(SCORCH[0], SCORCH[1]);
        let place = site_place(site).unwrap_or_else(|| get_place(SHELTER));
        burn(ctx, id, amount, place);
    }
    if out {
        put_out(ctx, site);
    }
}

/// The fire is out and the building still stands.
fn put_out(ctx: &mut Ctx, site: &str) {
    let helpers: Vec<Id> = helpers_of(ctx.state, site).into_iter().filter(|id| ctx.alive(id)).collect();
    if is_home_site(site) {
        if ctx.state.houses.shift_remove(site).is_none() {
            return;
        }
    } else {
        drop_object(ctx.state, site);
    }
    let name = site_name(site);
    ctx.chronicle(
        ChronicleKind::Fire,
        format!("The fire at {name} is out. {} carried the water that saved it.", names(ctx.state, &helpers)),
        ids_with(helpers.clone(), site),
    );
    for id in &helpers {
        let others: Vec<Id> = helpers.iter().filter(|other| *other != id).cloned().collect();
        for other in &others {
            adjust(ctx, id, other, RelationDelta { affinity: 2.0, trust: 3.0, familiarity: 2.0, ..Default::default() });
        }
        let text = if others.is_empty() {
            format!("Put out the fire at {name} alone.")
        } else {
            format!("Carried water with the whole village and saved {name} from the fire.")
        };
        ctx.remember(id, text, 7.0, MemoryKind::Event, others);
        ctx.stimulate(id, StimulusKind::Victory, format!("The fire at {name} is out; it is saved."), 1, None);
    }
    ctx.emit(SimEvent::SavePoint { reason: "fire out".into() });
}

/// The fire has won: a home or building is a ruin until it is built again, a prop is ash.
fn burn_down(ctx: &mut Ctx, site: &str) {
    let tick = ctx.tick();
    if is_home_site(site) {
        burn_down_home(ctx, site);
        return;
    }
    let name = site_name(site);
    match ctx.state.objects.get(site).cloned() {
        Some(WorldObject::Building { helpers, .. }) => {
            let ruin =
                WorldObject::Building { stage: BuildingStage::Ruin, since_tick: tick, work: 0.0, fire: 0.0, helpers };
            put_object(ctx.state, site, ruin);
            ctx.chronicle(
                ChronicleKind::Fire,
                format!("{} burned to the ground.", capitalised(&name)),
                vec![site.to_owned()],
            );
            for id in living_ids(ctx.state) {
                let text = format!("The village could not save {name} from the fire; it burned to the ground.");
                ctx.remember(&id, text, 6.0, MemoryKind::Event, Vec::new());
                let text = format!("{} has burned to the ground and must be raised again.", capitalised(&name));
                ctx.stimulate(&id, StimulusKind::Witness, text, 1, None);
            }
            ctx.emit(SimEvent::SavePoint { reason: "building burned down".into() });
        }
        Some(WorldObject::Prop { .. }) => {
            let ash = WorldObject::Prop { stage: PropStage::Ash, since_tick: tick, work: 0.0, fire: 0.0 };
            put_object(ctx.state, site, ash);
        }
        Some(WorldObject::Built { spec, .. }) => {
            let ash = WorldObject::Built { stage: PropStage::Ash, since_tick: tick, work: 0.0, fire: 0.0, spec };
            put_object(ctx.state, site, ash);
        }
        _ => {}
    }
}

/// The fire has won: the house is a ruin until it is built again.
fn burn_down_home(ctx: &mut Ctx, place_id: &str) {
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
        ids_with(homeless.clone(), place_id),
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
        ids_with(housed.clone(), place_id),
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

/// Each fire grows a little every game minute, and the village is kept at the buckets for the buildings.
pub fn update_fire(ctx: &mut Ctx) {
    let sites = burning_sites(ctx.state);
    if sites.is_empty() {
        return;
    }
    let tick = ctx.tick();
    if tick % TICKS_PER_GAME_MINUTE == 0 {
        for site in &sites {
            let Some(level) = fire_of(ctx.state, site) else { continue };
            let grown = level + GROWTH_PER_MINUTE + level * GROWTH_PER_FIRE;
            if grown >= 100.0 {
                burn_down(ctx, site);
                continue;
            }
            if is_home_site(site) {
                if let Some(house) = ctx.state.houses.get_mut(site) {
                    house.fire = grown;
                }
            } else if let Some(fire) = ctx.state.objects.get_mut(site).and_then(WorldObject::fire_mut) {
                *fire = grown;
            }
        }
    }
    let alarm = alarm_sites(ctx.state);
    if !alarm.is_empty() {
        muster(ctx, &alarm);
    }
}
