//! The hand-authored cast of the TypeScript game (thirteen founders, their starting feelings for one
//! another, and eleven newcomers), kept for tests only. The game itself founds every village from
//! what the language model invents, or from `fsw_sim::data::names`; nothing here is compiled into the
//! server. `data/cast.json` is edited by hand.

use std::sync::LazyLock;

use fsw_sim::create_state;
use fsw_sim::data::classes::ClassId;
use fsw_sim::state::{Id, Profile, Quirk, Relationship, SimState, present};
use fsw_sim::systems::arrivals::arrive_with;
use fsw_sim::systems::founding::{Blueprint, ChildSeed, found_village};
use fsw_sim::systems::relationships::label_for;
use fsw_sim::util::{Ctx, living, rel};
use fsw_world::places::ActivityId;
use serde::Deserialize;

/// A character as authored: who they are, how they look and where they live.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CastEntry {
    key: String,
    class_id: ClassId,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    texture: Option<String>,
    #[serde(default)]
    hide: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present")]
    main_hand: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    off_hand: Option<Option<String>>,
    #[serde(default)]
    home_id: Option<String>,
    #[serde(default)]
    jobs: Option<Vec<ActivityId>>,
    #[serde(default)]
    hobbies: Option<Vec<ActivityId>>,
    profile: Profile,
    /// For a newcomer: bonds with founders (by cast key) that take hold if both are alive when
    /// the newcomer arrives, such as kin come looking for each other.
    #[serde(default)]
    bonds: Vec<SeedBond>,
    #[serde(default)]
    quirks: Vec<Quirk>,
    /// Set for a child; the class is the one they will grow into.
    #[serde(default)]
    child: Option<ChildEntry>,
}

/// A child as authored: how naughty they are and who raises them, by cast key.
#[derive(Clone, Debug, Deserialize)]
struct ChildEntry {
    mischief: f64,
    #[serde(default)]
    parents: Vec<String>,
}

/// A relationship that exists before the simulation starts. Applied one way.
#[derive(Clone, Debug, Deserialize)]
struct SeedBond {
    from: String,
    to: String,
    affinity: Option<f64>,
    trust: Option<f64>,
    attraction: Option<f64>,
    grievance: Option<f64>,
    familiarity: Option<f64>,
    kin: Option<bool>,
    summary: String,
}

impl SeedBond {
    /// Writes the authored feelings over a relationship; unset numbers keep theirs.
    fn apply(&self, r: &mut Relationship) {
        r.affinity = self.affinity.unwrap_or(r.affinity);
        r.trust = self.trust.unwrap_or(r.trust);
        r.attraction = self.attraction.unwrap_or(0.0);
        r.grievance = self.grievance.unwrap_or(0.0);
        r.familiarity = self.familiarity.unwrap_or(r.familiarity);
        r.kin = self.kin.unwrap_or(false);
        r.summary = self.summary.clone();
        if r.grievance > 0.0 {
            r.last_offence_tick = 0;
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CastFile {
    cast: Vec<CastEntry>,
    seed_bonds: Vec<SeedBond>,
    newcomers: Vec<CastEntry>,
}

static CAST: LazyLock<CastFile> =
    LazyLock::new(|| serde_json::from_str(include_str!("../data/cast.json")).expect("crates/testing/data/cast.json"));

fn blueprint(entry: &CastEntry) -> Blueprint {
    Blueprint {
        class_id: entry.class_id,
        model: entry.model.clone(),
        texture: entry.texture.clone(),
        hide: entry.hide.clone().unwrap_or_default(),
        main_hand: entry.main_hand.clone(),
        off_hand: entry.off_hand.clone(),
        home_id: entry.home_id.clone(),
        jobs: entry.jobs.clone(),
        hobbies: entry.hobbies.clone(),
        profile: entry.profile.clone(),
        quirks: entry.quirks.clone(),
        // The parents' ids are filled in once everyone has one (`create_test_state`).
        child: entry.child.as_ref().map(|child| ChildSeed { mischief: child.mischief, parents: Vec::new() }),
        ambition: None,
    }
}

fn by_key(key: &str) -> Option<&'static CastEntry> {
    CAST.cast.iter().chain(&CAST.newcomers).find(|entry| entry.key == key)
}

/// A world founded with the authored cast at their doors at six in the morning of day one, as the
/// TypeScript game began: the same seed gives the same world it always did.
pub fn create_test_state(seed: u32) -> SimState {
    let mut state = create_state(seed);
    let blueprints: Vec<Blueprint> = CAST.cast.iter().map(blueprint).collect();
    found_village(&mut state, &blueprints, |state, ids| {
        for bond in &CAST.seed_bonds {
            let index = |key: &str| CAST.cast.iter().position(|entry| entry.key == key);
            let (Some(from), Some(to)) = (index(&bond.from), index(&bond.to)) else {
                panic!("Seed bond refers to unknown cast member {} -> {}", bond.from, bond.to);
            };
            bond.apply(&mut state.characters[&ids[from]].relationships[&ids[to]]);
        }
        // A child knows who raises them; the authored keys become ids now that everyone has one.
        for (index, entry) in CAST.cast.iter().enumerate() {
            let Some(child) = &entry.child else { continue };
            let parents: Vec<Id> = child
                .parents
                .iter()
                .map(|key| {
                    let at = CAST.cast.iter().position(|e| &e.key == key);
                    ids[at.unwrap_or_else(|| panic!("{}: unknown parent {key}", entry.key))].clone()
                })
                .collect();
            if let Some(childhood) = &mut state.characters[&ids[index]].child {
                childhood.parents = parents;
            }
        }
    });
    state
}

/// The full names of the authored founders, in cast order.
pub fn founder_names() -> Vec<&'static str> {
    CAST.cast.iter().map(|entry| entry.profile.name.as_str()).collect()
}

/// The cast keys of the authored newcomers, in the order they used to arrive.
pub fn newcomer_keys() -> Vec<&'static str> {
    CAST.newcomers.iter().map(|entry| entry.key.as_str()).collect()
}

/// Brings one of the authored newcomers into the village now, by the west road, with the bonds the
/// cast gives them to whoever of their people is still alive.
pub fn admit(ctx: &mut Ctx, key: &str) -> Id {
    let entry = CAST.newcomers.iter().find(|entry| entry.key == key).unwrap_or_else(|| panic!("no newcomer {key}"));
    let id = arrive_with(ctx, &blueprint(entry), &[], "");
    let resolve = |ctx: &Ctx, bond_key: &str| -> Option<Id> {
        if bond_key == entry.key {
            return Some(id.clone());
        }
        let name = &by_key(bond_key)?.profile.name;
        living(ctx.state).find(|c| &c.profile.name == name).map(|c| c.id.clone())
    };
    for bond in &entry.bonds {
        if let (Some(from), Some(to)) = (resolve(ctx, &bond.from), resolve(ctx, &bond.to)) {
            let r = rel(ctx.c(&from), &to);
            bond.apply(r);
            r.label = label_for(r, false);
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsw_world::assets::asset_manifest;

    #[test]
    fn every_authored_model_is_in_the_asset_manifest() {
        let manifest = asset_manifest();
        for entry in CAST.cast.iter().chain(&CAST.newcomers) {
            let model = entry.model.as_deref().unwrap_or(entry.class_id.def().model);
            let character = manifest.characters.get(model).unwrap_or_else(|| panic!("{}: no model {model}", entry.key));
            assert!(manifest.rigs.contains_key(&character.rig), "{model}: no rig {}", character.rig);
        }
    }

    #[test]
    fn keys_are_unique_and_newcomer_bonds_name_known_people() {
        let all: Vec<&CastEntry> = CAST.cast.iter().chain(&CAST.newcomers).collect();
        for entry in &all {
            assert_eq!(all.iter().filter(|other| other.key == entry.key).count(), 1, "{}", entry.key);
            for bond in &entry.bonds {
                assert!(bond.from == entry.key || bond.to == entry.key, "{}: a bond must involve them", entry.key);
                assert!(by_key(&bond.from).is_some() && by_key(&bond.to).is_some(), "{}", entry.key);
            }
        }
    }

    #[test]
    fn the_test_world_is_founded_and_housed() {
        let state = create_test_state(1);
        assert_eq!(state.order.len(), 18);
        assert!(!state.director.founding);
        assert_eq!(founder_names()[0], "Aldric Vane");
        assert_eq!(newcomer_keys()[0], "bjarke");
    }
}
