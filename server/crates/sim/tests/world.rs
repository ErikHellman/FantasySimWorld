//! The world that changes: felled trees free the ground they stood on, stumps grow back into
//! trees on a fixed clock, the walkability overlay never drifts from the objects it is derived
//! from, and all of it survives a save.

use fsw_core::math2d::dist;
use fsw_core::time::TICKS_PER_DAY;
use fsw_core::vec2;
use fsw_sim::state::{SimState, TreeStage, WorldObject};
use fsw_sim::systems::world::{FALL_TICKS, SAPLING_DAYS, STUMP_DAYS, YOUNG_DAYS, fell_tree, scatter_id, tree_standing};
use fsw_sim::util::Ctx;
use fsw_sim::{Sim, create_state};
use fsw_world::navgrid::{AGENT_RADIUS, nav_grid};
use fsw_world::places::{get_place, obstacle_contains};
use fsw_world::scatter::{is_tree, scatter};

fn round_trip(state: &SimState) -> SimState {
    serde_json::from_str(&serde_json::to_string(state).unwrap()).unwrap()
}

/// Trees of the map within `reach` of a place, nearest first.
fn trees_near(place: &str, reach: f64) -> Vec<usize> {
    let origin = get_place(place).origin;
    let mut trees: Vec<(f64, usize)> = scatter()
        .models
        .iter()
        .enumerate()
        .filter(|(i, m)| is_tree(&m.model) && scatter().obstacle_of_model(*i).is_some())
        .map(|(i, m)| (dist(origin, vec2(m.x, m.z)), i))
        .filter(|(d, _)| *d < reach)
        .collect();
    trees.sort_by(|a, b| a.0.total_cmp(&b.0));
    trees.into_iter().map(|(_, i)| i).collect()
}

fn fell(sim: &mut Sim, index: usize) -> bool {
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut sim.state, &mut events);
    fell_tree(&mut ctx, index)
}

fn walkable_cells(state: &SimState) -> usize {
    state.nav.costs().iter().filter(|&&c| c > 0.0).count()
}

#[test]
fn an_untouched_world_keeps_the_static_grid() {
    // The first hour of a new world: breakfast, before anyone has gone out to the woods.
    let mut sim = Sim::new(create_state(3));
    for _ in 0..600 {
        sim.step();
    }
    assert!(sim.state.objects.is_empty());
    assert!(!sim.state.nav.is_live());
    assert!(std::ptr::eq(sim.state.nav.costs().as_ptr(), nav_grid().cost.as_ptr()));
    let json = serde_json::to_string(&sim.state).unwrap();
    assert!(!json.contains("\"objects\""), "an empty objects map is left out of a save");
}

#[test]
fn a_felled_tree_frees_the_ground_round_it_but_for_the_stump() {
    let mut sim = Sim::new(create_state(5));
    let tree = trees_near("lumber_camp", 40.0)[0];
    let model = &scatter().models[tree];
    let trunk = scatter().obstacle_of_model(tree).unwrap();
    let before = walkable_cells(&sim.state);
    assert!(fell(&mut sim, tree));
    assert!(!fell(&mut sim, tree), "a tree comes down once");
    assert!(!tree_standing(&sim.state, tree));
    // While it leans over it still blocks the ground.
    assert!(!sim.state.nav.is_live());
    for _ in 0..FALL_TICKS {
        sim.step();
    }
    assert!(matches!(sim.state.objects[&scatter_id(tree)], WorldObject::Tree { stage: TreeStage::Stump, .. }));
    assert!(sim.state.nav.is_live());
    let after = walkable_cells(&sim.state);
    assert!(after > before, "the ground the canopy shaded is free again ({before} -> {after})");

    let grid = nav_grid();
    let others: Vec<_> = scatter()
        .obstacles
        .iter()
        .filter(|o| !std::ptr::eq(*o, trunk))
        .chain(get_place("lumber_camp").obstacles.iter())
        .collect();
    for (index, (&now, &was)) in sim.state.nav.costs().iter().zip(&grid.cost).enumerate() {
        let centre = fsw_world::navgrid::cell_center(index);
        let near = dist(centre, vec2(model.x, model.z)) < 4.0;
        if !near {
            assert_eq!(now.to_bits(), was.to_bits(), "a cell far from the tree changed");
            continue;
        }
        let in_trunk = obstacle_contains(trunk, centre.x, centre.z, AGENT_RADIUS);
        let in_stump = dist(centre, vec2(model.x, model.z)) <= fsw_sim::systems::world::STUMP_RADIUS + AGENT_RADIUS;
        let in_other = others.iter().any(|o| obstacle_contains(o, centre.x, centre.z, AGENT_RADIUS));
        if in_stump || in_other {
            assert_eq!(now, 0.0, "cell {index} by the stump or another obstacle should be blocked");
        } else if in_trunk && was == 0.0 {
            assert!(
                now > 0.0 || fsw_world::navgrid::terrain_cost(centre.x, centre.z) == 0.0,
                "cell {index} should be free"
            );
        } else {
            assert_eq!(now.to_bits(), was.to_bits(), "cell {index} outside the trunk changed");
        }
    }
}

#[test]
fn a_stump_grows_back_into_a_tree_on_the_clock() {
    let mut sim = Sim::new(create_state(5));
    let tree = trees_near("glade", 40.0)[0];
    let id = scatter_id(tree);
    assert!(fell(&mut sim, tree));
    let felled_at = sim.state.tick;
    let stage = |state: &SimState| match state.objects.get(&id) {
        Some(WorldObject::Tree { stage, .. }) => Some(*stage),
        Some(_) => panic!("not a tree"),
        None => None,
    };
    let run_to = |sim: &mut Sim, tick: i64| {
        while sim.state.tick < tick {
            sim.step();
        }
    };
    run_to(&mut sim, felled_at + FALL_TICKS - 1);
    assert_eq!(stage(&sim.state), Some(TreeStage::Falling));
    run_to(&mut sim, felled_at + FALL_TICKS);
    assert_eq!(stage(&sim.state), Some(TreeStage::Stump));
    let stump_at = sim.state.tick;
    run_to(&mut sim, stump_at + STUMP_DAYS * TICKS_PER_DAY - 20);
    assert_eq!(stage(&sim.state), Some(TreeStage::Stump));
    run_to(&mut sim, stump_at + STUMP_DAYS * TICKS_PER_DAY + 20);
    assert_eq!(stage(&sim.state), Some(TreeStage::Sapling));
    let sapling_at = sim.state.objects[&id].since_tick();
    run_to(&mut sim, sapling_at + SAPLING_DAYS * TICKS_PER_DAY + 20);
    assert_eq!(stage(&sim.state), Some(TreeStage::Young));
    let young_at = sim.state.objects[&id].since_tick();
    assert!(sim.state.nav.is_live());
    run_to(&mut sim, young_at + YOUNG_DAYS * TICKS_PER_DAY + 20);
    assert_eq!(stage(&sim.state), None, "a grown tree is the map's own again");
    assert!(tree_standing(&sim.state, tree));
    // The woodcutters have been at other trees meanwhile; only this one is checked to be whole again.
    assert!(!sim.state.objects.contains_key(&id));
}

#[test]
fn the_overlay_never_drifts_from_the_objects() {
    let mut sim = Sim::new(create_state(11));
    let trees = trees_near("lumber_camp", 60.0);
    for tick in 0..2000 {
        if tick % 150 == 0 {
            let index = trees[(tick / 150) as usize % trees.len()];
            fell(&mut sim, index);
        }
        sim.step();
        let mut fresh = sim.state.clone();
        fresh.nav = Default::default();
        fsw_sim::nav::refresh(&mut fresh);
        assert_eq!(fresh.nav.costs(), sim.state.nav.costs(), "tick {}", sim.state.tick);
        assert_eq!(fresh.nav.is_live(), sim.state.nav.is_live());
    }
    assert!(sim.state.objects.len() >= 10);
}

#[test]
fn felled_trees_survive_a_save_and_load() {
    let mut sim = Sim::new(create_state(7));
    let trees = trees_near("lumber_camp", 50.0);
    fell(&mut sim, trees[0]);
    for _ in 0..10 {
        sim.step();
    }
    // Mid-fall.
    let saved = round_trip(&sim.state);
    assert_eq!(saved, sim.state);
    let mut resumed = Sim::new(saved);
    assert_eq!(resumed.state.nav.costs(), sim.state.nav.costs());
    fell(&mut sim, trees[1]);
    fell(&mut resumed, trees[1]);
    for _ in 0..3000 {
        sim.step();
        resumed.step();
    }
    assert!(resumed.state == sim.state, "a reloaded felling went a different way");
    assert_eq!(resumed.state.nav.costs(), sim.state.nav.costs());

    // And as a sapling, days later.
    while sim.state.tick < (STUMP_DAYS + 1) * TICKS_PER_DAY {
        sim.step();
    }
    assert!(matches!(sim.state.objects[&scatter_id(trees[0])], WorldObject::Tree { stage: TreeStage::Sapling, .. }));
    let mut resumed = Sim::new(round_trip(&sim.state));
    for _ in 0..TICKS_PER_DAY {
        sim.step();
        resumed.step();
    }
    assert!(resumed.state == sim.state, "a reloaded sapling went a different way");
}

// ---- Work that changes the world: felling, mining, carrying -------------------------------------

use fsw_sim::Command;
use fsw_sim::commands::TuningPatch;
use fsw_sim::state::{Intent, IntentAction, IntentSource, Resource, RockStage, Store, WorldJob};
use fsw_sim::systems::world::{STOCKPILES, deposit, job_accepts, rock_whole, stockpile_for};
use fsw_sim::util::living;

fn quiet(seed: u32) -> Sim {
    let mut sim = Sim::new(create_state(seed));
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

fn set_work(sim: &mut Sim, id: &str, job: WorldJob, target: &str) {
    let epoch = sim.state.characters[id].epoch;
    let intent = Intent::new(IntentAction::WorldWork { job, target: target.into(), minutes: None }, IntentSource::Llm);
    sim.enqueue(Command::SetIntents { id: id.into(), epoch, intents: vec![intent] });
}

fn wood_at(state: &SimState, place: &str) -> i64 {
    state.stockpiles.get(place).map_or(0, |store| store.get(Resource::Wood))
}

#[test]
fn woodcutters_fell_real_trees_and_stack_the_logs_at_the_camp() {
    let mut sim = quiet(1);
    let woodcutters: Vec<String> = living(&sim.state)
        .filter(|c| c.jobs.contains(&fsw_sim::state::ActivityId::ChopWood))
        .map(|c| c.id.clone())
        .collect();
    assert!(!woodcutters.is_empty());
    let felled = |state: &SimState| state.objects.values().any(|o| matches!(o, WorldObject::Tree { .. }));
    let took = run_until(&mut sim, TICKS_PER_DAY, felled);
    assert!(took < TICKS_PER_DAY, "no tree came down in a day");
    let logs_home = |state: &SimState| wood_at(state, "lumber_camp") > 0;
    let took = run_until(&mut sim, TICKS_PER_DAY, logs_home);
    assert!(took < TICKS_PER_DAY, "no logs reached the camp in a day");
    // A woodcutter was at it, with an axe, on the way or at work: the status line says so.
    let lines: Vec<String> = sim.state.chronicle.iter().map(|e| e.text.clone()).collect();
    assert!(!lines.iter().any(|l| l.contains("felled")), "felling a tree is not chronicle news: {lines:?}");
    for id in &woodcutters {
        if let Some(c) = sim.state.characters.get(id) {
            assert!(c.carried.total() <= fsw_sim::state::CARRY_CAP);
        }
    }
    // Felled trees are only ever those within reach of the camp.
    let camp = get_place("lumber_camp").origin;
    for (id, object) in &sim.state.objects {
        if let WorldObject::Tree { .. } = object {
            let index = fsw_sim::systems::world::scatter_index(id).unwrap();
            let model = &scatter().models[index];
            assert!(dist(camp, vec2(model.x, model.z)) <= fsw_sim::systems::world::FELLING_REACH + 0.01);
        }
    }
}

#[test]
fn two_at_one_tree_bring_it_down_sooner() {
    let tree = scatter_id(trees_near("green", 30.0)[0]);
    let time_to_fell = |workers: &[&str]| {
        let mut sim = quiet(4);
        sim.state.director.next_wave_tick = i64::MAX;
        for id in workers {
            set_work(&mut sim, id, WorldJob::Fell, &tree);
        }
        let down = |state: &SimState| {
            matches!(
                state.objects.get(&tree),
                Some(WorldObject::Tree { stage: TreeStage::Falling | TreeStage::Stump, .. })
            )
        };
        run_until(&mut sim, 6000, down)
    };
    let alone = time_to_fell(&["c1"]);
    let together = time_to_fell(&["c1", "c2"]);
    assert!(alone < 6000 && together < 6000, "{alone} {together}");
    assert!(together < alone - 300, "two should be well ahead of one: {together} vs {alone}");
}

#[test]
fn mining_a_rock_leaves_it_mined_out_and_the_stone_goes_to_the_mine() {
    let mut sim = quiet(2);
    sim.state.director.next_wave_tick = i64::MAX;
    let mine = get_place("mine").origin;
    let rock = scatter()
        .models
        .iter()
        .enumerate()
        .find(|(_, m)| m.model.starts_with("Rock") && dist(mine, vec2(m.x, m.z)) < 45.0)
        .map(|(i, _)| scatter_id(i))
        .expect("a rock near the mine");
    assert!(rock_whole(&sim.state, fsw_sim::systems::world::scatter_index(&rock).unwrap()));
    set_work(&mut sim, "c1", WorldJob::Mine, &rock);
    let mined = |state: &SimState| {
        matches!(state.objects.get(&rock), Some(WorldObject::Rock { stage: RockStage::MinedOut, .. }))
    };
    assert!(run_until(&mut sim, 8000, mined) < 8000);
    assert!(!job_accepts(&sim.state, WorldJob::Mine, &rock));
    let stone_home = |state: &SimState| state.stockpiles.get("mine").is_some_and(|s| s.get(Resource::Stone) > 0);
    assert!(run_until(&mut sim, 4000, stone_home) < 4000, "the stone never reached the mine");
}

#[test]
fn carrying_is_the_sims_own_doing_and_the_arms_hold_only_so_much() {
    let mut sim = quiet(3);
    set_work(&mut sim, "c1", WorldJob::Deliver, "lumber_camp");
    sim.step();
    assert!(
        !sim.state.characters["c1"].intent.as_ref().is_some_and(|i| matches!(i.action, IntentAction::WorldWork { .. }))
    );

    let mut arms = Store::default();
    assert_eq!(arms.add(Resource::Wood, 4, fsw_sim::state::CARRY_CAP), 4);
    assert_eq!(arms.add(Resource::Stone, 5, fsw_sim::state::CARRY_CAP - arms.total()), 2);
    assert_eq!(arms.total(), 6);
    assert_eq!(arms.describe(), "4 logs of wood and 2 blocks of stone");
    assert_eq!(arms.take(Resource::Wood, 10), 4);
    assert_eq!(arms.get(Resource::Wood), 0);
    assert_eq!(serde_json::to_string(&arms).unwrap(), r#"{"stone":2}"#);
    assert_eq!(serde_json::to_string(&Store::default()).unwrap(), "{}");
}

#[test]
fn a_full_stockpile_overflows_to_the_market() {
    let mut state = create_state(1);
    let (camp, _, cap) = STOCKPILES[0];
    assert_eq!(stockpile_for(Resource::Wood), camp);
    assert_eq!(stockpile_for(Resource::Ore), "mine");
    let mut arms = Store::default();
    arms.add(Resource::Wood, cap + 5, i64::MAX);
    let put = deposit(&mut state, camp, &mut arms);
    assert_eq!(put.get(Resource::Wood), cap + 5);
    assert_eq!(state.stockpiles[camp].get(Resource::Wood), cap);
    assert_eq!(state.stockpiles["market"].get(Resource::Wood), 5);
    assert!(arms.is_empty());
    // Stone has no place at the camp: it goes straight to the market.
    arms.add(Resource::Stone, 2, i64::MAX);
    deposit(&mut state, camp, &mut arms);
    assert_eq!(state.stockpiles["market"].get(Resource::Stone), 2);
    let json = serde_json::to_string(&state).unwrap();
    assert!(json.contains(r#""stockpiles":{"lumber_camp":{"wood":60},"market":{"wood":5,"stone":2}}"#), "{json}");
}
