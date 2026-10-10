# Fantasy Sim World — design and implementation plan

## Context

The repo is empty (README + .gitignore). The goal is a browser-only 3D world in the KayKit style where 10–12 LLM-driven fantasy characters live in a village, talk, form relationships, fight occasional monsters and, if pushed far enough, each other. The viewer is a spectator with a free top-down camera. The LLM is used only for decisions, through tool-calling on an OpenAI-compatible endpoint; everything real-time (walking, fighting, animation) runs locally.

Decisions made with the owner:

| Topic | Decision |
|---|---|
| LLM target | Local and cloud models equally: strict request queue, compact prompts, configurable concurrency |
| Population | 10–12 characters |
| Mortality | Permadeath; newcomers occasionally arrive |
| World | One village plus surrounding wilds, as a KayKit hex diorama (hex tiles scaled ×7) |
| Ambient audio | Mixer with named slots; the owner supplies the audio files, missing slots are silent |
| Speech | Bubbles + chronicle now; a `SpeechBus` seam for TTS later |

Stack: TypeScript, Vite, pnpm, three.js r186 (`three/webgpu` `WebGPURenderer`, automatic WebGL 2 fallback), Preact + signals for the HUD, IndexedDB via `idb`, Vitest.

## Since the server migration (October 2026)

The design below was written for a browser-only game. The world now runs on a Rust server, around the clock, and every signed-in person watches the same village from their own camera ([the migration plan](2026-10-02-backend-migration-plan.md) has the full design and what was measured). What still holds, and what changed:

- **The simulation, world and brain are the same design**, ported one to one to Rust: `server/crates/{core,world,sim,brain}` are `src/{core,world,sim,brain}` (20 Hz ticks, systems in the same order, the same RNG, commands at tick boundaries, the same prompts, tools, governor and validation). The TypeScript simulation is kept at the `ts-sim-reference` tag as the reference the port was checked against: identical per-tick traces for ten game days on several seeds, and statistics over 10 seeds × 30 days within a few percent.
- **The server** (`server/crates/server`) owns the world: a world task runs the sim on its clock and encodes each tick once into a diff frame; a digest task computes the roster, status line and inspector details four times a second (once per watched villager, shared by everyone watching); a brain task asks the model and sends commands back through the world's input queue; a store thread writes SQLite. Speed is global and set by admins (pause, 1×, 2×, 4×); while the model has a backlog the clock holds at 1×.
- **Persistence** is SQLite (WAL) on the server's volume, replacing IndexedDB: compressed snapshots (every 30 s, after notable events, on shutdown), the chronicle, the command record, the decision log, settings and the model key (never sent to a browser), and the users, sessions and allowlist. A restart resumes from the newest snapshot. Browser exports still import, and the server's exports are the same format. Litestream can stream the database to object storage ([deploy.md](../../deploy.md)).
- **Access**: Google and Apple sign-in, limited to an allowlist that admins manage, or one shared token for local work ([sign-in.md](../../sign-in.md)).
- **Billboards** (October 2026) stand in five places and show a slogan a day, or text an admin put up; villagers read them and react. See [the billboards design](2026-10-03-billboards-design.md).
- **House fires** (October 2026): when nobody is awake the dead set a home on fire; the whole village carries water, and a house that is lost is rebuilt while its people sleep rough. See [the house fire design](2026-10-03-house-fire-design.md).
- **Children** (October 2026): five children live in the village from the first day, lovers have children, children play, get into trouble and grow into adults after about a month. See [the children design](2026-10-08-children-design.md).
- **A world that changes** (October 2026): villagers fell trees, mine rocks, mow and dig, take props and buildings apart and set them alight; materials go to stockpiles and the village builds with them. The sim lays its changes over the static map and over the nav grid. See [the dynamic world design](2026-10-07-dynamic-world-design.md).
- **Memory by meaning** (October 2026): with an embedding model (`EMBED_*`), memories are recalled by how close they are in meaning to the moment, not only by shared words. The vectors live in the server's database, never in the world. See [the memory design](2026-10-04-memory-embeddings-design.md).
- **An invented cast** (October 2026): the hand-authored twelve and their newcomers are gone from the game. A new world is empty, and the language model invents each founder and each newcomer (one request per person, by the casting task), or the server does from its own lists without a model; every villager reaches the sim as a command, so worlds still replay. The authored cast lives on in `server/crates/testing` for the tests that know people by name. See [the newcomer design](2026-10-04-newcomer-profiles-design.md).
- **Weddings and funerals** (October 2026): a couple who have lasted are married on the green before the whole village and move in together; the dead are buried the next morning with everyone at the graveside. The model writes the vows and the eulogy ahead of time, with scripted lines as the fallback. See [the weddings and funerals design](2026-10-08-weddings-funerals-design.md).
- **Music** (October 2026): the first entertainment. Some villagers sing or play, settled by who they are rather than by the RNG; of an evening one strikes up outside the tavern, the idle gather to listen, and a good tune heartens its listeners for a few hours. See [the music design](2026-10-08-music-design.md).
- **Coin** (October 2026): villagers are paid for what they put down at the stores and sell at the market, buy from its stalls, and a passing trader buys the surplus once a day. The dead drop coin and valuables, villagers buy from one another, and whoever has the gold pays for a bigger house or something for the yard. See [the economy design](2026-10-08-economy-design.md).
- **Animals** (October 2026): a herd and hens at the windmill farm, ducks, birds and an owl, and dogs and cats that their keepers name; farmers tend the herd for food, villagers make a fuss of their pets, and anyone setting off on a long walk rides a free horse. The models are made in Blender in the KayKit style. See [the animals design](2026-10-08-animals-design.md).
- **Where to look** (October 2026): the server ranks what is worth watching once a second and sends the best few in the status; an Auto camera follows the best one while the viewer leaves the camera alone, and big moments raise a "Happening now" card. See [the hotspots design](2026-10-10-hotspots-design.md).
- **The browser is a viewer** (`src/net`, `src/render`, `src/ui`, `src/audio`, `src/speech`): it never loads the simulation, the brain or saves (an architecture test enforces this). It keeps `src/world` for terrain, scenery and camera bounds; the server's `hello` carries a hash of the navigation grid so a client with a different layout notices.

### Wire protocol

Every type on the wire is a Rust struct exported to `src/generated/api.ts` (a test fails when the file is out of date).

- `GET /api/stream` (server-sent events, `?viewer=<id>` names the tab): `hello` (boot id, tick, speed, protocol version, world hash, viewer count, role), then either the ticks missed since `Last-Event-ID` (the server keeps the last 100) or a `snapshot` (every entity's static and dynamic fields, roster, chronicle tail, graves, alarm, model status, recent voices), then one `tick` per sim tick: added entities, removed ids, changed dynamic fields only (position in centimetres, facing, animation and its sequence number, HP, visibility, hand items, activity, asleep), and the tick's one-shot events (speech with the full utterance, hits, projectiles, emotes, deaths, chronicle entries, graves, and billboards that show something new; the snapshot carries what every billboard shows). `status` and `roster` follow when they change, `detail` for the villager this tab follows, `brainlog` for admins, and `world_reset` when an admin starts a new world or imports one. Event ids are `<bootId>:<tick>`.
- REST: `GET /api/me`, `POST /api/watch`, `GET /api/characters/{id}`, `GET /api/chronicle`; for admins `POST /api/admin/speed`, `POST /api/admin/billboards`, world new, import and export, health, the model settings, test, model list and decision log, and the allowlist and refused sign-ins. Mutating routes take JSON only (part of the CSRF defence).
- Bandwidth per viewer: 1 to 6 KB/s, most of it while villagers move.

## Asset facts the design relies on

Source: `/Users/hellman/Documents/The Complete KayKit Collection v7` (CC0). The used subset is processed into `public/assets/` and committed, so a clone runs without the collection.

- **Characters** (`KayKit Adventurers 2.0/Characters/gltf/*.glb`): Knight, Barbarian, Mage, Ranger, Rogue, Rogue_Hooded, Druid, Engineer. Mesh + 23-bone skeleton only, 2.2–2.7 units tall, facing +Z. Texture variants in `Textures/<class>_texture_alt_{A,B,C}.png`; helmet, cape, hat are separate meshes that can be hidden.
- **Shared rig**: every `Rig_Medium` character (Adventurers, Skeletons, fantasy Mystery characters) has an identical skeleton, so one clip set drives all of them. Clips: `KayKit Character Animations 1.1/Animations/gltf/Rig_Medium/Rig_Medium_{General,MovementBasic,MovementAdvanced,CombatMelee,CombatRanged,Simulation,Special,Tools}.glb`. Each file also embeds a mannequin mesh and a duplicate `T-Pose`.
- **Weapons** are separate `.gltf` files (`KayKit Adventurers 2.0/Assets/gltf/`, `KayKit Skeletons 1.1/assets/gltf/`), grip at origin, parented to the hand-slot bone. three.js strips dots from bone names: `handslot.r` becomes `handslotr`.
- **Monsters**: `KayKit Skeletons 1.1/characters/gltf/` Minion, Warrior, Rogue, Mage, Necromancer (all Rig_Medium), with `Skeletons_Spawn_Ground` and friends in `Rig_Medium_Special.glb`.
- **Terrain and buildings**: `KayKit Medieval Hexagon Pack 1.0.1/Assets/gltf/` is miniature scale. At ×7 a hex is 14 units across (column pitch 14, row pitch 12.12) and a home is about 6.5 tall. Use its tiles, buildings and bridges only; its fences, carts and props are out of proportion at ×7. Water sits 1.4 below ground and bridge decks arc to 1.75 above it.
- **Character-scale scenery**: Forest Nature Pack (`Color1` only), Halloween Bits (graveyard, crypt, fences, wagon, signposts, lanterns), Dungeon Pack (ruins, tables, benches, barrels, beds, torches), Resource Bits, RPG Tools (anvil, grindstone), plus campfire, tent, cauldron, training dummy and farm plots from the Mystery packs.
- Environment packs have one 1024² palette atlas and one material per pack, 50–1,500 triangles per model.

## Design

### Principles

1. **The local brain is the baseline; the LLM overrides it.** Every character always has a locally generated routine (daily schedule + needs), so nobody stands idle waiting for a response and the world lives with no endpoint configured.
2. **The sim is headless plain data.** State is JSON-serialisable (ids, no classes or object references, RNG state included). The brain only submits commands, applied at tick boundaries. Given the same command stream, the sim is deterministic.
3. **Renderer reconciles from state.** Entities, transforms and animation state are read from sim state every frame, interpolated between the last two ticks. Events are used only for one-shots (hit, speech line, death, sounds).
4. Dependency rule: `core ← world ← sim ← brain`; `render`, `ui`, `audio` read sim types and `sim` imports none of them.

### Simulation (`src/sim/`, `src/world/`)

- Fixed 20 Hz tick; speed runs N ticks per frame (pause, 1×, 2×; 4× added in the last slice). One game hour is about one real minute at 1×.
- **World**: hand-authored hex layout in TypeScript plus seeded scatter for trees, rocks and grass. Hexes are ground only; buildings are placed freely along roads. The sim is 2D; `heightAt(x, z)` is used only by the renderer (bridges).
- **Places** are smart objects: entrance, door offset, activity slots with position, facing, animation and reservation. Buildings are closed shells, so activity slots are outdoors (tavern tables, anvil, market, campfire, training yard, farm, docks, forest clearing); interiors are only for sleeping.
- **Navigation**: 1-unit square grid rasterised from obstacle footprints, water and bounds; A* with string-pulling; roads are cheaper; separation steering.
- **Characters**: profile (name, class, backstory, traits, goals, speech style, voice profile), HP, needs (hunger, energy, social), three-item inventory (food, potion, gift), equipment, home, current plan and intent, relationships, memories.
- **Cast**: 12 hand-authored profiles (two Knights, Barbarian, two Mages, two Rangers, Rogue, hooded Rogue, two Druids, Engineer) differentiated by texture variant, headgear and weapon, with a few seeded relationships. Newcomers come from a template pool that adds Paladin, Cleric, Witch, Tiefling, Lorekeeper and Farmers.
- **Combat** is fully local: weapon profile per class (one-handed, two-handed, bow, magic, dagger), cooldowns matched to clip durations, range, HP. Ranged hits are instant with a render-only projectile. Being attacked triggers an immediate local defend reflex.
- **Monsters and the night loop**: skeletons rise from the graveyard from dusk, a rare Necromancer leads larger groups. Monsters are local state machines. A watch rota keeps some characters awake, and an alarm stimulus wakes fighters.
- **Director**: spawn and arrival rates follow population and recent deaths so the village neither empties nor idles.
- **Death**: permadeath, a named gravestone, grief and grudge memories for those who knew the character, an obituary in the chronicle, and that character's pending requests cancelled.

### Relationships and hostility

- Per directed pair: affinity, trust, attraction, a label with hysteresis (stranger, acquaintance, friend, close friend, lover, rival, enemy) and a short text summary.
- Changes come from sim events (fought side by side, gift, insult, attacked) and from a five-step feeling the LLM reports after each conversation, mapped to clamped numbers. Lovers require both sides' attraction and an accepted confession.
- Hostility is an explicit ladder driven by a separate grievance value that rises on specific events and decays over days:

| Rung | Unlocks |
|---|---|
| Tension | Prompt hints only |
| Grudge | `confront` (a hostile conversation) |
| Feud | `brawl` (unarmed, ends when one side yields at about 30% HP) |
| Blood feud | `attack` (lethal), only with a recent trigger, a temper roll, a global cooldown and a population floor |

- At the yield moment the LLM chooses `spare` or `finish`; spare is the default on timeout. Witnesses lose affinity for an aggressor; apology, gifts and time de-escalate.
- A profile's `spite` (0 to 1, 0 for nearly everyone; Garrick has 1) bends the ladder for that one villager. The spiteful pick quarrels with anyone who is not a real friend, grudge or no grudge, never apologise, let grudges cool half as fast, and may brawl from a grudge of 45 instead of a feud. The lethal rung is unchanged.

### Brain (`src/brain/`)

- **Request kinds**, each offering only the tools that make sense. Every action tool has optional `thought` (stored as a memory) and `say` fields.

| Kind | Trigger | Tools |
|---|---|---|
| `plan_day` | Waking | `set_plan` (4–6 blocks) |
| `decide` | Plan block ends, notable stimulus, critical need | `go_to`, `do_activity`, `talk_to`, `give`, `continue`; `confront`, `brawl`, `attack` when the ladder unlocks them. Up to three ordered steps |
| `react` | Monster nearby, attacked | `fight`, `flee` |
| `converse` | Conversation | `conversation` (batched) or `reply` (turn by turn) |
| `mercy` | Opponent yields | `spare`, `finish` |
| `reflect` | Asleep | `reflection` |

- **Scheduler and governor**: stimuli become requests with a priority (spoken to > threat > critical need > plan block ended > idle), coalesced per character. Cooldowns and time-to-live are in real time. The governor measures latency and scales trigger rates to keep demand near 70% of capacity, so cloud endpoints get richer behaviour and local models get sparser but never laggy behaviour. If a visible reaction waits too long the speed clamps to 1×.
- **Staleness**: the prompt is built at dispatch time; each character has a context epoch (bumped when attacked, when a conversation starts, on death) and responses from an older epoch are discarded and their fetch aborted.
- **LLM client**: plain `fetch` to `{baseUrl}/chat/completions`. Two wire protocols behind one internal tool-call contract: native tool-calling (primary), and `response_format: json_schema` for servers where tool-calling is unreliable. "Test connection" probes the endpoint and picks the protocol.
- **Robustness**: static tool schemas with valid options listed in the prompt and repaired before rejecting (case, aliases, nearest name); a prompt token budget that drops the lowest-priority sections first; `<think>` blocks stripped; `arguments` accepted as string or object; timeout, retry, circuit breaker, `Retry-After`; `Authorization` omitted when the key is empty; request and token meter.
- **Conversations** are a sim entity (approaching → hailed → awaiting lines → playing → ended). Hailing is a local reflex: the target stops, turns and waves. Two participants; bystanders get "overheard" memories. Two drivers behind a `ConversationDriver` interface:
  - *Batched* (default at low capacity): the initiator's `talk_to` supplies the opening line and one call writes the remaining 3–5 lines plus each side's feeling.
  - *Turn by turn* (when capacity allows): each character answers from its own memories.
  - Lines play on local timers; a timeout falls back to templated small talk.
- **Memory**: a per-character stream of `{time, text, importance, kind, subjects}`. Observations are templated by the sim; `thought` fields and conversation summaries are added. Retrieval scores recency, importance and subject overlap. Pruning is local so an offline world stays bounded. A nightly `reflect` call condenses the day and updates the self-summary and relationship summaries.

### Rendering (`src/render/`)

- `WebGPURenderer` with `?renderer=webgl|webgpu` override and the active backend shown in a debug HUD. All three.js imports stay in `render/`.
- Scenery instanced per model in spatial chunks, one canonical material per pack. Buildings are merged with the rest of the scenery, except the homes, which stay one object each because a home can burn down.
- Characters via `SkeletonUtils.clone`, one `AnimationMixer` each, clips merged by name at runtime, weapons on hand-slot bones, `frustumCulled = false`. The sim emits abstract animation states; the renderer maps them to clips.
- One shadowed directional sun plus hemisphere light on a day/night cycle; a fixed pool of point lights for lanterns and fires (intensity changes, never count changes).
- DOM overlay with own world-to-screen projection for speech bubbles, name tags and health bars. Model text is always rendered as text.
- Top-down camera: pan (drag, WASD), zoom, rotate, bounds, click a character to follow.

### UI (`src/ui/`)

Start overlay (also unlocks audio), clock and speed controls, chronicle feed, roster, read-only inspector (profile, needs, plan, last thought, memories, relationships), settings (endpoint presets for OpenAI, OpenRouter, Ollama and LM Studio; key, model, concurrency, custom headers, test connection; audio slots; export, import, reset), and an LLM log panel (prompt, raw response, parsed calls, validation result, latency, tokens).

### Audio and speech (`src/audio/`, `src/speech/`)

- Ambient mixer driven by camera position, zoom, time of day and nearby activity. Slots: `birds_day`, `wind`, `forest`, `river`, `shore`, `crickets_night`, `village_day`, `tavern`, `smithy`, `campfire`, and effects `sword_hit`, `arrow_shot`, `magic_cast`, `hurt`, `death`, `monster_spawn`. Files go in `public/audio/` (documented in `public/audio/README.md`); a missing file is a silent slot and the settings panel shows which slots are filled.
- `SpeechBus`: every spoken line passes through it with speaker, text, emotion, `durationMs` and the character's voice profile. Sinks now: bubble and chronicle. A TTS sink (OpenAI-compatible `/audio/speech` or Web Speech, positional) can be added later without touching the sim, because line pacing already takes its duration from the bus.

### Persistence (`src/persist/`)

*Superseded by SQLite on the server (see "Since the server migration"); kept as the original design.*

- One save document in IndexedDB, written with a single `put`, plus one rotating backup. Settings in a separate store; the API key never leaves the browser and is excluded from exports.
- `saveVersion` with ordered migration functions; before a first release, an epoch bump discards old saves instead.
- Save every 30 s, on day rollover, on deaths and on `visibilitychange` to hidden. The sim pauses while hidden. Paths, queued decisions and projectiles are recomputed on load.
- `navigator.storage.persist()` and a Web Lock so a second tab cannot clobber the save.

### Asset pipeline (`tools/`)

- `tools/assets/build-assets.mjs`: copies the used models into `public/assets/`, strips mannequin meshes from animation files, and writes `src/generated/assetManifest.ts` including clip durations.
- `tools/blender/*.py` (headless Blender 5.2, `/Applications/Blender.app`): generate pieces the collection lacks at character scale, in the KayKit palette — a wooden village fence and gate, and wildflower and mushroom clusters. More only where dressing the world shows a gap.

## Layout

```
src/
  main.ts
  app/        loop.ts  wiring.ts  config.ts
  core/       rng.ts  events.ts  ids.ts  math2d.ts  time.ts
  world/      hex.ts  map.ts  places.ts  navgrid.ts  pathfind.ts  height.ts  scatter.ts
  sim/        state.ts  sim.ts  commands.ts  events.ts  serialize.ts
    systems/  clock needs intents movement activities perception memory conversation
              relationships hostility combat monsters death arrivals director chronicle
    data/     classes weapons activities monsters cast newcomers animTimings
  brain/      scheduler.ts  governor.ts  situation.ts  utilityBrain.ts
    prompts/  system decide planDay react converse mercy reflect budget
    tools/    definitions.ts  validate.ts
    llm/      client.ts  protocol.ts  parse.ts  probe.ts  transport.ts  usage.ts
    memory/   retrieval.ts  reflection.ts  prune.ts
  speech/     speechBus.ts  timing.ts  voices.ts
  render/     renderer scene assets terrain scenery characters animation lighting
              water camera fx overlay debug
  audio/      engine.ts  ambient.ts  sfx.ts  slots.ts
  ui/         App.tsx  store.ts  Hud  Roster  Inspector  Chronicle  Settings  BrainLog
  persist/    db.ts  saveStore.ts  settingsStore.ts  migrations.ts  exportImport.ts  autosave.ts
  generated/  assetManifest.ts
tools/        assets/build-assets.mjs  blender/*.py  llm-probe.ts  mock-llm.mjs
docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md
```

## Build order

Each slice ends runnable in the browser, with tests passing and a commit on `claude/fantasy-sim-world-2044dc`. Sim and brain logic is written test-first.

0. **Spec and scaffold.** Save this design as the spec file above; Vite + TypeScript + Vitest scaffold; update README.
1. **Walking characters.** Asset script, renderer with backend override, a small hex patch, 12 cloned characters wandering with idle/walk crossfade and a weapon in hand, instanced trees, shadows, camera, fixed-tick loop with interpolation. Checks cloned skinned meshes, bone-attached weapons, instancing and shadows on both backends.
2. **World and navigation.** Full map, places and slots, nav grid, A*, separation, bridge height, debug overlays (hex coordinates, nav grid, click to print position).
3. **Life without LLM.** Clock and day/night lighting, needs, activities, intent executor, routine brain, sleeping, HUD with roster, inspector and chronicle.
4. **Persistence and settings.** Autosave, export, import, reset, tab lock, settings panel.
5. **LLM client.** Client, both protocols, tolerant parser, probe, mock server, `tools/llm-probe.ts` reporting latency and valid-call rate.
6. **LLM decisions.** Scheduler, governor, prompt budget, `plan_day` and `decide`, memory stream and retrieval, speech bus with bubbles, LLM log panel.
7. **Conversations and relationships.** Conversation entity, hail reflex, batched and turn-by-turn drivers, feeling-based changes, labels, gifts.
8. **Combat, monsters, death, newcomers.** Skeleton spawns, reflexes and `react`, watch rota and alarm, gravestones, arrivals, director.
9. **Hostility ladder.** Grievance, `confront`, `brawl`, lethal gate, `mercy`, witnesses.
10. **Reflection and pruning.**
11. **Audio and polish.** Ambient mixer and slots, Blender props, water, night lights, 4× speed.

## Verification

- **Unit tests** (`pnpm test`): hex maths, loop accumulator, a path between every pair of places, no slot in a blocked cell, needs bounded over 10 days, scheduler priority, coalescing and staleness, golden prompts, parser and repair cases against mocked fetch, conversation state machine and timeout fallback, label hysteresis, hostility gates, an import-rule test for the dependency rule.
- **Determinism**: 2,000 ticks straight versus 1,000, save, load, 1,000 more, asserting deep equality.
- **Soak**: a headless 200-day run with the routine brain, asserting the population stays in range, memory stays bounded and lethal attacks stay rare.
- **In the browser** (`pnpm dev`, built-in browser pane): screenshots on both backends, no console errors, characters following routines, reload mid-walk resuming correctly, export and import.
- **LLM end to end**: first against `tools/mock-llm.mjs`, then against a real endpoint. Ollama and LM Studio are installed on this machine but not running; I will ask before starting either, and for a cloud key if that path should be tested too.
- `pnpm build` produces a static bundle that runs from `pnpm preview`.

## Later, not in this plan

Character speech (TTS sink), viewer interaction, orc raiders and large-rig bosses, block and dodge, embedding-based memory retrieval, LLM-generated newcomer profiles, seasons, a map editor.

## Implementation notes

What was built follows the design above, with these differences:

- **The local routine brain lives in the sim** (`src/sim/systems/routine.ts`) rather than in `src/brain/`, so a headless sim is alive on its own and the long-run tests need nothing else.
- **Module layout** is a little flatter than planned: prompts are one file (`src/brain/prompts.ts`), the orchestrator is `src/brain/brain.ts`, and the request queue is `src/brain/llm/queue.ts`.
- **Ground heights are sampled by the asset build** (`src/generated/tileHeights.ts`), so the world layer itself knows where water, sand, roads and the bridge deck are; the plan had this as a render-only concern.
- **Rivers and lake edges are not shaded specially**; the KayKit water tiles and a sea plane are used as they are.
- **Romance has a fixed "chemistry" per pair**, so attraction can grow between some pairs and never between others.
- **Some characters flirt for sport** (2026-10-03). A profile's `allure` (0 to 1; only Mira has any) drives `server/crates/sim/src/systems/charm.rs`:
  - a charmer flirts on her own with anyone already a little drawn to her, whoever has a partner;
  - her flirting raises attraction without chemistry, while her own grows slowly;
  - admirers who watch her flirt resent the man she flirts with, up to just below a blood feud;
  - after a welcome flirt she turns the admirer against her enemy and takes his gift;
  - she enjoys it when admirers quarrel or brawl, and the prompts tell her who wants her.

  Rivals usually come to blows within two or three weeks. Profiles without the field load as 0.
- **A killer is not exiled**; the village's hostility towards them is the consequence.
- **No automated browser test** was added; rendering was checked by hand on both backends.
- **Audio ships with the game** (2026-10-02) instead of being supplied by the owner: CC0, public domain and CC BY sounds, cut and levelled by `tools/audio/build-audio.ts`. **Background music** was added as well, with a mood that follows what the viewer watches (`src/audio/mood.ts`, `music.ts`). Ambience, effects and music each have an on/off switch and a volume, kept in the browser.
- **Tested against one real model**: `qwen/qwen3.6-35b-a3b` in LM Studio, one request at a time. That run led to these changes: a "Model reasoning" setting (sent as `reasoning_effort`, off by default); reading answers that a server puts in the reasoning channel; growing the output limit when an answer is cut off; accepting a play script or a bare tool name written as prose; switching to JSON output after the first missed tool call; expiring old stimuli and dating memories so villagers do not react to last night's attack at noon; sharing decisions fairly between villagers; and letting the model write the opening line of conversations villagers start on their own. No cloud endpoint has been tested.
