//! The deterministic 20 Hz village simulation (port of `src/sim`; the TypeScript
//! original is kept at the `ts-sim-reference` tag, as are the other `src/…`
//! and `tools/…` files that comments in these crates name).
//!
//! `SimState` is plain data with the exact JSON shape of the TypeScript sim,
//! so browser exports load unchanged (`tests/fixtures/export-day-*.json`).
//! The outside world reaches it only through `Command`s applied at the start
//! of a tick; each tick returns its one-shot `SimEvent`s.

pub mod commands;
pub mod create;
pub mod data;
pub mod describe;
pub mod events;
pub mod nav;
pub mod sim;
pub mod state;
pub mod systems;
pub mod util;

pub use commands::Command;
pub use create::{DEFAULT_SEED, FOUNDERS, create_state};
pub use events::SimEvent;
pub use sim::Sim;
pub use state::{SaveFile, SimState};
