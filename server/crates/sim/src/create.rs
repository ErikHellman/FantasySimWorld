//! A new world: the authored cast at their doors at six in the morning of day one.

use std::collections::HashMap;

use fsw_core::jsmath::js_round;
use fsw_core::rng::range;

use crate::data::cast::{CastEntry, SeedBond, cast, seed_bonds};
use crate::state::{ChronicleEntry, ChronicleKind, Director, Id, SAVE_VERSION, SimState, Tuning};
use crate::systems::arrivals::{free_home, make_character};
use crate::systems::director::tick_at;
use crate::systems::relationships::{charm_welcome, label_for};
use crate::util::new_relationship;

pub const DEFAULT_SEED: u32 = 1337;

pub fn create_state(seed: u32) -> SimState {
    create_state_with(seed, cast(), seed_bonds())
}

/// A new world with a given founding cast. Tests use it to rebuild worlds from
/// before the cast grew; everything else wants [`create_state`].
pub fn create_state_with(seed: u32, founders: &[CastEntry], bonds: &[SeedBond]) -> SimState {
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
            population_target: founders.len(),
        },
        tuning: Tuning { chatter: 0.5, script_wait_ticks: 0, mercy_wait_ticks: 40 },
    };

    let mut by_key: HashMap<&str, Id> = HashMap::new();
    let mut cast_ids = Vec::new();
    for entry in founders {
        let home = entry.home_id.clone().unwrap_or_else(|| free_home(&state));
        let c = make_character(&mut state, entry, &home);
        let id = c.id.clone();
        state.characters.insert(id.clone(), c);
        state.order.push(id.clone());
        by_key.insert(&entry.key, id.clone());
        cast_ids.push(id);
    }

    // Everyone in a village this small knows everyone, at least a little.
    for a in &cast_ids {
        for b in &cast_ids {
            if a == b {
                continue;
            }
            let mut r = new_relationship();
            r.familiarity = range(&mut state.rng, 18.0, 32.0);
            r.affinity = js_round(range(&mut state.rng, 0.0, 14.0));
            r.trust = js_round(range(&mut state.rng, 0.0, 12.0));
            r.affinity += charm_welcome(&state.characters[b]);
            state.characters[a].relationships.insert(b.clone(), r);
        }
    }
    for bond in bonds {
        let (Some(from), Some(to)) = (by_key.get(bond.from.as_str()), by_key.get(bond.to.as_str())) else {
            panic!("Seed bond refers to unknown cast member {} -> {}", bond.from, bond.to);
        };
        bond.apply(&mut state.characters[from].relationships[to]);
    }
    for a in &cast_ids {
        for r in state.characters[a].relationships.values_mut() {
            r.label = label_for(r, false);
        }
    }
    state.chronicle.push(ChronicleEntry {
        tick: 0,
        kind: ChronicleKind::World,
        text: "A new day dawns over the valley.".into(),
        ids: Vec::new(),
    });
    state
}
