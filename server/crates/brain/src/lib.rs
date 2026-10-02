//! The language-model brain (port of `src/brain`).
//!
//! Done so far: the client, its request queue and the governor, none of which
//! touch the sim. Prompts, validation and the orchestrator follow in phase 4.

pub mod governor;
pub mod llm;
pub mod settings;
