//! Hex map, places, scatter, nav grid and path finding (port of `src/world`).
//!
//! The world is static: it is built once, on first use, from the hand-authored
//! layout and the two generated data files in `server/data/`. Module for
//! module it mirrors the TypeScript world layer, which the web client keeps
//! for drawing; `tests/golden/*.json` is that layer's output (written by
//! `tools/dump-world.ts`) and the port reproduces it exactly (categories, nav
//! costs, paths) or to 1e-9 (coordinates). [`navgrid::world_hash`] lets the
//! server and a client check they agree.

pub mod assets;
pub mod height;
pub mod hex;
pub mod layout;
pub mod map;
pub mod navgrid;
pub mod pathfind;
pub mod place_defs;
pub mod places;
pub mod scatter;
pub mod tiles;

/// Builds every lazily built part of the world now (the nav grid pulls in the rest).
pub fn init() {
    navgrid::nav_grid();
}
