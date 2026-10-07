//! The authored cast, embedded from `data/cast.json` (written from
//! `src/sim/data/cast.ts` by `bun tools/export-cast.ts` at the `ts-sim-reference`
//! tag; edit the JSON directly now).

use std::sync::LazyLock;

use fsw_world::places::ActivityId;
use serde::Deserialize;

use super::classes::ClassId;
use crate::state::{Profile, Quirk, Relationship, present};

/// A character as authored: who they are, how they look and where they live.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CastEntry {
    pub key: String,
    pub class_id: ClassId,
    /// model override (defaults to the class model)
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub texture: Option<String>,
    #[serde(default)]
    pub hide: Option<Vec<String>>,
    /// `None` keeps the class weapon; `Some(None)` is empty-handed
    #[serde(default, deserialize_with = "present")]
    pub main_hand: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    pub off_hand: Option<Option<String>>,
    #[serde(default)]
    pub home_id: Option<String>,
    #[serde(default)]
    pub jobs: Option<Vec<ActivityId>>,
    #[serde(default)]
    pub hobbies: Option<Vec<ActivityId>>,
    pub profile: Profile,
    /// For a newcomer: bonds with founders (by cast key) that take hold if both are
    /// alive when the newcomer arrives, such as kin come looking for each other.
    #[serde(default)]
    pub bonds: Vec<SeedBond>,
    /// what the sim makes of them beyond the numbers in the profile
    #[serde(default)]
    pub quirks: Vec<Quirk>,
    /// Set for a child; the class is the one they will grow into.
    #[serde(default)]
    pub child: Option<ChildEntry>,
}

/// A child as authored: how naughty they are and who raises them.
#[derive(Clone, Debug, Deserialize)]
pub struct ChildEntry {
    /// 0 never up to anything .. 1 trouble every day
    pub mischief: f64,
    /// cast keys of the people who raise them; empty for a child the village raises
    #[serde(default)]
    pub parents: Vec<String>,
}

/// A relationship that exists before the simulation starts. Applied one way.
#[derive(Clone, Debug, Deserialize)]
pub struct SeedBond {
    pub from: String,
    pub to: String,
    pub affinity: Option<f64>,
    pub trust: Option<f64>,
    pub attraction: Option<f64>,
    pub grievance: Option<f64>,
    pub familiarity: Option<f64>,
    pub kin: Option<bool>,
    pub summary: String,
}

impl SeedBond {
    /// Writes the authored feelings over a relationship; unset numbers keep theirs.
    pub fn apply(&self, r: &mut Relationship) {
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
pub struct CastFile {
    pub cast: Vec<CastEntry>,
    pub seed_bonds: Vec<SeedBond>,
    /// People who may arrive later, in order of preference.
    pub newcomers: Vec<CastEntry>,
}

static CAST: LazyLock<CastFile> =
    LazyLock::new(|| serde_json::from_str(include_str!("../../data/cast.json")).expect("crates/sim/data/cast.json"));

pub fn cast() -> &'static [CastEntry] {
    &CAST.cast
}

pub fn seed_bonds() -> &'static [SeedBond] {
    &CAST.seed_bonds
}

pub fn newcomers() -> &'static [CastEntry] {
    &CAST.newcomers
}

/// The authored entry for a character of this name, founding cast or newcomer.
pub fn authored(name: &str) -> Option<&'static CastEntry> {
    CAST.cast.iter().chain(CAST.newcomers.iter()).find(|entry| entry.profile.name == name)
}

/// The authored person with this cast key, founder or newcomer.
pub fn by_key(key: &str) -> Option<&'static CastEntry> {
    CAST.cast.iter().chain(&CAST.newcomers).find(|entry| entry.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsw_world::assets::asset_manifest;

    #[test]
    fn every_authored_model_is_in_the_asset_manifest() {
        let manifest = asset_manifest();
        for entry in cast().iter().chain(newcomers()) {
            let model = entry.model.as_deref().unwrap_or(entry.class_id.def().model);
            let character = manifest.characters.get(model).unwrap_or_else(|| panic!("{}: no model {model}", entry.key));
            assert!(manifest.rigs.contains_key(&character.rig), "{model}: no rig {}", character.rig);
        }
    }

    #[test]
    fn a_cast_entry_may_be_a_child_raised_by_named_people() {
        let json = r#"{"key":"pip","classId":"knight","model":"Child_Knight","child":{"mischief":0.5,"parents":["aldric"]},
            "profile":{"name":"Pip Vane","pronouns":"he/him","age":11,"occupation":"child","backstory":"","traits":[],
            "goals":[],"speechStyle":"","temper":0.5,"bravery":0.5,"sociability":0.5,
            "voice":{"pitch":1.2,"rate":1.1,"timbre":"piping"}}}"#;
        let entry: CastEntry = serde_json::from_str(json).unwrap();
        let child = entry.child.expect("a child");
        assert_eq!(child.mischief, 0.5);
        assert_eq!(child.parents, vec!["aldric".to_owned()]);
        assert!(cast()[0].child.is_none(), "the first founder is an adult");
    }

    #[test]
    fn keys_are_unique_and_newcomer_bonds_name_known_people() {
        let all: Vec<&CastEntry> = cast().iter().chain(newcomers()).collect();
        for entry in &all {
            assert_eq!(all.iter().filter(|other| other.key == entry.key).count(), 1, "{}", entry.key);
            for bond in &entry.bonds {
                assert!(bond.from == entry.key || bond.to == entry.key, "{}: a bond must involve them", entry.key);
                assert!(by_key(&bond.from).is_some() && by_key(&bond.to).is_some(), "{}", entry.key);
            }
        }
    }
}
