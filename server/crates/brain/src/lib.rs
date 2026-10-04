//! The language-model brain (port of `src/brain`, kept at the `ts-sim-reference`
//! tag): the client, its request
//! queue and the governor (none of which touch the sim), and the prompts,
//! validation and orchestrator that connect a model to the village.

pub mod brain;
pub mod governor;
pub mod llm;
pub mod prompts;
pub mod recall;
pub mod retrieval;
pub mod settings;
pub mod tools;
pub mod validate;
