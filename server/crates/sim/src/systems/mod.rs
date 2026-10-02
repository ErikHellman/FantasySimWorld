//! The twelve systems, run each tick in the order `Sim::step` lists.

pub mod arrivals;
pub mod combat;
pub mod conversation;
pub mod death;
pub mod director;
pub mod intents;
pub mod monsters;
pub mod movement;
pub mod needs;
pub mod perception;
pub mod relationships;
pub mod routine;
