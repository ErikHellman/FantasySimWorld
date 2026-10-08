//! The walking dead.

use serde::{Deserialize, Serialize};

use super::anims::AnimId::{self, *};
use super::classes::WeaponStyle;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonsterKind {
    SkeletonMinion,
    SkeletonWarrior,
    SkeletonRogue,
    SkeletonMage,
    Necromancer,
}

pub struct MonsterDef {
    pub kind: MonsterKind,
    pub label: &'static str,
    pub model: &'static str,
    pub texture: Option<&'static str>,
    pub main_hand: Option<&'static str>,
    pub off_hand: Option<&'static str>,
    pub max_hp: f64,
    pub damage: [i64; 2],
    pub range: f64,
    pub interval: f64,
    pub speed: f64,
    pub style: WeaponStyle,
    pub attack_anims: &'static [AnimId],
    /// how far it notices villagers
    pub sight: f64,
    /// relative chance in an ordinary wave
    pub weight: f64,
    /// the chance of turning a blow aside
    pub block: f64,
    /// the chance of stepping out of a blow
    pub dodge: f64,
}

/// In the TypeScript object's key order (`Object.values(MONSTERS)` order matters for weighted picks).
pub static MONSTERS: [MonsterDef; 5] = [
    MonsterDef {
        kind: MonsterKind::SkeletonMinion,
        label: "skeleton",
        model: "Skeleton_Minion",
        texture: None,
        main_hand: Some("Skeleton_Dagger"),
        off_hand: None,
        max_hp: 45.0,
        damage: [6, 10],
        range: 1.9,
        interval: 1.45,
        speed: 2.7,
        style: WeaponStyle::Melee,
        attack_anims: &[Attack1hChop, Attack1hStab],
        sight: 18.0,
        weight: 5.0,
        block: 0.0,
        dodge: 0.0,
    },
    MonsterDef {
        kind: MonsterKind::SkeletonWarrior,
        label: "skeleton warrior",
        model: "Skeleton_Warrior",
        texture: None,
        main_hand: Some("Skeleton_Blade"),
        off_hand: Some("Skeleton_Shield_Large_A"),
        max_hp: 82.0,
        damage: [10, 16],
        range: 2.1,
        interval: 1.55,
        speed: 2.5,
        style: WeaponStyle::Melee,
        attack_anims: &[Attack1hChop, Attack1hSlice],
        sight: 18.0,
        weight: 3.0,
        block: 0.3,
        dodge: 0.0,
    },
    MonsterDef {
        kind: MonsterKind::SkeletonRogue,
        label: "skeleton stalker",
        model: "Skeleton_Rogue",
        texture: None,
        main_hand: Some("Skeleton_Dagger"),
        off_hand: Some("Skeleton_Dagger"),
        max_hp: 56.0,
        damage: [8, 13],
        range: 1.9,
        interval: 0.95,
        speed: 3.4,
        style: WeaponStyle::Melee,
        attack_anims: &[AttackDualChop, AttackDualSlice],
        sight: 21.0,
        weight: 2.0,
        block: 0.0,
        dodge: 0.2,
    },
    MonsterDef {
        kind: MonsterKind::SkeletonMage,
        label: "skeleton mage",
        model: "Skeleton_Mage",
        texture: None,
        main_hand: Some("Skeleton_Staff"),
        off_hand: None,
        max_hp: 49.0,
        damage: [9, 15],
        range: 12.0,
        interval: 2.35,
        speed: 2.4,
        style: WeaponStyle::Magic,
        attack_anims: &[MagicShoot],
        sight: 20.0,
        weight: 2.0,
        block: 0.0,
        dodge: 0.0,
    },
    MonsterDef {
        kind: MonsterKind::Necromancer,
        label: "necromancer",
        model: "Necromancer",
        texture: None,
        main_hand: Some("Skeleton_Scythe"),
        off_hand: None,
        max_hp: 200.0,
        damage: [14, 21],
        range: 12.0,
        interval: 2.1,
        speed: 2.3,
        style: WeaponStyle::Magic,
        attack_anims: &[MagicShoot, MagicCast],
        sight: 23.0,
        weight: 0.0,
        block: 0.0,
        dodge: 0.0,
    },
];

impl MonsterKind {
    pub fn def(self) -> &'static MonsterDef {
        MONSTERS.iter().find(|def| def.kind == self).expect("every monster has a definition")
    }
}
