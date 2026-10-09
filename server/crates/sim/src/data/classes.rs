//! Character classes: model, hit points, weapon, and the work and leisure they lean to.

use fsw_world::places::ActivityId::{self, *};
use serde::{Deserialize, Serialize};

use super::anims::AnimId::{self, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClassId {
    Knight,
    Barbarian,
    Mage,
    Ranger,
    Rogue,
    Scout,
    Druid,
    Engineer,
    Paladin,
    Cleric,
    Witch,
    Tiefling,
    Lorekeeper,
    Farmer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponStyle {
    Melee,
    Bow,
    Crossbow,
    Magic,
}

pub struct Weapon {
    pub style: WeaponStyle,
    /// damage per hit, uniform in this range
    pub damage: [i64; 2],
    /// reach in world units
    pub range: f64,
    /// seconds between attacks
    pub interval: f64,
    /// attack animations, picked at random
    pub anims: &'static [AnimId],
    /// stance while fighting but not swinging
    pub stance: AnimId,
    /// the chance of turning a blow aside, before the wielder's own skill and state
    pub block: f64,
    /// the chance of stepping out of a blow, likewise
    pub dodge: f64,
}

pub struct ClassDef {
    pub id: ClassId,
    pub label: &'static str,
    pub model: &'static str,
    pub max_hp: f64,
    pub weapon: Weapon,
    pub main_hand: Option<&'static str>,
    pub off_hand: Option<&'static str>,
    pub jobs: &'static [ActivityId],
    pub hobbies: &'static [ActivityId],
}

const fn melee(damage: [i64; 2], interval: f64, anims: &'static [AnimId], stance: AnimId) -> Weapon {
    Weapon { style: WeaponStyle::Melee, damage, range: 2.1, interval, anims, stance, block: 0.0, dodge: 0.0 }
}

const fn ranged(
    style: WeaponStyle,
    damage: [i64; 2],
    range: f64,
    interval: f64,
    anims: &'static [AnimId],
    stance: AnimId,
) -> Weapon {
    Weapon { style, damage, range, interval, anims, stance, block: 0.0, dodge: 0.0 }
}

pub static CLASSES: [ClassDef; 14] = [
    ClassDef {
        id: ClassId::Knight,
        label: "knight",
        model: "Knight",
        max_hp: 120.0,
        weapon: Weapon { block: 0.35, ..melee([14, 20], 1.3, &[Attack1hChop, Attack1hSlice, Attack1hStab], Block) },
        main_hand: Some("sword_1handed"),
        off_hand: Some("shield_badge"),
        jobs: &[Watch, Train],
        hobbies: &[Drink, Pray, Stroll],
    },
    ClassDef {
        id: ClassId::Barbarian,
        label: "barbarian",
        model: "Barbarian",
        max_hp: 130.0,
        weapon: Weapon { block: 0.20, ..melee([20, 28], 1.8, &[Attack2hChop, Attack2hSlice], Guard) },
        main_hand: Some("axe_2handed"),
        off_hand: None,
        jobs: &[ChopWood, ActivityId::Mine],
        hobbies: &[Drink, Train, Relax],
    },
    ClassDef {
        id: ClassId::Mage,
        label: "mage",
        model: "Mage",
        max_hp: 70.0,
        weapon: ranged(WeaponStyle::Magic, [16, 24], 14.0, 2.0, &[MagicShoot], Idle),
        main_hand: Some("staff"),
        off_hand: None,
        jobs: &[Brew, Trade],
        hobbies: &[Pray, Stroll, Rest],
    },
    ClassDef {
        id: ClassId::Ranger,
        label: "ranger",
        model: "Ranger",
        max_hp: 85.0,
        weapon: Weapon { dodge: 0.20, ..ranged(WeaponStyle::Bow, [12, 18], 16.0, 1.7, &[BowShoot], BowIdle) },
        main_hand: None,
        off_hand: Some("bow_withString"),
        jobs: &[Forage, ActivityId::Fish, Watch],
        hobbies: &[Train, Relax, Stroll],
    },
    ClassDef {
        id: ClassId::Rogue,
        label: "rogue",
        model: "Rogue",
        max_hp: 85.0,
        weapon: Weapon { dodge: 0.25, ..melee([9, 13], 0.95, &[AttackDualChop, AttackDualSlice], Idle) },
        main_hand: Some("dagger"),
        off_hand: Some("dagger"),
        jobs: &[Trade, ActivityId::Fish],
        hobbies: &[Drink, Stroll, Shop],
    },
    ClassDef {
        id: ClassId::Scout,
        label: "scout",
        model: "Rogue_Hooded",
        max_hp: 80.0,
        weapon: Weapon { dodge: 0.25, ..ranged(WeaponStyle::Crossbow, [13, 19], 13.0, 2.0, &[CrossbowShoot], Idle) },
        main_hand: Some("crossbow_1handed"),
        off_hand: None,
        jobs: &[Watch, Forage],
        hobbies: &[Stroll, Relax, Train],
    },
    ClassDef {
        id: ClassId::Druid,
        label: "druid",
        model: "Druid",
        max_hp: 90.0,
        weapon: ranged(WeaponStyle::Magic, [12, 18], 12.0, 1.9, &[MagicCast], Idle),
        main_hand: Some("druid_staff"),
        off_hand: None,
        jobs: &[Forage, Brew, Farm],
        hobbies: &[Pray, Relax, Stroll],
    },
    ClassDef {
        id: ClassId::Engineer,
        label: "engineer",
        model: "Engineer",
        max_hp: 100.0,
        weapon: melee([13, 19], 1.35, &[Attack1hChop, Attack1hSlice], Idle),
        main_hand: Some("engineer_Wrench"),
        off_hand: None,
        jobs: &[Smith, ActivityId::Mine],
        hobbies: &[Drink, Shop, Rest],
    },
    ClassDef {
        id: ClassId::Paladin,
        label: "paladin",
        model: "Paladin",
        max_hp: 125.0,
        weapon: Weapon { block: 0.30, ..melee([15, 21], 1.4, &[Attack1hChop, Attack1hSlice], Block) },
        main_hand: Some("paladin_hammer"),
        off_hand: Some("paladin_shield"),
        jobs: &[Watch, Train],
        hobbies: &[Pray, Stroll, Drink],
    },
    ClassDef {
        id: ClassId::Cleric,
        label: "cleric",
        model: "Cleric",
        max_hp: 100.0,
        weapon: melee([12, 18], 1.4, &[Attack1hChop, Attack1hSlice], Block),
        main_hand: Some("Cleric_Mace"),
        off_hand: Some("Cleric_Shield"),
        jobs: &[Brew, Trade],
        hobbies: &[Pray, Rest, Stroll],
    },
    ClassDef {
        id: ClassId::Witch,
        label: "witch",
        model: "Witch",
        max_hp: 70.0,
        weapon: ranged(WeaponStyle::Magic, [15, 22], 13.0, 2.0, &[MagicShoot], Idle),
        main_hand: Some("Broom"),
        off_hand: None,
        jobs: &[Brew, Forage],
        hobbies: &[Relax, Shop, Stroll],
    },
    ClassDef {
        id: ClassId::Tiefling,
        label: "swordsman",
        model: "Tiefling",
        max_hp: 95.0,
        weapon: Weapon { block: 0.25, ..melee([15, 21], 1.15, &[Attack1hSlice, Attack1hStab, Attack1hChop], Idle) },
        main_hand: Some("Tiefling_Sword"),
        off_hand: None,
        jobs: &[Watch, Trade],
        hobbies: &[Drink, Train, Stroll],
    },
    ClassDef {
        id: ClassId::Lorekeeper,
        label: "lorekeeper",
        model: "Lorekeeper",
        max_hp: 70.0,
        weapon: ranged(WeaponStyle::Magic, [14, 20], 13.0, 2.1, &[MagicCast], Idle),
        main_hand: Some("Lorekeeper_Staff"),
        off_hand: None,
        jobs: &[Trade, Brew],
        hobbies: &[Pray, Rest, Stroll],
    },
    ClassDef {
        id: ClassId::Farmer,
        label: "farmer",
        model: "Farmer_A",
        max_hp: 85.0,
        weapon: melee([11, 16], 1.6, &[Attack2hSlice, Attack2hChop], Guard),
        main_hand: Some("farmer_pitchfork"),
        off_hand: None,
        jobs: &[Farm, Tend, ChopWood],
        hobbies: &[Drink, Rest, ActivityId::Fish],
    },
];

impl ClassId {
    /// Every class, in the order of the table (and of `data::names::OCCUPATIONS`).
    pub const ALL: [ClassId; 14] = [
        ClassId::Knight,
        ClassId::Barbarian,
        ClassId::Mage,
        ClassId::Ranger,
        ClassId::Rogue,
        ClassId::Scout,
        ClassId::Druid,
        ClassId::Engineer,
        ClassId::Paladin,
        ClassId::Cleric,
        ClassId::Witch,
        ClassId::Tiefling,
        ClassId::Lorekeeper,
        ClassId::Farmer,
    ];

    pub fn def(self) -> &'static ClassDef {
        CLASSES.iter().find(|def| def.id == self).expect("every class has a definition")
    }
}

/// Bare fists, used in brawls whatever the class.
pub static FISTS: Weapon = Weapon {
    style: WeaponStyle::Melee,
    damage: [4, 7],
    range: 1.7,
    interval: 1.1,
    anims: &[AttackPunch, AttackKick],
    stance: UnarmedIdle,
    block: 0.0,
    dodge: 0.0,
};

pub const WALK_SPEED: f64 = 3.1;
pub const RUN_SPEED: f64 = 6.2;
