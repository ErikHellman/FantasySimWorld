//! The cost-slice API the sim uses for a world that changes: against the static
//! grid it must answer exactly as the plain functions do, and the pieces the
//! static grid is built from must rebuild it bit for bit.

use fsw_core::{Vec2, vec2};
use fsw_world::map::world_map;
use fsw_world::navgrid::{
    CELL, block, cells_touched_by, is_walkable, is_walkable_on, line_of_walk, line_of_walk_on, nav_grid,
    nearest_walkable, nearest_walkable_on, terrain_cost,
};
use fsw_world::pathfind::{find_path, find_path_on, find_route, find_route_on};
use fsw_world::places::{obstacle_contains, places};
use fsw_world::scatter::{is_rock, is_tree, scatter};

fn sample_points() -> Vec<Vec2> {
    let map = world_map();
    let mut points = Vec::new();
    let mut rng = 12345u32;
    for _ in 0..400 {
        let x = map.min_x + fsw_core::rng::next_float(&mut rng) * (map.max_x - map.min_x);
        let z = map.min_z + fsw_core::rng::next_float(&mut rng) * (map.max_z - map.min_z);
        points.push(vec2(x, z));
    }
    points
}

#[test]
fn the_slice_functions_match_the_static_ones() {
    let cost = &nav_grid().cost;
    let points = sample_points();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        assert_eq!(is_walkable_on(cost, a.x, a.z), is_walkable(a.x, a.z));
        assert_eq!(nearest_walkable_on(cost, a, 6), nearest_walkable(a, 6));
        assert_eq!(line_of_walk_on(cost, a, b), line_of_walk(a, b));
        assert_eq!(find_route_on(cost, a, b), find_route(a, b));
        assert_eq!(find_path_on(cost, a, b), find_path(a, b));
    }
    for place in places() {
        for other in places() {
            assert_eq!(find_path_on(cost, place.door, other.door), find_path(place.door, other.door));
        }
    }
}

#[test]
fn terrain_and_block_rebuild_the_static_grid_bit_for_bit() {
    let grid = nav_grid();
    let mut cost = vec![0.0f32; grid.width * grid.height];
    for iz in 0..grid.height {
        for ix in 0..grid.width {
            let x = grid.origin_x + (ix as f64 + 0.5) * CELL;
            let z = grid.origin_z + (iz as f64 + 0.5) * CELL;
            cost[iz * grid.width + ix] = terrain_cost(x, z);
        }
    }
    for place in places() {
        for obstacle in &place.obstacles {
            block(&mut cost, obstacle);
        }
    }
    for obstacle in &scatter().obstacles {
        block(&mut cost, obstacle);
    }
    assert!(cost.iter().zip(&grid.cost).all(|(a, b)| a.to_bits() == b.to_bits()));
}

#[test]
fn cells_touched_cover_everything_an_obstacle_blocks() {
    let grid = nav_grid();
    for obstacle in scatter().obstacles.iter().chain(places().iter().flat_map(|p| p.obstacles.iter())) {
        let touched = cells_touched_by(obstacle);
        let mut cost = vec![1.0f32; grid.width * grid.height];
        block(&mut cost, obstacle);
        for (index, &c) in cost.iter().enumerate() {
            if c == 0.0 {
                assert!(touched.contains(&index), "a blocked cell outside the touched set");
                let ix = index % grid.width;
                let iz = index / grid.width;
                let x = grid.origin_x + (ix as f64 + 0.5) * CELL;
                let z = grid.origin_z + (iz as f64 + 0.5) * CELL;
                assert!(obstacle_contains(obstacle, x, z, fsw_world::navgrid::AGENT_RADIUS));
            }
        }
    }
}

#[test]
fn every_solid_model_owns_exactly_one_obstacle() {
    let scatter = scatter();
    assert_eq!(scatter.obstacle_of.len(), scatter.models.len());
    let mut owned = vec![0usize; scatter.obstacles.len()];
    for (index, model) in scatter.models.iter().enumerate() {
        if let Some(obstacle) = scatter.obstacle_of_model(index) {
            assert_eq!((obstacle.x(), obstacle.z()), (model.x, model.z), "an obstacle stands where its model does");
            owned[scatter.obstacle_of[index].unwrap()] += 1;
        }
    }
    assert!(owned.iter().all(|&n| n == 1), "every scatter obstacle belongs to one model");
    let trees =
        scatter.models.iter().enumerate().filter(|(i, m)| is_tree(&m.model) && scatter.obstacle_of[*i].is_some());
    assert!(trees.count() > 400, "the trees are solid");
    assert!(scatter.models.iter().any(|m| is_rock(&m.model)));

    for place in places() {
        assert_eq!(place.obstacle_of.len(), place.models.len(), "{}", place.id);
        if place.has_building {
            assert!(place.obstacle_of[0].is_some(), "{}: the building blocks", place.id);
            assert!(place.models[0].model.starts_with("building_"), "{}", place.id);
        }
        let mut owned = vec![0usize; place.obstacles.len()];
        for index in place.obstacle_of.iter().flatten() {
            owned[*index] += 1;
        }
        // Billboards cast obstacles without a model of their own in `models`.
        assert_eq!(owned.iter().filter(|&&n| n == 0).count(), place.billboards.len(), "{}", place.id);
        assert!(owned.iter().all(|&n| n <= 1), "{}", place.id);
    }
}
