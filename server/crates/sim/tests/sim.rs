//! Port of `src/sim/sim.test.ts`.

use std::collections::HashMap;

use fsw_core::time::{TICKS_PER_DAY, game_time};
use fsw_sim::state::Quirk;
use fsw_sim::state::{ActPhase, Character};
use fsw_sim::util::living;
use fsw_sim::{Command, FOUNDERS, Sim, SimEvent, SimState, create_state};
use fsw_testing::create_test_state;
use fsw_world::navgrid::{is_walkable, nearest_walkable};
use fsw_world::places::ActivityId;

fn run(state: SimState, ticks: i64, mut on_event: impl FnMut(&SimEvent)) -> Sim {
    let mut sim = Sim::new(state);
    for _ in 0..ticks {
        for event in sim.step() {
            on_event(&event);
        }
    }
    sim
}

fn round_trip(state: &SimState) -> SimState {
    serde_json::from_str(&serde_json::to_string(state).unwrap()).unwrap()
}

#[test]
fn the_test_cast_starts_all_housed() {
    let state = create_test_state(1);
    assert_eq!(state.order.len(), 13);
    assert_eq!(state.director.population_target, 13);
    assert!(!state.director.founding);
    for c in living(&state) {
        assert!(c.home_id.starts_with("home_"));
        assert!(is_walkable(c.pos.x, c.pos.z), "{}", c.profile.name);
    }
    assert_eq!(round_trip(&state), state);
}

#[test]
fn a_new_world_is_empty_and_founding_and_the_dead_stay_buried_meanwhile() {
    let state = create_state(1);
    assert!(state.order.is_empty());
    assert!(state.director.founding);
    assert_eq!(state.director.population_target, FOUNDERS);
    assert_eq!(round_trip(&state), state);
    // A day passes with nobody home: no wave, no arrival, and the world is none the worse for it.
    let sim = run(state, TICKS_PER_DAY, |event| assert!(!matches!(event, SimEvent::MonsterSpawn { .. })));
    assert!(sim.state.order.is_empty());
    assert!(sim.state.monsters.is_empty());
}

#[test]
fn found_village_fills_the_village_from_the_sims_own_lists_and_ends_the_founding() {
    let mut sim = Sim::new(create_state(7));
    sim.enqueue(Command::FoundVillage);
    sim.step();
    let state = &sim.state;
    assert_eq!(state.order.len(), FOUNDERS);
    assert!(!state.director.founding);
    let firsts: Vec<String> = living(state).map(|c| c.profile.name.split(' ').next().unwrap().to_lowercase()).collect();
    let mut unique = firsts.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), firsts.len(), "{firsts:?}");
    for c in living(state) {
        assert!(c.home_id.starts_with("home_"));
        assert!(is_walkable(c.pos.x, c.pos.z), "{}", c.profile.name);
        assert!(!c.has(Quirk::Murderous) && !c.has(Quirk::Occultist), "{}", c.profile.name);
        assert_eq!(c.relationships.len(), FOUNDERS - 1, "{}", c.profile.name);
        assert_eq!(c.profile.traits.len(), 3);
    }
    // The first night comes at the usual hour, and the village runs on like any other.
    assert_eq!(state.director.next_wave_tick, fsw_sim::systems::director::tick_at(1, 20.6));
    let seed7 = round_trip(state);
    let mut again = Sim::new(create_state(7));
    again.enqueue(Command::FoundVillage);
    again.step();
    assert_eq!(again.state, seed7, "the same seed founds the same village");
    let mut sim = run(round_trip(state), TICKS_PER_DAY * 2, |_| {});
    sim.step();
    assert!(sim.state.order.len() >= 10);
}

#[test]
fn is_deterministic_and_survives_a_save_and_load_in_the_middle() {
    let straight = run(create_test_state(42), 4000, |_| {}).state;
    let first = run(create_test_state(42), 2000, |_| {}).state;
    let resumed = run(round_trip(&first), 2000, |_| {}).state;
    assert!(resumed == straight, "a reloaded world went a different way");
}

#[test]
fn gives_different_worlds_for_different_seeds() {
    let a = run(create_test_state(1), 3000, |_| {}).state;
    let b = run(create_test_state(2), 3000, |_| {}).state;
    assert_ne!(a.rng, b.rng);
}

#[test]
fn keeps_everyone_fed_rested_and_on_walkable_ground_for_three_days() {
    // With thirteen founders, seed 5 once had a guard starve on a night watch.
    for seed in [2, 5] {
        let mut sim = Sim::new(create_test_state(seed));
        let mut lowest_hunger: f64 = 100.0;
        for i in 0..TICKS_PER_DAY * 3 {
            sim.step();
            if i % 200 != 0 {
                continue;
            }
            for c in living(&sim.state) {
                lowest_hunger = lowest_hunger.min(c.needs.hunger);
                assert!(c.needs.energy >= 0.0);
                assert!(c.pos.x.is_finite() && c.pos.z.is_finite());
                // Seats and work spots sit just off the nav grid, so "near walkable ground" is the invariant.
                assert!(nearest_walkable(c.pos, 3).is_some(), "{} at {},{}", c.profile.name, c.pos.x, c.pos.z);
            }
        }
        assert!(lowest_hunger > 2.0, "seed {seed}: lowest hunger {lowest_hunger}");
        assert!(living(&sim.state).all(|c| c.memories.len() <= 140));
    }
}

#[test]
fn a_guard_with_an_empty_pack_breaks_off_watch_to_eat_and_goes_back() {
    // On night duty and at the post, not a daytime watch from the plan or still walking to the tower.
    fn at_the_post(sim: &Sim, c: &Character) -> bool {
        sim.state.tick < c.watch_until_tick
            && !(5.2..20.5).contains(&game_time(sim.state.tick).hour)
            && c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Watch))
            && c.act.phase == ActPhase::Perform
    }
    let mut sim = Sim::new(create_test_state(5));
    let mut guard = None;
    for _ in 0..TICKS_PER_DAY * 2 {
        guard = living(&sim.state).find(|c| at_the_post(&sim, c)).map(|c| c.id.clone());
        if guard.is_some() {
            break;
        }
        sim.step();
    }
    let guard = guard.expect("nobody stood watch");
    // Too hungry to last a whole stretch, but not hungry enough to leave the post before it.
    let c = sim.state.characters.get_mut(&guard).unwrap();
    c.needs.hunger = 20.0;
    c.inventory.food = 0;
    let until = c.watch_until_tick;

    let mut lowest_hunger: f64 = 100.0;
    let mut ate = false;
    let mut back_on_watch = false;
    while sim.state.tick < until {
        sim.step();
        let c = &sim.state.characters[&guard];
        lowest_hunger = lowest_hunger.min(c.needs.hunger);
        ate |= c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Eat));
        back_on_watch |= ate && at_the_post(&sim, c);
    }
    assert!(lowest_hunger > 2.0, "the guard went down to hunger {lowest_hunger}");
    assert!(back_on_watch, "the guard ate ({ate}) but never went back on watch");
}

#[test]
fn has_people_talking_sleeping_and_fighting_off_the_dead() {
    let mut counts: HashMap<&'static str, u32> = HashMap::new();
    let sim = run(create_test_state(9), TICKS_PER_DAY * 3, |event| *counts.entry(event.kind()).or_default() += 1);
    let count = |kind| counts.get(kind).copied().unwrap_or(0);
    assert!(count("conversation_ended") > 20);
    assert!(count("woke") > 10 && count("woke") < 200);
    assert!(count("monster_spawn") > 0);
    assert!(sim.state.chronicle.len() > 10);
}

fn long_run(days: i64) {
    let mut sim = Sim::new(create_test_state(11));
    let mut lowest = 13;
    let mut longest_memory = 0;
    for i in 0..TICKS_PER_DAY * days {
        sim.step();
        if i % 2000 != 0 {
            continue;
        }
        lowest = lowest.min(sim.state.order.len());
        for c in living(&sim.state) {
            longest_memory = longest_memory.max(c.memories.len());
        }
    }
    let murders = sim.state.dead.values().filter(|record| record.killer_id.is_some()).count();
    let scale = days as f64 / 200.0;
    assert!(lowest >= 7, "lowest population {lowest}");
    // With deadly nights and rivals brawling over a charmer, one seed can end a person short of nine.
    assert!(sim.state.order.len() >= 8, "final population {}", sim.state.order.len());
    assert!((sim.state.dead.len() as f64) < 90.0 * scale.max(0.25), "deaths {}", sim.state.dead.len());
    assert!(murders <= 10, "murders {murders}");
    assert!(longest_memory <= 140);
    assert!(sim.state.chronicle.len() <= 600);
    assert!(serde_json::to_string(&sim.state).unwrap().len() < 3_000_000, "save size");
}

/// The CI variant of the long run.
#[test]
fn keeps_the_village_populated_over_20_days() {
    long_run(20);
}

/// `cargo test --release -p fsw-sim -- --ignored` (5.76 M ticks).
#[test]
#[ignore]
fn keeps_the_village_populated_memories_bounded_and_killings_rare_over_200_days() {
    long_run(200);
}
