//! A new world: empty at six in the morning of day one, waiting to be founded.
//!
//! The founders arrive by `Command::Settle` (one each, as the language model invents them) or all at
//! once by `Command::FoundVillage` (the sim's own lists). Until the village has `FOUNDERS` people the
//! director spawns no waves and brings no newcomers.

use crate::state::{ChronicleEntry, ChronicleKind, Director, SAVE_VERSION, SimState, Tuning};
use crate::systems::director::tick_at;

pub const DEFAULT_SEED: u32 = 1337;

/// How many villagers a village is founded with, and kept topped up to.
pub const FOUNDERS: usize = 13;

pub fn create_state(seed: u32) -> SimState {
    let mut state = SimState {
        version: SAVE_VERSION,
        seed,
        rng: seed,
        tick: 0,
        next_id: 1,
        characters: Default::default(),
        order: Vec::new(),
        monsters: Default::default(),
        conversations: Default::default(),
        graves: Vec::new(),
        corpses: Vec::new(),
        dead: Default::default(),
        chronicle: Vec::new(),
        slots: Default::default(),
        director: Director {
            next_wave_tick: tick_at(1, 20.6),
            next_arrival_tick: 0,
            last_lethal_tick: -1_000_000_000,
            alarm_until_tick: 0,
            alarm_pos: None,
            watch_day: 0,
            death_ticks: Vec::new(),
            arrivals: 0,
            population_target: FOUNDERS,
            last_fire_tick: 0,
            last_birth_tick: 0,
            founding: true,
            newcomer_draft: None,
            newcomer_deadline_tick: 0,
            awaiting_funeral: Vec::new(),
            next_project_tick: 0,
        },
        tuning: Tuning { chatter: 0.5, script_wait_ticks: 0, mercy_wait_ticks: 40 },
        billboards: Default::default(),
        houses: Default::default(),
        ceremony: None,
        objects: Default::default(),
        stockpiles: Default::default(),
        projects: Vec::new(),
        performances: Vec::new(),
        economy: Default::default(),
        loot: Default::default(),
        animals: Default::default(),
        fauna: Default::default(),
        nav: Default::default(),
    };
    state.chronicle.push(ChronicleEntry {
        tick: 0,
        kind: ChronicleKind::World,
        text: "A new day dawns over the valley.".into(),
        ids: Vec::new(),
    });
    state
}
