//! Everything that crosses the wire between the server and the web client.
//!
//! These types are the single source of truth: `cargo run --bin export-api`
//! writes them to `src/generated/api.ts` with ts-rs, and a test fails when the
//! committed file is out of date. Field names are camelCase on the wire.
//!
//! Stream events (`GET /api/stream`, server-sent events), by event name:
//! `hello` [`Hello`], `snapshot` [`Snapshot`], `tick` [`TickFrame`], `status` [`Status`],
//! `roster` (a list of [`RosterEntry`]), `detail` [`CharacterDetail`], `brainlog` [`BrainLogRecord`] (admins only),
//! `world_reset` [`WorldReset`], `chat_welcome` [`ChatWelcome`], `chat` [`ChatMessage`], `chat_you` [`ChatYou`].
//! Event ids are `"<bootId>:<tick>"`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bumped whenever a change here is not backwards compatible; a client that
/// sees a different number in `hello` reloads once.
pub const PROTOCOL_VERSION: u32 = 1;

/// Character, monster, corpse and conversation ids (`c1`, `m7`, …).
pub type Id = String;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub z: f64,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Viewer,
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
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Ranged {
    Arrow,
    Bolt,
    Magic,
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
    pub monsters: u32,
    pub alarm: bool,
    /// "night", "early morning", "morning", "midday", "afternoon", "evening"
    pub part: String,
    pub ambient: AmbientCounts,
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
    pub home: String,
    pub kills: u32,
    pub plan_source: String,
    pub plan: Vec<PlanRow>,
    pub relations: Vec<RelationRow>,
    /// newest first, at most 40
    pub memories: Vec<Memory>,
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
    /// the author's name when they sent it; for a join or a leave, who came or went
    pub author: String,
    /// markdown, already tidied and filtered by the server; render it as text, never as HTML.
    /// Empty for a join or a leave.
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

/// `POST /api/watch`: which character's `detail` this viewer (one tab) receives.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct WatchRequest {
    /// random per tab
    pub viewer: String,
    pub id: Option<Id>,
}

/// `GET /api/me`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub email: String,
    pub display_name: String,
    pub role: Role,
}

/// `GET /auth/providers`: how this server signs people in, for the login page (no session needed).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct SignInOptions {
    /// `AUTH_MODE=off`: no sign-in needed (local development)
    pub open: bool,
    /// `/auth/token` works (`AUTH_MODE=token`)
    pub token: bool,
    /// `/auth/google/start` works
    pub google: bool,
    /// `/auth/apple/start` works
    pub apple: bool,
}

/// `GET /api/chronicle?before=<tick>&limit=<n>`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct ChroniclePage {
    /// oldest first
    pub entries: Vec<ChronicleEntry>,
    /// older entries exist
    pub more: bool,
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
}

/// `POST /api/admin/llm/test`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct LlmTestResult {
    pub ok: bool,
    pub message: String,
}

/// `GET /api/admin/allowlist` rows.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AllowlistEntry {
    pub email: String,
    pub role: Role,
    pub added_by: String,
    /// ms since the epoch
    #[ts(type = "number")]
    pub added_at: u64,
    pub note: String,
}

/// `POST /api/admin/allowlist`
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct AllowlistAdd {
    pub email: String,
    pub role: Role,
    #[serde(default)]
    pub note: String,
}

/// `GET /api/admin/attempts`: refused sign-ins, newest first.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
pub struct AuthAttempt {
    #[ts(type = "number")]
    pub id: u64,
    /// "google", "apple"
    pub provider: String,
    pub email: String,
    pub outcome: String,
    /// ms since the epoch
    #[ts(type = "number")]
    pub at: u64,
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
        SignInOptions,
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
        Utterance,
        WireEvent,
        ChronicleKind,
        ChronicleEntry,
        Grave,
        BillboardTone,
        Billboard,
        BillboardRequest,
        LlmState,
        LlmStatus,
        AmbientCounts,
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
        ApiError,
        SpeedRequest,
        WireProtocol,
        ConversationMode,
        ReasoningMode,
        LlmSettingsView,
        LlmTestResult,
        AllowlistEntry,
        AllowlistAdd,
        AuthAttempt,
        NewWorldRequest,
        LlmUsage,
        HealthReport,
    );
    out.push_str(
        "/** Payload of each named event on `GET /api/stream`. */\n\
         export type StreamEvents = {\n  \
         hello: Hello;\n  snapshot: Snapshot;\n  tick: TickFrame;\n  status: Status;\n  roster: RosterEntry[];\n  \
         detail: CharacterDetail;\n  brainlog: BrainLogRecord;\n  world_reset: WorldReset;\n  \
         chat_welcome: ChatWelcome;\n  chat: ChatMessage;\n  chat_you: ChatYou;\n};\n",
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
        };
        assert!(serde_json::to_string(&hit).unwrap().contains(r#""attackerId":"c1""#));
    }
}
