//! The parts of the asset manifest the world and the sim depend on: model
//! bounds (obstacles) and animation clip lengths (combat timing). Written by
//! `tools/assets/build-assets.mjs` to `server/data/asset-manifest.json`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct AssetManifest {
    pub bundles: HashMap<String, BundleEntry>,
    pub characters: HashMap<String, CharacterAssetEntry>,
    /// rig id -> its clip file; every rig carries the same clip names
    pub rigs: HashMap<String, Rig>,
}

/// The rig whose clip lengths the sim times actions by; other rigs are
/// stretched to match when they are drawn.
pub const TIMING_RIG: &str = "medium";

#[derive(Deserialize, Debug)]
pub struct BundleEntry {
    pub url: String,
    pub models: HashMap<String, ModelEntry>,
}

#[derive(Deserialize, Debug)]
pub struct ModelEntry {
    /// `[minX, minY, minZ, maxX, maxY, maxZ]` in model units
    pub bounds: [f64; 6],
}

#[derive(Deserialize, Debug)]
pub struct CharacterAssetEntry {
    pub url: String,
    pub texture: Option<String>,
    /// standing height in world units, after `scale`
    pub height: f64,
    /// which entry of `rigs` this skeleton plays
    pub rig: String,
    /// uniform scale the model is drawn at
    pub scale: f64,
}

#[derive(Deserialize, Debug)]
pub struct Rig {
    pub url: String,
    /// clip name -> duration in seconds
    pub clips: HashMap<String, f64>,
}

static MANIFEST: LazyLock<AssetManifest> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../data/asset-manifest.json")).expect("server/data/asset-manifest.json")
});

pub fn asset_manifest() -> &'static AssetManifest {
    &MANIFEST
}

/// Clip lengths of the timing rig.
pub fn timing_clips() -> &'static HashMap<String, f64> {
    &MANIFEST.rigs.get(TIMING_RIG).expect("the timing rig is in the asset manifest").clips
}

/// Bounds of `model` in `bundle`, if the manifest has it.
pub fn model_bounds(bundle: &str, model: &str) -> Option<&'static [f64; 6]> {
    MANIFEST.bundles.get(bundle)?.models.get(model).map(|entry| &entry.bounds)
}
