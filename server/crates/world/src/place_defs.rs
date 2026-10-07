//! Hand-authored places. Coordinates are local to the place: the origin is the
//! hex centre (plus `offset`), +z points out of the front (towards the door /
//! the road), +x is to the right when looking at the front from outside... i.e.
//! for a place facing south (0°) local +x is east. Angles are degrees.

use serde::{Deserialize, Serialize};

use ActivityId::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityId {
    Eat,
    Drink,
    Sleep,
    Rest,
    Smith,
    Farm,
    ChopWood,
    Fish,
    Mine,
    Trade,
    Shop,
    Pray,
    Train,
    Forage,
    Watch,
    Relax,
    Brew,
    Mourn,
    Stroll,
    ReadBoard,
    Rebuild,
}

impl ActivityId {
    pub const ALL: [ActivityId; 21] = [
        Eat, Drink, Sleep, Rest, Smith, Farm, ChopWood, Fish, Mine, Trade, Shop, Pray, Train, Forage, Watch, Relax,
        Brew, Mourn, Stroll, ReadBoard, Rebuild,
    ];

    /// What a mind may always choose: everything but reading a billboard, which only the routine sends people
    /// to, and rebuilding, which is only there to choose while a house lies in ruins.
    pub const OFFERED: [ActivityId; 19] = [
        Eat, Drink, Sleep, Rest, Smith, Farm, ChopWood, Fish, Mine, Trade, Shop, Pray, Train, Forage, Watch, Relax,
        Brew, Mourn, Stroll,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Eat => "eat",
            Drink => "drink",
            Sleep => "sleep",
            Rest => "rest",
            Smith => "smith",
            Farm => "farm",
            ChopWood => "chop_wood",
            Fish => "fish",
            Mine => "mine",
            Trade => "trade",
            Shop => "shop",
            Pray => "pray",
            Train => "train",
            Forage => "forage",
            Watch => "watch",
            Relax => "relax",
            Brew => "brew",
            Mourn => "mourn",
            Stroll => "stroll",
            ReadBoard => "read_board",
            Rebuild => "rebuild",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.as_str() == text)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaceKind {
    #[default]
    Home,
    Tavern,
    Green,
    Market,
    Smithy,
    Chapel,
    Training,
    Farm,
    Lumber,
    Glade,
    Mine,
    Docks,
    Graveyard,
    Ruins,
    Watchtower,
    Road,
}

#[derive(Clone, Debug)]
pub struct PropDef {
    /// "bundle/model"
    pub model: &'static str,
    pub at: [f64; 2],
    pub rot: Option<f64>,
    pub scale: Option<f64>,
    pub y: Option<f64>,
    /// blocking radius for navigation; `None` = derived from the model size, 0 = walk-through
    pub block: Option<f64>,
}

fn prop(model: &'static str, x: f64, z: f64) -> PropDef {
    PropDef { model, at: [x, z], rot: None, scale: None, y: None, block: None }
}

impl PropDef {
    fn rot(mut self, rot: f64) -> Self {
        self.rot = Some(rot);
        self
    }
    fn scale(mut self, scale: f64) -> Self {
        self.scale = Some(scale);
        self
    }
    fn y(mut self, y: f64) -> Self {
        self.y = Some(y);
        self
    }
    fn block(mut self, block: f64) -> Self {
        self.block = Some(block);
        self
    }
}

#[derive(Clone, Debug)]
pub struct SlotDef {
    pub activities: Vec<ActivityId>,
    pub at: [f64; 2],
    /// direction the character faces, local degrees (0 = out of the front)
    pub face: f64,
    /// animation override (an anim id)
    pub anim: Option<&'static str>,
    /// for training: a spot to shoot from rather than strike from
    pub ranged: bool,
}

fn slot(activities: &[ActivityId], x: f64, z: f64, face: f64) -> SlotDef {
    SlotDef { activities: activities.to_vec(), at: [x, z], face, anim: None, ranged: false }
}

impl SlotDef {
    fn anim(mut self, anim: &'static str) -> Self {
        self.anim = Some(anim);
        self
    }
    fn ranged(mut self) -> Self {
        self.ranged = true;
        self
    }
}

/// A large billboard on two posts. Its face looks along board-local +z, towards the people reading it.
#[derive(Clone, Debug)]
pub struct BillboardDef {
    pub id: &'static str,
    pub at: [f64; 2],
    /// local degrees
    pub rot: f64,
}

fn board(id: &'static str, x: f64, z: f64, rot: f64) -> BillboardDef {
    BillboardDef { id, at: [x, z], rot }
}

/// Size of a billboard's face, in metres.
pub const BILLBOARD_WIDTH: f64 = 3.4;
pub const BILLBOARD_HEIGHT: f64 = 1.9;
/// Height of the bottom of the face above the ground.
pub const BILLBOARD_LIFT: f64 = 1.3;
/// Depth of the footprint people walk around.
pub const BILLBOARD_DEPTH: f64 = 0.5;
/// The two reading spots, board-local: (±x, z).
pub const BILLBOARD_READ_AT: [f64; 2] = [0.8, 2.0];

#[derive(Clone, Debug)]
pub struct BuildingDef {
    /// "bundle/model"
    pub model: String,
    pub at: Option<[f64; 2]>,
    pub scale: Option<f64>,
    pub rot: Option<f64>,
}

fn building(model: &str, x: f64, z: f64) -> Option<BuildingDef> {
    Some(BuildingDef { model: model.into(), at: Some([x, z]), scale: None, rot: None })
}

/// What the village may put up on a pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadKind {
    Fence,
    LogStack,
    Bench,
    Crate,
    Planter,
}

impl PadKind {
    pub const ALL: [PadKind; 5] = [Self::Fence, Self::LogStack, Self::Bench, Self::Crate, Self::Planter];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fence => "fence",
            Self::LogStack => "log_stack",
            Self::Bench => "bench",
            Self::Crate => "crate",
            Self::Planter => "planter",
        }
    }
}

/// A spot inside a place's clearing where the village may build something small later. Pads are the
/// server's business only: what stands on one reaches the client with its own placement.
#[derive(Clone, Debug)]
pub struct PadDef {
    pub at: [f64; 2],
    /// turn in degrees, as a prop's
    pub rot: f64,
    pub kinds: &'static [PadKind],
}

const fn pad(x: f64, z: f64, rot: f64, kinds: &'static [PadKind]) -> PadDef {
    PadDef { at: [x, z], rot, kinds }
}

#[derive(Clone, Debug, Default)]
pub struct PlaceDef {
    pub id: String,
    pub name: &'static str,
    pub kind: PlaceKind,
    /// one line for LLM prompts
    pub description: &'static str,
    pub hex: [i32; 2],
    pub offset: Option<[f64; 2]>,
    /// world direction of local +z, degrees: 0 south, 90 east, 180 north, 270 west
    pub facing: f64,
    pub building: Option<BuildingDef>,
    /// where characters go in and out; default just in front of the building
    pub door: Option<[f64; 2]>,
    pub props: Vec<PropDef>,
    pub slots: Vec<SlotDef>,
    /// people who can live here
    pub beds: Option<u32>,
    /// scatter (trees, rocks, grass) is kept this far from the origin
    pub clear: Option<f64>,
    /// local points where the dead are buried / monsters rise
    pub grave_plots: Vec<[f64; 2]>,
    pub spawn_points: Vec<[f64; 2]>,
    pub billboards: Vec<BillboardDef>,
    /// where the village may build small things
    pub pads: Vec<PadDef>,
}

fn seats_around(cx: f64, cz: f64, r: f64, activities: &[ActivityId]) -> Vec<SlotDef> {
    vec![
        slot(activities, cx - r, cz, 90.0).anim("sit_chair"),
        slot(activities, cx + r, cz, 270.0).anim("sit_chair"),
        slot(activities, cx, cz - r, 0.0).anim("sit_chair"),
        slot(activities, cx, cz + r, 180.0).anim("sit_chair"),
    ]
}

fn stools_around(cx: f64, cz: f64, r: f64) -> Vec<PropDef> {
    vec![
        prop("dungeon/stool", cx - r, cz).block(0.0),
        prop("dungeon/stool", cx + r, cz).block(0.0),
        prop("dungeon/stool", cx, cz - r).block(0.0),
        prop("dungeon/stool", cx, cz + r).block(0.0),
    ]
}

const FLOWERS: [&str; 4] =
    ["custom/flowers_red", "custom/flowers_yellow", "custom/flowers_violet", "custom/flowers_mixed"];

fn home(n: usize, name: &'static str, hex: [i32; 2], facing: f64, model: &'static str) -> PlaceDef {
    let big = model.contains("home_B");
    let front = if big { 3.9 } else { 2.7 };
    PlaceDef {
        id: format!("home_{n}"),
        name,
        kind: PlaceKind::Home,
        description: "A cottage where villagers sleep.",
        hex,
        facing,
        building: building(&format!("hex/{model}"), 0.0, -1.5),
        door: Some([0.0, front - 1.5 + 1.4]),
        beds: Some(2),
        clear: Some(7.5),
        props: vec![
            prop("halloween/bench", -2.6, front - 0.7).rot(0.0).block(0.0),
            prop("dungeon/barrel_small", 2.9, front - 1.0),
            prop("halloween/post_lantern", 3.6, front + 1.6).rot(90.0).block(0.3),
            prop(FLOWERS[n % FLOWERS.len()], 1.5, front - 1.0).scale(1.5).block(0.0),
            prop(FLOWERS[(n + 2) % FLOWERS.len()], -1.3, front - 1.1).scale(1.3).rot(70.0).block(0.0),
        ],
        slots: vec![
            slot(&[Rest], -3.0, front - 0.7, 0.0).anim("sit_chair"),
            slot(&[Rest], -2.2, front - 0.7, 0.0).anim("sit_chair"),
        ],
        pads: vec![
            pad(5.4, front - 2.4, 0.0, &[PadKind::LogStack, PadKind::Fence, PadKind::Crate]),
            pad(-4.8, front - 3.6, 0.0, &[PadKind::Fence, PadKind::Planter, PadKind::LogStack]),
        ],
        ..PlaceDef::default()
    }
}

pub fn place_defs() -> Vec<PlaceDef> {
    vec![
        PlaceDef {
            id: "tavern".into(),
            name: "The Gilded Tankard",
            kind: PlaceKind::Tavern,
            description: "The village tavern: food, ale and gossip at the tables outside.",
            hex: [5, 6],
            facing: 0.0,
            building: building("hex/building_tavern_red", 0.0, -3.0),
            door: Some([0.0, 2.8]),
            clear: Some(10.0),
            props: [
                vec![
                    prop("dungeon/table_medium_decorated_A", -2.8, 5.4),
                    prop("dungeon/table_medium_decorated_B", 2.8, 5.4),
                ],
                stools_around(-2.8, 5.4, 1.55),
                stools_around(2.8, 5.4, 1.55),
                vec![
                    prop("dungeon/keg_decorated", -5.6, 1.6).rot(20.0),
                    prop("dungeon/barrel_large", 5.6, 1.4),
                    prop("dungeon/barrel_small_stack", 6.2, 3.2).rot(30.0),
                    prop("halloween/post_lantern", -6.2, 6.6).rot(90.0).block(0.3),
                    prop("halloween/post_lantern", 6.2, 6.6).rot(270.0).block(0.3),
                    prop("dungeon/banner_red", 2.2, 1.0).y(1.6).block(0.0),
                ],
            ]
            .concat(),
            slots: [seats_around(-2.8, 5.4, 1.55, &[Eat, Drink]), seats_around(2.8, 5.4, 1.55, &[Eat, Drink])].concat(),
            billboards: vec![board("tavern_board", -4.6, 9.0, 10.0)],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "green".into(),
            name: "the village green",
            kind: PlaceKind::Green,
            description: "The open green at the crossroads, with the well and benches. Everyone passes through.",
            hex: [7, 6],
            facing: 0.0,
            offset: Some([-1.0, 1.5]),
            clear: Some(9.0),
            pads: vec![
                pad(-6.2, 0.4, 90.0, &[PadKind::Bench, PadKind::Planter]),
                pad(3.4, -4.6, 0.0, &[PadKind::Bench, PadKind::Planter]),
            ],
            props: vec![
                prop("hex/building_well_blue", 0.0, 0.0).scale(4.2).block(1.9),
                prop("halloween/bench", -4.2, 1.5).rot(90.0).block(0.0),
                prop("halloween/bench", 4.2, 1.5).rot(270.0).block(0.0),
                prop("halloween/bench", 0.0, -4.4).rot(0.0).block(0.0),
                prop("halloween/sign_both", -5.5, 6.2).rot(20.0).block(0.3),
                prop("halloween/post_lantern", 5.2, 5.6).rot(270.0).block(0.3),
                prop("custom/notice_board", 6.2, -2.4).rot(-35.0).block(0.9),
                prop("custom/flowers_mixed", 2.6, -2.2).scale(1.6).block(0.0),
                prop("custom/flowers_yellow", -2.4, -2.6).scale(1.5).block(0.0),
                prop("custom/flowers_red", -3.0, 4.6).scale(1.5).block(0.0),
            ],
            slots: vec![
                slot(&[Rest], -4.2, 1.0, 90.0).anim("sit_chair"),
                slot(&[Rest], -4.2, 2.0, 90.0).anim("sit_chair"),
                slot(&[Rest], 4.2, 1.0, 270.0).anim("sit_chair"),
                slot(&[Rest], 4.2, 2.0, 270.0).anim("sit_chair"),
                slot(&[Rest], -0.5, -4.4, 0.0).anim("sit_chair"),
                slot(&[Rest], 0.5, -4.4, 0.0).anim("sit_chair"),
            ],
            billboards: vec![board("green_board", -2.0, 8.0, 0.0)],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "market".into(),
            name: "the market",
            kind: PlaceKind::Market,
            description: "Market stalls with produce, cloth and trinkets; a place to trade and to buy food or small gifts.",
            hex: [8, 6],
            facing: 0.0,
            building: building("hex/building_market_yellow", 0.0, -2.6),
            door: Some([0.0, 4.0]),
            clear: Some(10.0),
            pads: vec![pad(-5.8, 6.8, 10.0, &[PadKind::Crate]), pad(5.2, 6.6, -10.0, &[PadKind::Crate])],
            props: vec![
                prop("props/crate_carrots", -4.6, 3.4).rot(8.0),
                prop("props/crate_tomatoes", -2.4, 3.6).rot(-4.0),
                prop("props/crate_cheese", 2.6, 3.5).rot(5.0),
                prop("props/crate_buns", 4.8, 3.3).rot(-8.0),
                prop("resource/Textiles_Stack_Large_Colored", 6.6, 1.2).rot(15.0),
                prop("resource/Food_Basket_A_Berries", 0.2, 3.2).block(0.0),
                prop("halloween/wagon", -7.6, 1.0).rot(100.0),
            ],
            slots: vec![
                slot(&[Trade], -3.4, 1.7, 0.0).anim("work_a"),
                slot(&[Trade], 3.6, 1.7, 0.0).anim("work_a"),
                slot(&[Shop], -3.4, 5.4, 180.0).anim("interact"),
                slot(&[Shop], 0.2, 5.2, 180.0).anim("interact"),
                slot(&[Shop], 3.6, 5.4, 180.0).anim("interact"),
            ],
            billboards: vec![board("market_board", 8.4, 6.4, -40.0)],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "smithy".into(),
            name: "the smithy",
            kind: PlaceKind::Smithy,
            description: "The forge, with the anvil and grindstone out front. Weapons are mended and tools made here.",
            hex: [7, 8],
            facing: 180.0,
            building: building("hex/building_blacksmith_blue", 0.0, -2.4),
            door: Some([1.5, 3.4]),
            clear: Some(9.5),
            props: vec![
                prop("tools/anvil", -2.6, 4.6).rot(90.0),
                prop("tools/grindstone", 3.6, 4.4).rot(0.0),
                prop("resource/Iron_Bars", -4.8, 3.2).rot(30.0),
                prop("dungeon/bucket", -1.0, 3.6).block(0.0),
                prop("dungeon/sword_shield", 5.6, 2.2).y(0.8).rot(0.0).block(0.0),
                prop("resource/Stone_Chunks_Large", -5.6, 5.2),
            ],
            slots: vec![
                slot(&[Smith], -2.6, 5.9, 180.0).anim("hammer"),
                slot(&[Smith], 3.6, 6.3, 180.0).anim("work_b"),
            ],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "chapel".into(),
            name: "the chapel",
            kind: PlaceKind::Chapel,
            description: "A small stone chapel at the end of the north lane. Quiet; people come to pray and think.",
            hex: [6, 3],
            facing: 0.0,
            building: building("hex/building_church_blue", 0.0, -2.4),
            door: Some([0.0, 3.2]),
            clear: Some(9.0),
            props: vec![
                prop("props/Cleric_Font", 3.6, 4.4),
                prop("props/paladin_statue", -4.0, 3.6).rot(20.0),
                prop("halloween/bench", -2.0, 7.0).rot(180.0).block(0.0),
                prop("halloween/bench", 2.0, 7.0).rot(180.0).block(0.0),
                prop("halloween/lantern_standing", -1.6, 2.4).block(0.0),
                prop("halloween/lantern_standing", 1.6, 2.4).block(0.0),
            ],
            slots: vec![
                slot(&[Pray], -1.2, 4.6, 180.0).anim("sit_floor"),
                slot(&[Pray], 1.2, 4.6, 180.0).anim("sit_floor"),
                slot(&[Pray, Rest], -2.0, 7.0, 180.0).anim("sit_chair"),
                slot(&[Pray, Rest], 2.0, 7.0, 180.0).anim("sit_chair"),
            ],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "training_yard".into(),
            name: "the training yard",
            kind: PlaceKind::Training,
            description: "The barracks yard with practice dummies and archery targets.",
            hex: [3, 6],
            facing: 0.0,
            building: building("hex/building_barracks_blue", -1.5, -3.0),
            door: Some([-1.5, 3.8]),
            clear: Some(10.0),
            props: vec![
                prop("halloween/scarecrow", -5.0, 5.6).rot(0.0).block(0.6),
                prop("halloween/scarecrow", -1.6, 6.4).rot(0.0).block(0.6),
                prop("hex/target", 5.6, -1.0).scale(7.0).rot(0.0).block(0.8),
                prop("hex/target", 7.6, 0.4).scale(7.0).rot(-20.0).block(0.8),
                prop("hex/weaponrack", 3.4, 2.6).scale(7.0).rot(0.0).block(0.8),
                prop("halloween/haybale", 1.6, 6.6).rot(30.0),
                prop("custom/fence_wood", -3.3, 10.2),
                prop("custom/fence_wood", -7.8, 6.4).rot(90.0),
            ],
            slots: vec![
                slot(&[Train], -5.0, 7.3, 180.0),
                slot(&[Train], -1.6, 8.1, 180.0),
                slot(&[Train], 5.6, 6.6, 180.0).ranged(),
                slot(&[Train], 7.9, 7.6, 180.0).ranged(),
            ],
            billboards: vec![board("training_board", 3.0, 9.6, 0.0)],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "farm".into(),
            name: "the windmill farm",
            kind: PlaceKind::Farm,
            description: "The windmill, vegetable plots and grain fields south-west of the village.",
            hex: [2, 9],
            facing: 90.0,
            building: building("hex/building_windmill_blue", -2.0, -2.5),
            door: Some([-2.0, 1.8]),
            clear: Some(10.0),
            pads: vec![
                pad(-6.6, 6.2, 0.0, &[PadKind::Fence, PadKind::Crate]),
                pad(6.4, -4.6, 0.0, &[PadKind::Crate, PadKind::Fence]),
            ],
            props: [
                [-3.3, -1.1, 1.1, 3.3]
                    .into_iter()
                    .flat_map(|x: f64| {
                        [
                            prop("props/dirt_plot", x + 2.4, 4.4).scale(2.2).block(0.0),
                            prop("props/dirt_plot", x + 2.4, 6.6).scale(2.2).block(0.0),
                        ]
                    })
                    .collect::<Vec<_>>(),
                vec![
                    prop("props/lettuce", -0.9, 4.4).y(0.5).block(0.0),
                    prop("props/lettuce", 1.3, 6.6).y(0.5).block(0.0),
                    prop("props/carrot", 3.5, 4.4).y(0.7).block(0.0),
                    prop("props/carrot", 5.7, 6.6).y(0.7).block(0.0),
                    prop("props/lettuce", 5.7, 4.4).y(0.5).block(0.0),
                    prop("props/wheelbarrow", -5.4, 3.6).rot(60.0),
                    prop("halloween/haybale", 3.2, -0.6).rot(10.0),
                    prop("halloween/haybale", 4.4, -2.4).rot(80.0),
                    prop("halloween/scarecrow", 6.6, 1.6).rot(30.0).block(0.5),
                    prop("custom/fence_wood", 0.4, 8.5),
                    prop("custom/fence_wood_broken", 4.7, 8.5),
                    prop("custom/fence_wood", 7.6, 6.2).rot(90.0),
                    prop("custom/fence_wood_post", 7.6, 3.9).block(0.3),
                ],
            ]
            .concat(),
            slots: vec![
                slot(&[Farm], -0.9, 5.5, 180.0).anim("dig"),
                slot(&[Farm], 2.4, 5.5, 0.0).anim("dig"),
                slot(&[Farm], 4.6, 5.5, 180.0).anim("work_a"),
            ],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "lumber_camp".into(),
            name: "the lumber camp",
            kind: PlaceKind::Lumber,
            description: "A sawmill at the edge of the western forest, stacked with logs.",
            hex: [2, 5],
            facing: 90.0,
            building: building("hex/building_lumbermill_blue", 0.0, -2.5),
            door: Some([0.0, 3.6]),
            clear: Some(9.5),
            pads: vec![pad(-5.9, -5.5, 20.0, &[PadKind::LogStack]), pad(-2.9, 8.5, -30.0, &[PadKind::LogStack])],
            props: vec![
                prop("resource/Wood_Log_Stack", -4.6, 4.4).rot(20.0),
                prop("props/log_stacks", 5.0, 3.2).rot(-30.0),
                prop("hex/tree_single_A_cut", -1.8, 5.6).scale(7.0).block(0.5),
                prop("hex/tree_single_B_cut", 2.2, 6.4).scale(7.0).block(0.5),
                prop("resource/Wood_Planks_Stack_Medium", 5.6, 6.2).rot(70.0),
            ],
            slots: vec![
                slot(&[ChopWood], -1.8, 7.0, 180.0).anim("chop"),
                slot(&[ChopWood], 2.2, 7.8, 180.0).anim("chop"),
            ],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "glade".into(),
            name: "the Whispering Glade",
            kind: PlaceKind::Glade,
            description: "A clearing in the western forest with an old shrine, a campfire and herbs to gather.",
            hex: [3, 3],
            facing: 0.0,
            clear: Some(8.0),
            props: vec![
                prop("halloween/shrine", 0.0, -4.2).block(0.7),
                prop("props/Campfire_Base", 0.0, 2.2).block(0.9),
                prop("props/Campfire_Logs", 0.0, 2.2).y(0.15).block(0.0),
                prop("props/Tent", -5.0, -0.4).rot(70.0),
                prop("resource/Wood_Log_A", -2.4, 2.6).rot(15.0).y(0.3).block(0.0),
                prop("resource/Wood_Log_B", 2.4, 2.8).rot(-20.0).y(0.3).block(0.0),
                prop("props/Potionstation", 4.8, -1.6).rot(-40.0),
                prop("props/Cauldron", 5.4, 1.0).block(0.7),
                prop("custom/mushrooms_red", -3.2, 5.2).scale(1.6).block(0.0),
                prop("custom/mushrooms_brown", 3.6, 5.8).scale(1.5).block(0.0),
                prop("custom/mushrooms_red", -1.2, -6.0).scale(1.8).rot(120.0).block(0.0),
                prop("custom/flowers_violet", -4.4, 3.6).scale(1.5).block(0.0),
                prop("props/Basket_Mushrooms", 3.6, -3.2).block(0.0),
            ],
            slots: vec![
                slot(&[Pray], 0.0, -2.6, 180.0).anim("sit_floor"),
                slot(&[Relax], -2.2, 2.4, 90.0).anim("sit_floor"),
                slot(&[Relax], 2.2, 2.4, 270.0).anim("sit_floor"),
                slot(&[Relax], 0.0, 4.4, 180.0).anim("sit_floor"),
                slot(&[Brew], 3.9, -0.5, 140.0).anim("work_c"),
                slot(&[Forage], -3.2, 6.2, 180.0).anim("pick_up"),
                slot(&[Forage], 4.4, 6.4, 200.0).anim("pick_up"),
                slot(&[Forage], -1.2, -7.0, 0.0).anim("pick_up"),
            ],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "mine".into(),
            name: "the old mine",
            kind: PlaceKind::Mine,
            description: "A mine shaft in the northern hills where ore and stone are dug.",
            hex: [8, 2],
            facing: 0.0,
            building: building("hex/building_mine_blue", 0.0, -1.5),
            door: Some([0.0, 5.6]),
            clear: Some(10.0),
            props: vec![
                prop("dungeon/rocks_gold", -5.6, 5.4).rot(30.0),
                prop("dungeon/rocks", 5.8, 5.0).rot(-20.0),
                prop("resource/Iron_Nuggets", 3.2, 6.0).block(0.0),
            ],
            slots: vec![
                slot(&[Mine], -4.6, 7.4, 150.0).anim("pickaxe"),
                slot(&[Mine], 4.8, 7.2, 200.0).anim("pickaxe"),
            ],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "docks".into(),
            name: "the lake shore",
            kind: PlaceKind::Docks,
            description: "The sandy shore of the lake, with a boat pulled up on the beach. Good fishing.",
            hex: [8, 12],
            offset: Some([-3.5, 1.5]),
            facing: 90.0,
            door: Some([0.0, -1.0]),
            clear: Some(7.0),
            props: vec![
                prop("hex/boat", 3.6, 1.0).scale(7.0).rot(25.0).block(1.4),
                prop("hex/boatrack", 3.2, -3.2).scale(5.0).rot(-20.0).block(1.2),
                prop("resource/Food_Barrel_Fish", -1.0, -1.6),
                prop("tools/fishing_tacklebox", -0.8, 0.5).block(0.0),
                prop("tools/bucket_metal", -3.2, 0.2).block(0.0),
            ],
            slots: vec![
                slot(&[Fish], 0.5, 1.9, 0.0).anim("fish"),
                slot(&[Fish], -2.0, 1.9, 0.0).anim("fish"),
                slot(&[Fish], -4.5, 1.9, 0.0).anim("fish"),
            ],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "watchtower".into(),
            name: "the watchtower",
            kind: PlaceKind::Watchtower,
            description: "The watch post by the bridge road, looking east towards the graveyard.",
            hex: [9, 8],
            facing: 180.0,
            building: building("hex/building_watchtower_blue", 0.0, -2.0),
            door: Some([0.0, 2.8]),
            clear: Some(8.0),
            props: vec![
                prop("custom/banner_board", -4.4, 1.0).block(0.9),
                prop("dungeon/banner_shield_blue", -4.4, 1.0).y(0.0).block(0.0),
                prop("dungeon/torch_lit", 2.4, 2.2).y(1.6).block(0.0),
                prop("dungeon/crates_stacked", 4.6, 0.6).rot(20.0),
            ],
            slots: vec![slot(&[Watch], -2.4, 6.4, 300.0).anim("guard"), slot(&[Watch], 3.4, 6.8, 290.0).anim("idle")],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "graveyard".into(),
            name: "the graveyard",
            kind: PlaceKind::Graveyard,
            description: "The old graveyard east of the river. The dead are buried here, and at night some of them do not rest.",
            hex: [15, 4],
            facing: -30.0,
            clear: Some(10.0),
            props: vec![
                prop("halloween/crypt", 0.0, -5.6).rot(0.0),
                prop("halloween/fence", -4.0, 6.6).block(0.0),
                prop("halloween/fence_broken", 4.0, 6.6).block(0.0),
                prop("halloween/arch_gate", 0.0, 6.6).block(0.0),
                prop("halloween/grave_A", -4.8, -5.0).rot(0.0).block(0.9),
                prop("halloween/grave_B", 4.8, -5.0).rot(0.0).block(0.9),
                prop("halloween/grave_A_destroyed", -4.8, -2.5).rot(0.0).block(0.9),
                prop("halloween/gravemarker_A", 4.8, -2.5).rot(0.0).block(0.4),
                prop("halloween/tree_dead_large", -6.6, 2.6).rot(40.0).block(0.5),
                prop("halloween/tree_dead_medium", 6.4, 4.6).rot(0.0).block(0.4),
                prop("halloween/lantern_standing", -1.9, 5.4).block(0.0),
                prop("halloween/lantern_standing", 1.9, 5.4).block(0.0),
                prop("halloween/skull", 5.8, 0.4).rot(60.0).block(0.0),
                prop("halloween/bone_A", -6.0, -0.6).rot(20.0).block(0.0),
                prop("halloween/ribcage", 6.4, 2.2).rot(110.0).y(0.4).block(0.0),
            ],
            slots: vec![
                slot(&[Mourn, Pray], -2.4, 0.4, 270.0).anim("sit_floor"),
                slot(&[Mourn, Pray], 2.4, 0.4, 90.0).anim("idle_alt"),
            ],
            grave_plots: vec![
                [-4.8, 0.0],
                [4.8, 0.0],
                [-4.8, 2.5],
                [4.8, 2.5],
                [-2.9, 3.0],
                [2.9, 3.0],
                [-7.0, -3.6],
                [7.0, -3.6],
                [-7.0, -1.0],
                [7.0, -1.0],
            ],
            spawn_points: vec![
                [-3.4, -1.4],
                [3.4, -1.4],
                [-6.2, 4.4],
                [6.0, -5.6],
                [-2.2, 2.0],
                [2.2, 2.0],
                [0.0, -0.6],
            ],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "ruins".into(),
            name: "the old ruins",
            kind: PlaceKind::Ruins,
            description: "Crumbling walls deep in the eastern woods. Few go there by choice.",
            hex: [15, 2],
            facing: 200.0,
            clear: Some(9.0),
            props: vec![
                prop("dungeon/wall_broken", -4.0, -4.0).rot(0.0),
                prop("dungeon/wall_cracked", 0.0, -4.0).rot(0.0),
                prop("dungeon/wall_corner", 3.2, -3.2).rot(0.0),
                prop("dungeon/wall_half", 4.0, 0.2).rot(90.0),
                prop("dungeon/rubble_large", -5.0, 2.6).rot(70.0),
                prop("dungeon/rubble_half", 5.2, 4.6).rot(200.0),
                prop("dungeon/pillar", -1.6, 3.6),
                prop("dungeon/floor_tile_large_rocks", 0.0, 0.0).block(0.0),
                prop("dungeon/floor_tile_small_broken_A", -3.0, 0.0).block(0.0),
                prop("dungeon/floor_tile_small_weeds_B", 0.0, 3.0).block(0.0),
                prop("dungeon/chest_gold", 1.6, -2.6).rot(200.0),
                prop("halloween/skull", -2.2, -1.6).rot(30.0).block(0.0),
            ],
            spawn_points: vec![[0.0, 0.6], [-2.6, 1.6], [2.4, 2.2], [0.0, 5.8]],
            ..PlaceDef::default()
        },
        PlaceDef {
            id: "west_road".into(),
            name: "the west road",
            kind: PlaceKind::Road,
            description: "Where the road from the outside world enters the valley. Newcomers arrive here.",
            hex: [0, 7],
            facing: 90.0,
            clear: Some(4.0),
            props: vec![
                prop("halloween/sign_right", 3.2, 4.0).rot(180.0).block(0.3),
                prop("halloween/post_lantern", -3.0, 5.0).rot(180.0).block(0.3),
            ],
            billboards: vec![board("west_road_board", 4.4, 1.0, -90.0)],
            ..PlaceDef::default()
        },
        home(1, "the red cottage", [4, 6], 0.0, "building_home_A_red"),
        home(2, "the blue house", [9, 6], 0.0, "building_home_B_blue"),
        home(3, "the green cottage", [8, 8], 180.0, "building_home_A_green"),
        home(4, "the yellow house", [5, 8], 180.0, "building_home_B_yellow"),
        home(5, "the lane cottage", [5, 5], 90.0, "building_home_A_blue"),
        home(6, "the tall house", [7, 5], 270.0, "building_home_B_red"),
        home(7, "the south cottage", [5, 9], 90.0, "building_home_A_yellow"),
        home(8, "the lakeside house", [7, 9], 270.0, "building_home_B_green"),
    ]
}
