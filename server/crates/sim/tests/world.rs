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
    let mut sim = Sim::new(create_state(3));
    for _ in 0..TICKS_PER_DAY {
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
    assert!(!sim.state.nav.is_live(), "with nothing changed the static grid is back");
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
