// Generated from server/crates/server/src/api/wire.rs by `cargo run --bin export-api`. Do not edit.

export const PROTOCOL_VERSION = 2;

export type Vec2 = { x: number, z: number, };

export type Role = "admin" | "viewer";

/**
 * `GET /auth/providers`: how this server signs people in, for the login page (no session needed).
 */
export type SignInOptions = { 
/**
 * `AUTH_MODE=off`: no sign-in needed (local development)
 */
open: boolean, 
/**
 * `/auth/token` works (`AUTH_MODE=token`)
 */
token: boolean, 
/**
 * `/auth/{provider}/start` works for these, in the order `AUTH_PROVIDERS` lists them
 */
providers: Array<SignInProvider>, };

/**
 * A sign-in provider, by the name it has in URLs, `AUTH_PROVIDERS` and the database.
 */
export type SignInProvider = "google" | "apple" | "microsoft" | "linkedin" | "github" | "discord";

/**
 * First event on every connection.
 */
export type Hello = { 
/**
 * Random per server start; part of every event id so replay never crosses a restart.
 */
bootId: string, tick: number, 
/**
 * 0 (paused), 1, 2 or 4
 */
speed: number, protocolVersion: number, 
/**
 * Hash of the static world layout; the client's own copy must agree.
 */
worldHash: string, viewers: number, role: Role, };

export type EntityKind = "villager" | "monster" | "corpse";

/**
 * How a character or monster is drawn (the sim's `Look`).
 */
export type Look = { model: string, 
/**
 * texture id override (recolour); null keeps the model's own
 */
texture: string | null, 
/**
 * mesh names to hide (helmet, cape, hat)
 */
hide: Array<string>, mainHand: string | null, offHand: string | null, };

/**
 * How a character should sound (reserved for text-to-speech).
 */
export type VoiceProfile = { 
/**
 * 0.5 (deep) .. 1.5 (high)
 */
pitch: number, 
/**
 * 0.7 (slow) .. 1.3 (fast)
 */
rate: number, timbre: string, };

/**
 * The parts of an entity that never change while it exists.
 */
export type EntityStatic = { id: string, kind: EntityKind, name: string, look: Look, maxHp: number, hostile: boolean, 
/**
 * villagers only
 */
voice: VoiceProfile | null, };

/**
 * The parts of an entity that change from tick to tick, quantised so an idle
 * entity produces no delta.
 */
export type Dyn = { 
/**
 * position × 100, rounded: `[x, z]`
 */
p: [number, number], 
/**
 * facing (radians) × 100, rounded
 */
f: number, 
/**
 * animation id and its sequence number (bumped to restart a one-shot clip)
 */
a: [string, number], 
/**
 * hit points, rounded
 */
hp: number, 
/**
 * visible (false while indoors)
 */
v: boolean, 
/**
 * main-hand item ("bundle/model"), after the at-ease rule
 */
mh: string | null, 
/**
 * off-hand item, after the at-ease rule
 */
oh: string | null, 
/**
 * current activity id, if performing one
 */
act: string | null, 
/**
 * asleep
 */
z: boolean, };

export type Entity = { static: EntityStatic, dyn: Dyn, };

/**
 * The fields of [`Dyn`] that changed this tick; absent means unchanged.
 */
export type DynDelta = { id: string, p?: [number, number], f?: number, a?: [string, number], hp?: number, v?: boolean, mh?: string | null, oh?: string | null, act?: string | null, z?: boolean, };

/**
 * One `tick` event per sim tick. Empty lists are left out.
 */
export type TickFrame = { t: number, 
/**
 * entities that appeared this tick
 */
add?: Array<Entity>, 
/**
 * ids of entities that are gone
 */
del?: Array<string>, 
/**
 * changed dynamic fields
 */
d?: Array<DynDelta>, 
/**
 * one-shot events, in the order the sim raised them
 */
ev?: Array<WireEvent>, };

/**
 * Sent on connect (and whenever replay is impossible): everything needed to draw the world.
 */
export type Snapshot = { tick: number, speed: number, entities: Array<Entity>, roster: Array<RosterEntry>, 
/**
 * the newest 120 entries; older ones come from `GET /api/chronicle`
 */
chronicle: Array<ChronicleEntry>, 
/**
 * keyed on the client by `plot:characterId`
 */
graves: Array<Grave>, alarm: boolean, llm: LlmStatus, 
/**
 * recent spoken lines, newest first
 */
voices: Array<Utterance>, 
/**
 * what every billboard shows, in map order
 */
billboards: Array<Billboard>, };

export type WorldReset = { 
/**
 * "new_world" or "import"
 */
reason: string, };

export type Emote = "heart" | "anger" | "alert" | "thinking" | "sleep" | "grief" | "laugh" | "broken_heart" | "gift";

export type Ranged = "arrow" | "bolt" | "magic";

/**
 * One spoken line, with everything a sink needs to show it or say it aloud.
 */
export type Utterance = { tick: number, speakerId: string, speakerName: string, toId: string | null, toName: string | null, text: string, 
/**
 * shouted rather than said
 */
loud: boolean, 
/**
 * how long the sim allows for the line, at 1x speed
 */
durationMs: number, voice: VoiceProfile, 
/**
 * where the speaker stands, for positional audio
 */
position: Vec2, };

/**
 * The one-shot happenings of a tick that viewers need (the brain's internal events stay on the server).
 */
export type WireEvent = { "type": "speech" } & Utterance | { "type": "emote", id: string, emote: Emote, } | { "type": "attack", attackerId: string, targetId: string, ranged: Ranged | null, from: Vec2, to: Vec2, } | { "type": "hit", attackerId: string, targetId: string, damage: number, pos: Vec2, fatal: boolean, } | { "type": "death", id: string, name: string, pos: Vec2, monster: boolean, } | { "type": "monster_spawn", id: string, pos: Vec2, } | { "type": "chronicle", entry: ChronicleEntry, } | { "type": "grave", grave: Grave, } | { "type": "billboard", billboard: Billboard, };

export type ChronicleKind = "world" | "social" | "romance" | "conflict" | "combat" | "death" | "arrival";

export type ChronicleEntry = { tick: number, kind: ChronicleKind, text: string, ids: Array<string>, };

export type Grave = { plot: number, characterId: string, name: string, day: number, cause: string, };

export type BillboardTone = "uplifting" | "absurd" | "pushy" | "ominous" | "wholesome";

/**
 * What a billboard shows. Where it stands comes from the client's own copy of the map (`src/world`).
 */
export type Billboard = { id: string, 
/**
 * the name of the place it stands in
 */
place: string, text: string, tone: BillboardTone, 
/**
 * put up by an admin rather than the day's slogan
 */
custom: boolean, };

/**
 * `POST /api/admin/billboards`: puts text up on a billboard, or with no text gives it back to the daily slogans.
 */
export type BillboardRequest = { id: string, text: string | null, 
/**
 * defaults to `absurd`
 */
tone: BillboardTone | null, };

export type LlmState = "off" | "ready" | "busy" | "error";

export type LlmStatus = { state: LlmState, inFlight: number, queued: number, requests: number, failures: number, tokens: number, latencyMs: number, protocol: string, message: string, };

/**
 * What the ambient mixer needs from the world (the rest depends on each viewer's camera).
 */
export type AmbientCounts = { awake: number, eating: number, drinking: number, smithing: number, relaxing: number, };

/**
 * Sent when it changes, at most once a second.
 */
export type Status = { llm: LlmStatus, viewers: number, speed: number, 
/**
 * the world is held at 1x because the model has a backlog
 */
throttled: boolean, population: number, monsters: number, alarm: boolean, 
/**
 * "night", "early morning", "morning", "midday", "afternoon", "evening"
 */
part: string, ambient: AmbientCounts, };

export type Mood = "ok" | "fight" | "sleep" | "talk" | "hurt";

export type RosterEntry = { id: string, name: string, role: string, 
/**
 * rendered activity line, e.g. "Eating at the Tavern"
 */
status: string, 
/**
 * fraction of max hit points, 0..1
 */
hp: number, mood: Mood, partner: boolean, };

export type RelationLabel = "stranger" | "acquaintance" | "friend" | "close friend" | "lover" | "rival" | "enemy" | "family";

export type RelationRow = { id: string, name: string, label: RelationLabel, affinity: number, attraction: number, grievance: number, 
/**
 * rendered rung of the hostility ladder
 */
rung: string, summary: string, alive: boolean, };

export type MemoryKind = "event" | "talk" | "thought" | "reflection";

export type Memory = { tick: number, day: number, text: string, 
/**
 * 1 (trivial) .. 10 (life-changing)
 */
importance: number, kind: MemoryKind, subjects: Array<string>, };

export type Needs = { hunger: number, energy: number, social: number, };

export type Inventory = { food: number, potion: number, gift: number, };

export type PlanRow = { 
/**
 * "08:00"
 */
time: string, text: string, current: boolean, };

/**
 * The inspector, with every label already rendered (the client has no sim helpers).
 */
export type CharacterDetail = { id: string, name: string, pronouns: string, age: number, occupation: string, className: string, backstory: string, traits: Array<string>, goals: Array<string>, status: string, thought: string, summary: string, hp: number, maxHp: number, needs: Needs, inventory: Inventory, home: string, kills: number, planSource: string, plan: Array<PlanRow>, relations: Array<RelationRow>, 
/**
 * newest first, at most 40
 */
memories: Array<Memory>, };

export type RequestKind = "mercy" | "react" | "reply" | "converse" | "decide" | "plan_day" | "billboard" | "reflect";

export type LogStatus = "ok" | "fallback" | "error" | "stale";

export type BrainLogRecord = { id: number, tick: number, 
/**
 * wall clock, ms since the epoch
 */
at: number, character: string, kind: RequestKind, status: LogStatus, latencyMs: number, promptTokens: number, completionTokens: number, summary: string, prompt: string, response: string, 
/**
 * one line per command the answer produced
 */
commands: Array<string>, };

/**
 * What a line in the chat is.
 */
export type ChatKind = "say" | "join" | "leave";

/**
 * One line in the viewers' chat (`chat` on the stream, and the history in [`ChatWelcome`]).
 */
export type ChatMessage = { 
/**
 * rises by one per line since the server started
 */
id: number, 
/**
 * ms since the epoch
 */
at: number, kind: ChatKind, 
/**
 * the author's name when they sent it; for a join or a leave, who came or went
 */
author: string, 
/**
 * markdown, already tidied and filtered by the server; render it as text, never as HTML.
 * Empty for a join or a leave.
 */
text: string, };

/**
 * `chat_welcome`, sent after the opening events (and again if a stream falls behind on chat).
 */
export type ChatWelcome = { 
/**
 * this viewer's chat name; `null` when they can only read
 */
you: string | null, 
/**
 * the last messages, oldest first; replaces whatever the client had
 */
history: Array<ChatMessage>, 
/**
 * the longest message the server accepts, in UTF-16 code units
 */
maxLength: number, };

/**
 * `chat_you` on the stream, and the answer to `POST /api/chat/name`: this viewer's chat name now.
 */
export type ChatYou = { name: string, };

/**
 * `POST /api/chat`
 */
export type ChatPost = { 
/**
 * random per tab, as in `?viewer=` on the stream
 */
viewer: string, text: string, };

/**
 * `POST /api/chat/name`
 */
export type ChatRename = { 
/**
 * random per tab, as in `?viewer=` on the stream
 */
viewer: string, name: string, };

/**
 * `POST /api/watch`: which character's `detail` this viewer (one tab) receives.
 */
export type WatchRequest = { 
/**
 * random per tab
 */
viewer: string, id: string | null, };

/**
 * `GET /api/me`
 */
export type Me = { email: string, displayName: string, role: Role, };

/**
 * `GET /api/chronicle?before=<tick>&limit=<n>`
 */
export type ChroniclePage = { 
/**
 * oldest first
 */
entries: Array<ChronicleEntry>, 
/**
 * older entries exist
 */
more: boolean, };

/**
 * The body of every 4xx / 5xx answer from `/api`.
 */
export type ApiError = { error: string, };

/**
 * `POST /api/admin/speed`
 */
export type SpeedRequest = { 
/**
 * 0, 1, 2 or 4
 */
speed: number, };

export type WireProtocol = "auto" | "tools" | "json";

export type ConversationMode = "auto" | "batched" | "turns";

export type ReasoningMode = "off" | "low" | "default";

/**
 * `GET /api/admin/settings/llm`: the model the server was started with, from its `LLM_*` environment
 * variables. Read-only. The API key itself is never sent: only whether one is set, and its last characters.
 */
export type LlmSettingsView = { enabled: boolean, baseUrl: string, model: string, apiKeySet: boolean, 
/**
 * e.g. "…x7Qa"
 */
apiKeyHint: string | null, concurrency: number, protocol: WireProtocol, conversations: ConversationMode, reasoning: ReasoningMode, temperature: number, maxTokens: number, 
/**
 * the names of the extra request headers, one per line (their values stay on the server)
 */
headers: string, 
/**
 * 0 = no cap
 */
requestsPerMinute: number, 
/**
 * spread through the day; 0 = no cap
 */
maxRequestsPerDay: number, 
/**
 * spread through the day; 0 = no cap
 */
maxTokensPerDay: number, };

/**
 * `POST /api/admin/llm/test`
 */
export type LlmTestResult = { ok: boolean, message: string, };

/**
 * `GET /api/admin/allowlist` rows.
 */
export type AllowlistEntry = { email: string, role: Role, addedBy: string, 
/**
 * ms since the epoch
 */
addedAt: number, note: string, };

/**
 * `POST /api/admin/allowlist`
 */
export type AllowlistAdd = { email: string, role: Role, note: string, };

/**
 * `GET /api/admin/attempts`: refused sign-ins, newest first.
 */
export type AuthAttempt = { id: number, 
/**
 * a [`SignInProvider`]'s name, such as "google" or "github"
 */
provider: string, email: string, 
/**
 * "not_allowlisted", "email_unverified" or "no_email"
 */
outcome: string, 
/**
 * ms since the epoch
 */
at: number, };

/**
 * `POST /api/admin/world/new`
 */
export type NewWorldRequest = { 
/**
 * random when left out
 */
seed?: number, };

export type LlmUsage = { requests24h: number, failures24h: number, tokens24h: number, };

/**
 * `GET /api/admin/health`
 */
export type HealthReport = { tick: number, tickJitterP50Ms: number, tickJitterP99Ms: number, 
/**
 * times the tick loop fell more than 10 s behind and skipped ahead
 */
clockSkips: number, 
/**
 * viewers dropped because they could not keep up with the stream
 */
backlogDrops: number, viewers: number, dbBytes: number, snapshotAgeSeconds: number | null, llm: LlmUsage, };

/** Payload of each named event on `GET /api/stream`. */
export type StreamEvents = {
  hello: Hello;
  snapshot: Snapshot;
  tick: TickFrame;
  status: Status;
  roster: RosterEntry[];
  detail: CharacterDetail;
  brainlog: BrainLogRecord;
  world_reset: WorldReset;
  chat_welcome: ChatWelcome;
  chat: ChatMessage;
  chat_you: ChatYou;
};
