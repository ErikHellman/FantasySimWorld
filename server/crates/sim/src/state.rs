//! The whole simulation state. Plain data: ids instead of references, RNG
//! state included, so it can be saved, loaded and compared. The serde shape is
//! exactly the TypeScript `SimState` (camelCase, absent optional fields left
//! out), so browser exports load unchanged and server exports load in a browser.

use fsw_core::Vec2;
pub use fsw_world::places::ActivityId;
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};

pub use crate::data::anims::AnimId;
pub use crate::data::classes::ClassId;
pub use crate::data::monsters::MonsterKind;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemId {
    Food,
    Potion,
    Gift,
}

impl ItemId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Food => "food",
            Self::Potion => "potion",
            Self::Gift => "gift",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    pub food: i64,
    pub potion: i64,
    pub gift: i64,
}

impl Inventory {
    pub fn get(&self, item: ItemId) -> i64 {
        match item {
            ItemId::Food => self.food,
            ItemId::Potion => self.potion,
            ItemId::Gift => self.gift,
        }
    }

    pub fn get_mut(&mut self, item: ItemId) -> &mut i64 {
        match item {
            ItemId::Food => &mut self.food,
            ItemId::Potion => &mut self.potion,
            ItemId::Gift => &mut self.gift,
        }
    }
}

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
        }
    }

    pub fn target_id(&self) -> Option<&Id> {
        match self {
            Self::Talk { target_id, .. }
            | Self::Give { target_id, .. }
            | Self::Fight { target_id, .. }
            | Self::Stalk { target_id } => Some(target_id),
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
    /// one of the powers that watch the valley did the village a kindness
    Favour,
    /// one of the powers that watch the valley sent harm
    Wrath,
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
    pub combat: CombatState,
    /// bumped whenever the situation changes enough to invalidate a pending decision
    pub epoch: i64,
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
}

impl Character {
    pub fn has(&self, quirk: Quirk) -> bool {
        self.quirks.contains(&quirk)
    }

    /// Drunk at this tick.
    pub fn drunk(&self, tick: i64) -> bool {
        tick < self.quirk_state.drunk_until_tick
    }
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
    /// raised by a viewer: daylight does not touch it, and it crumbles at this tick whatever the hour
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_tick: Option<i64>,
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
    /// name -> what the village has had from that viewer; see [`Power`]
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub powers: IndexMap<String, Power>,
}

/// A viewer as the villagers know them: an unseen power that watches the valley and now and
/// then sends a gift, a blessing or the dead. Villagers thank, plead with and pray to them by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Power {
    /// gifts and blessings sent
    pub favours: i64,
    /// monsters and raids sent
    pub wraths: i64,
    /// tick of the last thing they did
    pub last_tick: i64,
}

impl Power {
    /// One word for how the village sees them.
    pub fn repute(&self) -> &'static str {
        if self.favours > self.wraths * 2 {
            "generous"
        } else if self.wraths > self.favours * 2 {
            "cruel"
        } else {
            "fickle"
        }
    }
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
pub(crate) fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}
