//! Port of `src/sim/sim.test.ts`.

use std::collections::HashMap;

use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::util::living;
use fsw_sim::{Sim, SimEvent, SimState, create_state};
use fsw_world::navgrid::{is_walkable, nearest_walkable};

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
fn starts_with_the_authored_cast_all_housed() {
    let state = create_state(1);
    assert_eq!(state.order.len(), 13);
    assert_eq!(state.director.population_target, 13);
    for c in living(&state) {
        assert!(c.home_id.starts_with("home_"));
        assert!(is_walkable(c.pos.x, c.pos.z), "{}", c.profile.name);
    }
    assert_eq!(round_trip(&state), state);
}

#[test]
fn is_deterministic_and_survives_a_save_and_load_in_the_middle() {
    let straight = run(create_state(42), 4000, |_| {}).state;
    let first = run(create_state(42), 2000, |_| {}).state;
    let resumed = run(round_trip(&first), 2000, |_| {}).state;
    assert!(resumed == straight, "a reloaded world went a different way");
}

#[test]
fn gives_different_worlds_for_different_seeds() {
    let a = run(create_state(1), 3000, |_| {}).state;
    let b = run(create_state(2), 3000, |_| {}).state;
    assert_ne!(a.rng, b.rng);
}

#[test]
fn keeps_everyone_fed_rested_and_on_walkable_ground_for_three_days() {
    let mut sim = Sim::new(create_state(2));
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
    assert!(lowest_hunger > 2.0);
    assert!(living(&sim.state).all(|c| c.memories.len() <= 140));
}

#[test]
fn has_people_talking_sleeping_and_fighting_off_the_dead() {
    let mut counts: HashMap<&'static str, u32> = HashMap::new();
    let sim = run(create_state(9), TICKS_PER_DAY * 3, |event| *counts.entry(event.kind()).or_default() += 1);
    let count = |kind| counts.get(kind).copied().unwrap_or(0);
    assert!(count("conversation_ended") > 20);
    assert!(count("woke") > 10 && count("woke") < 200);
    assert!(count("monster_spawn") > 0);
    assert!(sim.state.chronicle.len() > 10);
}

fn long_run(days: i64) {
    let mut sim = Sim::new(create_state(11));
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
