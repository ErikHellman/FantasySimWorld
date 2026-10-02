//! A* over the nav grid, then string-pulled into a few straight legs.
//!
//! Tie-breaks decide paths, so this follows `src/world/pathfind.ts` exactly:
//! g-scores are stored as f32 (a `Float32Array` there) but summed in f64, the
//! heap key is the unrounded f64, and the heap's `<=`/`<` choices are the same.

use std::cell::RefCell;

use fsw_core::Vec2;
use fsw_core::jsmath::js_hypot;
use serde::Serialize;

use crate::navgrid::{cell_center, cell_index, line_of_walk, nav_grid, nearest_walkable};

pub use crate::navgrid::CELL;

const SQRT2: f64 = std::f64::consts::SQRT_2;
const NEIGHBOURS: [(i64, i64, f64); 8] = [
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, SQRT2),
    (1, -1, SQRT2),
    (-1, 1, SQRT2),
    (-1, -1, SQRT2),
];
/// How far nearest_walkable looks for the start and goal (the TypeScript default).
const ENDPOINT_RADIUS: i64 = 6;

/// Scratch buffers reused across searches on one thread; `stamp` marks which entries belong to the current one.
struct Scratch {
    g_score: Vec<f32>,
    came_from: Vec<i64>,
    visited: Vec<u32>,
    closed: Vec<u32>,
    stamp: u32,
}

thread_local! {
    static SCRATCH: RefCell<Scratch> = RefCell::new({
        let size = nav_grid().width * nav_grid().height;
        Scratch { g_score: vec![0.0; size], came_from: vec![-1; size], visited: vec![0; size], closed: vec![0; size], stamp: 0 }
    });
}

/// Binary min-heap of cell indices keyed by f-score.
#[derive(Default)]
struct Heap {
    items: Vec<usize>,
    keys: Vec<f64>,
}

impl Heap {
    fn push(&mut self, item: usize, key: f64) {
        let mut i = self.items.len();
        self.items.push(item);
        self.keys.push(key);
        while i > 0 {
            let parent = (i - 1) >> 1;
            if self.keys[parent] <= key {
                break;
            }
            self.items[i] = self.items[parent];
            self.keys[i] = self.keys[parent];
            i = parent;
        }
        self.items[i] = item;
        self.keys[i] = key;
    }

    fn pop(&mut self) -> usize {
        let top = self.items[0];
        let last = self.items.pop().expect("pop() on an empty heap");
        let last_key = self.keys.pop().expect("keys match items");
        let n = self.items.len();
        if n > 0 {
            let mut i = 0;
            loop {
                let mut child = 2 * i + 1;
                if child >= n {
                    break;
                }
                if child + 1 < n && self.keys[child + 1] < self.keys[child] {
                    child += 1;
                }
                if self.keys[child] >= last_key {
                    break;
                }
                self.items[i] = self.items[child];
                self.keys[i] = self.keys[child];
                i = child;
            }
            self.items[i] = last;
            self.keys[i] = last_key;
        }
        top
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Route {
    pub start: Vec2,
    /// cell centres from the first step to the goal (the goal itself last), before smoothing
    pub cells: Vec<Vec2>,
    /// true when no search was needed and `cells` is just the goal
    pub direct: bool,
}

/// Waypoints from (but not including) `from` to the reachable point nearest `to`, or `None` when there is no route.
pub fn find_path(from: Vec2, to: Vec2) -> Option<Vec<Vec2>> {
    let route = find_route(from, to)?;
    Some(if route.direct { route.cells } else { smooth(route.start, &route.cells) })
}

/// The A* part of [`find_path`], unsmoothed.
pub fn find_route(from: Vec2, to: Vec2) -> Option<Route> {
    let start = nearest_walkable(from, ENDPOINT_RADIUS)?;
    let goal = nearest_walkable(to, ENDPOINT_RADIUS)?;
    let start_index = cell_index(start.x, start.z)?;
    let goal_index = cell_index(goal.x, goal.z)?;
    if start_index == goal_index || line_of_walk(start, goal) {
        return Some(Route { start, cells: vec![goal], direct: true });
    }

    let grid = nav_grid();
    let (width, height, cost) = (grid.width, grid.height, &grid.cost);
    let goal_x = (goal_index % width) as f64;
    let goal_z = (goal_index / width) as f64;
    let heuristic = |index: usize| {
        let dx = ((index % width) as f64 - goal_x).abs();
        let dz = ((index / width) as f64 - goal_z).abs();
        dx.max(dz) + (SQRT2 - 1.0) * dx.min(dz)
    };

    SCRATCH.with_borrow_mut(|s| {
        s.stamp = s.stamp.wrapping_add(1);
        let stamp = s.stamp;
        let mut open = Heap::default();
        s.g_score[start_index] = 0.0;
        s.visited[start_index] = stamp;
        s.came_from[start_index] = -1;
        open.push(start_index, heuristic(start_index));

        let mut found = false;
        while !open.items.is_empty() {
            let current = open.pop();
            if current == goal_index {
                found = true;
                break;
            }
            if s.closed[current] == stamp {
                continue;
            }
            s.closed[current] = stamp;
            let cx = (current % width) as i64;
            let cz = (current / width) as i64;
            for &(dx, dz, step) in &NEIGHBOURS {
                let nx = cx + dx;
                let nz = cz + dz;
                if nx < 0 || nz < 0 || nx >= width as i64 || nz >= height as i64 {
                    continue;
                }
                let next = nz as usize * width + nx as usize;
                let cell_cost = cost[next];
                if cell_cost <= 0.0 || s.closed[next] == stamp {
                    continue;
                }
                // No cutting corners between two blocked cells.
                if dx != 0
                    && dz != 0
                    && (cost[cz as usize * width + nx as usize] <= 0.0
                        || cost[nz as usize * width + cx as usize] <= 0.0)
                {
                    continue;
                }
                let tentative = f64::from(s.g_score[current]) + step * f64::from(cell_cost);
                if s.visited[next] == stamp && tentative >= f64::from(s.g_score[next]) {
                    continue;
                }
                s.visited[next] = stamp;
                s.g_score[next] = tentative as f32;
                s.came_from[next] = current as i64;
                open.push(next, tentative + heuristic(next));
            }
        }
        if !found {
            return None;
        }

        let mut cells = Vec::new();
        let mut index = goal_index as i64;
        while index != -1 {
            cells.push(cell_center(index as usize));
            index = s.came_from[index as usize];
        }
        cells.reverse();
        *cells.last_mut().expect("the goal is in the route") = goal;
        Some(Route { start, cells, direct: false })
    })
}

/// Drops waypoints that can be skipped in a straight walkable line.
fn smooth(start: Vec2, cells: &[Vec2]) -> Vec<Vec2> {
    let mut result = Vec::new();
    let mut anchor = start;
    let mut i = 0;
    while i < cells.len() {
        // Furthest waypoint visible from the anchor (look ahead a bounded distance to keep this cheap).
        let mut furthest = i;
        let limit = (cells.len() - 1).min(i + 40);
        for j in (i + 1..=limit).rev() {
            if line_of_walk(anchor, cells[j]) {
                furthest = j;
                break;
            }
        }
        anchor = cells[furthest];
        result.push(anchor);
        i = furthest + 1;
    }
    result
}

pub fn path_length(from: Vec2, path: &[Vec2]) -> f64 {
    let mut total = 0.0;
    let mut previous = from;
    for &point in path {
        total += js_hypot(point.x - previous.x, point.z - previous.z);
        previous = point;
    }
    total
}
