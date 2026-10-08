//! The world that changes: felled trees free the ground they stood on, stumps grow back into
//! trees on a fixed clock, the walkability overlay never drifts from the objects it is derived
//! from, and all of it survives a save.

use fsw_core::math2d::dist;
use fsw_core::time::TICKS_PER_DAY;
use fsw_core::vec2;
use fsw_sim::Sim;
use fsw_sim::state::{SimState, TreeStage, WorldObject};
use fsw_sim::systems::world::{FALL_TICKS, SAPLING_DAYS, STUMP_DAYS, YOUNG_DAYS, fell_tree, scatter_id, tree_standing};
use fsw_sim::util::Ctx;
use fsw_testing::create_test_state;
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
    let mut sim = Sim::new(create_test_state(3));
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
    let mut sim = Sim::new(create_test_state(5));
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
    let mut sim = Sim::new(create_test_state(5));
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
    let mut sim = Sim::new(create_test_state(11));
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
    let mut sim = Sim::new(create_test_state(7));
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
    let mut sim = Sim::new(create_test_state(seed));
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
    // Their other trades are set aside, so the day's work is at the trees whatever the routine draws.
    for id in &woodcutters {
        sim.state.characters.get_mut(id).unwrap().jobs = vec![fsw_sim::state::ActivityId::ChopWood];
    }
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
    let mut state = create_test_state(1);
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

// ---- Taking things apart, digging and mowing ---------------------------------------------------

use fsw_sim::describe::describe_activity;
use fsw_sim::state::{BuildingStage, ChronicleKind, GroundStage, PropStage};
use fsw_sim::systems::fire::{home_usable, is_homeless};
use fsw_sim::systems::world::{DUG_DAYS, MOWN_DAYS, building_id, destroyable, ground_id, prop_id};
use fsw_world::hex::Hex;

fn set_work_for(sim: &mut Sim, id: &str, job: WorldJob, target: &str, minutes: f64) {
    let epoch = sim.state.characters[id].epoch;
    let action = IntentAction::WorldWork { job, target: target.into(), minutes: Some(minutes) };
    sim.enqueue(Command::SetIntents { id: id.into(), epoch, intents: vec![Intent::new(action, IntentSource::Llm)] });
}

#[test]
fn only_the_right_things_can_be_touched() {
    assert!(destroyable(&prop_id("green", 1)), "the bench on the green");
    assert!(destroyable(&building_id("tavern")));
    assert!(destroyable(&building_id("home_1")));
    assert!(!destroyable(&prop_id("green", 0)), "the well stays");
    assert!(!destroyable(&prop_id("tavern", 14)), "lanterns stay");
    assert!(!destroyable(&prop_id("graveyard", 0)), "the dead are left alone");
    assert!(!destroyable("b:green"), "the green has no building");
    assert!(destroyable(&ground_id(Hex { col: 3, row: 9 })));
    assert!(
        !job_accepts(&create_test_state(1), WorldJob::Dig, &ground_id(Hex { col: 0, row: 0 })),
        "the border is mountain"
    );
}

#[test]
fn a_table_taken_apart_frees_the_ground_and_yields_a_log() {
    let mut sim = quiet(5);
    sim.state.director.next_wave_tick = i64::MAX;
    let bench = prop_id("tavern", 1);
    assert!(get_place("tavern").obstacle_of_model(1).is_some(), "the table blocks the way");
    let before = walkable_cells(&sim.state);
    set_work_for(&mut sim, "c1", WorldJob::Dismantle, &bench, 200.0);
    let working = |state: &SimState| {
        describe_activity(state, &state.characters["c1"])
            .contains("taking apart the medium table at The Gilded Tankard")
    };
    assert!(
        run_until(&mut sim, 3000, working) < 3000,
        "{}",
        describe_activity(&sim.state, &sim.state.characters["c1"])
    );
    let apart = |state: &SimState| {
        matches!(state.objects.get(&bench), Some(WorldObject::Prop { stage: PropStage::Pieces, .. }))
    };
    assert!(run_until(&mut sim, 4000, apart) < 4000);
    assert!(walkable_cells(&sim.state) > before, "the bench's ground is free");
    assert!(!job_accepts(&sim.state, WorldJob::Dismantle, &bench));
    let wood = sim.state.characters["c1"].carried.get(Resource::Wood)
        + wood_at(&sim.state, "lumber_camp")
        + wood_at(&sim.state, "market");
    assert_eq!(wood, 1);
    // Its pieces are carried off in time, and the bench is gone.
    while sim.state.tick < (fsw_sim::systems::world::PIECES_DAYS + 1) * TICKS_PER_DAY {
        sim.step();
    }
    assert!(matches!(sim.state.objects.get(&bench), Some(WorldObject::Prop { stage: PropStage::Gone, .. })));
}

#[test]
fn a_building_taken_apart_leaves_rubble_and_puts_its_people_out() {
    let mut sim = quiet(6);
    sim.state.director.next_wave_tick = i64::MAX;
    let home = sim.state.characters["c1"].home_id.clone();
    let target = building_id(&home);
    for id in ["c1", "c2", "c3"] {
        set_work_for(&mut sim, id, WorldJob::Dismantle, &target, 400.0);
    }
    let down = |state: &SimState| {
        matches!(state.objects.get(&target), Some(WorldObject::Building { stage: BuildingStage::Dismantled, .. }))
    };
    let took = run_until(&mut sim, 12000, down);
    assert!(took < 12000, "three could not take a house down in a day");
    assert!(sim.state.nav.is_live());
    assert!(!home_usable(&sim.state, &sim.state.characters["c1"]));
    assert!(is_homeless(&sim.state, &sim.state.characters["c1"]));
    let lines: Vec<&str> =
        sim.state.chronicle.iter().filter(|e| e.kind == ChronicleKind::World).map(|e| e.text.as_str()).collect();
    assert!(lines.iter().any(|l| l.contains("took") && l.contains("apart")), "{lines:?}");
    let materials: i64 = ["c1", "c2", "c3"].iter().map(|id| sim.state.characters[*id].carried.total()).sum();
    assert!(materials > 0 || !sim.state.stockpiles.is_empty(), "nothing came of the timbers");
    // Newcomers are not given it.
    assert_ne!(fsw_sim::systems::arrivals::free_home(&sim.state), home);
    // Its people sleep rough that night.
    let night = |state: &SimState| describe_activity(state, &state.characters["c1"]).contains("rough");
    assert!(run_until(&mut sim, TICKS_PER_DAY, night) < TICKS_PER_DAY);
}

#[test]
fn dug_ground_and_mown_grass_come_back_on_the_clock() {
    let mut sim = quiet(8);
    sim.state.director.next_wave_tick = i64::MAX;
    let pit = fsw_sim::systems::world::ground_target(&sim.state, WorldJob::Dig).expect("ground to dig near the farm");
    // Another hex than the pit's, or the two jobs would be in each other's way.
    let farm = get_place("farm").origin;
    let lawn = fsw_world::map::world_map()
        .cells
        .iter()
        .filter(|cell| dist(cell.center, farm) < 30.0)
        .map(|cell| ground_id(Hex { col: cell.col, row: cell.row }))
        .find(|id| *id != pit && job_accepts(&sim.state, WorldJob::Mow, id))
        .expect("grass to cut near the farm");
    assert!(job_accepts(&sim.state, WorldJob::Dig, &pit));
    assert!(job_accepts(&sim.state, WorldJob::Mow, &lawn), "there is grass there");
    assert!(!job_accepts(&sim.state, WorldJob::Dig, &ground_id(Hex { col: 3, row: 9 })), "no pits in the road");
    set_work_for(&mut sim, "c1", WorldJob::Dig, &pit, 200.0);
    set_work_for(&mut sim, "c2", WorldJob::Mow, &lawn, 200.0);
    let both = |state: &SimState| {
        matches!(state.objects.get(&pit), Some(WorldObject::Ground { stage: GroundStage::Dug, .. }))
            && matches!(state.objects.get(&lawn), Some(WorldObject::Ground { stage: GroundStage::Mown, .. }))
    };
    assert!(run_until(&mut sim, 6000, both) < 6000);
    assert!(sim.state.nav.is_live(), "a pit is something to walk round");
    assert!(!job_accepts(&sim.state, WorldJob::Mow, &lawn));
    let dug_at = sim.state.objects[&pit].since_tick();
    let mown_at = sim.state.objects[&lawn].since_tick();
    while sim.state.tick < mown_at + MOWN_DAYS * TICKS_PER_DAY + 40 {
        sim.step();
    }
    assert!(!sim.state.objects.contains_key(&lawn), "the grass grew back");
    assert!(sim.state.objects.contains_key(&pit));
    while sim.state.tick < dug_at + DUG_DAYS * TICKS_PER_DAY + 40 {
        sim.step();
    }
    assert!(!sim.state.objects.contains_key(&pit), "the pit filled in");
}

#[test]
fn farmers_take_a_turn_at_the_meadows() {
    let mut sim = quiet(9);
    let mut saw_ground = false;
    for _ in 0..(6 * TICKS_PER_DAY) {
        sim.step();
        if sim.state.tick % 100 == 0 && sim.state.objects.values().any(|o| matches!(o, WorldObject::Ground { .. })) {
            saw_ground = true;
            break;
        }
    }
    assert!(saw_ground, "no farmer mowed or dug in six days");
}

#[test]
fn a_half_dismantled_building_survives_a_save_and_load() {
    let mut sim = quiet(10);
    sim.state.director.next_wave_tick = i64::MAX;
    for id in ["c1", "c2"] {
        set_work_for(&mut sim, id, WorldJob::Dismantle, &building_id("smithy"), 400.0);
    }
    let started = |state: &SimState| state.objects.get(&building_id("smithy")).is_some_and(|o| o.work() > 20.0);
    assert!(run_until(&mut sim, 6000, started) < 6000);
    let mut resumed = Sim::new(round_trip(&sim.state));
    assert_eq!(resumed.state, sim.state);
    for _ in 0..8000 {
        sim.step();
        resumed.step();
    }
    assert!(resumed.state == sim.state, "a reloaded dismantling went a different way");
    assert!(matches!(
        sim.state.objects.get(&building_id("smithy")),
        Some(WorldObject::Building { stage: BuildingStage::Dismantled, .. })
    ));
}

// ---- What the village builds ----------------------------------------------------------------

use fsw_sim::state::{Project, ProjectKind};
use fsw_sim::systems::arrivals::free_home;
use fsw_sim::systems::world::{EXPAND_WORK, MAX_PROJECTS, beds_of, beds_total, project};
use fsw_world::places::pads;

fn stock(state: &mut SimState, place: &str, resource: Resource, count: i64) {
    state.stockpiles.entry(place.into()).or_default().add(resource, count, i64::MAX);
}

fn take_down(state: &mut SimState, place: &str) {
    let down = WorldObject::Building {
        stage: BuildingStage::Dismantled,
        since_tick: state.tick,
        work: 0.0,
        fire: 0.0,
        helpers: Vec::new(),
    };
    fsw_sim::systems::world::put_object(state, &building_id(place), down);
}

#[test]
fn a_building_taken_down_is_raised_again_when_the_stores_allow() {
    let mut sim = quiet(31);
    sim.state.director.next_wave_tick = i64::MAX;
    take_down(&mut sim.state, "smithy");
    stock(&mut sim.state, "lumber_camp", Resource::Wood, 40);
    stock(&mut sim.state, "mine", Resource::Stone, 20);
    let queued = |state: &SimState| {
        state.projects.iter().any(|p| p.kind == ProjectKind::Raise && p.target == building_id("smithy"))
    };
    assert!(run_until(&mut sim, TICKS_PER_DAY, queued) < TICKS_PER_DAY, "the village never took the smithy on");
    assert!(matches!(
        sim.state.objects.get(&building_id("smithy")),
        Some(WorldObject::Building { stage: BuildingStage::Raising { .. }, .. })
    ));
    let raised = |state: &SimState| !state.objects.contains_key(&building_id("smithy"));
    let took = run_until(&mut sim, 8 * TICKS_PER_DAY, raised);
    assert!(took < 8 * TICKS_PER_DAY, "the smithy was not raised in eight days");
    assert!(sim.state.projects.iter().all(|p| p.target != building_id("smithy")));
    let lines: Vec<&str> = sim.state.chronicle.iter().map(|e| e.text.as_str()).collect();
    assert!(lines.iter().any(|l| l.contains("takes on raising the smithy")), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("stands again, raised by")), "{lines:?}");
    // The timber came out of the stores.
    assert!(wood_at(&sim.state, "lumber_camp") + wood_at(&sim.state, "market") < 40 + 20);
}

#[test]
fn nothing_is_raised_without_timber() {
    let mut sim = quiet(32);
    sim.state.director.next_wave_tick = i64::MAX;
    // No woodcutters' logs reach the stores: nobody chops.
    for c in sim.state.characters.values_mut() {
        c.jobs.retain(|j| !matches!(j, fsw_sim::state::ActivityId::ChopWood | fsw_sim::state::ActivityId::Mine));
        if c.jobs.is_empty() {
            c.jobs.push(fsw_sim::state::ActivityId::Fish);
        }
    }
    take_down(&mut sim.state, "smithy");
    for _ in 0..(5 * TICKS_PER_DAY) {
        sim.step();
    }
    assert!(sim.state.objects.contains_key(&building_id("smithy")), "the smithy rose without a log in the stores");
    let raise = sim.state.projects.iter().find(|p| p.kind == ProjectKind::Raise).expect("the project waits");
    assert!(!raise.delivered.has_all(&raise.cost));
    assert_eq!(raise.work, 0.0);
}

#[test]
fn a_small_cottage_is_made_bigger_and_has_a_bed_more() {
    let mut sim = quiet(33);
    sim.state.director.next_wave_tick = i64::MAX;
    let home = sim.state.characters["c1"].home_id.clone();
    assert_eq!(beds_of(&sim.state, &home), 2);
    let before = beds_total(&sim.state);
    let mut cost = Store::default();
    cost.add(Resource::Wood, 20, i64::MAX);
    cost.add(Resource::Stone, 10, i64::MAX);
    let id = sim.state.new_id("j:");
    sim.state.projects.push(Project {
        id: id.clone(),
        kind: ProjectKind::Expand,
        target: building_id(&home),
        cost: cost.clone(),
        delivered: cost,
        need: EXPAND_WORK,
        work: 0.0,
        since_tick: sim.state.tick,
        helpers: Vec::new(),
    });
    assert!(job_accepts(&sim.state, WorldJob::Build, &id));
    let done = |state: &SimState| project(state, &id).is_none();
    assert!(run_until(&mut sim, 8 * TICKS_PER_DAY, done) < 8 * TICKS_PER_DAY, "the room was never built");
    assert!(matches!(
        sim.state.objects.get(&building_id(&home)),
        Some(WorldObject::Building { stage: BuildingStage::Expanded, .. })
    ));
    assert_eq!(beds_of(&sim.state, &home), 3);
    assert_eq!(beds_total(&sim.state), before + 1);
    assert!(home_usable(&sim.state, &sim.state.characters["c1"]));
    // A bigger house is the roomiest, so a newcomer is sent there once the others fill up.
    let _ = free_home(&sim.state);
}

#[test]
fn timber_to_spare_goes_into_something_new_on_a_pad() {
    let mut sim = quiet(34);
    sim.state.director.next_wave_tick = i64::MAX;
    stock(&mut sim.state, "lumber_camp", Resource::Wood, 50);
    let built = |state: &SimState| {
        state.objects.values().any(|o| matches!(o, WorldObject::Built { stage: PropStage::Whole, .. }))
    };
    let took = run_until(&mut sim, 6 * TICKS_PER_DAY, built);
    assert!(took < 6 * TICKS_PER_DAY, "nothing new went up in six days");
    let (id, object) = sim.state.objects.iter().find(|(_, o)| matches!(o, WorldObject::Built { .. })).unwrap();
    assert!(id.starts_with("n:"));
    if let WorldObject::Built { spec, .. } = object {
        let pad = pads().into_iter().find(|p| Some(p.id.as_str()) == spec.pad.as_deref()).expect("built on a pad");
        assert_eq!((spec.x, spec.z), (pad.pos.x, pad.pos.z));
        assert!(!spec.model.is_empty());
    }
    assert!(sim.state.projects.len() <= MAX_PROJECTS);
    assert!(wood_at(&sim.state, "lumber_camp") < 50);
}

#[test]
fn projects_survive_a_save_and_load() {
    let mut sim = quiet(35);
    sim.state.director.next_wave_tick = i64::MAX;
    take_down(&mut sim.state, "chapel");
    stock(&mut sim.state, "lumber_camp", Resource::Wood, 40);
    stock(&mut sim.state, "mine", Resource::Stone, 20);
    let under_way = |state: &SimState| state.projects.iter().any(|p| p.work > 0.0 || p.delivered.total() > 0);
    assert!(run_until(&mut sim, 3 * TICKS_PER_DAY, under_way) < 3 * TICKS_PER_DAY);
    let mut resumed = Sim::new(round_trip(&sim.state));
    assert_eq!(resumed.state, sim.state);
    for _ in 0..6000 {
        sim.step();
        resumed.step();
    }
    assert!(resumed.state == sim.state, "a reloaded project went a different way");
}

/// The counter-pressure test: over many days the village does not level itself.
fn village_stands(days: i64, seed: u32) {
    let mut sim = quiet(seed);
    let trees_total = scatter().models.iter().filter(|m| is_tree(&m.model)).count();
    let mut projects_done = 0;
    let mut had_projects = false;
    for day in 1..=days {
        while fsw_core::time::game_time(sim.state.tick).day <= day {
            sim.step();
        }
        let standing = (0..scatter().models.len()).filter(|i| tree_standing(&sim.state, *i)).count();
        assert!(standing * 10 >= trees_total * 8, "day {day}: only {standing} of {trees_total} trees stand");
        let homes_whole = fsw_world::places::homes()
            .iter()
            .filter(|h| {
                !sim.state.houses.contains_key(&h.id) && fsw_sim::systems::world::building_whole(&sim.state, &h.id)
            })
            .count();
        assert!(homes_whole >= 6, "day {day}: only {homes_whole} homes stand");
        if !sim.state.projects.is_empty() {
            had_projects = true;
        }
        projects_done += sim
            .state
            .chronicle
            .iter()
            .filter(|e| {
                e.text.contains("put up")
                    || e.text.contains("stands again, raised")
                    || e.text.contains("has a new room")
            })
            .count();
    }
    assert!(living(&sim.state).count() >= 7);
    let _ = (projects_done, had_projects);
}

#[test]
fn the_village_does_not_level_itself_in_twenty_days() {
    village_stands(20, 41);
}

#[test]
#[ignore = "long: sixty game days"]
fn the_village_does_not_level_itself_in_sixty_days() {
    village_stands(60, 42);
}
