//! Orc raids: now and then, by day, a war band comes up the west road to plunder, and goes back down it
//! when it has its loot or has had enough (docs/superpowers/specs/2026-10-04-combat-design.md, step 2).
//!
//! Nothing here draws from the RNG or writes to the world until a raid comes: the first is fixed by the
//! seed, so a world that has not reached it runs exactly as it did before raids existed.

use fsw_core::math2d::dist;
use fsw_core::rng::hash_string;
use fsw_core::time::{TICKS_PER_DAY, TICKS_PER_GAME_MINUTE, minutes_to_ticks, seconds_to_ticks};
use fsw_core::{Vec2, vec2};
use fsw_world::places::{find_place, get_place, homes};

use crate::data::anims::AnimId;
use crate::data::monsters::Faction;
use crate::events::{Emote, RaidPhase, SimEvent};
use crate::state::{
    Band, ChronicleKind, Haul, Id, ItemId, MemoryKind, MonsterKind, MonsterOrigin, MonsterPhase, RaidOrder, Resource,
    SimState, StimulusKind,
};
use crate::systems::director::tick_at;
use crate::systems::fire::burning;
use crate::systems::monsters::{MAX_MONSTERS, chase_and_strike, find_target, id_number, land_swing, spawn_monster};
use crate::systems::movement::{advance, face, route_to};
use crate::systems::world::building_whole;
use crate::util::{Ctx, adult_count, join_names, living, living_ids, set_anim};

/// The first raid comes on this day or up to two after, fixed by the seed.
const FIRST_RAID_DAY: i64 = 4;
/// Days from one raid to the next.
const RAID_GAP_DAYS: [i64; 2] = [5, 9];
/// Raids come up the road between these hours.
pub const RAID_HOURS: [f64; 2] = [9.0, 16.0];
/// A village smaller than this is beneath a war band's notice.
const RAID_LEAST_ALIVE: usize = 8;
/// A raid an admin calls still needs someone to raid.
const CALLED_LEAST_ALIVE: usize = 3;
const BAND_LEAST: i64 = 3;
/// Every third raid from this day on is led by a warchief, when the village is big enough to be worth it.
const WARCHIEF_FROM_DAY: i64 = 12;
const WARCHIEF_LEAST_ALIVE: i64 = 10;
const BAND_MOST: i64 = 6;
/// How long after the band appears the village hears of it.
const WARNING_SECONDS: f64 = 20.0;
/// How long a raider spends ransacking a place.
const PLUNDER_SECONDS: f64 = 5.0;
/// A raider gives up a chase that takes it this far from where it was going.
const LEASH: f64 = 12.0;
/// A raider on its own below this share of its health runs for the road.
const ROUT_SHARE: f64 = 0.25;
/// Band-mates within this distance keep a hurt raider fighting.
const BAND_MATE_RANGE: f64 = 10.0;
/// The band turns for home when its health, all told, is below this share.
const BAND_SPENT_SHARE: f64 = 0.4;
/// And at this hour, whatever it has.
const LEAVE_HOUR: f64 = 17.0;
/// A band still in the valley a day after it came is gone by morning, however it got stuck.
const BAND_MOST_TICKS: i64 = TICKS_PER_DAY;
/// The most coin a raider takes from the chest.
const PLUNDER_COINS: i64 = 12;
/// The most goods a raider takes off the market's stalls.
const PLUNDER_GOODS: i64 = 3;
/// The most materials a raider carries off from a stockpile.
const PLUNDER_MATERIALS: i64 = 10;
/// A fallen raider lies this long before it is gone.
pub const ORC_CORPSE_MINUTES: f64 = 120.0;
/// Close enough to a door to be at the place.
const AT_DOOR: f64 = 1.6;
/// The places worth robbing, besides any home with nobody in it.
const TARGETS: [&str; 5] = ["market", "farm", "smithy", "lumber_camp", "mine"];

/// Whether work at this place yields nothing, because raiders have plundered it.
pub fn plundered(state: &SimState, place_id: &str) -> bool {
    state.plundered.get(place_id).is_some_and(|&until| state.tick < until)
}

/// Whether an orc war band is in the valley.
pub fn raid_on(state: &SimState) -> bool {
    state.raid.is_some()
}

/// When the first raid of a world is due: fixed by its seed, so nothing is drawn for it.
fn first_raid_tick(seed: u32) -> i64 {
    let roll = hash_string(&format!("raid@{seed}"));
    let day = FIRST_RAID_DAY + i64::from(roll % 3);
    let hour = RAID_HOURS[0] + f64::from((roll / 3) % 7);
    tick_at(day, hour)
}

/// When the next raid is due.
pub fn next_raid_tick(state: &SimState) -> i64 {
    match state.director.next_raid_tick {
        0 => first_raid_tick(state.seed),
        tick => tick,
    }
}

/// An admin's call: the next raid comes as soon as it may. False when one is already on or on its way.
pub fn call_raid(ctx: &mut Ctx) -> bool {
    let director = &ctx.state.director;
    if director.founding || director.raid_called || ctx.state.raid.is_some() {
        return false;
    }
    ctx.state.director.raid_called = true;
    true
}

/// Run from the director each tick: a raid comes when it is due, and the band in the valley is kept to its
/// purpose.
pub fn update_raids(ctx: &mut Ctx) {
    let tick = ctx.tick();
    if tick % TICKS_PER_GAME_MINUTE == 0 && !ctx.state.plundered.is_empty() {
        ctx.state.plundered.retain(|_, until| *until > tick);
    }
    if ctx.state.raid.is_some() {
        run_band(ctx);
        return;
    }
    let called = ctx.state.director.raid_called;
    let due = next_raid_tick(ctx.state);
    if !called && tick < due {
        return;
    }
    let hour = ctx.time.hour;
    let least = if called { CALLED_LEAST_ALIVE } else { RAID_LEAST_ALIVE };
    let room = MAX_MONSTERS.saturating_sub(ctx.state.monsters.len()) as i64;
    let open = (RAID_HOURS[0]..RAID_HOURS[1]).contains(&hour)
        && burning(ctx.state).is_none()
        && adult_count(ctx.state) >= least
        && room >= BAND_LEAST;
    if open {
        spawn_band(ctx);
    } else if !called {
        // Not today: the same hour tomorrow. A called raid waits for the first chance instead.
        ctx.state.director.next_raid_tick = due + TICKS_PER_DAY;
    }
}

fn spawn_band(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let day = ctx.time.day;
    let alive = adult_count(ctx.state) as i64;
    let mut room = MAX_MONSTERS.saturating_sub(ctx.state.monsters.len()) as i64;
    let led = ctx.state.director.raids % 3 == 2
        && day >= WARCHIEF_FROM_DAY
        && alive >= WARCHIEF_LEAST_ALIVE
        && room > BAND_LEAST;
    if led {
        room -= 1;
    }
    let size = (BAND_LEAST + alive / 6).clamp(BAND_LEAST, BAND_MOST).min(room);
    let road = get_place("west_road");
    let mut members = Vec::new();
    let mut max_hp = 0.0;
    let mut warchief = None;
    // The warchief first, then the band, so the draws come in the same order on every machine.
    for i in (if led { -1 } else { 0 })..size {
        let z = road.origin.z + ctx.range(-2.0, 2.0);
        let pos = ctx.state.nav.nearest_walkable(vec2(road.origin.x - 2.0, z), 6).unwrap_or(road.door);
        let kind = match i {
            -1 => MonsterKind::OrcWarchief,
            i if i % 2 == 0 => MonsterKind::OrcAxe,
            _ => MonsterKind::OrcClub,
        };
        let id = spawn_monster(ctx, kind, pos, road.facing, MonsterOrigin::Road);
        // Every band is a little mixed: two hides of orc, and some who travel without a pack.
        let chief = kind == MonsterKind::OrcWarchief;
        if chief {
            warchief = Some(id.clone());
        }
        let looks = hash_string(&format!("{id}@{}", ctx.state.seed));
        let m = ctx.m(&id);
        // The brute keeps its own hide and gear.
        if !chief && looks % 2 == 1 {
            m.look.texture = Some("orc_texture_B".into());
        }
        if !chief && (looks / 2).is_multiple_of(3) {
            m.look.hide.push("Orc_Warpack".into());
        }
        m.raid = Some(RaidOrder::default());
        max_hp += kind.def().max_hp;
        members.push(id);
    }
    let gap = ctx.int_range(RAID_GAP_DAYS[0], RAID_GAP_DAYS[1]);
    let hour = ctx.range(RAID_HOURS[0], RAID_HOURS[1]);
    let director = &mut ctx.state.director;
    director.next_raid_tick = tick_at(day + gap, hour);
    director.raids += 1;
    director.raid_called = false;
    ctx.state.raid = Some(Band {
        started_tick: tick,
        size,
        members,
        fallen: 0,
        plundered: Vec::new(),
        max_hp,
        warned: false,
        leaving: false,
        carried_off: Haul::default(),
        warchief,
    });
    ctx.emit(SimEvent::Raid { phase: RaidPhase::Coming });
}

/// The orcs of the band still on their feet.
fn standing(state: &SimState, band: &Band) -> Vec<Id> {
    band.members
        .iter()
        .filter(|id| state.monsters.get(*id).is_some_and(|m| m.phase != MonsterPhase::Dying))
        .cloned()
        .collect()
}

fn run_band(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let Some(band) = ctx.state.raid.as_ref() else { return };
    if !band.warned && !band.leaving && tick >= band.started_tick + seconds_to_ticks(WARNING_SECONDS) {
        warn(ctx);
    }
    let Some(band) = ctx.state.raid.as_ref() else { return };
    let up = standing(ctx.state, band);
    if up.is_empty() {
        end_raid(ctx);
        return;
    }
    if tick - band.started_tick > BAND_MOST_TICKS {
        // Lost in the woods: what is left of it slips away.
        for id in up {
            vanish(ctx, &id);
        }
        return;
    }
    if band.leaving {
        return;
    }
    let health: f64 = up.iter().map(|id| ctx.state.monsters[id].hp.max(0.0)).sum();
    let had_enough = band.plundered.len() as i64 >= band.size
        || band.fallen * 2 >= band.size
        || health < band.max_hp * BAND_SPENT_SHARE
        || ctx.time.hour >= LEAVE_HOUR;
    if had_enough {
        turn_for_home(ctx);
    }
}

/// The band makes for the road, every one of it.
pub fn turn_for_home(ctx: &mut Ctx) {
    let Some(band) = ctx.state.raid.as_mut() else { return };
    if band.leaving {
        return;
    }
    band.leaving = true;
    let up = standing(ctx.state, ctx.state.raid.as_ref().expect("a band"));
    for id in &up {
        let order = ctx.m(id).raid.get_or_insert_with(RaidOrder::default);
        order.leaving = true;
        order.target = None;
        order.work_until_tick = 0;
    }
    ctx.chronicle(ChronicleKind::Combat, "The orcs fall back towards the west road.", Vec::new());
    ctx.emit(SimEvent::Raid { phase: RaidPhase::Leaving });
}

/// The village hears of the band: a shout, the alarm, and everyone knows.
fn warn(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let Some(band) = ctx.state.raid.as_mut() else { return };
    band.warned = true;
    let size = band.size;
    let led = band.warchief.is_some();
    let up = standing(ctx.state, ctx.state.raid.as_ref().expect("a band"));
    let road = get_place("west_road");
    let pos = up.first().map_or(road.door, |id| ctx.state.monsters[id].pos);
    // Unless someone has seen them already and shouted it, the news comes up the road ahead of them.
    if !ctx.state.director.alarm_raid {
        let crier = living(ctx.state).find(|c| !c.is_child() && !c.asleep && c.inside.is_none()).map(|c| c.id.clone());
        if let Some(crier) = &crier {
            ctx.say(crier, "Orcs on the road! To arms!", None, true);
            ctx.emit(SimEvent::Emote { id: crier.clone(), emote: Emote::Alert });
        }
        let led = if led { ", led by a warchief," } else { "" };
        ctx.chronicle(
            ChronicleKind::Combat,
            format!("A war band of {size} orcs{led} is coming up the west road!"),
            crier.into_iter().collect(),
        );
    }
    let director = &mut ctx.state.director;
    director.alarm_until_tick = tick + minutes_to_ticks(40.0);
    director.alarm_pos = Some(pos);
    director.alarm_raid = true;
    let first = up.first().cloned();
    for id in living_ids(ctx.state) {
        ctx.stimulate(&id, StimulusKind::Alarm, "Orc raiders are coming up the west road!", 2, first.as_deref());
    }
}

fn end_raid(ctx: &mut Ctx) {
    let Some(band) = ctx.state.raid.take() else { return };
    let names: Vec<String> =
        band.plundered.iter().map(|id| find_place(id).map_or_else(|| id.clone(), |p| p.name.to_owned())).collect();
    let places = join_names(&names);
    let text = match band.carried_off.describe() {
        Some(what) if !names.is_empty() => format!("Orcs raided {places}, and carried off {what}."),
        Some(what) => format!("Orcs raided the village and carried off {what}."),
        None if !names.is_empty() => {
            format!("Orcs plundered {places}, but the village drove them off before they could carry anything away.")
        }
        None => "The village drove the raiders off before they could take anything.".to_owned(),
    };
    ctx.chronicle(ChronicleKind::Combat, text.clone(), Vec::new());
    let everyone: Vec<Id> = living(ctx.state).filter(|c| !c.is_child()).map(|c| c.id.clone()).collect();
    for id in everyone {
        ctx.remember(&id, text.clone(), 5.0, MemoryKind::Event, Vec::new());
    }
    ctx.emit(SimEvent::Raid { phase: RaidPhase::Gone });
    ctx.emit(SimEvent::SavePoint { reason: "raid over".into() });
}

/// A raider reaches the edge of the valley and is gone, with whatever it carries.
fn vanish(ctx: &mut Ctx, id: &str) {
    let Some(m) = ctx.state.monsters.shift_remove(id) else { return };
    if let (Some(band), Some(order)) = (ctx.state.raid.as_mut(), m.raid) {
        band.carried_off.add(&order.haul);
    }
}

/// A raider has fallen: one fewer in the band. When it is the warchief, the band breaks and makes for home.
pub fn on_fallen(ctx: &mut Ctx, id: &str) {
    let Some(band) = ctx.state.raid.as_mut() else { return };
    if band.warchief.as_deref() == Some(id) {
        turn_for_home(ctx);
    } else {
        band.fallen += 1;
    }
}

/// Where the band leaves the valley: the spot on the west road it came up.
fn exit_point(state: &SimState) -> Vec2 {
    let road = get_place("west_road");
    state.nav.nearest_walkable(vec2(road.origin.x - 2.0, road.origin.z), 6).unwrap_or(road.door)
}

fn door_of(place_id: &str) -> Option<Vec2> {
    find_place(place_id).map(|p| p.door)
}

/// Where this raider means to be: its target's door, or the road home.
fn goal_of(state: &SimState, order: &RaidOrder) -> Option<Vec2> {
    if order.leaving { Some(exit_point(state)) } else { order.target.as_deref().and_then(door_of) }
}

/// Another of the band, on its feet, close by.
fn band_mate_near(state: &SimState, id: &str, pos: Vec2) -> bool {
    state.monsters.values().any(|m| {
        m.id != id
            && m.kind.faction() == Faction::Orc
            && m.phase != MonsterPhase::Dying
            && dist(m.pos, pos) < BAND_MATE_RANGE
    })
}

/// The nearest place worth robbing that nobody has robbed today and no other raider is making for.
fn choose_target(state: &SimState, id: &str) -> Option<String> {
    let m = &state.monsters[id];
    let claimed: Vec<&str> = state
        .monsters
        .values()
        .filter(|o| o.id != id)
        .filter_map(|o| o.raid.as_ref().and_then(|r| r.target.as_deref()))
        .collect();
    let empty_home =
        |home: &str| !state.houses.contains_key(home) && !living(state).any(|c| c.inside.as_deref() == Some(home));
    let mut options: Vec<(f64, String)> = TARGETS
        .iter()
        .map(|t| (*t).to_owned())
        .chain(homes().iter().map(|h| h.id.clone()).filter(|h| empty_home(h)))
        .filter(|p| !plundered(state, p) && !claimed.contains(&p.as_str()) && building_whole(state, p))
        .filter_map(|p| door_of(&p).map(|door| (dist(m.pos, door), p)))
        .collect();
    options.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    options.into_iter().next().map(|(_, p)| p)
}

/// What a raider carries off from a place: the chest's coin and the stalls' goods at the market, and
/// the materials in any stockpile. A farm, a smithy or a home is only spoiled.
fn take_from(state: &mut SimState, place_id: &str) -> Haul {
    let mut haul = Haul::default();
    if place_id == "market" {
        let coins = state.economy.purse.clamp(0, PLUNDER_COINS);
        state.economy.purse -= coins;
        haul.coins = coins;
        let mut room = PLUNDER_GOODS;
        for item in ItemId::ALL {
            let have = state.economy.in_stock(item);
            let taken = have.min(room);
            if taken <= 0 {
                continue;
            }
            if taken == have {
                state.economy.stock.shift_remove(&item);
            } else {
                state.economy.stock[&item] -= taken;
            }
            haul.goods.insert(item, taken);
            room -= taken;
        }
    }
    if let Some(store) = state.stockpiles.get_mut(place_id) {
        let mut room = PLUNDER_MATERIALS;
        for resource in Resource::ALL {
            let taken = store.take(resource, room);
            haul.materials.add(resource, taken, i64::MAX);
            room -= taken;
        }
    }
    haul
}

fn plunder(ctx: &mut Ctx, id: &str, place_id: &str) {
    let tick = ctx.tick();
    ctx.state.plundered.insert(place_id.to_owned(), tick + TICKS_PER_DAY);
    let taken = take_from(ctx.state, place_id);
    let m = ctx.m(id);
    let idle = m.kind.def().anims.idle;
    set_anim(m, idle, false);
    let order = m.raid.get_or_insert_with(RaidOrder::default);
    order.haul.add(&taken);
    order.target = None;
    order.work_until_tick = 0;
    if let Some(band) = ctx.state.raid.as_mut()
        && !band.plundered.iter().any(|p| p == place_id)
    {
        band.plundered.push(place_id.to_owned());
    }
    let name = find_place(place_id).map_or(place_id, |p| p.name);
    let text = match taken.describe() {
        Some(what) => format!("Orc raiders plundered {name} and made off with {what}."),
        None => format!("Orc raiders ransacked {name}."),
    };
    ctx.chronicle(ChronicleKind::Combat, text, Vec::new());
}

/// Walks a raider towards `goal`; true once it is there.
fn walk_to(ctx: &mut Ctx, id: &str, goal: Vec2, speed_share: f64) -> Option<bool> {
    let tick = ctx.tick();
    let nav = ctx.state.nav.clone();
    let m = ctx.m(id);
    if dist(m.pos, goal) < AT_DOOR {
        m.path.clear();
        return Some(true);
    }
    if m.path.is_empty() || tick >= m.repath_tick {
        m.repath_tick = tick + seconds_to_ticks(3.0);
        m.path = route_to(&nav, m.pos, goal)?;
    }
    let def = m.kind.def();
    let mut path = std::mem::take(&mut m.path);
    advance(m, &mut path, def.speed * speed_share);
    m.path = path;
    set_anim(m, def.anims.walk, false);
    Some(false)
}

/// Every so often a raider looks about: it runs if it is beaten and alone, gives up a chase that drags
/// it off its errand, and turns on anyone who comes too close.
fn look_about(ctx: &mut Ctx, id: &str) {
    let m = &ctx.state.monsters[id];
    let def = m.kind.def();
    let order = m.raid.clone().unwrap_or_default();
    let beaten = m.hp < def.max_hp * ROUT_SHARE;
    if beaten && !band_mate_near(ctx.state, id, m.pos) {
        // Routed: it runs for the road and stops for nobody.
        let m = ctx.m(id);
        let order = m.raid.get_or_insert_with(RaidOrder::default);
        order.leaving = true;
        order.target = None;
        order.work_until_tick = 0;
        m.phase = MonsterPhase::Roam;
        m.target_id = None;
        return;
    }
    if order.leaving && beaten {
        ctx.m(id).phase = MonsterPhase::Roam;
        return;
    }
    let goal = goal_of(ctx.state, &order);
    if m.phase == MonsterPhase::Chase {
        let chased = m.target_id.as_deref().and_then(|t| ctx.state.characters.get(t));
        let lost = chased.is_none_or(|c| c.inside.is_some());
        let too_far = goal.is_some_and(|goal| dist(m.pos, goal) > LEASH);
        if lost || too_far {
            let m = ctx.m(id);
            m.phase = MonsterPhase::Roam;
            m.target_id = None;
        }
        return;
    }
    if let Some(found) = find_target(ctx.state, m, def.sight) {
        let m = ctx.m(id);
        m.phase = MonsterPhase::Chase;
        m.target_id = Some(found);
        let order = m.raid.get_or_insert_with(RaidOrder::default);
        order.work_until_tick = 0;
    }
}

/// One tick of a raider: a swing in the air, a fight, the walk to the next place, the plundering, the
/// road home.
pub fn update_orc(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    if ctx.state.monsters[id].hit_tick > 0 {
        land_swing(ctx, id);
        return;
    }
    if (tick + id_number(id)) % 10 == 0 {
        look_about(ctx, id);
    }
    let m = &ctx.state.monsters[id];
    if m.phase == MonsterPhase::Chase {
        let chased = m.target_id.as_deref().and_then(|t| ctx.state.characters.get(t)).filter(|c| c.inside.is_none());
        if let Some((target_id, pos)) = chased.map(|c| (c.id.clone(), c.pos)) {
            chase_and_strike(ctx, id, &target_id, pos);
            return;
        }
        let m = ctx.m(id);
        m.phase = MonsterPhase::Roam;
        m.target_id = None;
    }

    let order = ctx.state.monsters[id].raid.clone().unwrap_or_default();
    if order.leaving {
        let exit = exit_point(ctx.state);
        // A routed raider runs; the rest walk off with their plunder.
        let hurt = ctx.state.monsters[id].hp < ctx.state.monsters[id].kind.def().max_hp * ROUT_SHARE;
        match walk_to(ctx, id, exit, if hurt { 1.2 } else { 0.9 }) {
            Some(false) => {}
            Some(true) | None => vanish(ctx, id),
        }
        return;
    }

    if order.work_until_tick > 0 {
        let Some(target) = order.target else {
            ctx.m(id).raid.get_or_insert_with(RaidOrder::default).work_until_tick = 0;
            return;
        };
        if tick >= order.work_until_tick {
            plunder(ctx, id, &target);
        } else if (order.work_until_tick - tick) % seconds_to_ticks(AnimId::Interact.seconds()).max(1) == 0 {
            set_anim(ctx.m(id), AnimId::Interact, true);
        }
        return;
    }

    let target = match order.target.filter(|t| !plundered(ctx.state, t)) {
        Some(target) => target,
        None => match choose_target(ctx.state, id) {
            Some(target) => {
                ctx.m(id).raid.get_or_insert_with(RaidOrder::default).target = Some(target.clone());
                target
            }
            None => {
                // Nothing left worth taking: home.
                ctx.m(id).raid.get_or_insert_with(RaidOrder::default).leaving = true;
                return;
            }
        },
    };
    let Some(door) = door_of(&target) else { return };
    match walk_to(ctx, id, door, 1.0) {
        Some(false) => {}
        Some(true) => {
            let origin = find_place(&target).map_or(door, |p| p.origin);
            let m = ctx.m(id);
            face(m, origin);
            set_anim(m, AnimId::Interact, true);
            m.raid.get_or_insert_with(RaidOrder::default).work_until_tick = tick + seconds_to_ticks(PLUNDER_SECONDS);
        }
        // No way there: this raider gives up and goes home.
        None => ctx.m(id).raid.get_or_insert_with(RaidOrder::default).leaving = true,
    }
}

/// "the dead" or "the raiders": who an alarm is about.
pub fn foe_noun(faction: Faction) -> &'static str {
    match faction {
        Faction::Undead => "the dead",
        Faction::Orc => "the raiders",
    }
}
