//! The monsters: the walking dead, and the orc raiders who come up the road.

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
    OrcAxe,
    OrcClub,
    SkeletonGolem,
    OrcWarchief,
}

/// Whose side a monster is on. Rising at night, crumbling at dawn, burial and arson are the undead's alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Faction {
    Undead,
    Orc,
}

/// A boss's blow that hits more than the one it is aimed at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Area {
    /// everyone out of doors within `radius` of where it lands; it cannot be blocked
    Slam { radius: f64 },
    /// the one it is aimed at and up to `more` others in front of it
    Cleave { more: usize },
}

/// The clips a monster moves with.
pub struct MonsterAnims {
    /// played while it climbs out of the ground; one that does not rise is ready at once
    pub rise: Option<AnimId>,
    pub idle: AnimId,
    pub walk: AnimId,
    /// on the heels of a villager
    pub chase: AnimId,
    pub death: AnimId,
}

const ORC: MonsterAnims = MonsterAnims { rise: None, idle: Idle, walk: Walk, chase: Run, death: Death };

const UNDEAD: MonsterAnims = MonsterAnims {
    rise: Some(SkeletonSpawn),
    idle: SkeletonIdle,
    walk: SkeletonWalk,
    chase: SkeletonWalk,
    death: SkeletonDeath,
};

pub struct MonsterDef {
    pub kind: MonsterKind,
    pub faction: Faction,
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
    pub anims: MonsterAnims,
    /// how far it notices villagers
    pub sight: f64,
    /// relative chance in an ordinary wave
    pub weight: f64,
    /// the chance of turning a blow aside
    pub block: f64,
    /// the chance of stepping out of a blow
    pub dodge: f64,
    /// the whole village must rally to bring it down
    pub boss: bool,
    /// seconds from the start of a swing to the blow, when not the clip's own timing
    pub windup: Option<f64>,
    /// a blow that hits more than one
    pub area: Option<Area>,
}

/// In the TypeScript object's key order (`Object.values(MONSTERS)` order matters for weighted picks). The orcs
/// come last and never in a wave of the dead (weight 0), so the picks among the dead are as they were.
pub static MONSTERS: [MonsterDef; 9] = [
    MonsterDef {
        kind: MonsterKind::SkeletonMinion,
        faction: Faction::Undead,
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
        anims: UNDEAD,
        sight: 18.0,
        weight: 5.0,
        block: 0.0,
        dodge: 0.0,
        boss: false,
        windup: None,
        area: None,
    },
    MonsterDef {
        kind: MonsterKind::SkeletonWarrior,
        faction: Faction::Undead,
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
        anims: UNDEAD,
        sight: 18.0,
        weight: 3.0,
        block: 0.3,
        dodge: 0.0,
        boss: false,
        windup: None,
        area: None,
    },
    MonsterDef {
        kind: MonsterKind::SkeletonRogue,
        faction: Faction::Undead,
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
        anims: MonsterAnims { chase: Run, ..UNDEAD },
        sight: 21.0,
        weight: 2.0,
        block: 0.0,
        dodge: 0.2,
        boss: false,
        windup: None,
        area: None,
    },
    MonsterDef {
        kind: MonsterKind::SkeletonMage,
        faction: Faction::Undead,
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
        anims: UNDEAD,
        sight: 20.0,
        weight: 2.0,
        block: 0.0,
        dodge: 0.0,
        boss: false,
        windup: None,
        area: None,
    },
    MonsterDef {
        kind: MonsterKind::Necromancer,
        faction: Faction::Undead,
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
        anims: UNDEAD,
        sight: 23.0,
        weight: 0.0,
        block: 0.0,
        dodge: 0.0,
        boss: false,
        windup: None,
        area: None,
    },
    MonsterDef {
        kind: MonsterKind::OrcAxe,
        faction: Faction::Orc,
        label: "orc raider",
        model: "OrcRaider",
        texture: None,
        main_hand: Some("Orc_Axe"),
        off_hand: None,
        max_hp: 150.0,
        damage: [13, 19],
        range: 2.1,
        interval: 1.5,
        speed: 3.0,
        style: WeaponStyle::Melee,
        attack_anims: &[Attack1hChop, Attack1hSlice],
        anims: ORC,
        sight: 3.0,
        weight: 0.0,
        block: 0.15,
        dodge: 0.0,
        boss: false,
        windup: None,
        area: None,
    },
    MonsterDef {
        kind: MonsterKind::OrcClub,
        faction: Faction::Orc,
        label: "orc raider",
        model: "OrcRaider",
        texture: None,
        main_hand: Some("Orc_Club"),
        off_hand: None,
        max_hp: 130.0,
        damage: [11, 16],
        range: 2.1,
        interval: 1.3,
        speed: 3.1,
        style: WeaponStyle::Melee,
        attack_anims: &[Attack1hChop, Attack1hStab],
        anims: ORC,
        sight: 3.0,
        weight: 0.0,
        block: 0.0,
        dodge: 0.1,
        boss: false,
        windup: None,
        area: None,
    },
    MonsterDef {
        kind: MonsterKind::SkeletonGolem,
        faction: Faction::Undead,
        label: "skeleton golem",
        model: "Skeleton_Golem",
        texture: None,
        main_hand: None,
        off_hand: None,
        max_hp: 600.0,
        damage: [18, 28],
        range: 2.6,
        interval: 3.2,
        speed: 1.9,
        style: WeaponStyle::Melee,
        attack_anims: &[Attack2hSlice],
        anims: UNDEAD,
        sight: 22.0,
        weight: 0.0,
        block: 0.0,
        dodge: 0.0,
        boss: true,
        windup: Some(1.2),
        area: Some(Area::Slam { radius: 2.8 }),
    },
    MonsterDef {
        kind: MonsterKind::OrcWarchief,
        faction: Faction::Orc,
        label: "orc warchief",
        model: "OrcBrute",
        texture: None,
        main_hand: Some("Orc_Axe_Large"),
        off_hand: None,
        max_hp: 420.0,
        damage: [20, 32],
        range: 2.4,
        interval: 2.0,
        speed: 2.8,
        style: WeaponStyle::Melee,
        attack_anims: &[Attack2hChop, Attack2hSlice],
        anims: ORC,
        sight: 3.0,
        weight: 0.0,
        block: 0.15,
        dodge: 0.0,
        boss: true,
        windup: None,
        area: Some(Area::Cleave { more: 1 }),
    },
];

impl MonsterKind {
    pub fn def(self) -> &'static MonsterDef {
        MONSTERS.iter().find(|def| def.kind == self).expect("every monster has a definition")
    }

    pub fn faction(self) -> Faction {
        self.def().faction
    }
}
