//! The whole simulation state. Plain data: ids instead of references, RNG
//! state included, so it can be saved, loaded and compared. The serde shape is
//! exactly the TypeScript `SimState` (camelCase, absent optional fields left
//! out), so browser exports load unchanged and server exports load in a browser.

use fsw_core::Vec2;
pub use fsw_world::places::ActivityId;
use fsw_world::places::{Obstacle, PadKind};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};

pub use crate::data::anims::AnimId;
pub use crate::data::classes::ClassId;
pub use crate::data::monsters::MonsterKind;
pub use crate::nav::NavOverlay;

pub const SAVE_VERSION: u32 = 1;

pub type Id = String;
pub type Map<V> = IndexMap<Id, V>;

/// Reserved for text-to-speech: how this character should sound.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoiceProfile {
    /// 0.5 (deep) .. 1.5 (high)
    pub pitch: f64,
    /// 0.7 (slow) .. 1.3 (fast)
    pub rate: f64,
    pub timbre: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pronouns {
    #[serde(rename = "she/her")]
    SheHer,
    #[serde(rename = "he/him")]
    HeHim,
    #[serde(rename = "they/them")]
    TheyThem,
}

impl Pronouns {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SheHer => "she/her",
            Self::HeHim => "he/him",
            Self::TheyThem => "they/them",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub name: String,
    pub pronouns: Pronouns,
    pub age: i64,
    pub occupation: String,
    pub backstory: String,
    pub traits: Vec<String>,
    pub goals: Vec<String>,
    pub speech_style: String,
    /// 0 calm .. 1 hot-headed
    pub temper: f64,
    /// 0 timid .. 1 fearless
    pub bravery: f64,
    /// 0 solitary .. 1 gregarious
    pub sociability: f64,
    /// 0 lets things go .. 1 goes looking for trouble: quarrels without a grudge, nurses grudges and
    /// comes to blows sooner. Left out of saves at 0, so the old browser saves round-trip unchanged.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub spite: f64,
    /// 0 none .. 1 shameless: flirts freely, charms admirers and enjoys being
    /// fought over (`systems::charm`). Left out of saves when 0, so saves made
    /// before it existed load and write back unchanged.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub allure: f64,
    /// 0 at ease .. 1 tongue-tied with anyone they do not yet trust (worse with women)
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shyness: f64,
    /// 0 unremarkable .. 1 striking: others warm to them and fall for them faster,
    /// whatever they do (unlike `allure`, which is flirting on purpose)
    #[serde(default, skip_serializing_if = "is_zero")]
    pub looks: f64,
    pub voice: VoiceProfile,
    /// what this character hides from everyone; only their own prompts see it
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub secret: String,
}

/// A fixed leaning the sim itself acts on, not just words in a prompt. Authored
/// characters carry them; wanderers have none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quirk {
    /// now and then stalks someone who is alone and strikes from behind
    Murderous,
    /// breaks and runs at the first blow
    Coward,
    /// shirks work for naps and idling
    Lazy,
    /// picks on the timid; nobody warms to them
    Bully,
    /// works through the hours others spend at leisure
    Diligent,
    /// hits harder and holds on longer than their class would
    Fighter,
    /// only ever falls for women
    LovesWomen,
    /// never takes a lover; romance and warmth bewilder them
    Chaste,
    /// dabbles in necromancy by night, and the dead answer
    Occultist,
    /// tells stories about the neighbours, most of them made up
    Rumourmonger,
    /// drinks too much, and gets into fights when drunk
    Drunkard,
}

/// Someone new for the valley, as the language model or the sim's own lists describe them. A
/// `Command::Settle` or `Command::NewcomerDraft` carries one; the sim turns it into a character,
/// drawing the look and the voice's pitch and rate itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewVillager {
    pub class_id: crate::data::classes::ClassId,
    /// "First Last"; the first name must be nobody else's, living or dead
    pub name: String,
    pub pronouns: Pronouns,
    pub age: i64,
    pub occupation: String,
    pub backstory: String,
    pub traits: Vec<String>,
    pub goals: Vec<String>,
    pub speech_style: String,
    /// the voice's colour, in words
    pub timbre: String,
    pub temper: f64,
    pub bravery: f64,
    pub sociability: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub spite: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub allure: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shyness: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub looks: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub secret: String,
    /// why they came up the road (newcomers), for the chronicle
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quirks: Vec<Quirk>,
    /// people already here they know
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bonds: Vec<Bond>,
}

/// A tie a newcomer brings with them to someone living, by full name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bond {
    pub to: String,
    pub relation: BondKind,
    /// how the newcomer puts it, in the first person; the sim words the other side
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BondKind {
    Sibling,
    Parent,
    Child,
    Spouse,
    Friend,
    Comrade,
    Rival,
}

/// Bookkeeping for quirks, kept with the character so it survives a save.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuirkState {
    /// drunk until this tick
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub drunk_until_tick: i64,
    /// the last day a hunt was decided (whether or not it was a hunting day)
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub hunt_rolled_day: i64,
    /// hunting on this day
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub hunt_day: i64,
    /// a night spent over forbidden books: the day it began
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub study_day: i64,
    /// next tick a quirk may act on its own (drunken trouble, the dead answering)
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub next_roll_tick: i64,
}

impl QuirkState {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

/// What a child is until they come of age; `None` for adults, and left out of a
/// save, so worlds saved before children existed load unchanged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Childhood {
    /// the sim day they next turn a year older
    pub next_birthday_day: i64,
    /// 0 never up to anything .. 1 trouble every day
    pub mischief: f64,
    /// ids of the people who raise them; empty for a child the village raises
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<Id>,
    /// today's piece of trouble, decided at dawn and carried out when the chance comes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<MischiefPlan>,
}

/// The kinds of trouble a child gets into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mischief {
    PinchFood,
    WanderOff,
    PlayWithFire,
    HideTool,
    TallTale,
    ThrowFood,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MischiefPlan {
    pub kind: Mischief,
    /// where it happens, for the kinds that need a place
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_id: Option<String>,
    /// the tick it was carried out, or 0 while it is still to come
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub done_tick: i64,
}

fn is_zero_tick(value: &i64) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Look {
    pub model: String,
    /// texture id override (recolour); `None` keeps the model's own
    pub texture: Option<String>,
    /// mesh names to hide (helmet, cape, hat)
    pub hide: Vec<String>,
    pub main_hand: Option<String>,
    pub off_hand: Option<String>,
}

/// 0..100, higher is better: fed, rested, in good company.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Needs {
    pub hunger: f64,
    pub energy: f64,
    pub social: f64,
}

/// What a villager keeps in their pack: food, potions and gifts of their own making, and the
/// valuables the dead leave behind (a trinket, or a relic of the necromancer's), which are worth
/// nothing but what the market pays for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemId {
    Food,
    Potion,
    Gift,
    Trinket,
    Relic,
}

impl ItemId {
    pub const ALL: [ItemId; 5] = [Self::Food, Self::Potion, Self::Gift, Self::Trinket, Self::Relic];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Food => "food",
            Self::Potion => "potion",
            Self::Gift => "gift",
            Self::Trinket => "trinket",
            Self::Relic => "relic",
        }
    }

    /// "2 food", "1 healing potion", "3 small gifts", "1 trinket", "2 relics".
    pub fn label(self, count: i64) -> String {
        let one = count == 1;
        let noun = match self {
            Self::Food => "food",
            Self::Potion if one => "healing potion",
            Self::Potion => "healing potions",
            Self::Gift if one => "small gift",
            Self::Gift => "small gifts",
            Self::Trinket if one => "trinket",
            Self::Trinket => "trinkets",
            Self::Relic if one => "relic",
            Self::Relic => "relics",
        };
        format!("{count} {noun}")
    }

    /// Whether this is something the dead leave behind, kept only to be sold.
    pub fn is_valuable(self) -> bool {
        matches!(self, Self::Trinket | Self::Relic)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    pub food: i64,
    pub potion: i64,
    pub gift: i64,
    /// valuables from the dead, left out of a save when there are none so older saves read as they did
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub trinket: i64,
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub relic: i64,
}

impl Inventory {
    pub fn get(&self, item: ItemId) -> i64 {
        match item {
            ItemId::Food => self.food,
            ItemId::Potion => self.potion,
            ItemId::Gift => self.gift,
            ItemId::Trinket => self.trinket,
            ItemId::Relic => self.relic,
        }
    }

    pub fn get_mut(&mut self, item: ItemId) -> &mut i64 {
        match item {
            ItemId::Food => &mut self.food,
            ItemId::Potion => &mut self.potion,
            ItemId::Gift => &mut self.gift,
            ItemId::Trinket => &mut self.trinket,
            ItemId::Relic => &mut self.relic,
        }
    }

    /// How many valuables are in the pack.
    pub fn valuables(&self) -> i64 {
        self.trinket + self.relic
    }
}

/// Raw materials: what felling, mining and taking things apart yield, and what building costs. They
/// are counted, not carried as items, and move between villagers' arms and the village's stockpiles.
/// Coin is not one of them: it is `Character.gold`, so that neither the arms' cap nor a stockpile's
/// holds it back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Resource {
    Wood,
    Stone,
    Ore,
}

impl Resource {
    pub const ALL: [Resource; 3] = [Self::Wood, Self::Stone, Self::Ore];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wood => "wood",
            Self::Stone => "stone",
            Self::Ore => "ore",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == text)
    }

    /// "3 logs of wood", "1 block of stone", "2 lumps of ore".
    pub fn label(self, count: i64) -> String {
        let unit = match (self, count) {
            (Self::Wood, 1) => "log",
            (Self::Wood, _) => "logs",
            (Self::Stone, 1) => "block",
            (Self::Stone, _) => "blocks",
            (Self::Ore, 1) => "lump",
            (Self::Ore, _) => "lumps",
        };
        format!("{count} {unit} of {}", self.as_str())
    }
}

/// A count of each material, in a villager's arms or a stockpile. Zero counts are not kept, so an
/// empty store serialises to nothing and is left out of saves.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Store(IndexMap<Resource, i64>);

impl Store {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, resource: Resource) -> i64 {
        self.0.get(&resource).copied().unwrap_or(0)
    }

    /// Everything in it, in a fixed order.
    pub fn iter(&self) -> impl Iterator<Item = (Resource, i64)> + '_ {
        Resource::ALL.into_iter().filter(|&r| self.get(r) > 0).map(|r| (r, self.get(r)))
    }

    pub fn total(&self) -> i64 {
        self.0.values().sum()
    }

    /// Adds up to `count`, no more than `room` in all; returns how many went in.
    pub fn add(&mut self, resource: Resource, count: i64, room: i64) -> i64 {
        let added = count.min(room).max(0);
        if added > 0 {
            *self.0.entry(resource).or_insert(0) += added;
        }
        added
    }

    /// Takes up to `count`; returns how many came out.
    pub fn take(&mut self, resource: Resource, count: i64) -> i64 {
        let have = self.get(resource);
        let taken = count.min(have).max(0);
        if taken > 0 {
            if taken == have {
                self.0.shift_remove(&resource);
            } else {
                self.0[&resource] -= taken;
            }
        }
        taken
    }

    /// Whether it holds at least everything in `cost`.
    pub fn has_all(&self, cost: &Store) -> bool {
        cost.iter().all(|(r, n)| self.get(r) >= n)
    }

    /// "3 logs of wood and 1 block of stone".
    pub fn describe(&self) -> String {
        let parts: Vec<String> = self.iter().map(|(r, n)| r.label(n)).collect();
        match parts.len() {
            0 => "nothing".into(),
            1 => parts[0].clone(),
            n => format!("{} and {}", parts[..n - 1].join(", "), parts[n - 1]),
        }
    }
}

/// How much a villager carries in all.
pub const CARRY_CAP: i64 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationLabel {
    #[serde(rename = "stranger")]
    Stranger,
    #[serde(rename = "acquaintance")]
    Acquaintance,
    #[serde(rename = "friend")]
    Friend,
    #[serde(rename = "close friend")]
    CloseFriend,
    #[serde(rename = "lover")]
    Lover,
    #[serde(rename = "rival")]
    Rival,
    #[serde(rename = "enemy")]
    Enemy,
    #[serde(rename = "family")]
    Family,
    #[serde(rename = "spouse")]
    Spouse,
}

impl RelationLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stranger => "stranger",
            Self::Acquaintance => "acquaintance",
            Self::Friend => "friend",
            Self::CloseFriend => "close friend",
            Self::Lover => "lover",
            Self::Rival => "rival",
            Self::Enemy => "enemy",
            Self::Family => "family",
            Self::Spouse => "spouse",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relationship {
    /// -100 (hatred) .. 100 (devotion)
    pub affinity: f64,
    /// -100 .. 100
    pub trust: f64,
    /// 0 .. 100 romantic interest
    pub attraction: f64,
    /// 0 .. 100 unresolved anger; drives the hostility ladder
    pub grievance: f64,
    /// 0 .. 100 how well they know each other
    pub familiarity: f64,
    pub label: RelationLabel,
    pub kin: bool,
    /// one line the character would use to describe the other; updated by reflection
    pub summary: String,
    pub last_talk_tick: i64,
    /// tick of the last thing that made this character angry at the other
    pub last_offence_tick: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    Event,
    Talk,
    Thought,
    Reflection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub tick: i64,
    pub day: i64,
    pub text: String,
    /// 1 (trivial) .. 10 (life-changing)
    pub importance: i64,
    pub kind: MemoryKind,
    pub subjects: Vec<Id>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanBlock {
    /// hour of day the block starts, 0..24
    pub hour: f64,
    pub activity: ActivityId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TalkTone {
    Friendly,
    Romantic,
    Hostile,
    Apology,
}

impl TalkTone {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Friendly => "friendly",
            Self::Romantic => "romantic",
            Self::Hostile => "hostile",
            Self::Apology => "apology",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FightMode {
    Monster,
    Brawl,
    Lethal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum IntentAction {
    GoTo {
        place_id: String,
    },
    Activity {
        activity: ActivityId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minutes: Option<f64>,
    },
    Talk {
        target_id: Id,
        opener: String,
        tone: TalkTone,
    },
    Give {
        target_id: Id,
        item: ItemId,
    },
    Fight {
        target_id: Id,
        mode: FightMode,
    },
    Flee,
    Wait {
        minutes: f64,
    },
    /// lies down on the grass for a nap, out in the open
    Nap {
        minutes: f64,
    },
    /// shadows someone, waiting for them to be alone, then strikes from behind
    Stalk {
        target_id: Id,
    },
    /// carries water, bucket by bucket, to a house that is on fire
    FightFire {
        place_id: String,
    },
    /// runs after a child who has wandered off and brings them home
    Fetch {
        target_id: Id,
    },
    /// stands with the village at a wedding or a funeral
    Attend {
        place_id: String,
    },
    /// stands in the audience while someone sings or plays, until the music stops
    Listen {
        performer_id: Id,
    },
    /// changes the world: fells a tree, mines a rock, digs, mows, takes something apart, sets it alight,
    /// builds, or carries materials to where they are kept. `target` is an object id, a hex id, a project
    /// id or a stockpile's place id, as the job needs
    WorldWork {
        job: WorldJob,
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minutes: Option<f64>,
    },
    /// walks to something the dead left (`SimState.loot`) and picks it up
    PickUp {
        loot_id: String,
    },
    /// walks up to someone and buys `item` from them, or sells it to them, at the market's price
    Trade {
        target_id: Id,
        item: ItemId,
        buy: bool,
    },
    /// walks to the market and pays for something of their own: `what` is a `Commission` key
    /// (`systems::economy`)
    Commission {
        what: String,
    },
}

/// Work that changes the world; see `systems::world` and `data::jobs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldJob {
    Fell,
    Mine,
    Dig,
    Mow,
    Dismantle,
    Burn,
    Build,
    Deliver,
}

impl WorldJob {
    pub const ALL: [WorldJob; 8] =
        [Self::Fell, Self::Mine, Self::Dig, Self::Mow, Self::Dismantle, Self::Burn, Self::Build, Self::Deliver];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fell => "fell",
            Self::Mine => "mine",
            Self::Dig => "dig",
            Self::Mow => "mow",
            Self::Dismantle => "dismantle",
            Self::Burn => "burn",
            Self::Build => "build",
            Self::Deliver => "deliver",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|j| j.as_str() == text)
    }
}

impl IntentAction {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::GoTo { .. } => "go_to",
            Self::Activity { .. } => "activity",
            Self::Talk { .. } => "talk",
            Self::Give { .. } => "give",
            Self::Fight { .. } => "fight",
            Self::Flee => "flee",
            Self::Wait { .. } => "wait",
            Self::Nap { .. } => "nap",
            Self::Stalk { .. } => "stalk",
            Self::FightFire { .. } => "fight_fire",
            Self::Fetch { .. } => "fetch",
            Self::Attend { .. } => "attend",
            Self::Listen { .. } => "listen",
            Self::WorldWork { .. } => "world_work",
            Self::PickUp { .. } => "pick_up",
            Self::Trade { .. } => "trade",
            Self::Commission { .. } => "commission",
        }
    }

    pub fn target_id(&self) -> Option<&Id> {
        match self {
            Self::Talk { target_id, .. }
            | Self::Give { target_id, .. }
            | Self::Trade { target_id, .. }
            | Self::Fight { target_id, .. }
            | Self::Stalk { target_id }
            | Self::Fetch { target_id }
            | Self::Listen { performer_id: target_id } => Some(target_id),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntentSource {
    Routine,
    Llm,
    Reflex,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    #[serde(flatten)]
    pub action: IntentAction,
    pub source: IntentSource,
    /// said aloud when the intent starts
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
    /// private reasoning, kept as a memory
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought: Option<String>,
}

impl Intent {
    pub fn new(action: IntentAction, source: IntentSource) -> Self {
        Self { action, source, say: None, thought: None }
    }

    pub fn is_fight(&self) -> bool {
        matches!(self.action, IntentAction::Fight { .. })
    }

    /// Fighting `target`.
    pub fn fights(&self, target: &str) -> bool {
        matches!(&self.action, IntentAction::Fight { target_id, .. } if target_id == target)
    }

    /// An activity of this kind.
    pub fn is_activity(&self, id: ActivityId) -> bool {
        matches!(self.action, IntentAction::Activity { activity, .. } if activity == id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActPhase {
    Start,
    Travel,
    Perform,
    Done,
}

/// Progress of the current intent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Act {
    pub phase: ActPhase,
    pub path: Vec<Vec2>,
    pub dest: Option<Vec2>,
    /// tick the current phase began
    pub since_tick: i64,
    /// when a timed phase ends
    pub until_tick: i64,
    pub repath_tick: i64,
    /// ticks spent making no progress
    pub stuck: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StimulusKind {
    Threat,
    Attacked,
    Alarm,
    Need,
    BlockEnded,
    Witness,
    Death,
    Gift,
    Talked,
    Arrival,
    Woke,
    Victory,
    Hostility,
    Romance,
    /// a gift from a viewer, from the days when viewers could send them; kept so those saves still load
    Favour,
    /// harm a viewer sent, from the same days; kept so those saves still load
    Wrath,
    /// a birth, a child coming of age, or a child's mischief reported to those who raise them
    Family,
    /// music heard or made: how a performance went, for the performer and the people who listened
    Music,
    /// something the dead left lying where they fell
    Loot,
}

/// Something that just happened to a character and may deserve a decision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stimulus {
    pub kind: StimulusKind,
    pub tick: i64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<Id>,
    /// 0 (idle curiosity) .. 3 (life or death)
    pub urgency: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombatState {
    pub target_id: Option<Id>,
    pub mode: FightMode,
    pub next_attack_tick: i64,
    /// a swing in flight: lands on this tick
    pub hit_tick: i64,
    pub hit_target_id: Option<Id>,
    pub last_hit_by_tick: i64,
    pub last_hit_by_id: Option<Id>,
    /// beaten and waiting on the attacker's mercy
    pub yielded: bool,
    /// attacker waiting for a spare / finish decision until this tick
    pub mercy_until_tick: i64,
    pub mercy_target_id: Option<Id>,
    /// after a block or a dodge, no other until this tick
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub defend_ready_tick: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanSource {
    Routine,
    Llm,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: Id,
    pub profile: Profile,
    pub class_id: ClassId,
    pub look: Look,
    pub home_id: String,
    /// work this character usually does
    pub jobs: Vec<ActivityId>,
    /// leisure this character enjoys
    pub hobbies: Vec<ActivityId>,
    pub arrived_tick: i64,

    pub pos: Vec2,
    pub facing: f64,
    /// moved this tick (for separation and footsteps)
    pub moving: bool,
    pub hp: f64,
    pub needs: Needs,
    pub inventory: Inventory,
    /// raw materials in their arms, on the way to a stockpile or a building site
    #[serde(default, skip_serializing_if = "Store::is_empty")]
    pub carried: Store,
    /// coin in their purse; see `systems::economy`
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub gold: i64,

    pub plan: Vec<PlanBlock>,
    pub plan_day: i64,
    pub plan_source: PlanSource,
    /// index of the plan block that last produced an intent
    pub plan_block: i64,
    pub intent: Option<Intent>,
    pub queue: Vec<Intent>,
    pub act: Act,
    pub slot_id: Option<String>,
    /// place id while indoors (not visible, safe from monsters)
    pub inside: Option<String>,
    pub asleep: bool,
    /// on night watch until this tick
    pub watch_until_tick: i64,
    /// sleeps in after a night watch until this tick
    pub sleep_in_until_tick: i64,

    pub anim: AnimId,
    pub anim_seq: i64,
    /// tool held instead of the weapon while working ("bundle/model")
    pub tool: Option<String>,

    pub convo_id: Option<Id>,
    pub partner_id: Option<Id>,
    /// the partner this character is married to; a spouse is always a partner as well
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spouse_id: Option<Id>,
    /// tick the current courtship began; 0 for couples from before this was recorded, who count as long together
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub partner_since_tick: i64,
    pub combat: CombatState,
    /// bumped whenever the situation changes enough to invalidate a pending decision
    pub epoch: i64,
    /// the tick this character last set something alight; 0 for the innocent
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub last_arson_tick: i64,
    pub thought: String,
    /// what came of the last intent, for the next decision
    pub outcome: String,
    pub stimuli: Vec<Stimulus>,
    pub relationships: Map<Relationship>,
    pub memories: Vec<Memory>,
    /// rolling self-summary written by reflection
    pub summary: String,
    pub last_reflection_day: i64,
    pub kills: i64,
    /// set when this character has killed another villager
    pub killer: bool,
    pub next_social_tick: i64,
    pub next_perception_tick: i64,
    /// billboard id -> hash of the text last read there, so nobody reads the same thing twice
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub boards_read: IndexMap<String, u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quirks: Vec<Quirk>,
    #[serde(default, skip_serializing_if = "QuirkState::is_default")]
    pub quirk_state: QuirkState,
    /// set while this character is a child
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<Childhood>,
    /// what practice has made of this character's music; left out of a save until they first perform
    #[serde(default, skip_serializing_if = "MusicState::is_default")]
    pub music: MusicState,
    /// in good spirits after a fine tune until this tick (`systems::music`); 0 when not
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub heartened_until_tick: i64,
}

impl Character {
    /// Heartened by music at this tick: lighter on their feet, quicker to heal, braver.
    pub fn heartened(&self, tick: i64) -> bool {
        tick < self.heartened_until_tick
    }

    pub fn has(&self, quirk: Quirk) -> bool {
        self.quirks.contains(&quirk)
    }

    pub fn is_child(&self) -> bool {
        self.child.is_some()
    }

    /// How far this character's legs carry them, as a share of an adult's: a child
    /// is an adult model drawn small, so they cover ground at the same fraction of
    /// the pace or their feet slide over it.
    pub fn stride(&self) -> f64 {
        if self.is_child() { crate::data::children::CHILD_SCALE } else { 1.0 }
    }

    /// Drunk at this tick.
    pub fn drunk(&self, tick: i64) -> bool {
        tick < self.quirk_state.drunk_until_tick
    }

    pub fn married(&self) -> bool {
        self.spouse_id.is_some()
    }
}

/// What a villager sings or plays. Which one, and how well, follows from who they are (`systems::music::talent`);
/// nothing about it is drawn from the RNG, so a village lives exactly as it did before music until someone plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Instrument {
    Voice,
    Lute,
    Fiddle,
    Pipe,
    Drum,
}

impl Instrument {
    pub const ALL: [Instrument; 5] = [Self::Voice, Self::Lute, Self::Fiddle, Self::Pipe, Self::Drum];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Voice => "voice",
            Self::Lute => "lute",
            Self::Fiddle => "fiddle",
            Self::Pipe => "pipe",
            Self::Drum => "drum",
        }
    }

    /// "sang" or "played the lute".
    pub fn did(self) -> &'static str {
        match self {
            Self::Voice => "sang",
            Self::Lute => "played the lute",
            Self::Fiddle => "played the fiddle",
            Self::Pipe => "played the pipe",
            Self::Drum => "beat the drum",
        }
    }

    /// "singing" or "playing the lute".
    pub fn doing(self) -> &'static str {
        match self {
            Self::Voice => "singing",
            Self::Lute => "playing the lute",
            Self::Fiddle => "playing the fiddle",
            Self::Pipe => "playing the pipe",
            Self::Drum => "beating the drum",
        }
    }

    /// "sing" or "play the lute".
    pub fn to_do(self) -> &'static str {
        match self {
            Self::Voice => "sing",
            Self::Lute => "play the lute",
            Self::Fiddle => "play the fiddle",
            Self::Pipe => "play the pipe",
            Self::Drum => "beat the drum",
        }
    }
}

/// Bookkeeping for music, kept with the character so practice survives a save.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicState {
    /// performances given; each one makes the next a little better, up to a point
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub performances: i64,
    /// the day they last performed, so nobody sings twice in one evening of their own accord
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub last_day: i64,
}

impl MusicState {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

/// Someone singing or playing in public right now, from the first note to the last. One per performer,
/// in `SimState.performances`; removed when the music stops.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Performance {
    pub performer_id: Id,
    pub instrument: Instrument,
    /// how well it is going, 0..1, settled when the first note sounds
    pub quality: f64,
    /// what they are playing, for the chronicle ("a ballad of the old road")
    pub tune: String,
    /// where: the tavern or the green
    pub place_id: String,
    /// where the performer stands
    pub pos: Vec2,
    pub since_tick: i64,
    pub until_tick: i64,
    /// everyone who has heard a minute of it, in the order they came within earshot
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listeners: Vec<Id>,
    /// when the audience next shows what it thinks
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub next_reaction_tick: i64,
}

/// Where one of the dead came from. Kept for the viewer's card; nothing in the world reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MonsterOrigin {
    Graveyard,
    Ruins,
    /// called up by a necromancer
    Summoned,
    /// raised by a villager reading what they should not
    Raised,
    /// a villager who gave themself to the dark
    Turned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MonsterPhase {
    Rising,
    Roam,
    Chase,
    Dying,
}

impl MonsterPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rising => "rising",
            Self::Roam => "roam",
            Self::Chase => "chase",
            Self::Dying => "dying",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monster {
    pub id: Id,
    pub kind: MonsterKind,
    pub look: Look,
    pub pos: Vec2,
    pub facing: f64,
    pub moving: bool,
    pub hp: f64,
    pub phase: MonsterPhase,
    pub since_tick: i64,
    pub target_id: Option<Id>,
    pub path: Vec<Vec2>,
    pub repath_tick: i64,
    pub next_attack_tick: i64,
    pub hit_tick: i64,
    pub hit_target_id: Option<Id>,
    pub next_summon_tick: i64,
    pub anim: AnimId,
    pub anim_seq: i64,
    pub stuck: i64,
    /// a villager who became one of the dead keeps their name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// does not crumble at dawn; lies low by day instead
    #[serde(default, skip_serializing_if = "is_false")]
    pub lasting: bool,
    /// the villager this monster once was
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was_id: Option<Id>,
    /// the house this monster means to set on fire
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arson: Option<Arson>,
    // What follows is history for the viewer's card and changes nothing in the world. Saves from before it was kept
    // load with the defaults: an unknown origin and no tally.
    /// the tick it rose; unknown for one from an older save
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rose_tick: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<MonsterOrigin>,
    /// the villager who raised it, for `MonsterOrigin::Raised`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raised_by: Option<Id>,
    /// villagers it has slain
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub kills: i64,
    /// blows it has landed on a villager
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub blows: i64,
    /// after a block or a dodge, no other until this tick
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub defend_ready_tick: i64,
}

/// A monster's plan to burn a house while the village sleeps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Arson {
    pub place_id: String,
    /// when the fire catches; 0 until the monster has reached the door
    pub light_tick: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HousePhase {
    Burning,
    Ruined,
}

/// A home that is not whole: on fire, or burned down and being rebuilt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct House {
    pub phase: HousePhase,
    /// how far the fire has taken hold, 0..100; at 100 the house is lost
    pub fire: f64,
    /// villager-minutes of work put into rebuilding
    pub built: f64,
    /// tick the current phase began
    pub since_tick: i64,
    /// who carried water (while burning) or worked on it (while in ruins)
    pub helpers: Vec<Id>,
}

/// A tree's life after someone takes an axe to it. A standing tree with no entry is the map's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeStage {
    /// still up, being felled (`work` counts the swings)
    Standing,
    /// coming down: a second and a half of leaning over
    Falling,
    Stump,
    Sapling,
    Young,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RockStage {
    /// still there, being mined
    Whole,
    MinedOut,
}

/// A prop's life: whole, taken apart, alight, burnt to ash, or gone altogether.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropStage {
    Whole,
    Pieces,
    Burning,
    Ash,
    Gone,
}

/// What a village builds on a cleared plot: the same thing again, or a bigger one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildTarget {
    Same,
    Expand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum BuildingStage {
    Whole,
    Burning,
    Ruin,
    Dismantled,
    Raising { target: BuildTarget },
    Expanded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroundStage {
    Dug,
    Mown,
}

/// Where and what something the village built is, so it can be drawn and walked around.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltSpec {
    pub bundle: String,
    pub model: String,
    pub x: f64,
    pub z: f64,
    pub rotation: f64,
    pub scale: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footprint: Option<Obstacle>,
    /// the pad it stands on (`"{place}/{index}"`), for things built on one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad: Option<String>,
}

/// What the village is building: a building raised again, a prop put back, a home made bigger, or
/// something new on a pad. Materials are carried to it, then hands are put to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// `j:<n>`
    pub id: String,
    pub kind: ProjectKind,
    /// the object worked on (`b:<place>`, `p:<place>:<index>`), or for `Construct` the pad id
    pub target: String,
    pub cost: Store,
    #[serde(default, skip_serializing_if = "Store::is_empty")]
    pub delivered: Store,
    /// villager-minutes of work it takes, and how many have been put in
    pub need: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub work: f64,
    pub since_tick: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub helpers: Vec<Id>,
    /// the villager who paid for it, and what they paid into the chest; the village's own projects have neither
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sponsor: Option<Id>,
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub price: i64,
    /// what a sponsored `Construct` puts up; the village's own are chosen by the project's id
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad_kind: Option<PadKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Raise,
    Restore,
    Expand,
    Construct,
}

/// Something in the world that differs from the static map: a tree someone felled, a rock mined
/// out, a prop taken apart or burnt, a building not as it was built, ground dug or mown, or a
/// thing the village put up. Keyed in `SimState.objects` by `s:<scatter index>`, `p:<place>:<model
/// index>`, `b:<place>`, `h:<col>,<row>` or `n:<n>`. An entry is removed when the thing is as the
/// map has it again, so a world nobody has changed holds none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum WorldObject {
    Tree {
        stage: TreeStage,
        since_tick: i64,
        #[serde(default, skip_serializing_if = "is_zero")]
        work: f64,
    },
    Rock {
        stage: RockStage,
        since_tick: i64,
        #[serde(default, skip_serializing_if = "is_zero")]
        work: f64,
    },
    Prop {
        stage: PropStage,
        since_tick: i64,
        #[serde(default, skip_serializing_if = "is_zero")]
        work: f64,
        #[serde(default, skip_serializing_if = "is_zero")]
        fire: f64,
    },
    Building {
        #[serde(flatten)]
        stage: BuildingStage,
        since_tick: i64,
        #[serde(default, skip_serializing_if = "is_zero")]
        work: f64,
        #[serde(default, skip_serializing_if = "is_zero")]
        fire: f64,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        helpers: Vec<Id>,
    },
    Ground {
        stage: GroundStage,
        since_tick: i64,
    },
    Built {
        stage: PropStage,
        since_tick: i64,
        #[serde(default, skip_serializing_if = "is_zero")]
        work: f64,
        #[serde(default, skip_serializing_if = "is_zero")]
        fire: f64,
        spec: BuiltSpec,
    },
}

impl WorldObject {
    /// The tick the current stage began.
    pub fn since_tick(&self) -> i64 {
        match self {
            Self::Tree { since_tick, .. }
            | Self::Rock { since_tick, .. }
            | Self::Prop { since_tick, .. }
            | Self::Building { since_tick, .. }
            | Self::Ground { since_tick, .. }
            | Self::Built { since_tick, .. } => *since_tick,
        }
    }

    /// Villager-minutes of work put into whatever is being done to it; ground takes none.
    pub fn work(&self) -> f64 {
        match self {
            Self::Tree { work, .. }
            | Self::Rock { work, .. }
            | Self::Prop { work, .. }
            | Self::Building { work, .. }
            | Self::Built { work, .. } => *work,
            Self::Ground { .. } => 0.0,
        }
    }

    pub fn work_mut(&mut self) -> Option<&mut f64> {
        match self {
            Self::Tree { work, .. }
            | Self::Rock { work, .. }
            | Self::Prop { work, .. }
            | Self::Building { work, .. }
            | Self::Built { work, .. } => Some(work),
            Self::Ground { .. } => None,
        }
    }

    /// How far a fire has taken hold of it, 0..100; 0 for anything not alight.
    pub fn fire(&self) -> f64 {
        match self {
            Self::Prop { fire, .. } | Self::Building { fire, .. } | Self::Built { fire, .. } => *fire,
            Self::Tree { .. } | Self::Rock { .. } | Self::Ground { .. } => 0.0,
        }
    }

    pub fn fire_mut(&mut self) -> Option<&mut f64> {
        match self {
            Self::Prop { fire, .. } | Self::Building { fire, .. } | Self::Built { fire, .. } => Some(fire),
            Self::Tree { .. } | Self::Rock { .. } | Self::Ground { .. } => None,
        }
    }

    /// The stage as the wire and the chronicle name it.
    pub fn stage_name(&self) -> &'static str {
        match self {
            Self::Tree { stage, .. } => match stage {
                TreeStage::Standing => "standing",
                TreeStage::Falling => "falling",
                TreeStage::Stump => "stump",
                TreeStage::Sapling => "sapling",
                TreeStage::Young => "young",
            },
            Self::Rock { stage, .. } => match stage {
                RockStage::Whole => "whole",
                RockStage::MinedOut => "mined_out",
            },
            Self::Prop { stage, .. } | Self::Built { stage, .. } => match stage {
                PropStage::Whole => "whole",
                PropStage::Pieces => "pieces",
                PropStage::Burning => "burning",
                PropStage::Ash => "ash",
                PropStage::Gone => "gone",
            },
            Self::Building { stage, .. } => match stage {
                BuildingStage::Whole => "whole",
                BuildingStage::Burning => "burning",
                BuildingStage::Ruin => "ruin",
                BuildingStage::Dismantled => "dismantled",
                BuildingStage::Raising { .. } => "raising",
                BuildingStage::Expanded => "expanded",
            },
            Self::Ground { stage, .. } => match stage {
                GroundStage::Dug => "dug",
                GroundStage::Mown => "mown",
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feeling {
    MuchWorse,
    Worse,
    Same,
    Better,
    MuchBetter,
}

impl Feeling {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MuchWorse => "much_worse",
            Self::Worse => "worse",
            Self::Same => "same",
            Self::Better => "better",
            Self::MuchBetter => "much_better",
        }
    }

    pub fn is_good(self) -> bool {
        matches!(self, Self::Better | Self::MuchBetter)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationLine {
    pub speaker_id: Id,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConvoPhase {
    Hail,
    Play,
    Await,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConvoAuthor {
    Local,
    Llm,
}

/// Locally written continuation, used if no LLM lines arrive in time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fallback {
    pub lines: Vec<ConversationLine>,
    pub topic: String,
    pub feelings: Map<Feeling>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: Id,
    /// initiator
    pub a: Id,
    pub b: Id,
    pub tone: TalkTone,
    pub phase: ConvoPhase,
    pub lines: Vec<ConversationLine>,
    /// index of the line being spoken, or the next one to speak (-1 before the first)
    pub index: i64,
    pub line_end_tick: i64,
    pub started_tick: i64,
    /// while awaiting: give up and use local lines at this tick
    pub await_until_tick: i64,
    /// no more lines will be added
    pub closed: bool,
    /// who wrote the lines so far
    pub author: ConvoAuthor,
    pub topic: String,
    pub feelings: Map<Feeling>,
    pub fallback: Fallback,
    /// a story the initiator means to pass on
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rumour: Option<Rumour>,
}

/// A story about someone who is not there, told by a gossip; true or not.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rumour {
    /// who the story is about
    pub about: Id,
    /// the story as it is told
    pub text: String,
    /// whether it actually happened
    pub truthful: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grave {
    pub plot: i64,
    pub character_id: Id,
    pub name: String,
    pub day: i64,
    pub cause: String,
}

/// A body left where someone fell; shown for a while, then removed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Corpse {
    pub id: Id,
    pub look: Look,
    pub pos: Vec2,
    pub facing: f64,
    pub anim: AnimId,
    pub until_tick: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeadRecord {
    pub name: String,
    pub day: i64,
    pub cause: String,
    pub killer_id: Option<Id>,
    /// who really did it, when nobody saw
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_killer_id: Option<Id>,
    /// who they were married to when they died
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spouse_id: Option<Id>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChronicleKind {
    World,
    Social,
    Romance,
    Conflict,
    Combat,
    Death,
    Arrival,
    Fire,
    Trade,
}

impl ChronicleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::World => "world",
            Self::Social => "social",
            Self::Romance => "romance",
            Self::Conflict => "conflict",
            Self::Combat => "combat",
            Self::Death => "death",
            Self::Arrival => "arrival",
            Self::Fire => "fire",
            Self::Trade => "trade",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChronicleEntry {
    pub tick: i64,
    pub kind: ChronicleKind,
    pub text: String,
    pub ids: Vec<Id>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Director {
    pub next_wave_tick: i64,
    pub next_arrival_tick: i64,
    pub last_lethal_tick: i64,
    /// fighters keep responding to an alarm until this tick
    pub alarm_until_tick: i64,
    pub alarm_pos: Option<Vec2>,
    pub watch_day: i64,
    pub death_ticks: Vec<i64>,
    pub arrivals: i64,
    /// newcomers arrive while fewer than this many are alive: the founding cast's size
    #[serde(default = "founding_population", skip_serializing_if = "is_founding_population")]
    pub population_target: usize,
    /// tick the last house fire was lit; the dead leave the houses alone for a while after one
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub last_fire_tick: i64,
    /// the village is still being settled: no waves, no arrivals, until `population_target` are here
    #[serde(default, skip_serializing_if = "is_false")]
    pub founding: bool,
    /// the model's description of the next newcomer, held until an arrival is due
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newcomer_draft: Option<Box<NewVillager>>,
    /// an arrival is due and the server has been asked for a newcomer; past this tick the sim invents one (0: not asked)
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub newcomer_deadline_tick: i64,
    /// the dead who have a grave but have not yet been buried before the village
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub awaiting_funeral: Vec<Id>,
    /// tick the last child was born; births are spaced out
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub last_birth_tick: i64,
    /// when the village next looks over what wants building
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub next_project_tick: i64,
}

/// The size of the founding cast before it grew; worlds saved without a target keep it.
pub const FOUNDING_POPULATION: usize = 12;

fn founding_population() -> usize {
    FOUNDING_POPULATION
}

fn is_founding_population(target: &usize) -> bool {
    *target == FOUNDING_POPULATION
}

/// Knobs the brain layer may turn (through a command) without breaking determinism.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tuning {
    /// how eagerly characters start conversations on their own, 0..1
    pub chatter: f64,
    /// ticks a conversation waits for LLM-written lines before using local ones; 0 = never wait
    pub script_wait_ticks: i64,
    /// ticks an attacker waits for a spare / finish decision
    pub mercy_wait_ticks: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimState {
    pub version: u32,
    pub seed: u32,
    pub rng: u32,
    pub tick: i64,
    pub next_id: i64,
    pub characters: Map<Character>,
    /// ids of living characters in a stable order
    pub order: Vec<Id>,
    pub monsters: Map<Monster>,
    pub conversations: Map<Conversation>,
    pub graves: Vec<Grave>,
    pub corpses: Vec<Corpse>,
    pub dead: Map<DeadRecord>,
    pub chronicle: Vec<ChronicleEntry>,
    /// slot id -> character id
    pub slots: IndexMap<String, Id>,
    pub director: Director,
    pub tuning: Tuning,
    /// billboard id -> what an admin (or, later, an advertiser) put up; boards not listed show the day's slogan
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub billboards: IndexMap<String, BoardNotice>,
    /// home id -> its condition, for homes that are burning or in ruins; a home not listed stands whole
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub houses: IndexMap<String, House>,
    /// the wedding or funeral the village has arranged, if any; one at a time
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceremony: Option<Ceremony>,
    /// what differs from the static map, by object id; see [`WorldObject`]
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub objects: IndexMap<String, WorldObject>,
    /// place id -> the materials kept there; see `systems::world::STOCKPILES`
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub stockpiles: IndexMap<String, Store>,
    /// what the village is building; see [`Project`]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<Project>,
    /// who is singing or playing in public right now; see [`Performance`]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub performances: Vec<Performance>,
    /// the village's chest and the market's stalls; see [`Economy`] and `systems::economy`
    #[serde(default, skip_serializing_if = "Economy::is_default")]
    pub economy: Economy,
    /// what the dead left lying where they fell, by id (`l:<n>`); see [`Loot`]
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub loot: IndexMap<String, Loot>,
    /// where one can walk, with `objects` applied: derived, never saved, refreshed by `nav::refresh`
    #[serde(skip)]
    pub nav: NavOverlay,
}

/// The coin side of the village: the chest the market pays from and is paid into, and the goods
/// on its stalls. A world that has never traded holds the starting chest and nothing on the
/// stalls, and is left out of a save, so older saves read as they did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Economy {
    /// the village chest: what the market pays wages and purchases from, and takes sales into
    pub purse: i64,
    /// the goods on the market's stalls, by item; zero counts are not kept
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub stock: IndexMap<ItemId, i64>,
}

impl Default for Economy {
    fn default() -> Self {
        Self { purse: crate::data::economy::STARTING_PURSE, stock: IndexMap::new() }
    }
}

impl Economy {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn in_stock(&self, item: ItemId) -> i64 {
        self.stock.get(&item).copied().unwrap_or(0)
    }

    /// Puts goods on the stalls.
    pub fn stock_up(&mut self, item: ItemId, count: i64) {
        if count > 0 {
            *self.stock.entry(item).or_insert(0) += count;
        }
    }

    /// Takes up to `count` off the stalls; returns how many came off.
    pub fn take_stock(&mut self, item: ItemId, count: i64) -> i64 {
        let have = self.in_stock(item);
        let taken = count.min(have).max(0);
        if taken > 0 {
            if taken == have {
                self.stock.shift_remove(&item);
            } else {
                self.stock[&item] -= taken;
            }
        }
        taken
    }

    /// "4 food and 2 small gifts": what the stalls hold, in a fixed order.
    pub fn describe_stock(&self) -> String {
        let parts: Vec<String> =
            ItemId::ALL.into_iter().filter(|&i| self.in_stock(i) > 0).map(|i| i.label(self.in_stock(i))).collect();
        match parts.len() {
            0 => "nothing".into(),
            1 => parts[0].clone(),
            n => format!("{} and {}", parts[..n - 1].join(", "), parts[n - 1]),
        }
    }
}

/// Something one of the dead left where it fell, for whoever picks it up: coins straight into the
/// purse, or a valuable for the pack. It lies on the ground, blocks nothing, and is gone after a few
/// days. Kept out of `objects`, which is the map's business.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Loot {
    pub kind: LootKind,
    pub pos: Vec2,
    pub since_tick: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LootKind {
    Coins { count: i64 },
    Trinket,
    Relic,
}

impl LootKind {
    /// "4 coins", "a trinket", "a relic".
    pub fn label(self) -> String {
        match self {
            Self::Coins { count: 1 } => "a coin".into(),
            Self::Coins { count } => format!("{count} coins"),
            Self::Trinket => "a trinket".into(),
            Self::Relic => "a relic".into(),
        }
    }

    /// The item a valuable goes into the pack as; coins go into the purse instead.
    pub fn item(self) -> Option<ItemId> {
        match self {
            Self::Coins { .. } => None,
            Self::Trinket => Some(ItemId::Trinket),
            Self::Relic => Some(ItemId::Relic),
        }
    }
}

/// What a ceremony is for: a couple to be wed, or the dead to be buried.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Rite {
    Wedding { a: Id, b: Id },
    Funeral { dead: Vec<Id> },
}

/// Where a ceremony has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CeremonyPhase {
    /// arranged; the village goes about its day
    Announced,
    /// the village is being called to the place
    Gathering,
    /// the rite is being spoken
    Rite,
    /// the feast or the wake at the tavern
    Feast,
}

/// Words the model wrote for someone to say at a ceremony.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Speech {
    pub speaker_id: Id,
    pub text: String,
}

/// A wedding or a funeral, from the moment it is arranged to the end of the feast.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ceremony {
    /// so a speech written for it can be matched to it
    pub id: Id,
    pub rite: Rite,
    pub place_id: String,
    /// when the rite is due to begin
    pub start_tick: i64,
    pub phase: CeremonyPhase,
    /// the step within the rite (opening, vows, pronouncement, ...)
    pub step: i64,
    /// when the current step or phase ends
    pub until_tick: i64,
    pub officiant: Id,
    /// speeches written by the model and held until the rite; scripted lines fill the gaps
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub speeches: Vec<Speech>,
    /// who was called, in the order they were called, which fixes each one's spot
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attendees: Vec<Id>,
    /// times it was put off to the next day
    #[serde(default, skip_serializing_if = "is_zero_tick")]
    pub slips: i64,
}

impl SimState {
    pub fn new_id(&mut self, prefix: &str) -> Id {
        let id = format!("{prefix}{}", self.next_id);
        self.next_id += 1;
        id
    }
}

/// The flavour of what a billboard says, which colours how people take it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoardTone {
    Uplifting,
    Absurd,
    Pushy,
    Ominous,
    Wholesome,
}

impl BoardTone {
    pub const ALL: [BoardTone; 5] = [Self::Uplifting, Self::Absurd, Self::Pushy, Self::Ominous, Self::Wholesome];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uplifting => "uplifting",
            Self::Absurd => "absurd",
            Self::Pushy => "pushy",
            Self::Ominous => "ominous",
            Self::Wholesome => "wholesome",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tone| tone.as_str() == text)
    }
}

/// How someone took what a billboard says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoardReaction {
    Laugh,
    Love,
    Scoff,
    Annoyed,
    Uneasy,
}

impl BoardReaction {
    pub const ALL: [BoardReaction; 5] = [Self::Laugh, Self::Love, Self::Scoff, Self::Annoyed, Self::Uneasy];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Laugh => "laugh",
            Self::Love => "love",
            Self::Scoff => "scoff",
            Self::Annoyed => "annoyed",
            Self::Uneasy => "uneasy",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|reaction| reaction.as_str() == text)
    }
}

/// Text put up on a billboard from outside the sim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardNotice {
    pub text: String,
    pub tone: BoardTone,
}

/// The browser's export file: `{version, savedAt, state}`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveFile {
    pub version: u32,
    /// ms since the epoch
    pub saved_at: u64,
    pub state: SimState,
}

/// For `Option<Option<T>>` fields where a missing key and `null` mean different things.
pub fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}
