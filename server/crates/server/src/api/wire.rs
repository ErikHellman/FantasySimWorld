//! Everything that crosses the wire between the server and the web client.
//!
//! These types are the single source of truth: `cargo run --bin export-api`
//! writes them to `src/generated/api.ts` with ts-rs, and a test fails when the
//! committed file is out of date. Field names are camelCase on the wire.
//!
//! Stream events (`GET /api/stream`, server-sent events), by event name:
//! `hello` [`Hello`], `snapshot` [`Snapshot`], `tick` [`TickFrame`], `status` [`Status`],
//! `roster` (a list of [`RosterEntry`]), `detail` [`Detail`], `site` [`SiteDetail`],
//! `brainlog` [`BrainLogRecord`] (admins only),
//! `world_reset` [`WorldReset`], `chat_welcome` [`ChatWelcome`], `chat` [`ChatMessage`], `chat_you` [`ChatYou`],
//! `guest_end` [`GuestEnd`] (guests only, the last event on their stream).
//! Event ids are `"<bootId>:<tick>"`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bumped whenever a change here is not backwards compatible; a client that
/// sees a different number in `hello` reloads once.
pub const PROTOCOL_VERSION: u32 = 15;

/// Character, monster, ghost, animal, corpse and conversation ids (`c1`, `m7`, `g9`, `a3`, …).
pub type Id = String;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub z: f64,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// holds the admin's token (or sign-in is off): runs the world
    Admin,
    /// holds a guest seat: watches and chats
    Guest,
}

// ---------------------------------------------------------------------------
// Stream: hello, snapshot, tick
// ---------------------------------------------------------------------------

/// First event on every connection.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    /// Random per server start; part of every event id so replay never crosses a restart.
    pub boot_id: String,
    #[ts(type = "number")]
    pub tick: u64,
    /// 0 (paused), 1, 2 or 4
    pub speed: u8,
    pub protocol_version: u32,
    /// Hash of the static world layout; the client's own copy must agree.
    pub world_hash: String,
    pub viewers: u32,
    pub role: Role,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EntityKind {
    Villager,
    Monster,
    Corpse,
    /// a villager slain by the dead, walking again for a few nights; drawn see-through, and seen only by night
    Ghost,
    /// one of the valley's animals; its look's model is in the `animals` bundle, and its name is empty unless it is
    /// a pet or a horse
    Animal,
}

/// How a character or monster is drawn (the sim's `Look`).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Look {
    pub model: String,
    /// texture id override (recolour); null keeps the model's own
    pub texture: Option<String>,
    /// mesh names to hide (helmet, cape, hat)
    pub hide: Vec<String>,
    pub main_hand: Option<String>,
    pub off_hand: Option<String>,
}

/// How a character should sound (reserved for text-to-speech).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct VoiceProfile {
    /// 0.5 (deep) .. 1.5 (high)
    pub pitch: f64,
    /// 0.7 (slow) .. 1.3 (fast)
    pub rate: f64,
    pub timbre: String,
}

/// The parts of an entity that never change while it exists.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntityStatic {
    pub id: Id,
    pub kind: EntityKind,
    pub name: String,
    pub look: Look,
    pub max_hp: f64,
    pub hostile: bool,
    /// villagers only
    pub voice: Option<VoiceProfile>,
    /// set (true) for a boss the whole village must rally against; left out otherwise
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub boss: Option<bool>,
}

/// The parts of an entity that change from tick to tick, quantised so an idle
/// entity produces no delta.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct Dyn {
    /// position × 100, rounded: `[x, z]`
    pub p: [i32; 2],
    /// facing (radians) × 100, rounded
    pub f: i32,
    /// animation id and its sequence number (bumped to restart a one-shot clip)
    pub a: (String, u32),
    /// hit points, rounded
    pub hp: i32,
    /// visible (false while indoors)
    pub v: bool,
    /// main-hand item ("bundle/model"), after the at-ease rule
    pub mh: Option<String>,
    /// off-hand item, after the at-ease rule
    pub oh: Option<String>,
    /// current activity id, if performing one
    pub act: Option<String>,
    /// asleep
    pub z: bool,
    /// for a villager, the horse they ride; for a horse, who rides it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub mt: Option<String>,
    /// height above the ground × 100, rounded, for one in the air (the dragon); absent on the ground
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub y: Option<i32>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct Entity {
    #[serde(rename = "static")]
    pub fixed: EntityStatic,
    #[serde(rename = "dyn")]
    pub state: Dyn,
}

/// The fields of [`Dyn`] that changed this tick; absent means unchanged.
#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq)]
pub struct DynDelta {
    pub id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub p: Option<[i32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub f: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub a: Option<(String, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hp: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub v: Option<bool>,
    #[serde(default, with = "present", skip_serializing_if = "Option::is_none")]
    #[ts(as = "Option<Option<String>>", optional)]
    pub mh: Option<Option<String>>,
    #[serde(default, with = "present", skip_serializing_if = "Option::is_none")]
    #[ts(as = "Option<Option<String>>", optional)]
    pub oh: Option<Option<String>>,
    #[serde(default, with = "present", skip_serializing_if = "Option::is_none")]
    #[ts(as = "Option<Option<String>>", optional)]
    pub act: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub z: Option<bool>,
    #[serde(default, with = "present", skip_serializing_if = "Option::is_none")]
    #[ts(as = "Option<Option<String>>", optional)]
    pub mt: Option<Option<String>>,
    #[serde(default, with = "present", skip_serializing_if = "Option::is_none")]
    #[ts(as = "Option<Option<i32>>", optional)]
    pub y: Option<Option<i32>>,
}

/// A delta field that is present (possibly as `null`) reads as `Some(_)`; a missing one as `None`.
mod present {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer, T: Serialize>(value: &Option<Option<T>>, serializer: S) -> Result<S::Ok, S::Error> {
        // `None` is skipped before it gets here.
        value.as_ref().and_then(Option::as_ref).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(deserializer).map(Some)
    }
}

/// One `tick` event per sim tick. Empty lists are left out.
#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq)]
pub struct TickFrame {
    #[ts(type = "number")]
    pub t: u64,
    /// entities that appeared this tick
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add: Vec<Entity>,
    /// ids of entities that are gone
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub del: Vec<Id>,
    /// changed dynamic fields
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub d: Vec<DynDelta>,
    /// one-shot events, in the order the sim raised them
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ev: Vec<WireEvent>,
}

/// Sent on connect (and whenever replay is impossible): everything needed to draw the world.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[ts(type = "number")]
    pub tick: u64,
    pub speed: u8,
    pub entities: Vec<Entity>,
    pub roster: Vec<RosterEntry>,
    /// the newest 120 entries; older ones come from `GET /api/chronicle`
    pub chronicle: Vec<ChronicleEntry>,
    /// keyed on the client by `plot:characterId`
    pub graves: Vec<Grave>,
    pub alarm: bool,
    pub llm: LlmStatus,
    /// recent spoken lines, newest first
    pub voices: Vec<Utterance>,
    /// what every billboard shows, in map order
    pub billboards: Vec<Billboard>,
    /// the homes that are burning or in ruins; every other home stands whole
    pub houses: Vec<House>,
    /// the wedding or funeral the village has arranged, if any
    pub ceremony: Option<Ceremony>,
    /// everything that differs from the map: felled trees, mined rocks, things taken apart, built or burnt
    pub world_objects: Vec<WorldObject>,
    /// the materials the village has put by, for the stockpiles that hold any
    pub stockpiles: Vec<Stockpile>,
    /// the village chest and the market's stalls
    pub economy: Economy,
    /// what the dead left lying about
    pub loot: Vec<Loot>,
    /// what the sky is doing
    pub weather: Weather,
    /// the Valley Herald's latest issue, if one is out yet; earlier ones come from `GET /api/herald`
    pub herald: Option<HeraldIssue>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorldReset {
    /// "new_world" or "import"
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Emote {
    Heart,
    Anger,
    Alert,
    Thinking,
    Sleep,
    Grief,
    Laugh,
    BrokenHeart,
    Gift,
    /// a note over someone singing or playing
    Music,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Ranged {
    Arrow,
    Bolt,
    Magic,
    /// an apple thrown by a child
    Food,
    /// the dragon's breath, from its mouth to the ground
    Fire,
}

/// Where an orc raid has got to.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RaidPhase {
    Coming,
    Leaving,
    Gone,
}

/// How a blow ended for the one it was aimed at.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HitOutcome {
    Hit,
    /// turned aside: a quarter of the damage
    Blocked,
    /// stepped out of: no damage
    Dodged,
}

/// One spoken line, with everything a sink needs to show it or say it aloud.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Utterance {
    #[ts(type = "number")]
    pub tick: u64,
    pub speaker_id: Id,
    pub speaker_name: String,
    pub to_id: Option<Id>,
    pub to_name: Option<String>,
    pub text: String,
    /// shouted rather than said
    pub loud: bool,
    /// how long the sim allows for the line, at 1x speed
    pub duration_ms: u32,
    pub voice: VoiceProfile,
    /// where the speaker stands, for positional audio
    pub position: Vec2,
}

/// The one-shot happenings of a tick that viewers need (the brain's internal events stay on the server).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum WireEvent {
    Speech(Utterance),
    Emote {
        id: Id,
        emote: Emote,
    },
    Attack {
        attacker_id: Id,
        target_id: Id,
        ranged: Option<Ranged>,
        from: Vec2,
        to: Vec2,
    },
    Hit {
        attacker_id: Id,
        target_id: Id,
        damage: f64,
        pos: Vec2,
        fatal: bool,
        /// left out when the blow simply landed
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        outcome: Option<HitOutcome>,
    },
    Death {
        id: Id,
        name: String,
        pos: Vec2,
        monster: bool,
    },
    MonsterSpawn {
        id: Id,
        pos: Vec2,
    },
    /// an orc war band came up the road, turned for home, or is gone
    Raid {
        phase: RaidPhase,
    },
    Chronicle {
        entry: ChronicleEntry,
    },
    /// a new grave in the graveyard
    Grave {
        grave: Grave,
    },
    /// a billboard shows something new (each morning, or when an admin changes it)
    Billboard {
        billboard: Billboard,
    },
    /// a home caught fire, the fire grew or shrank, the home burned down, was worked on, or stands whole again
    House {
        house: House,
    },
    /// a wedding or funeral was arranged, moved on to its next phase, or is over
    Ceremony {
        ceremony: Ceremony,
    },
    /// something of the world changed stage or was worked on; stage `intact` means it is the map's own again
    WorldObject {
        object: WorldObject,
    },
    /// a stockpile's contents changed
    Stockpile {
        stockpile: Stockpile,
    },
    /// the village chest or the market's stalls changed
    Economy {
        economy: Economy,
    },
    /// something the dead left appeared on the ground, or is gone
    Loot {
        loot: Loot,
    },
    /// the weather changed, or grew or eased
    Weather {
        weather: Weather,
    },
    /// the Valley Herald is out with a new issue
    Herald {
        issue: HeraldIssue,
    },
}

/// A villager's ambition as their page shows it.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AmbitionView {
    /// wealth, love, family, glory or friends
    pub kind: String,
    /// in their own words
    pub words: String,
    /// how far along, 0..1
    pub progress: f64,
    /// where they stand, or when it was won or given up, in a sentence
    pub detail: String,
    /// "won" or "abandoned" once it is over
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub outcome: Option<String>,
}

/// What a villager has come to make of the hints that someone beyond the sky is watching.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SkyStance {
    Believer,
    Defiant,
    Sceptic,
}

/// One morning's front page of the Valley Herald, about the day before.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeraldIssue {
    /// the day it came out; it tells of the day before
    pub day: u32,
    pub headline: String,
    /// the other stories on the page, the biggest first
    pub stories: Vec<String>,
    /// a line of gossip at the foot of the page
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub gossip: Option<String>,
    /// the model wrote it, rather than the sim setting it from the chronicle
    pub by_model: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WeatherKind {
    Clear,
    Rain,
    Wind,
    Hail,
}

/// What the sky is doing. Rounded, so weather coming on or dying away is told in a handful of steps.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Weather {
    pub kind: WeatherKind,
    /// how hard it rains, hails or blows, 0..100 in steps of 5; 0 under a clear sky
    pub intensity: u8,
    /// the wind, whatever the kind, 0..100 in steps of 5
    pub wind: u8,
    /// the way the wind blows toward, in degrees from +x toward +z, in steps of 15
    pub wind_dir: u16,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorldObjectKind {
    Tree,
    Rock,
    Prop,
    Building,
    Ground,
    Built,
}

/// Where something the village built stands, and what it looks like.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Placed {
    pub bundle: String,
    pub model: String,
    pub x: f64,
    pub z: f64,
    pub rotation: f64,
    pub scale: f64,
}

/// A thing of the world that is not as the map has it. Where it stands comes from the client's own
/// copy of the map, by id: `s:<scatter index>`, `p:<place>:<model index>`, `b:<place>`, `h:<col>,<row>`;
/// a thing the village built (`n:<n>`) carries its own placement.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorldObject {
    pub id: Id,
    pub kind: WorldObjectKind,
    /// what the village calls it: "the bench at the village green", "The Gilded Tankard"
    pub name: String,
    /// `standing`, `falling`, `stump`, `sapling`, `young`; `whole`, `mined_out`; `pieces`, `burning`, `ash`,
    /// `gone`; `ruin`, `dismantled`, `raising`, `expanded`; `dug`, `mown`; or `intact` when it is as the map
    /// has it again and can be forgotten
    pub stage: String,
    /// the tick the stage began, or (for a standing tree or whole rock) the last minute anyone worked at it
    #[ts(type = "number")]
    pub since_tick: u64,
    /// how far the work on it has got, in percent, in steps of 5
    pub progress: u8,
    /// how far a fire has taken hold of it, 0..100 in steps of 5
    pub fire: u8,
    pub placed: Option<Placed>,
}

/// The materials kept at one place.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stockpile {
    /// the place id
    pub place: Id,
    pub name: String,
    pub wood: u32,
    pub stone: u32,
    pub ore: u32,
}

/// Something the dead left on the ground, for whoever picks it up.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Loot {
    /// `l:<n>`
    pub id: String,
    pub kind: LootKind,
    /// how many coins, goods or materials; 0 for a trinket or a relic
    pub count: u32,
    /// for goods, the item (`food`, `potion`, `gift`); for materials, the material (`wood`, `stone`, `ore`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub item: Option<String>,
    pub pos: Vec2,
    /// picked up or blown away: forget it
    #[serde(default)]
    pub gone: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LootKind {
    Coins,
    Trinket,
    Relic,
    /// food, potions or gifts a fallen raider carried off; `item` says which
    Goods,
    /// wood, stone or ore a fallen raider carried off; `item` says which
    Materials,
}

/// The village chest and what the market's stalls hold.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Economy {
    /// coin in the village chest, which pays wages and purchases and takes the trader's coin
    pub purse: u32,
    pub food: u32,
    pub potion: u32,
    pub gift: u32,
    pub trinket: u32,
    pub relic: u32,
}

/// Raw materials in a villager's arms.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Goods {
    pub wood: u32,
    pub stone: u32,
    pub ore: u32,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HousePhase {
    Intact,
    Burning,
    Ruin,
}

/// The condition of a home. Where it stands comes from the client's own copy of the map (`src/world`).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct House {
    /// the place id of the home
    pub id: Id,
    /// its name, for the banner
    pub name: String,
    pub phase: HousePhase,
    /// how far the fire has taken hold, 0..100 in steps of 5
    pub fire: u8,
    /// how far a ruin has been rebuilt, in percent
    pub built: u8,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CeremonyKind {
    Wedding,
    Funeral,
    /// someone tried on the green for a killing nobody saw; `ids` are the accused and the accuser
    Trial,
    /// a feast on the green: `ids` is empty and `feast` says which
    Festival,
    /// the village chooses its elder on the green; `ids` are the candidates, the sitting elder first if they stand
    Election,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CeremonyPhase {
    /// arranged; the village goes about its day
    Announced,
    /// the village is being called to the place
    Gathering,
    /// the rite is being spoken
    Rite,
    /// the feast or the wake at the tavern
    Feast,
    /// sent once when it has ended or been called off
    Over,
}

/// A wedding, funeral or trial the village has arranged, as viewers are told of it.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Ceremony {
    pub id: Id,
    pub kind: CeremonyKind,
    pub phase: CeremonyPhase,
    /// where the rite is held
    pub place_id: String,
    pub place_name: String,
    /// the couple, the dead, or the accused and their accuser
    pub ids: Vec<Id>,
    /// their first names, in the same order
    pub names: Vec<String>,
    /// the game day and clock ("14:00") the rite begins
    pub day: u32,
    pub time: String,
    /// for a feast: "harvest" or "midwinter"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub feast: Option<String>,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BillboardTone {
    Uplifting,
    Absurd,
    Pushy,
    Ominous,
    Wholesome,
}

/// What a billboard shows. Where it stands comes from the client's own copy of the map (`src/world`).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Billboard {
    pub id: Id,
    /// the name of the place it stands in
    pub place: String,
    pub text: String,
    pub tone: BillboardTone,
    /// put up by an admin rather than the day's slogan
    pub custom: bool,
}

/// The answer to `POST /api/admin/raid`: when the band will come.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RaidCallAnswer {
    pub message: String,
}

/// The answer to `POST /api/admin/dragon`: when the dragon will come.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DragonCallAnswer {
    pub message: String,
}

/// `POST /api/admin/billboards`: puts text up on a billboard, or with no text gives it back to the daily slogans.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BillboardRequest {
    pub id: Id,
    #[serde(default)]
    pub text: Option<String>,
    /// defaults to `absurd`
    #[serde(default)]
    pub tone: Option<BillboardTone>,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
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
    /// coin changed hands: the passing trader, or someone paying for building
    Trade,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct ChronicleEntry {
    #[ts(type = "number")]
    pub tick: u64,
    pub kind: ChronicleKind,
    pub text: String,
    pub ids: Vec<Id>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Grave {
    pub plot: u32,
    pub character_id: Id,
    pub name: String,
    pub day: u32,
    pub cause: String,
}

// ---------------------------------------------------------------------------
// HUD: status, roster, inspector
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmState {
    Off,
    Ready,
    Busy,
    Error,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmStatus {
    pub state: LlmState,
    pub in_flight: u32,
    pub queued: u32,
    pub requests: u32,
    pub failures: u32,
    #[ts(type = "number")]
    pub tokens: u64,
    pub latency_ms: f64,
    pub protocol: String,
    pub message: String,
}

/// What the ambient mixer needs from the world (the rest depends on each viewer's camera).
#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AmbientCounts {
    pub awake: u32,
    pub eating: u32,
    pub drinking: u32,
    pub smithing: u32,
    pub relaxing: u32,
}

/// Sent when it changes, at most once a second.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub llm: LlmStatus,
    pub viewers: u32,
    pub speed: u8,
    /// the world is held at 1x because the model has a backlog
    pub throttled: bool,
    pub population: u32,
    /// the dead abroad
    pub monsters: u32,
    /// the dragon is over the valley
    pub dragon: bool,
    pub alarm: bool,
    /// an orc war band is in the valley
    pub raid: bool,
    /// "night", "early morning", "morning", "midday", "afternoon", "evening"
    pub part: String,
    /// "spring", "summer", "autumn" or "winter": a year is four seasons of seven days, from spring on day 1
    pub season: String,
    /// the year, from 1
    pub year: u32,
    /// the elder the village chose, if it has one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elder: Option<ElderView>,
    pub ambient: AmbientCounts,
    /// the village is still being founded: how many of its founders have settled; `null` once it is
    pub founding: Option<Founding>,
    /// what is worth watching right now, best first (at most five); the Auto camera and the "Happening now"
    /// cards read it
    pub hotspots: Vec<Hotspot>,
}

/// The village's elder, for the top bar.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ElderView {
    pub id: Id,
    pub name: String,
    /// the day the village chose them
    pub since: u32,
}

/// What a hotspot is about; the client decides from it which ones earn a "Happening now" card.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum HotspotKind {
    /// the dragon over the valley
    Dragon,
    /// a skeleton golem or an orc warchief
    Boss,
    /// a house or something the village built on fire
    Fire,
    /// an orc war band in the valley
    Raid,
    /// a wedding, a funeral or a trial under way
    Ceremony,
    /// villagers fighting the dead or raiders
    Battle,
    /// villagers fighting each other
    Brawl,
    /// a murderer trailing after their next victim, unseen by the village
    Stalking,
    /// the dead abroad, with nobody fighting them yet
    Dead,
    /// a ghost walking the village
    Ghost,
    /// a heated conversation
    Quarrel,
    /// someone singing or playing to an audience
    Music,
    /// a romantic conversation
    Romance,
    /// everyday life: a friendly chat, someone making up, a ride, children at play, a billboard being read
    Daily,
}

/// Something worth watching: what it is, whom to follow and where it is.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hotspot {
    pub kind: HotspotKind,
    /// a villager, one of the dead, a raider, the dragon or a ghost to follow; `null` for a place that has
    /// nobody to follow (a fire nobody is fighting yet)
    pub subject: Option<Id>,
    /// where it is, for a hotspot without a subject (and as a fallback when the subject is out of sight)
    pub at: Vec2,
    /// a short line for the card: "The tavern is on fire", "Bran and Edda are quarrelling"
    pub text: String,
    /// how much it is worth watching, 0 to 100
    pub score: u8,
}

/// How far a new world's founding has come.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Founding {
    pub settled: u32,
    pub total: u32,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mood {
    Ok,
    Fight,
    Sleep,
    Talk,
    Hurt,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct RosterEntry {
    pub id: Id,
    pub name: String,
    pub role: String,
    /// rendered activity line, e.g. "Eating at the Tavern"
    pub status: String,
    /// fraction of max hit points, 0..1
    pub hp: f64,
    pub mood: Mood,
    pub partner: bool,
    /// a child, not yet of age
    #[serde(default)]
    pub child: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct RelationRow {
    pub id: Id,
    pub name: String,
    pub label: RelationLabel,
    pub affinity: f64,
    pub attraction: f64,
    pub grievance: f64,
    /// rendered rung of the hostility ladder
    pub rung: String,
    pub summary: String,
    pub alive: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    Event,
    Talk,
    Thought,
    Reflection,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct Memory {
    #[ts(type = "number")]
    pub tick: u64,
    pub day: u32,
    pub text: String,
    /// 1 (trivial) .. 10 (life-changing)
    pub importance: u8,
    pub kind: MemoryKind,
    pub subjects: Vec<Id>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct Needs {
    pub hunger: f64,
    pub energy: f64,
    pub social: f64,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct Inventory {
    pub food: u32,
    pub potion: u32,
    pub gift: u32,
    /// valuables the dead left, kept to be sold
    #[serde(default)]
    pub trinket: u32,
    #[serde(default)]
    pub relic: u32,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct PlanRow {
    /// "08:00"
    pub time: String,
    pub text: String,
    pub current: bool,
}

/// The inspector, with every label already rendered (the client has no sim helpers).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterDetail {
    pub id: Id,
    pub name: String,
    pub pronouns: String,
    pub age: u32,
    pub occupation: String,
    pub class_name: String,
    /// a child, not yet of age
    #[serde(default)]
    pub child: bool,
    pub backstory: String,
    pub traits: Vec<String>,
    pub goals: Vec<String>,
    pub status: String,
    pub thought: String,
    pub summary: String,
    pub hp: f64,
    pub max_hp: f64,
    pub needs: Needs,
    pub inventory: Inventory,
    /// raw materials in their arms
    pub carried: Goods,
    /// coin in their purse
    #[serde(default)]
    pub gold: u32,
    pub home: String,
    /// that home has burned down and is not yet rebuilt
    pub homeless: bool,
    /// the first name of the one they are married to
    pub spouse: Option<String>,
    pub kills: u32,
    /// what only viewers are told about them: that they are a murderer, and whom they have killed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub secret: Option<String>,
    /// what they make of the watchers beyond the sky, once they have settled on it: "believer", "defiant" or
    /// "sceptic"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sky: Option<SkyStance>,
    /// the one long goal they have set their heart on; none for a child
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ambition: Option<AmbitionView>,
    /// the day the village chose them as its elder, while they are
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elder_since: Option<u32>,
    pub plan_source: String,
    pub plan: Vec<PlanRow>,
    pub relations: Vec<RelationRow>,
    /// newest first, at most 40
    pub memories: Vec<Memory>,
}

/// A villager to link to from a card.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NamedId {
    pub id: Id,
    /// their first name
    pub name: String,
}

/// The inspector for one of the dead, every label rendered on the server.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonsterDetail {
    pub id: Id,
    /// what its label says: the species, or the name of the villager it once was
    pub name: String,
    /// e.g. "skeleton warrior" (`kind` is the tag of [`Detail`])
    pub species: String,
    pub hp: f64,
    pub max_hp: f64,
    /// what it is doing now, e.g. "Hunting Mira near the well"
    pub status: String,
    /// the villager it hunts, if any
    pub target: Option<NamedId>,
    /// when and where it rose, e.g. "Rose from the graveyard on day 3 at 21:40"; empty when not known
    pub rose: String,
    /// the full name of the villager it once was
    pub was: Option<String>,
    /// does not crumble at dawn
    pub lasting: bool,
    /// one of the walking dead (false for the dragon)
    pub undead: bool,
    /// villagers it has slain
    pub kills: u32,
    /// blows it has landed on a villager
    pub blows: u32,
    /// the least and the most one of its blows takes
    pub damage: [u32; 2],
    /// one of the dead, an orc raider, or the dragon
    pub faction: MonsterFaction,
    /// a raider's plunder, e.g. "12 coins and 6 logs of wood"
    pub carrying: Option<String>,
    /// a boss: the skeleton golem or the orc warchief
    pub boss: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MonsterFaction {
    Undead,
    Orc,
    Dragon,
}

/// The card of one of the valley's animals.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnimalDetail {
    pub id: Id,
    /// a pet's or a horse's name, or what it is ("a sheep")
    pub name: String,
    /// e.g. "sheep" (`kind` is the tag of [`Detail`])
    pub species: String,
    /// what it is doing now, e.g. "Grazing in the meadow" or "At Mira's heels"
    pub status: String,
    /// the villager who keeps it, for a pet
    pub owner: Option<NamedId>,
    /// who is riding it, for a horse
    pub rider: Option<NamedId>,
    /// what tending it gives the village, e.g. "milk"
    pub yields: Option<String>,
}

/// The inspector for a ghost, every label rendered on the server.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GhostDetail {
    pub id: Id,
    /// the full name of the villager it was
    pub name: String,
    /// the villager it was
    pub was_id: Id,
    /// the day they died
    pub died_day: u32,
    /// how they died, e.g. "slain by a Skeleton Warrior at the green"
    pub cause: String,
    /// what it is doing now, e.g. "Haunting the village near the well"
    pub status: String,
    /// nights it still has to walk, counting tonight; 0 before it first rises
    pub nights_left: u32,
}

/// What `detail` on the stream carries: the inspector for whatever this viewer watches, tagged by `kind`.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Detail {
    // Boxed so the enum stays small whichever card is the bigger one.
    Villager(Box<CharacterDetail>),
    Monster(Box<MonsterDetail>),
    Ghost(Box<GhostDetail>),
    Animal(Box<AnimalDetail>),
}

/// What a site is: a place's building, or something the village put up on a pad.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SiteKind {
    Building,
    Built,
}

/// One material a project calls for, and how much of it has been carried to the site.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct SiteNeed {
    /// `wood`, `stone` or `ore`
    pub resource: String,
    pub delivered: u32,
    pub needed: u32,
}

/// The work the village has taken on at a site.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct SiteProject {
    /// "raising the smithy", "a fence by the red cottage"
    pub label: String,
    pub helpers: Vec<NamedId>,
    pub needs: Vec<SiteNeed>,
    /// how far the hands-on work has got, in percent
    pub progress: u8,
}

/// The page for a building or something the village built (a site: `b:<place>` or `n:<n>`), with every
/// label already rendered. Sent as the `site` event to the viewer watching it, and by `GET /api/sites/{id}`.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SiteDetail {
    pub id: Id,
    pub kind: SiteKind,
    /// "The Gilded Tankard", "a bench near the village green"
    pub name: String,
    /// the place it is, or stands near
    pub place_id: Id,
    pub place_name: String,
    pub description: String,
    /// as [`WorldObject::stage`]; `whole` for a building that is as the map has it
    pub stage: String,
    /// the tick the stage began; 0 for a building nobody has touched
    #[ts(type = "number")]
    pub since_tick: u64,
    /// how far the work on it has got, in percent
    pub progress: u8,
    /// how far a fire has taken hold of it, 0..100
    pub fire: u8,
    /// how many may sleep there; homes only
    pub beds: Option<u32>,
    /// who lives there; homes only
    pub residents: Vec<NamedId>,
    /// the villagers at the place right now
    pub present: Vec<NamedId>,
    pub project: Option<SiteProject>,
    /// the materials kept there, for a stockpile that holds any
    pub stockpile: Option<Goods>,
    /// chronicle lines about it, newest first, at most 20
    pub history: Vec<ChronicleEntry>,
}

// ---------------------------------------------------------------------------
// Brain log (admins)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    Mercy,
    React,
    Reply,
    Converse,
    Decide,
    PlanDay,
    Billboard,
    /// the day's words for a billboard
    Slogan,
    /// the Valley Herald's front page
    Herald,
    Ceremony,
    /// a name for a new pet
    Name,
    Reflect,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogStatus {
    Ok,
    Fallback,
    Error,
    Stale,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrainLogRecord {
    #[ts(type = "number")]
    pub id: u64,
    #[ts(type = "number")]
    pub tick: u64,
    /// wall clock, ms since the epoch
    #[ts(type = "number")]
    pub at: u64,
    pub character: String,
    pub kind: RequestKind,
    pub status: LogStatus,
    pub latency_ms: u32,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub summary: String,
    pub prompt: String,
    pub response: String,
    /// one line per command the answer produced
    pub commands: Vec<String>,
}

// ---------------------------------------------------------------------------
// Chat
// ---------------------------------------------------------------------------

/// What a line in the chat is.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatKind {
    /// something someone said
    Say,
    /// someone came to watch (and had not been here in the last few minutes)
    Join,
    /// someone has been gone for a few minutes
    Leave,
}

/// One line in the viewers' chat (`chat` on the stream, and the history in [`ChatWelcome`]).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct ChatMessage {
    /// rises by one per line since the server started
    #[ts(type = "number")]
    pub id: u64,
    /// ms since the epoch
    #[ts(type = "number")]
    pub at: u64,
    pub kind: ChatKind,
    /// the author's name when they sent it; for a join or a leave, who came or went;
    /// for an event, whose doing it was
    pub author: String,
    /// markdown, already tidied and filtered by the server; render it as text, never as HTML.
    /// Empty for a join or a leave. For an event, plain text: what they did.
    pub text: String,
}

/// `chat_welcome`, sent after the opening events (and again if a stream falls behind on chat).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatWelcome {
    /// this viewer's chat name; `null` when they can only read
    pub you: Option<String>,
    /// the last messages, oldest first; replaces whatever the client had
    pub history: Vec<ChatMessage>,
    /// the longest message the server accepts, in UTF-16 code units
    pub max_length: u32,
}

/// `chat_you` on the stream, and the answer to `POST /api/chat/name`: this viewer's chat name now.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct ChatYou {
    pub name: String,
}

/// `POST /api/chat`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct ChatPost {
    /// random per tab, as in `?viewer=` on the stream
    pub viewer: String,
    pub text: String,
}

/// `POST /api/chat/name`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct ChatRename {
    /// random per tab, as in `?viewer=` on the stream
    pub viewer: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// REST
// ---------------------------------------------------------------------------

/// `POST /api/watch`: which character's `detail`, or which site's `site`, this viewer (one tab) receives.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct WatchRequest {
    /// random per tab
    pub viewer: String,
    /// a villager id (`c7`) or a site id (`b:<place>`, `n:<n>`)
    pub id: Option<Id>,
}

/// `GET /api/me`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub role: Role,
}

/// `POST /auth/guest`: asks for a guest seat.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct GuestRequest {
    /// the ticket from the last answer, when waiting in line
    #[serde(default)]
    pub ticket: Option<String>,
}

/// The answer to `POST /auth/guest`. Asking again sooner than `retryAfterSeconds` answers 429.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum GuestAnswer {
    /// a seat: the guest cookie is set, and `/api/stream` works for as long as the guest watches
    Admitted,
    /// every seat is taken: ask again with `ticket` after `retryAfterSeconds`
    Queued {
        ticket: String,
        /// 1 is next
        position: u32,
        /// how many guests may watch at once
        seats: u32,
        retry_after_seconds: u32,
    },
}

/// Why a guest's stream ended.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GuestEndReason {
    /// another tab of this browser took the stream over
    Elsewhere,
    /// the guest left (`POST /api/logout`)
    Left,
}

/// `guest_end`: the last event on a guest's stream; the browser should not reconnect.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct GuestEnd {
    pub reason: GuestEndReason,
}

/// `GET /api/chronicle?before=<tick>&limit=<n>`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct ChroniclePage {
    /// oldest first
    pub entries: Vec<ChronicleEntry>,
    /// older entries exist
    pub more: bool,
}

/// `GET /api/herald`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct HeraldArchive {
    /// the issues kept, newest first
    pub issues: Vec<HeraldIssue>,
}

/// The body of every 4xx / 5xx answer from `/api`.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct ApiError {
    pub error: String,
}

/// `POST /api/admin/speed`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct SpeedRequest {
    /// 0, 1, 2 or 4
    pub speed: u8,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WireProtocol {
    Auto,
    Tools,
    Json,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConversationMode {
    Auto,
    Batched,
    Turns,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningMode {
    Off,
    Low,
    Default,
}

/// `GET /api/admin/settings/llm`: the model the server was started with, from its `LLM_*` environment
/// variables. Read-only. The API key itself is never sent: only whether one is set, and its last characters.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmSettingsView {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub api_key_set: bool,
    /// e.g. "…x7Qa"
    pub api_key_hint: Option<String>,
    pub concurrency: u32,
    pub protocol: WireProtocol,
    pub conversations: ConversationMode,
    pub reasoning: ReasoningMode,
    pub temperature: f64,
    pub max_tokens: u32,
    /// the names of the extra request headers, one per line (their values stay on the server)
    pub headers: String,
    /// 0 = no cap
    pub requests_per_minute: u32,
    /// spread through the day; 0 = no cap
    pub max_requests_per_day: u32,
    /// spread through the day; 0 = no cap
    pub max_tokens_per_day: u32,
    /// the embedding model's endpoint (`EMBED_BASE_URL`), when memories are recalled by meaning
    pub embed_base_url: Option<String>,
    /// `EMBED_MODEL`, or none when memories are recalled by shared words
    pub embed_model: Option<String>,
    pub embed_api_key_set: bool,
}

/// `POST /api/admin/llm/test`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct LlmTestResult {
    pub ok: bool,
    pub message: String,
}

/// `POST /api/admin/world/new`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct NewWorldRequest {
    /// random when left out
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub seed: Option<u32>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmUsage {
    pub requests_24h: u32,
    pub failures_24h: u32,
    #[ts(type = "number")]
    pub tokens_24h: u64,
}

/// `GET /api/admin/health`
#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    #[ts(type = "number")]
    pub tick: u64,
    pub tick_jitter_p50_ms: f64,
    pub tick_jitter_p99_ms: f64,
    /// times the tick loop fell more than 10 s behind and skipped ahead
    pub clock_skips: u32,
    /// viewers dropped because they could not keep up with the stream
    pub backlog_drops: u32,
    pub viewers: u32,
    #[ts(type = "number")]
    pub db_bytes: u64,
    pub snapshot_age_seconds: Option<f64>,
    pub llm: LlmUsage,
}

/// Writes every wire type as one TypeScript module (the content of `src/generated/api.ts`).
pub fn typescript() -> String {
    let cfg = ts_rs::Config::new().with_large_int("number");
    let mut out = String::from(
        "// Generated from server/crates/server/src/api/wire.rs by `cargo run --bin export-api`. Do not edit.\n\n",
    );
    out.push_str(&format!("export const PROTOCOL_VERSION = {PROTOCOL_VERSION};\n\n"));
    macro_rules! decl {
        ($($t:ty),* $(,)?) => {$(
            if let Some(docs) = <$t as TS>::docs() {
                out.push_str(&docs);
            }
            out.push_str("export ");
            out.push_str(&<$t as TS>::decl(&cfg));
            out.push_str("\n\n");
        )*};
    }
    decl!(
        Vec2,
        Role,
        GuestRequest,
        GuestAnswer,
        GuestEndReason,
        GuestEnd,
        Hello,
        EntityKind,
        Look,
        VoiceProfile,
        EntityStatic,
        Dyn,
        Entity,
        DynDelta,
        TickFrame,
        Snapshot,
        WorldReset,
        Emote,
        Ranged,
        HitOutcome,
        RaidCallAnswer,
        DragonCallAnswer,
        RaidPhase,
        Utterance,
        WireEvent,
        ChronicleKind,
        ChronicleEntry,
        Grave,
        BillboardTone,
        Billboard,
        BillboardRequest,
        HousePhase,
        House,
        CeremonyKind,
        CeremonyPhase,
        ElderView,
        Ceremony,
        WorldObjectKind,
        Placed,
        WorldObject,
        Stockpile,
        Economy,
        Loot,
        LootKind,
        WeatherKind,
        Weather,
        HeraldIssue,
        SkyStance,
        AmbitionView,
        Goods,
        LlmState,
        LlmStatus,
        AmbientCounts,
        Founding,
        HotspotKind,
        Hotspot,
        Status,
        Mood,
        RosterEntry,
        RelationLabel,
        RelationRow,
        MemoryKind,
        Memory,
        Needs,
        Inventory,
        PlanRow,
        CharacterDetail,
        NamedId,
        MonsterDetail,
        AnimalDetail,
        GhostDetail,
        MonsterFaction,
        Detail,
        SiteKind,
        SiteNeed,
        SiteProject,
        SiteDetail,
        RequestKind,
        LogStatus,
        BrainLogRecord,
        ChatKind,
        ChatMessage,
        ChatWelcome,
        ChatYou,
        ChatPost,
        ChatRename,
        WatchRequest,
        Me,
        ChroniclePage,
        HeraldArchive,
        ApiError,
        SpeedRequest,
        WireProtocol,
        ConversationMode,
        ReasoningMode,
        LlmSettingsView,
        LlmTestResult,
        NewWorldRequest,
        LlmUsage,
        HealthReport,
    );
    out.push_str(
        "/** Payload of each named event on `GET /api/stream`. */\n\
         export type StreamEvents = {\n  \
         hello: Hello;\n  snapshot: Snapshot;\n  tick: TickFrame;\n  status: Status;\n  roster: RosterEntry[];\n  \
         detail: Detail;\n  site: SiteDetail;\n  brainlog: BrainLogRecord;\n  world_reset: WorldReset;\n  \
         chat_welcome: ChatWelcome;\n  chat: ChatMessage;\n  chat_you: ChatYou;\n  guest_end: GuestEnd;\n};\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const API_TS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../src/generated/api.ts");

    #[test]
    fn generated_typescript_is_up_to_date() {
        let committed = std::fs::read_to_string(API_TS).unwrap_or_default();
        assert!(
            committed == typescript(),
            "src/generated/api.ts is stale; run `cargo run --bin export-api` in server/ and commit the result"
        );
    }

    #[test]
    fn deltas_tell_unchanged_from_cleared() {
        let delta = DynDelta { id: "c1".into(), mh: Some(None), act: Some(Some("smith".into())), ..Default::default() };
        let json = serde_json::to_string(&delta).unwrap();
        assert_eq!(json, r#"{"id":"c1","mh":null,"act":"smith"}"#);
        assert_eq!(serde_json::from_str::<DynDelta>(&json).unwrap(), delta);
    }

    #[test]
    fn idle_ticks_are_tiny() {
        let frame = TickFrame { t: 42, ..Default::default() };
        assert_eq!(serde_json::to_string(&frame).unwrap(), r#"{"t":42}"#);
    }

    #[test]
    fn events_are_tagged_with_camel_case_fields() {
        let event = WireEvent::MonsterSpawn { id: "m3".into(), pos: Vec2 { x: 1.5, z: -2.0 } };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"type":"monster_spawn","id":"m3","pos":{"x":1.5,"z":-2.0}}"#
        );
        let hit = WireEvent::Hit {
            attacker_id: "c1".into(),
            target_id: "m3".into(),
            damage: 4.0,
            pos: Vec2 { x: 0.0, z: 0.0 },
            fatal: false,
            outcome: None,
        };
        let json = serde_json::to_string(&hit).unwrap();
        assert!(json.contains(r#""attackerId":"c1""#));
        assert!(!json.contains("outcome"), "a plain hit says nothing of how it ended");
    }
}
