// Generated from server/crates/server/src/api/wire.rs by `cargo run --bin export-api`. Do not edit.

export const PROTOCOL_VERSION = 13;

export type Vec2 = { x: number, z: number, };

export type Role = "admin" | "guest";

/**
 * `POST /auth/guest`: asks for a guest seat.
 */
export type GuestRequest = { 
/**
 * the ticket from the last answer, when waiting in line
 */
ticket: string | null, };

/**
 * The answer to `POST /auth/guest`. Asking again sooner than `retryAfterSeconds` answers 429.
 */
export type GuestAnswer = { "kind": "admitted" } | { "kind": "queued", ticket: string, 
/**
 * 1 is next
 */
position: number, 
/**
 * how many guests may watch at once
 */
seats: number, retryAfterSeconds: number, };

/**
 * Why a guest's stream ended.
 */
export type GuestEndReason = "elsewhere" | "left";

/**
 * `guest_end`: the last event on a guest's stream; the browser should not reconnect.
 */
export type GuestEnd = { reason: GuestEndReason, };

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

export type EntityKind = "villager" | "monster" | "corpse" | "ghost" | "animal";

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
voice: VoiceProfile | null, 
/**
 * set (true) for a boss the whole village must rally against; left out otherwise
 */
boss?: boolean, };

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
z: boolean, 
/**
 * for a villager, the horse they ride; for a horse, who rides it
 */
mt?: string | null, 
/**
 * height above the ground × 100, rounded, for one in the air (the dragon); absent on the ground
 */
y?: number, };

export type Entity = { static: EntityStatic, dyn: Dyn, };

/**
 * The fields of [`Dyn`] that changed this tick; absent means unchanged.
 */
export type DynDelta = { id: string, p?: [number, number], f?: number, a?: [string, number], hp?: number, v?: boolean, mh?: string | null, oh?: string | null, act?: string | null, z?: boolean, mt?: string | null, y?: number | null, };

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
billboards: Array<Billboard>, 
/**
 * the homes that are burning or in ruins; every other home stands whole
 */
houses: Array<House>, 
/**
 * the wedding or funeral the village has arranged, if any
 */
ceremony: Ceremony | null, 
/**
 * everything that differs from the map: felled trees, mined rocks, things taken apart, built or burnt
 */
worldObjects: Array<WorldObject>, 
/**
 * the materials the village has put by, for the stockpiles that hold any
 */
stockpiles: Array<Stockpile>, 
/**
 * the village chest and the market's stalls
 */
economy: Economy, 
/**
 * what the dead left lying about
 */
loot: Array<Loot>, 
/**
 * what the sky is doing
 */
weather: Weather, };

export type WorldReset = { 
/**
 * "new_world" or "import"
 */
reason: string, };

export type Emote = "heart" | "anger" | "alert" | "thinking" | "sleep" | "grief" | "laugh" | "broken_heart" | "gift" | "music";

export type Ranged = "arrow" | "bolt" | "magic" | "food" | "fire";

/**
 * How a blow ended for the one it was aimed at.
 */
export type HitOutcome = "hit" | "blocked" | "dodged";

/**
 * The answer to `POST /api/admin/raid`: when the band will come.
 */
export type RaidCallAnswer = { message: string, };

/**
 * The answer to `POST /api/admin/dragon`: when the dragon will come.
 */
export type DragonCallAnswer = { message: string, };

/**
 * Where an orc raid has got to.
 */
export type RaidPhase = "coming" | "leaving" | "gone";

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
export type WireEvent = { "type": "speech" } & Utterance | { "type": "emote", id: string, emote: Emote, } | { "type": "attack", attackerId: string, targetId: string, ranged: Ranged | null, from: Vec2, to: Vec2, } | { "type": "hit", attackerId: string, targetId: string, damage: number, pos: Vec2, fatal: boolean, 
/**
 * left out when the blow simply landed
 */
outcome?: HitOutcome, } | { "type": "death", id: string, name: string, pos: Vec2, monster: boolean, } | { "type": "monster_spawn", id: string, pos: Vec2, } | { "type": "raid", phase: RaidPhase, } | { "type": "chronicle", entry: ChronicleEntry, } | { "type": "grave", grave: Grave, } | { "type": "billboard", billboard: Billboard, } | { "type": "house", house: House, } | { "type": "ceremony", ceremony: Ceremony, } | { "type": "world_object", object: WorldObject, } | { "type": "stockpile", stockpile: Stockpile, } | { "type": "economy", economy: Economy, } | { "type": "loot", loot: Loot, } | { "type": "weather", weather: Weather, };

export type ChronicleKind = "world" | "social" | "romance" | "conflict" | "combat" | "death" | "arrival" | "fire" | "trade";

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

export type HousePhase = "intact" | "burning" | "ruin";

/**
 * The condition of a home. Where it stands comes from the client's own copy of the map (`src/world`).
 */
export type House = { 
/**
 * the place id of the home
 */
id: string, 
/**
 * its name, for the banner
 */
name: string, phase: HousePhase, 
/**
 * how far the fire has taken hold, 0..100 in steps of 5
 */
fire: number, 
/**
 * how far a ruin has been rebuilt, in percent
 */
built: number, };

export type CeremonyKind = "wedding" | "funeral";

export type CeremonyPhase = "announced" | "gathering" | "rite" | "feast" | "over";

/**
 * A wedding or funeral the village has arranged, as viewers are told of it.
 */
export type Ceremony = { id: string, kind: CeremonyKind, phase: CeremonyPhase, 
/**
 * where the rite is held
 */
placeId: string, placeName: string, 
/**
 * the couple, or the dead
 */
ids: Array<string>, 
/**
 * their first names, in the same order
 */
names: Array<string>, 
/**
 * the game day and clock ("14:00") the rite begins
 */
day: number, time: string, };

export type WorldObjectKind = "tree" | "rock" | "prop" | "building" | "ground" | "built";

/**
 * Where something the village built stands, and what it looks like.
 */
export type Placed = { bundle: string, model: string, x: number, z: number, rotation: number, scale: number, };

/**
 * A thing of the world that is not as the map has it. Where it stands comes from the client's own
 * copy of the map, by id: `s:<scatter index>`, `p:<place>:<model index>`, `b:<place>`, `h:<col>,<row>`;
 * a thing the village built (`n:<n>`) carries its own placement.
 */
export type WorldObject = { id: string, kind: WorldObjectKind, 
/**
 * what the village calls it: "the bench at the village green", "The Gilded Tankard"
 */
name: string, 
/**
 * `standing`, `falling`, `stump`, `sapling`, `young`; `whole`, `mined_out`; `pieces`, `burning`, `ash`,
 * `gone`; `ruin`, `dismantled`, `raising`, `expanded`; `dug`, `mown`; or `intact` when it is as the map
 * has it again and can be forgotten
 */
stage: string, 
/**
 * the tick the stage began, or (for a standing tree or whole rock) the last minute anyone worked at it
 */
sinceTick: number, 
/**
 * how far the work on it has got, in percent, in steps of 5
 */
progress: number, 
/**
 * how far a fire has taken hold of it, 0..100 in steps of 5
 */
fire: number, placed: Placed | null, };

/**
 * The materials kept at one place.
 */
export type Stockpile = { 
/**
 * the place id
 */
place: string, name: string, wood: number, stone: number, ore: number, };

/**
 * The village chest and what the market's stalls hold.
 */
export type Economy = { 
/**
 * coin in the village chest, which pays wages and purchases and takes the trader's coin
 */
purse: number, food: number, potion: number, gift: number, trinket: number, relic: number, };

/**
 * Something the dead left on the ground, for whoever picks it up.
 */
export type Loot = { 
/**
 * `l:<n>`
 */
id: string, kind: LootKind, 
/**
 * how many coins, goods or materials; 0 for a trinket or a relic
 */
count: number, 
/**
 * for goods, the item (`food`, `potion`, `gift`); for materials, the material (`wood`, `stone`, `ore`)
 */
item?: string, pos: Vec2, 
/**
 * picked up or blown away: forget it
 */
gone: boolean, };

export type LootKind = "coins" | "trinket" | "relic" | "goods" | "materials";

export type WeatherKind = "clear" | "rain" | "wind" | "hail";

/**
 * What the sky is doing. Rounded, so weather coming on or dying away is told in a handful of steps.
 */
export type Weather = { kind: WeatherKind, 
/**
 * how hard it rains, hails or blows, 0..100 in steps of 5; 0 under a clear sky
 */
intensity: number, 
/**
 * the wind, whatever the kind, 0..100 in steps of 5
 */
wind: number, 
/**
 * the way the wind blows toward, in degrees from +x toward +z, in steps of 15
 */
windDir: number, };

/**
 * Raw materials in a villager's arms.
 */
export type Goods = { wood: number, stone: number, ore: number, };

export type LlmState = "off" | "ready" | "busy" | "error";

export type LlmStatus = { state: LlmState, inFlight: number, queued: number, requests: number, failures: number, tokens: number, latencyMs: number, protocol: string, message: string, };

/**
 * What the ambient mixer needs from the world (the rest depends on each viewer's camera).
 */
export type AmbientCounts = { awake: number, eating: number, drinking: number, smithing: number, relaxing: number, };

/**
 * How far a new world's founding has come.
 */
export type Founding = { settled: number, total: number, };

/**
 * What a hotspot is about; the client decides from it which ones earn a "Happening now" card.
 */
export type HotspotKind = "dragon" | "boss" | "fire" | "raid" | "ceremony" | "battle" | "brawl" | "dead" | "ghost" | "quarrel" | "music" | "romance" | "daily";

/**
 * Something worth watching: what it is, whom to follow and where it is.
 */
export type Hotspot = { kind: HotspotKind, 
/**
 * a villager, one of the dead, a raider, the dragon or a ghost to follow; `null` for a place that has
 * nobody to follow (a fire nobody is fighting yet)
 */
subject: string | null, 
/**
 * where it is, for a hotspot without a subject (and as a fallback when the subject is out of sight)
 */
at: Vec2, 
/**
 * a short line for the card: "The tavern is on fire", "Bran and Edda are quarrelling"
 */
text: string, 
/**
 * how much it is worth watching, 0 to 100
 */
score: number, };

/**
 * Sent when it changes, at most once a second.
 */
export type Status = { llm: LlmStatus, viewers: number, speed: number, 
/**
 * the world is held at 1x because the model has a backlog
 */
throttled: boolean, population: number, 
/**
 * the dead abroad
 */
monsters: number, 
/**
 * the dragon is over the valley
 */
dragon: boolean, alarm: boolean, 
/**
 * an orc war band is in the valley
 */
raid: boolean, 
/**
 * "night", "early morning", "morning", "midday", "afternoon", "evening"
 */
part: string, ambient: AmbientCounts, 
/**
 * the village is still being founded: how many of its founders have settled; `null` once it is
 */
founding: Founding | null, 
/**
 * what is worth watching right now, best first (at most five); the Auto camera and the "Happening now"
 * cards read it
 */
hotspots: Array<Hotspot>, };

export type Mood = "ok" | "fight" | "sleep" | "talk" | "hurt";

export type RosterEntry = { id: string, name: string, role: string, 
/**
 * rendered activity line, e.g. "Eating at the Tavern"
 */
status: string, 
/**
 * fraction of max hit points, 0..1
 */
hp: number, mood: Mood, partner: boolean, 
/**
 * a child, not yet of age
 */
child: boolean, };

export type RelationLabel = "stranger" | "acquaintance" | "friend" | "close friend" | "lover" | "rival" | "enemy" | "family" | "spouse";

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

export type Inventory = { food: number, potion: number, gift: number, 
/**
 * valuables the dead left, kept to be sold
 */
trinket: number, relic: number, };

export type PlanRow = { 
/**
 * "08:00"
 */
time: string, text: string, current: boolean, };

/**
 * The inspector, with every label already rendered (the client has no sim helpers).
 */
export type CharacterDetail = { id: string, name: string, pronouns: string, age: number, occupation: string, className: string, 
/**
 * a child, not yet of age
 */
child: boolean, backstory: string, traits: Array<string>, goals: Array<string>, status: string, thought: string, summary: string, hp: number, maxHp: number, needs: Needs, inventory: Inventory, 
/**
 * raw materials in their arms
 */
carried: Goods, 
/**
 * coin in their purse
 */
gold: number, home: string, 
/**
 * that home has burned down and is not yet rebuilt
 */
homeless: boolean, 
/**
 * the first name of the one they are married to
 */
spouse: string | null, kills: number, planSource: string, plan: Array<PlanRow>, relations: Array<RelationRow>, 
/**
 * newest first, at most 40
 */
memories: Array<Memory>, };

/**
 * A villager to link to from a card.
 */
export type NamedId = { id: string, 
/**
 * their first name
 */
name: string, };

/**
 * The inspector for one of the dead, every label rendered on the server.
 */
export type MonsterDetail = { id: string, 
/**
 * what its label says: the species, or the name of the villager it once was
 */
name: string, 
/**
 * e.g. "skeleton warrior" (`kind` is the tag of [`Detail`])
 */
species: string, hp: number, maxHp: number, 
/**
 * what it is doing now, e.g. "Hunting Mira near the well"
 */
status: string, 
/**
 * the villager it hunts, if any
 */
target: NamedId | null, 
/**
 * when and where it rose, e.g. "Rose from the graveyard on day 3 at 21:40"; empty when not known
 */
rose: string, 
/**
 * the full name of the villager it once was
 */
was: string | null, 
/**
 * does not crumble at dawn
 */
lasting: boolean, 
/**
 * one of the walking dead (false for the dragon)
 */
undead: boolean, 
/**
 * villagers it has slain
 */
kills: number, 
/**
 * blows it has landed on a villager
 */
blows: number, 
/**
 * the least and the most one of its blows takes
 */
damage: [number, number], 
/**
 * one of the dead, an orc raider, or the dragon
 */
faction: MonsterFaction, 
/**
 * a raider's plunder, e.g. "12 coins and 6 logs of wood"
 */
carrying: string | null, 
/**
 * a boss: the skeleton golem or the orc warchief
 */
boss: boolean, };

/**
 * The card of one of the valley's animals.
 */
export type AnimalDetail = { id: string, 
/**
 * a pet's or a horse's name, or what it is ("a sheep")
 */
name: string, 
/**
 * e.g. "sheep" (`kind` is the tag of [`Detail`])
 */
species: string, 
/**
 * what it is doing now, e.g. "Grazing in the meadow" or "At Mira's heels"
 */
status: string, 
/**
 * the villager who keeps it, for a pet
 */
owner: NamedId | null, 
/**
 * who is riding it, for a horse
 */
rider: NamedId | null, 
/**
 * what tending it gives the village, e.g. "milk"
 */
yields: string | null, };

/**
 * The inspector for a ghost, every label rendered on the server.
 */
export type GhostDetail = { id: string, 
/**
 * the full name of the villager it was
 */
name: string, 
/**
 * the villager it was
 */
wasId: string, 
/**
 * the day they died
 */
diedDay: number, 
/**
 * how they died, e.g. "slain by a Skeleton Warrior at the green"
 */
cause: string, 
/**
 * what it is doing now, e.g. "Haunting the village near the well"
 */
status: string, 
/**
 * nights it still has to walk, counting tonight; 0 before it first rises
 */
nightsLeft: number, };

export type MonsterFaction = "undead" | "orc" | "dragon";

/**
 * What `detail` on the stream carries: the inspector for whatever this viewer watches, tagged by `kind`.
 */
export type Detail = { "kind": "villager" } & CharacterDetail | { "kind": "monster" } & MonsterDetail | { "kind": "ghost" } & GhostDetail | { "kind": "animal" } & AnimalDetail;

/**
 * What a site is: a place's building, or something the village put up on a pad.
 */
export type SiteKind = "building" | "built";

/**
 * One material a project calls for, and how much of it has been carried to the site.
 */
export type SiteNeed = { 
/**
 * `wood`, `stone` or `ore`
 */
resource: string, delivered: number, needed: number, };

/**
 * The work the village has taken on at a site.
 */
export type SiteProject = { 
/**
 * "raising the smithy", "a fence by the red cottage"
 */
label: string, helpers: Array<NamedId>, needs: Array<SiteNeed>, 
/**
 * how far the hands-on work has got, in percent
 */
progress: number, };

/**
 * The page for a building or something the village built (a site: `b:<place>` or `n:<n>`), with every
 * label already rendered. Sent as the `site` event to the viewer watching it, and by `GET /api/sites/{id}`.
 */
export type SiteDetail = { id: string, kind: SiteKind, 
/**
 * "The Gilded Tankard", "a bench near the village green"
 */
name: string, 
/**
 * the place it is, or stands near
 */
placeId: string, placeName: string, description: string, 
/**
 * as [`WorldObject::stage`]; `whole` for a building that is as the map has it
 */
stage: string, 
/**
 * the tick the stage began; 0 for a building nobody has touched
 */
sinceTick: number, 
/**
 * how far the work on it has got, in percent
 */
progress: number, 
/**
 * how far a fire has taken hold of it, 0..100
 */
fire: number, 
/**
 * how many may sleep there; homes only
 */
beds: number | null, 
/**
 * who lives there; homes only
 */
residents: Array<NamedId>, 
/**
 * the villagers at the place right now
 */
present: Array<NamedId>, project: SiteProject | null, 
/**
 * the materials kept there, for a stockpile that holds any
 */
stockpile: Goods | null, 
/**
 * chronicle lines about it, newest first, at most 20
 */
history: Array<ChronicleEntry>, };

export type RequestKind = "mercy" | "react" | "reply" | "converse" | "decide" | "plan_day" | "billboard" | "slogan" | "ceremony" | "name" | "reflect";

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
 * the author's name when they sent it; for a join or a leave, who came or went;
 * for an event, whose doing it was
 */
author: string, 
/**
 * markdown, already tidied and filtered by the server; render it as text, never as HTML.
 * Empty for a join or a leave. For an event, plain text: what they did.
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
 * `POST /api/watch`: which character's `detail`, or which site's `site`, this viewer (one tab) receives.
 */
export type WatchRequest = { 
/**
 * random per tab
 */
viewer: string, 
/**
 * a villager id (`c7`) or a site id (`b:<place>`, `n:<n>`)
 */
id: string | null, };

/**
 * `GET /api/me`
 */
export type Me = { role: Role, };

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
maxTokensPerDay: number, 
/**
 * the embedding model's endpoint (`EMBED_BASE_URL`), when memories are recalled by meaning
 */
embedBaseUrl: string | null, 
/**
 * `EMBED_MODEL`, or none when memories are recalled by shared words
 */
embedModel: string | null, embedApiKeySet: boolean, };

/**
 * `POST /api/admin/llm/test`
 */
export type LlmTestResult = { ok: boolean, message: string, };

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
  detail: Detail;
  site: SiteDetail;
  brainlog: BrainLogRecord;
  world_reset: WorldReset;
  chat_welcome: ChatWelcome;
  chat: ChatMessage;
  chat_you: ChatYou;
  guest_end: GuestEnd;
};
