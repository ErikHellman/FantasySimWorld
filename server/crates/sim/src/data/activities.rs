//! What characters can do, and what it does for them.

use fsw_world::places::ActivityId;

use super::anims::AnimId;
use crate::state::{ItemId, Resource};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Need,
    Work,
    Leisure,
    Duty,
}

/// Change per game minute.
#[derive(Clone, Copy, Debug, Default)]
pub struct NeedsDelta {
    pub hunger: f64,
    pub energy: f64,
    pub social: f64,
}

pub struct ActivityDef {
    pub id: ActivityId,
    /// for prompts and the UI: "eat a meal at the tavern"
    pub label: &'static str,
    /// present participle for status lines: "eating"
    pub verb: &'static str,
    pub anim: AnimId,
    /// "bundle/model" held while doing it
    pub tool: Option<&'static str>,
    /// default length in game minutes
    pub minutes: [i64; 2],
    pub needs: Option<NeedsDelta>,
    /// hp per game minute
    pub heal: f64,
    /// item produced and how many game minutes of work one takes
    pub yields: Option<(ItemId, i64)>,
    /// raw material produced (into the worker's arms) and how many game minutes one unit takes
    pub materials: Option<(Resource, i64)>,
    /// done indoors, out of sight
    pub indoors: bool,
    pub kind: ActivityKind,
}

const fn needs(hunger: f64, energy: f64, social: f64) -> Option<NeedsDelta> {
    Some(NeedsDelta { hunger, energy, social })
}

const BASE: ActivityDef = ActivityDef {
    id: ActivityId::Stroll,
    label: "",
    verb: "",
    anim: AnimId::Idle,
    tool: None,
    minutes: [0, 0],
    needs: None,
    heal: 0.0,
    yields: None,
    materials: None,
    indoors: false,
    kind: ActivityKind::Leisure,
};

/// In the TypeScript object's key order (`ACTIVITY_IDS`).
pub static ACTIVITIES: [ActivityDef; 22] = [
    ActivityDef {
        id: ActivityId::Eat,
        label: "eat a meal at the tavern",
        verb: "eating",
        anim: AnimId::SitChair,
        minutes: [22, 30],
        needs: needs(3.4, 0.0, 0.25),
        kind: ActivityKind::Need,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Drink,
        label: "have a drink at the tavern",
        verb: "drinking",
        anim: AnimId::SitChair,
        tool: Some("weapons/mug_full"),
        minutes: [40, 80],
        needs: needs(0.0, -0.02, 0.45),
        kind: ActivityKind::Leisure,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Sleep,
        label: "sleep at home",
        verb: "sleeping",
        anim: AnimId::Lie,
        minutes: [420, 480],
        needs: needs(0.0, 0.24, 0.0),
        heal: 0.22,
        indoors: true,
        kind: ActivityKind::Need,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Rest,
        label: "sit and rest on a bench",
        verb: "resting",
        anim: AnimId::SitChair,
        minutes: [25, 50],
        needs: needs(0.0, 0.07, 0.08),
        heal: 0.05,
        kind: ActivityKind::Leisure,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Smith,
        label: "work at the forge",
        verb: "working the forge",
        anim: AnimId::Hammer,
        tool: Some("tools/hammer"),
        minutes: [90, 150],
        kind: ActivityKind::Work,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Farm,
        label: "tend the vegetable plots at the farm",
        verb: "farming",
        anim: AnimId::Dig,
        tool: Some("tools/shovel"),
        minutes: [90, 150],
        yields: Some((ItemId::Food, 70)),
        kind: ActivityKind::Work,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::ChopWood,
        label: "chop wood at the lumber camp",
        verb: "chopping wood",
        anim: AnimId::Chop,
        tool: Some("tools/axe"),
        minutes: [80, 140],
        materials: Some((Resource::Wood, 40)),
        kind: ActivityKind::Work,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Fish,
        label: "fish at the lake shore",
        verb: "fishing",
        anim: AnimId::Fish,
        tool: Some("tools/fishing_rod"),
        minutes: [70, 130],
        yields: Some((ItemId::Food, 60)),
        needs: needs(0.0, 0.02, 0.0),
        kind: ActivityKind::Work,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Mine,
        label: "dig for ore at the old mine",
        verb: "mining",
        anim: AnimId::Pickaxe,
        tool: Some("tools/pickaxe"),
        minutes: [80, 140],
        materials: Some((Resource::Stone, 50)),
        kind: ActivityKind::Work,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Trade,
        label: "tend a stall at the market",
        verb: "tending a market stall",
        anim: AnimId::WorkA,
        minutes: [90, 150],
        needs: needs(0.0, 0.0, 0.2),
        yields: Some((ItemId::Gift, 120)),
        kind: ActivityKind::Work,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Shop,
        label: "browse the market for food or a small gift",
        verb: "browsing the market",
        anim: AnimId::Interact,
        minutes: [12, 20],
        needs: needs(0.0, 0.0, 0.15),
        kind: ActivityKind::Leisure,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Pray,
        label: "pray or meditate in quiet",
        verb: "praying",
        anim: AnimId::SitFloor,
        minutes: [30, 55],
        kind: ActivityKind::Leisure,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Train,
        label: "practise fighting at the training yard",
        verb: "training",
        anim: AnimId::Idle,
        minutes: [50, 90],
        needs: needs(0.0, -0.04, 0.0),
        kind: ActivityKind::Work,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Forage,
        label: "gather herbs and flowers in the forest glade",
        verb: "foraging",
        anim: AnimId::PickUp,
        minutes: [60, 110],
        yields: Some((ItemId::Gift, 55)),
        kind: ActivityKind::Work,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Watch,
        label: "stand watch at the watchtower",
        verb: "standing watch",
        anim: AnimId::Guard,
        minutes: [120, 180],
        kind: ActivityKind::Duty,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Relax,
        label: "sit by the campfire in the glade",
        verb: "relaxing by the fire",
        anim: AnimId::SitFloor,
        minutes: [40, 80],
        needs: needs(0.0, 0.05, 0.12),
        kind: ActivityKind::Leisure,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Brew,
        label: "brew potions at the herb table in the glade",
        verb: "brewing potions",
        anim: AnimId::WorkC,
        minutes: [80, 130],
        yields: Some((ItemId::Potion, 85)),
        kind: ActivityKind::Work,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Mourn,
        label: "visit the graves",
        verb: "mourning at the graves",
        anim: AnimId::SitFloor,
        minutes: [25, 45],
        kind: ActivityKind::Leisure,
        ..BASE
    },
    ActivityDef {
        id: ActivityId::Stroll,
        label: "stroll around the village",
        verb: "strolling",
        anim: AnimId::Walk,
        minutes: [25, 50],
        needs: needs(0.0, 0.0, 0.05),
        kind: ActivityKind::Leisure,
        ..BASE
    },
    // Not in the TypeScript table: billboards came after the port.
    ActivityDef {
        id: ActivityId::ReadBoard,
        label: "read the billboard",
        verb: "reading the billboard",
        anim: AnimId::Idle,
        minutes: [2, 4],
        kind: ActivityKind::Leisure,
        ..BASE
    },
    // Nor is this: a house that burned down is built again by hand.
    ActivityDef {
        id: ActivityId::Rebuild,
        label: "help rebuild the house that burned down",
        verb: "rebuilding",
        anim: AnimId::Hammer,
        tool: Some("tools/hammer"),
        minutes: [90, 150],
        needs: needs(0.0, -0.02, 0.1),
        kind: ActivityKind::Work,
        ..BASE
    },
    // Children came after the port too: a game with no slot, run like a stroll between the play spots.
    ActivityDef {
        id: ActivityId::Play,
        label: "play with the other children",
        verb: "playing",
        anim: AnimId::Cheer,
        minutes: [40, 90],
        needs: needs(0.0, -0.03, 0.3),
        kind: ActivityKind::Leisure,
        ..BASE
    },
];

pub fn activity(id: ActivityId) -> &'static ActivityDef {
    ACTIVITIES.iter().find(|def| def.id == id).expect("every activity has a definition")
}

pub const ITEM_CAP: i64 = 3;
