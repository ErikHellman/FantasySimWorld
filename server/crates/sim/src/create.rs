//! A new world: the authored cast at their doors at six in the morning of day one.

use std::collections::HashMap;

use fsw_core::jsmath::js_round;
use fsw_core::rng::range;

use crate::data::cast::{authored, cast, seed_bonds};
use crate::state::{ChronicleEntry, ChronicleKind, Director, Id, SAVE_VERSION, SimState, Tuning};
use crate::systems::arrivals::{free_home, make_character};
use crate::systems::director::tick_at;
use crate::systems::relationships::{label_for, looks_welcome};
use crate::util::new_relationship;

pub const DEFAULT_SEED: u32 = 1337;

pub fn create_state(seed: u32) -> SimState {
    let founders = cast();
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
            population_target: founders.iter().filter(|entry| entry.child.is_none()).count(),
            last_fire_tick: 0,
            last_birth_tick: 0,
        },
        tuning: Tuning { chatter: 0.5, script_wait_ticks: 0, mercy_wait_ticks: 40 },
        billboards: Default::default(),
        powers: Default::default(),
        houses: Default::default(),
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

    // A child knows who raises them; the authored keys become ids now that everyone has one.
    for entry in founders {
        let Some(child) = &entry.child else { continue };
        let parents: Vec<Id> = child
            .parents
            .iter()
            .map(|key| {
                by_key.get(key.as_str()).unwrap_or_else(|| panic!("{}: unknown parent {key}", entry.key)).clone()
            })
            .collect();
        if let Some(childhood) = &mut state.characters[&by_key[entry.key.as_str()]].child {
            childhood.parents = parents;
        }
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
            r.affinity += looks_welcome(&state.characters[b]);
            state.characters[a].relationships.insert(b.clone(), r);
        }
    }
    for bond in seed_bonds() {
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

/// Brings the authored characters of a saved world up to date with the cast as
/// it is written now: their quirks, their secret and the words that describe
/// them. Their numbers, history and relationships are left as they are.
pub fn refresh_authored(state: &mut SimState) {
    for c in state.characters.values_mut() {
        let Some(entry) = authored(&c.profile.name) else { continue };
        // A child who has grown up is no longer the person the cast describes.
        if entry.child.is_some() && !c.is_child() {
            continue;
        }
        let written = &entry.profile;
        c.quirks = entry.quirks.clone();
        c.profile.secret = written.secret.clone();
        c.profile.backstory = written.backstory.clone();
        c.profile.traits = written.traits.clone();
        c.profile.goals = written.goals.clone();
        c.profile.speech_style = written.speech_style.clone();
    }
}
