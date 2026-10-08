//! Orc raids: when a war band comes, what it takes, how it leaves, and what it leaves behind.

use fsw_core::math2d::dist;
use fsw_core::time::{TICKS_PER_DAY, TICKS_PER_GAME_MINUTE, game_time};
use fsw_sim::Command;
use fsw_sim::Sim;
use fsw_sim::commands::TuningPatch;
use fsw_sim::data::monsters::Faction;
use fsw_sim::state::{
    ActivityId, Haul, Intent, IntentAction, IntentSource, ItemId, LootKind, MonsterPhase, RaidOrder, Resource, SimState,
};
use fsw_sim::systems::combat::damage_monster;
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::fire::ignite;
use fsw_sim::systems::raids::{next_raid_tick, plundered};
use fsw_sim::util::{Ctx, living};
use fsw_testing::create_test_state;
use fsw_world::places::get_place;

fn quiet(seed: u32) -> Sim {
    let mut sim = Sim::new(create_test_state(seed));
    sim.state.director.next_wave_tick = i64::MAX;
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    sim
}

fn run_until(sim: &mut Sim, ticks: i64, until: impl Fn(&SimState) -> bool) -> i64 {
    for i in 0..ticks {
        if until(&sim.state) {
            return i;
        }
        sim.step();
    }
    ticks
}

fn with<R>(sim: &mut Sim, f: impl FnOnce(&mut Ctx) -> R) -> R {
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut sim.state, &mut events);
    f(&mut ctx)
}

/// A quiet village a moment before its first raid is due.
fn before_the_first_raid(seed: u32) -> Sim {
    let mut sim = quiet(seed);
    let due = next_raid_tick(&sim.state);
    sim.state.tick = due - 5;
    sim
}

fn orcs(state: &SimState) -> Vec<String> {
    state.monsters.values().filter(|m| m.kind.faction() == Faction::Orc).map(|m| m.id.clone()).collect()
}

/// Everyone goes indoors and stays there, too timid to answer the alarm, so nobody fights the band while a test
/// watches it.
fn shut_in(state: &mut SimState) {
    for c in state.characters.values_mut() {
        c.profile.bravery = 0.0;
        c.inside = Some(c.home_id.clone());
        c.intent = Some(Intent::new(IntentAction::Wait { minutes: 900.0 }, IntentSource::Routine));
        c.queue.clear();
    }
}

#[test]
fn nothing_changes_before_a_raid_is_possible() {
    let mut with_raids = quiet(21);
    let mut without = quiet(21);
    without.state.director.next_raid_tick = i64::MAX;
    let first = next_raid_tick(&with_raids.state);
    assert!(game_time(first).day >= 4, "the first raid comes on day 4 at the earliest");
    for _ in 0..(first - 1).min(3 * TICKS_PER_DAY) {
        with_raids.step();
        without.step();
    }
    without.state.director.next_raid_tick = 0;
    assert!(with_raids.state == without.state, "a world before its first raid is the world it always was");
}

#[test]
fn a_raid_comes_up_the_west_road_on_schedule() {
    let mut sim = before_the_first_raid(22);
    let due = next_raid_tick(&sim.state);
    let hour = game_time(due).hour;
    assert!((9.0..16.0).contains(&hour), "by day: {hour}");
    run_until(&mut sim, 20, |s| s.raid.is_some());
    let band = sim.state.raid.clone().expect("a war band");
    assert_eq!(band.started_tick, due);
    assert!((3..=6).contains(&band.size), "{} orcs", band.size);
    let road = get_place("west_road");
    for id in &band.members {
        let m = &sim.state.monsters[id];
        assert_eq!(m.kind.faction(), Faction::Orc);
        assert_ne!(m.phase, MonsterPhase::Rising, "orcs do not climb out of the ground");
        assert!(dist(m.pos, road.origin) < 10.0, "came up the west road");
    }
    let next = sim.state.director.next_raid_tick;
    let days = (next - due) as f64 / TICKS_PER_DAY as f64;
    assert!((4.0..10.0).contains(&days), "next raid {days} days on");
    assert!((9.0..16.0).contains(&game_time(next).hour));
    assert_eq!(sim.state.director.raids, 1);

    // The village hears of it soon after.
    run_until(&mut sim, 30 * 20, |s| s.raid.as_ref().is_some_and(|b| b.warned));
    assert!(sim.state.director.alarm_raid, "the alarm is for raiders");
}

#[test]
fn no_raid_comes_while_a_house_burns_or_on_a_village_too_small_to_notice() {
    let mut sim = before_the_first_raid(23);
    let due = next_raid_tick(&sim.state);
    let home = sim.state.characters["c1"].home_id.clone();
    with(&mut sim, |ctx| ignite(ctx, &home));
    run_until(&mut sim, 20, |_| false);
    assert!(sim.state.raid.is_none(), "not with a house on fire");
    assert_eq!(sim.state.director.next_raid_tick, due + TICKS_PER_DAY, "it waits for the next day");

    let mut sim = before_the_first_raid(24);
    let due = next_raid_tick(&sim.state);
    let keep: Vec<String> = sim.state.order.iter().take(5).cloned().collect();
    sim.state.characters.retain(|id, _| keep.contains(id));
    sim.state.order.retain(|id| keep.contains(id));
    run_until(&mut sim, 20, |_| false);
    assert!(sim.state.raid.is_none(), "five villagers are beneath a war band's notice");
    assert_eq!(sim.state.director.next_raid_tick, due + TICKS_PER_DAY);
}

#[test]
fn raiders_plunder_coin_goods_and_materials_and_a_plundered_place_yields_nothing() {
    let mut sim = before_the_first_raid(25);
    sim.state.economy.stock_up(ItemId::Food, 2);
    sim.state.economy.stock_up(ItemId::Gift, 4);
    sim.state.stockpiles.entry("market".into()).or_default().add(Resource::Wood, 8, i64::MAX);
    run_until(&mut sim, 20, |s| s.raid.is_some());
    shut_in(&mut sim.state);
    let purse = sim.state.economy.purse;
    // One of them is set on the market, right at its door.
    let raider = orcs(&sim.state)[0].clone();
    let door = get_place("market").door;
    {
        let m = sim.state.monsters.get_mut(&raider).unwrap();
        m.pos = door;
        m.raid = Some(RaidOrder { target: Some("market".into()), ..Default::default() });
    }
    run_until(&mut sim, 60 * 20, |s| plundered(s, "market"));
    assert!(plundered(&sim.state, "market"));
    let haul = &sim.state.monsters[&raider].raid.as_ref().unwrap().haul;
    assert_eq!(haul.coins, purse.min(12), "coin from the chest");
    assert_eq!(sim.state.economy.purse, purse - haul.coins);
    assert_eq!(haul.goods.values().sum::<i64>(), 3, "three things off the stalls");
    assert_eq!(haul.materials.get(Resource::Wood), 8, "the market's wood");
    assert_eq!(haul.goods.get(&ItemId::Food), Some(&2), "food first, then what else the stalls hold");
    assert!(sim.state.raid.as_ref().unwrap().plundered.contains(&"market".to_owned()));
    assert!(sim.state.chronicle.iter().any(|e| e.text.starts_with("Orc raiders plundered the market")));
    let until = sim.state.plundered["market"];
    assert!(until - sim.state.tick <= TICKS_PER_DAY, "for a day");

    // Work at a spoiled farm yields no food; at a whole one it does.
    let food_after = |spoiled: bool| {
        let mut sim = quiet(26);
        if spoiled {
            sim.state.plundered.insert("farm".into(), i64::MAX);
        }
        let id = sim.state.order[0].clone();
        sim.state.characters.get_mut(&id).unwrap().inventory.food = 0;
        let epoch = sim.state.characters[&id].epoch;
        let farm = IntentAction::Activity { activity: ActivityId::Farm, place_id: None, minutes: Some(150.0) };
        sim.enqueue(Command::SetIntents { id: id.clone(), epoch, intents: vec![Intent::new(farm, IntentSource::Llm)] });
        run_until(&mut sim, 200 * TICKS_PER_GAME_MINUTE, |_| false);
        sim.state.characters[&id].inventory.food
    };
    assert!(food_after(false) > 0, "farming grows food");
    assert_eq!(food_after(true), 0, "not on a plundered farm");
}

#[test]
fn the_band_turns_for_home_with_its_loot_when_half_has_fallen_and_at_five() {
    // With its loot.
    let mut sim = before_the_first_raid(27);
    run_until(&mut sim, 20, |s| s.raid.is_some());
    shut_in(&mut sim.state);
    let size = sim.state.raid.as_ref().unwrap().size;
    sim.state.raid.as_mut().unwrap().plundered = (0..size).map(|i| format!("place{i}")).collect();
    sim.step();
    assert!(sim.state.raid.as_ref().unwrap().leaving);
    for id in orcs(&sim.state) {
        assert!(sim.state.monsters[&id].raid.as_ref().unwrap().leaving);
    }

    // With half of it fallen.
    let mut sim = before_the_first_raid(28);
    run_until(&mut sim, 20, |s| s.raid.is_some());
    shut_in(&mut sim.state);
    let band = orcs(&sim.state);
    let killer = sim.state.order[0].clone();
    for id in band.iter().take(band.len().div_ceil(2)) {
        with(&mut sim, |ctx| damage_monster(ctx, id, 10_000, &killer));
    }
    sim.step();
    assert!(sim.state.raid.as_ref().unwrap().leaving, "half the band has fallen");

    // At five in the afternoon.
    let mut sim = before_the_first_raid(29);
    run_until(&mut sim, 20, |s| s.raid.is_some());
    shut_in(&mut sim.state);
    sim.state.tick = tick_at(game_time(sim.state.tick).day, 17.0);
    sim.step();
    assert!(sim.state.raid.as_ref().unwrap().leaving, "the band goes home at five");
}

#[test]
fn a_band_that_gets_away_keeps_its_plunder_and_a_fallen_raider_drops_it() {
    let mut sim = before_the_first_raid(30);
    run_until(&mut sim, 20, |s| s.raid.is_some());
    shut_in(&mut sim.state);
    let haul = || {
        let mut haul = Haul { coins: 7, ..Default::default() };
        haul.goods.insert(ItemId::Food, 2);
        haul.materials.add(Resource::Stone, 4, i64::MAX);
        haul
    };
    let band = orcs(&sim.state);
    let (fallen, away) = (band[0].clone(), band[1].clone());
    for id in [&fallen, &away] {
        sim.state.monsters.get_mut(id).unwrap().raid = Some(RaidOrder { haul: haul(), ..Default::default() });
    }

    // One falls, on the green, away from the rest: its plunder lies where it fell.
    let killer = sim.state.order[0].clone();
    let at = get_place("green").door;
    sim.state.monsters.get_mut(&fallen).unwrap().pos = at;
    with(&mut sim, |ctx| damage_monster(ctx, &fallen, 10_000, &killer));
    let kinds: Vec<LootKind> = sim.state.loot.values().map(|l| l.kind).collect();
    assert!(kinds.contains(&LootKind::Coins { count: 7 }));
    assert!(kinds.contains(&LootKind::Goods { item: ItemId::Food, count: 2 }));
    assert!(kinds.contains(&LootKind::Materials { resource: Resource::Stone, count: 4 }));
    assert!(sim.state.loot.values().all(|l| dist(l.pos, at) < 1.5));

    // A villager picks the stone up and carries it.
    let stone = sim
        .state
        .loot
        .iter()
        .find(|(_, l)| matches!(l.kind, LootKind::Materials { .. }))
        .map(|(id, _)| id.clone())
        .unwrap();
    {
        let c = sim.state.characters.get_mut(&killer).unwrap();
        c.inside = None;
        c.carried = Default::default();
        c.pos = at;
        c.intent = None;
    }
    let epoch = sim.state.characters[&killer].epoch;
    let pick = IntentAction::PickUp { loot_id: stone.clone() };
    sim.enqueue(Command::SetIntents { id: killer.clone(), epoch, intents: vec![Intent::new(pick, IntentSource::Llm)] });
    run_until(&mut sim, 30 * 20, |s| !s.loot.contains_key(&stone));
    assert_eq!(sim.state.characters[&killer].carried.get(Resource::Stone), 4);

    // The rest run for the road; what they carry is gone with them.
    for id in orcs(&sim.state) {
        let m = sim.state.monsters.get_mut(&id).unwrap();
        m.raid.get_or_insert_with(RaidOrder::default).leaving = true;
        m.pos = get_place("west_road").origin;
    }
    run_until(&mut sim, 120 * 20, |s| s.raid.is_none());
    assert!(sim.state.raid.is_none(), "the band is gone");
    assert!(orcs(&sim.state).is_empty());
    let last = sim.state.chronicle.iter().rev().find(|e| e.text.contains("carried off")).expect("the news");
    assert!(last.text.contains("7 coins") && last.text.contains("2 food"), "{}", last.text);
}

#[test]
fn raiders_neither_crumble_nor_are_buried_and_lie_where_they_fall() {
    let mut sim = before_the_first_raid(31);
    run_until(&mut sim, 20, |s| s.raid.is_some());
    shut_in(&mut sim.state);
    run_until(&mut sim, 200, |_| false);
    assert!(orcs(&sim.state).iter().all(|id| sim.state.monsters[id].phase != MonsterPhase::Dying), "daylight");

    let raider = orcs(&sim.state)[0].clone();
    let killer = sim.state.order[0].clone();
    let graves = sim.state.graves.len();
    with(&mut sim, |ctx| damage_monster(ctx, &raider, 10_000, &killer));
    run_until(&mut sim, 6 * 20 + 2, |_| false);
    assert!(!sim.state.monsters.contains_key(&raider));
    assert!(sim.state.corpses.iter().any(|c| c.id == raider), "the body lies where it fell");
    assert_eq!(sim.state.graves.len(), graves, "and nobody buries it");
    let killer_memory = &sim.state.characters[&killer].memories;
    assert!(killer_memory.iter().any(|m| m.text.starts_with("Slew an orc raider")));
}

#[test]
fn a_save_in_the_middle_of_a_raid_comes_back_as_it_was() {
    let mut sim = before_the_first_raid(32);
    run_until(&mut sim, 30 * 20, |s| s.raid.as_ref().is_some_and(|b| b.warned));
    run_until(&mut sim, 200, |_| false);
    assert!(sim.state.raid.is_some());
    let json = serde_json::to_string(&sim.state).unwrap();
    let loaded: SimState = serde_json::from_str(&json).unwrap();
    assert!(loaded == sim.state);
    let mut again = Sim::new(loaded);
    for _ in 0..1500 {
        sim.step();
        again.step();
    }
    assert!(again.state == sim.state, "a reloaded raid goes on exactly as it would have");
    let alive = living(&sim.state).count();
    assert!(alive > 0);
}

#[test]
fn a_called_raid_comes_at_once_by_day_at_nine_by_night_and_never_twice() {
    // By day it comes on the next tick, long before one is due and whatever the village's size.
    let mut sim = quiet(33);
    sim.state.tick = tick_at(2, 12.0);
    let keep: Vec<String> = sim.state.order.iter().take(4).cloned().collect();
    sim.state.characters.retain(|id, _| keep.contains(id));
    sim.state.order.retain(|id| keep.contains(id));
    sim.enqueue(Command::CallRaid);
    run_until(&mut sim, 3, |s| s.raid.is_some());
    assert!(sim.state.raid.is_some(), "the band comes when it is called");
    assert!(!sim.state.director.raid_called);
    // A second call while it is in the valley is ignored.
    sim.enqueue(Command::CallRaid);
    sim.step();
    assert!(!sim.state.director.raid_called, "one band at a time");

    // By night it waits for morning.
    let mut sim = quiet(34);
    sim.state.tick = tick_at(2, 22.0);
    sim.enqueue(Command::CallRaid);
    sim.step();
    assert!(sim.state.director.raid_called && sim.state.raid.is_none());
    let came = run_until(&mut sim, TICKS_PER_DAY, |s| s.raid.is_some());
    assert!(came < TICKS_PER_DAY);
    let hour = game_time(sim.state.raid.as_ref().unwrap().started_tick).hour;
    assert!((9.0..9.1).contains(&hour), "at nine: {hour}");
}
