//! The valley's animals: what each kind is like, where it keeps, and what its own are called.
//!
//! The models are the Blender-made ones in the `animals` bundle (`tools/blender/make_animals.py`); a
//! bird's flying model is its own with `_flying` after it.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Species {
    Dog,
    Cat,
    Cow,
    Sheep,
    Pig,
    Goat,
    Horse,
    Chicken,
    Duck,
    Songbird,
    Crow,
    Owl,
}

/// What an animal is doing, as the viewer draws it. None of these is a clip: the client moves the
/// model's parts itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimalAnim {
    Idle,
    Walk,
    Run,
    /// head down in the grass
    Graze,
    /// pecking at the ground, for birds
    Peck,
    Sleep,
    /// sitting, for dogs and cats
    Sit,
    /// in the air, for birds
    Fly,
    /// carrying a rider
    Ridden,
}

impl AnimalAnim {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Walk => "walk",
            Self::Run => "run",
            Self::Graze => "graze",
            Self::Peck => "peck",
            Self::Sleep => "sleep",
            Self::Sit => "sit",
            Self::Fly => "fly",
            Self::Ridden => "ridden",
        }
    }
}

/// Where an animal keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haunt {
    /// the meadow south of the windmill farm
    Pasture,
    /// round the windmill itself
    Farmyard,
    /// the lake shore
    Shore,
    /// at its owner's heels, or by their door
    Home,
    /// the green, the market and the tavern
    Village,
    /// over the grain fields and the farm
    Fields,
    /// the Whispering Glade
    Glade,
}

pub struct SpeciesDef {
    pub species: Species,
    /// "a cow", for the inspector and the prompts
    pub label: &'static str,
    pub noun: &'static str,
    pub plural: &'static str,
    /// the colours it comes in, as models in the `animals` bundle
    pub models: &'static [&'static str],
    /// paces per second
    pub walk: f64,
    pub run: f64,
    pub haunt: Haunt,
    /// how far from the middle of its haunt it wanders
    pub roam: f64,
    /// crosses the valley through the air rather than on foot
    pub flies: bool,
    /// awake at night and asleep by day
    pub nocturnal: bool,
    /// what it does when it stops for a while
    pub pause: AnimalAnim,
    /// a villager may keep one; it follows them about and they name it
    pub pet: bool,
    /// a villager may ride one on a long trip
    pub rideable: bool,
    /// what tending it gives the village, if anything
    pub yields: Option<&'static str>,
    /// names its keeper or the sim may give it
    pub names: &'static [&'static str],
}

pub const SPECIES: [SpeciesDef; 12] = [
    SpeciesDef {
        species: Species::Dog,
        label: "a dog",
        noun: "dog",
        plural: "dogs",
        models: &["dog_brown", "dog_collie"],
        walk: 3.0,
        run: 6.4,
        haunt: Haunt::Home,
        roam: 4.0,
        flies: false,
        nocturnal: false,
        pause: AnimalAnim::Sit,
        pet: true,
        rideable: false,
        yields: None,
        names: &[
            "Biscuit", "Bramble", "Pip", "Rufus", "Bess", "Scout", "Tansy", "Bran", "Nettle", "Sorrel", "Patch",
            "Juniper",
        ],
    },
    SpeciesDef {
        species: Species::Cat,
        label: "a cat",
        noun: "cat",
        plural: "cats",
        models: &["cat_ginger", "cat_grey"],
        walk: 2.8,
        run: 6.0,
        haunt: Haunt::Home,
        roam: 4.0,
        flies: false,
        nocturnal: false,
        pause: AnimalAnim::Sit,
        pet: true,
        rideable: false,
        yields: None,
        names: &["Mittens", "Sooty", "Whisker", "Ember", "Thistle", "Pepper", "Smudge", "Marigold", "Tibbs", "Cinder"],
    },
    SpeciesDef {
        species: Species::Cow,
        label: "a cow",
        noun: "cow",
        plural: "cows",
        models: &["cow_holstein", "cow_brown"],
        walk: 1.0,
        run: 3.6,
        haunt: Haunt::Pasture,
        roam: 11.0,
        flies: false,
        nocturnal: false,
        pause: AnimalAnim::Graze,
        pet: false,
        rideable: false,
        yields: Some("milk"),
        names: &[],
    },
    SpeciesDef {
        species: Species::Sheep,
        label: "a sheep",
        noun: "sheep",
        plural: "sheep",
        models: &["sheep_white", "sheep_white", "sheep_white", "sheep_black"],
        walk: 1.1,
        run: 4.0,
        haunt: Haunt::Pasture,
        roam: 11.0,
        flies: false,
        nocturnal: false,
        pause: AnimalAnim::Graze,
        pet: false,
        rideable: false,
        yields: Some("wool"),
        names: &[],
    },
    SpeciesDef {
        species: Species::Pig,
        label: "a pig",
        noun: "pig",
        plural: "pigs",
        models: &["pig_pink", "pig_spotted"],
        walk: 1.0,
        run: 3.6,
        haunt: Haunt::Farmyard,
        roam: 7.0,
        flies: false,
        nocturnal: false,
        pause: AnimalAnim::Graze,
        pet: false,
        rideable: false,
        yields: None,
        names: &[],
    },
    SpeciesDef {
        species: Species::Goat,
        label: "a goat",
        noun: "goat",
        plural: "goats",
        models: &["goat_white", "goat_brown"],
        walk: 1.3,
        run: 4.6,
        haunt: Haunt::Pasture,
        roam: 12.0,
        flies: false,
        nocturnal: false,
        pause: AnimalAnim::Graze,
        pet: false,
        rideable: false,
        yields: Some("milk"),
        names: &[],
    },
    SpeciesDef {
        species: Species::Horse,
        label: "a horse",
        noun: "horse",
        plural: "horses",
        models: &["horse_chestnut", "horse_grey"],
        walk: 1.6,
        run: 5.8,
        haunt: Haunt::Pasture,
        roam: 10.0,
        flies: false,
        nocturnal: false,
        pause: AnimalAnim::Graze,
        pet: false,
        rideable: true,
        yields: None,
        names: &["Clover", "Duke", "Willow", "Thunder", "Hazel", "Barley", "Oakheart", "Dapple"],
    },
    SpeciesDef {
        species: Species::Chicken,
        label: "a chicken",
        noun: "chicken",
        plural: "chickens",
        models: &["chicken_white", "chicken_brown"],
        walk: 1.0,
        run: 3.4,
        haunt: Haunt::Farmyard,
        roam: 6.0,
        flies: false,
        nocturnal: false,
        pause: AnimalAnim::Peck,
        pet: false,
        rideable: false,
        yields: Some("eggs"),
        names: &[],
    },
    SpeciesDef {
        species: Species::Duck,
        label: "a duck",
        noun: "duck",
        plural: "ducks",
        models: &["duck_white", "duck_mallard"],
        walk: 0.9,
        run: 3.0,
        haunt: Haunt::Shore,
        roam: 6.0,
        flies: false,
        nocturnal: false,
        pause: AnimalAnim::Peck,
        pet: false,
        rideable: false,
        yields: Some("eggs"),
        names: &[],
    },
    SpeciesDef {
        species: Species::Songbird,
        label: "a songbird",
        noun: "songbird",
        plural: "songbirds",
        models: &["robin", "bluebird"],
        walk: 1.0,
        run: 7.0,
        haunt: Haunt::Village,
        roam: 9.0,
        flies: true,
        nocturnal: false,
        pause: AnimalAnim::Peck,
        pet: false,
        rideable: false,
        yields: None,
        names: &[],
    },
    SpeciesDef {
        species: Species::Crow,
        label: "a crow",
        noun: "crow",
        plural: "crows",
        models: &["crow"],
        walk: 1.1,
        run: 7.5,
        haunt: Haunt::Fields,
        roam: 12.0,
        flies: true,
        nocturnal: false,
        pause: AnimalAnim::Peck,
        pet: false,
        rideable: false,
        yields: None,
        names: &[],
    },
    SpeciesDef {
        species: Species::Owl,
        label: "an owl",
        noun: "owl",
        plural: "owls",
        models: &["owl_brown", "owl_snowy"],
        walk: 0.6,
        run: 6.0,
        haunt: Haunt::Glade,
        roam: 2.0,
        flies: false,
        nocturnal: true,
        pause: AnimalAnim::Idle,
        pet: false,
        rideable: false,
        yields: None,
        names: &[],
    },
];

impl Species {
    pub fn def(self) -> &'static SpeciesDef {
        SPECIES.iter().find(|def| def.species == self).expect("every species has a def")
    }
}

/// The animals a village is founded with, beside its pets: the farm's herd and flock, the ducks on the
/// shore and the birds about the village.
pub const FOUNDING_ANIMALS: [(Species, usize); 10] = [
    (Species::Cow, 2),
    (Species::Sheep, 4),
    (Species::Pig, 2),
    (Species::Goat, 2),
    (Species::Horse, 3),
    (Species::Chicken, 5),
    (Species::Duck, 3),
    (Species::Songbird, 4),
    (Species::Crow, 2),
    (Species::Owl, 1),
];

/// The chance that a home has a dog or a cat when the village is founded.
pub const PET_CHANCE: f64 = 0.5;
/// Of the pets, the share that are dogs.
pub const DOG_SHARE: f64 = 0.6;
