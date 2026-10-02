//! RNG, game time and 2D maths (port of `src/core`).
//!
//! Dependency rule, enforced by the Cargo graph: `core ← world ← sim ← brain ← server`.

pub mod jsmath;
pub mod math2d;
pub mod rng;
pub mod time;

pub use math2d::{Vec2, vec2};
