//! Hex map, places, scatter, nav grid and path finding (port of `src/world`). Ported in phase 1.
//!
//! `tests/golden/*.json` is the TypeScript world's output, written by `tools/dump-world.ts`;
//! the port must reproduce it exactly (categories, nav costs, paths) or to 1e-9 (coordinates).
