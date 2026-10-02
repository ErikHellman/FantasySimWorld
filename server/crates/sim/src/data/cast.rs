//! The authored cast, embedded from `data/cast.json` (written from
//! `src/sim/data/cast.ts` by `bun tools/export-cast.ts` at the `ts-sim-reference`
//! tag; edit the JSON directly now).

use std::sync::LazyLock;

use fsw_world::places::ActivityId;
use serde::Deserialize;

use super::classes::ClassId;
use crate::state::{Profile, present};

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
