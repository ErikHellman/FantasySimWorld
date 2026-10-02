# Fantasy Sim World: move the simulation to a Rust backend

## Progress

- **Phase 0 (artefacts and scaffold): done**, except the deployment check, which needs the owner's Fly.io account: `fly deploy`, then `node tools/sse-probe.mjs https://<app>.fly.dev 120` (and again with `INITIAL_SPEED=0` for the idle timeout, and across a `fly deploy`), and `curl -sI --http2` for HTTP/2. Notes on what was built:
  - Crates are named `fsw-core`, `fsw-world`, `fsw-sim`, `fsw-brain`, `fsw-server` (a crate called `core` would shadow Rust's own); the directories keep the names above.
  - The save fixtures (`export-day-1.json`, mid-conversation; `export-day-2.json`, skeletons out) come from `tools/make-export-fixture.ts`, which runs the TypeScript `Sim` and `Brain` against `tools/mock-llm.mjs` and writes exactly what the browser's Export writes. Neither has graves or corpses yet; add an export from a long-running browser world when one exists.
  - The golden world fixtures also hold `hexAt`, `roadDistance` and `nearestWalkable` samples and 340 routes (raw A* cells via the new `findRoute`, and smoothed). The nav costs are base64 of the raw little-endian `f32`s.
  - `AUTH_MODE=token`: `/auth/token` is a form that POSTs the token (so it never sits in a URL or a proxy log) and sets an HttpOnly cookie; `/api/*` also accepts `Authorization: Bearer`. `INITIAL_SPEED=0` starts paused so the idle stream can be tested.
  - The tick schedule (`TickClock`: 12 ticks per wake at most, skip ahead past 200 owed) is final and tested with paused tokio time.
- **Phase 1 (core and world in Rust): done.** `fsw-core` (`jsmath`, `rng`, `time`, `math2d`) and `fsw-world` (`hex`, `layout`, `tiles`, `height`, `map`, `place_defs`, `places`, `scatter`, `navgrid`, `pathfind`, `assets`) reproduce every golden fixture: the ground and road masks, nav costs bit for bit, places, scatter and all 340 routes and 2,000 `nearestWalkable` lookups (`tests/golden.rs` replays the dump's own RNG to rebuild the sample points). Notes:
  - The world is static and built lazily on first use (`fsw_world::init()` forces it); the module-level functions mirror the TypeScript exports (`ground_at`, `places()`, `find_path`, …). A* keeps its scratch buffers per thread.
  - `world_hash()` (FNV-1a over the nav costs' bytes) is in `hello`; `worldHash()` in `src/world/navgrid.ts` computes the same and `navgrid.json` records it.
  - `world.test.ts`'s check that slot animations exist moves to the sim crate, which owns the animation table.

## Context

Today the whole system runs in one browser tab: the 20 Hz headless simulation (`src/sim`, `src/world`, `src/core`), the LLM brain (`src/brain`), persistence in IndexedDB (`src/persist`), the three.js renderer and the Preact HUD. One tab equals one private world, the API key sits in the browser, and the browser talks to the LLM endpoint directly (CORS workarounds fill the README). The owner wants one shared world that several authenticated people watch at the same time, each from their own camera, on a server that is cheap to run.

Decisions taken with the owner:

| Topic | Decision |
|---|---|
| Backend language | Rust (lowest memory and CPU; serde keeps the exact JSON state shape so existing browser saves import unchanged) |
| Idle policy | The world runs 24/7 at 1x whether or not anyone is watching (one game day = 24 real minutes, about 60 game days per real day) |
| Hosting | One container on a platform such as Fly.io or Railway, persistent volume, platform-terminated TLS |
| Access | Google and Apple sign-in; only e-mails on an admin-managed allowlist get in; more providers later |
| Persistence | SQLite (WAL) on the volume: zero operations, one file, tiny footprint |
| Transport | SSE server-to-client at tick rate; a small REST surface client-to-server |

## What the analysis found

- **The sim is already headless and deterministic.** `src/sim`, `src/world`, `src/core` touch no DOM, network or LLM. State is plain JSON (`src/sim/state.ts:340-359`) with a single u32 mulberry32 RNG inside it (`src/core/rng.ts`). Fixed 20 Hz tick, systems run in a fixed order (`src/sim/sim.ts:157-185`), the outside only reaches it through `Command`s applied at tick start (`src/sim/commands.ts`), and one-shot `SimEvent`s come out of each tick (`src/sim/events.ts`). Measured: 70-90k ticks/s under Bun; JSON state 50 KB at start, 330-400 KB in steady state (memory cap 140 per villager, chronicle cap 600).
- **The world is static.** Hand-authored layout (`src/world/layout.ts`, `placeDefs.ts`) plus a fixed-seed scatter: 285 hexes, 23 places, 67 slots, 3,070 scatter models, a 273x187 nav grid. It depends on two generated files that are *not* render-only: `src/generated/assetManifest.ts` (model bounds drive obstacles, clip durations drive combat timing) and `src/generated/tileHeights.ts` (water vs. ground). The renderer, camera, audio and debug overlay import `worldMap`, `groundHeight`, `places`, `scatter` and `navGrid` directly (`src/render/terrain.ts:1-2`, `scene.ts:7-8`, `camera.ts:3`, `audio/ambient.ts:5-6`, `debug.ts:2-3`), so the world layer is needed on both sides.
- **The brain is a wall-clock sidecar.** `src/brain/brain.ts` scans state every 400 ms, asks the model through a priority queue with per-key replacement, RPM cap and circuit breaker (`src/brain/llm/queue.ts`), speaks OpenAI-compatible `chat/completions` with two wire protocols and HTTP 400 self-adaptation (`src/brain/llm/client.ts`), validates answers against live state (`src/brain/tools/validate.ts`) and sends commands. Its private state (`minds`, `turns`, governor, learned server quirks) is never saved. One impurity: prompt building calls `rel()` which *creates* relationship records outside the command stream (`src/sim/util.ts:64-71`).
- **The browser owns all state.** IndexedDB `fantasy-sim-world` with stores `saves` and `settings` (`src/persist/db.ts`); autosave every 30 s, 1.5 s after `save_point` events and on tab hide (`src/persist/autosave.ts`); a Web Lock keeps a second tab from saving; the API key is stored in plain text in IndexedDB.
- **The renderer reads state for everything continuous.** `WorldView.sync` (`src/render/scene.ts:120-157`) walks `order`, `monsters`, `corpses`, `graves` every tick and hands each `ActorView` `{x, z, facing, anim, animSeq, visible, mainHand, offHand}`; `ActorView` interpolates x, z and facing between the last two ticks (`src/render/characters.ts:115-137`). Events are only for one-shots. The HUD copies a view model out of state four times a second (`src/ui/store.ts:204-229`) using sim helpers (`describeActivity`, `rungOf`, `blockAt`, `CLASSES`, `ACTIVITIES`). A little logic hides in the renderer (the "at ease" weapon rule at `scene.ts:98-103`; a grave-count quirk at `scene.ts:160` that stops drawing new stones once the yard is full).
- **Tests worth keeping.** Determinism through save/load (`src/sim/sim.test.ts:30-36`), 3-day invariants, the 200-day soak, the drama tests, the world path tests, brain tests against a fake fetch, client and parse tests, and the import-rule test. `tools/mock-llm.mjs` is a usable stand-in for a model.

## Target architecture

```
browser (per viewer)                      server (one Rust process)                  outside
┌──────────────────────────┐  SSE /api/stream  ┌────────────────────────────────┐
│ three.js renderer        │◄──────────────────┤ world task: SimState @ 20 Hz   │
│ Preact HUD               │  REST /api/*      │   commands (mpsc) ─► tick      │   OpenAI-compatible
│ own camera + selection   ├──────────────────►│   frames (broadcast) ─► SSE    │   LLM endpoint
│ speech bubbles, audio    │                   │ digest task: roster/detail 4Hz │──────────────►
│ world geometry (static)  │                   │ brain task: queue, governor    │
│ login via Google / Apple │                   │ store task: SQLite (WAL)       │   Google / Apple
└──────────────────────────┘                   │ auth: OIDC, sessions, roles    │◄─────────────►
                                               │ static files: web build        │
                                               └────────────────────────────────┘
```

Dependency rule, now enforced by the Cargo graph: `core ← world ← sim ← brain ← server`. The web client keeps `core`, `world` (render-time geometry only), `render`, `ui`, `audio`, `speech` and a new `net`; it imports nothing from the sim or brain.

### Repository layout

The Vite app stays at the repo root (least churn). A Cargo workspace lives under `server/`.

```
server/
  Cargo.toml                       workspace: core, world, sim, brain, server (binary)
  crates/core/src/{rng,time,math2d,jsmath}.rs
  crates/world/src/{hex,layout,tiles,height,map,place_defs,places,scatter,navgrid,pathfind}.rs
  crates/world/tests/golden/*.json fixtures dumped from the TS world layer
  crates/sim/src/{state,create,commands,events,sim,util,describe}.rs
  crates/sim/src/data/{cast,classes,monsters,activities,smalltalk,anims}.rs
  crates/sim/src/systems/{director,death,perception,routine,monsters,needs,relationships,arrivals,combat,intents,conversation,movement}.rs
  crates/brain/src/{brain,governor,prompts,retrieval}.rs + llm/{client,queue,parse,probe,types}.rs + tools/{definitions,validate}.rs
  crates/server/src/{main,config}.rs + world_task.rs digest.rs
  crates/server/src/store/{db,migrations,snapshots,chronicle,brain_log,settings,users}.rs
  crates/server/src/auth/{provider,google,apple,session,allowlist,middleware}.rs
  crates/server/src/api/{router,stream,characters,chronicle,admin,wire}.rs   (wire = ts-rs exported types)
  crates/server/src/bin/{soak,probe}.rs
  data/asset-manifest.json, data/tile-heights.json   (emitted by tools/assets/build-assets.mjs, include_str!)
src/                 web client: core, world, render, ui, audio, speech, net, generated (api.ts from ts-rs)
tools/               build-assets.mjs (also writes server/data/*.json), mock-llm.mjs, dump-world.ts, ref-server.ts
Dockerfile, fly.toml, .github/workflows/ci.yml
```

Leaves the client at the end: `src/sim`, `src/brain`, `src/persist`, `src/app`. `src/sim/data/anims.ts` moves to `src/render/anims.ts`. `src/world` stays for terrain, scenery placement, camera bounds and `groundHeight`; the Rust port of the same layer drives navigation, and a `worldHash` in the SSE `hello` plus the golden fixtures guarantee both agree. (The layout tables can later become shared JSON read by both sides; not part of this migration.)

### Server runtime

- **World task** owns `SimState` behind `Arc<parking_lot::RwLock<_>>`; only this task holds the write side (a `SharedState` newtype exposes `read()` only, so the brain cannot mutate; `rel()` is ported as a read-only lookup returning a default, with `rel_mut` private to the sim crate). Hand-rolled schedule on `tokio::time`: sleep until the next 50 ms deadline, compute the ticks owed, run at most 12 per wake; if the backlog exceeds 200 ticks (10 s, a host pause or deploy) rebase the clock and log a `clock_skip` instead of fast-forwarding (same policy as the hidden-tab rule in `src/app/loop.ts:23-27`). Commands drain from an `mpsc` channel at the start of each tick, FIFO, exactly like `Sim.pending`.
- **Frame production** happens on the world task after releasing the lock: diff the quantised dynamic fields of every entity against a tracker, encode one `tick` frame per tick, publish one `broadcast` message per wake (one tick normally, up to 12 during catch-up). SSE tasks forward bytes and never touch the lock. A `watch` channel carries `tick`, `speed`, `bootId` for anyone who needs them.
- **Digest task** at 4 Hz computes roster, status and the inspector `detail` for each *watched* character once, hashes, and publishes only on change. Twenty viewers watching the same villager share one computation.
- **Brain task**: same algorithm as `brain.ts`, a 400 ms scan interval, events from the broadcast, per-job `tokio::spawn` with a `CancellationToken` per key (`c:<id>`, `v:<convoId>`), short read locks to scan and build prompts, awaits outside locks, `try_send` commands. `clippy::await_holding_lock` is `deny` in the server crate.
- **Store task** owns the single SQLite write connection on a blocking thread, fed by an `mpsc<StoreOp>`; reads use `spawn_blocking` connections. Snapshots: clone state under a read lock (about 1 ms), serialise + zstd + insert off-lock.
- **Speed** is global and admin-only: 0, 1, 2, 4. The "hold at 1x while the model is backlogged" rule from `src/main.ts:84-87` moves here.
- **Shutdown**: SIGTERM stops ticking, writes a snapshot (`reason='shutdown'`), checkpoints the WAL, exits. Platform kill timeout set to 30 s.

### Wire protocol

All wire types are Rust structs exported by `ts-rs` to `src/generated/api.ts`; CI fails if the committed file differs.

- `GET /api/stream?viewer=<uuid>` (SSE, cookie-authenticated, `retry: 500`, keep-alive comment every 15 s, `Cache-Control: no-cache`, `X-Accel-Buffering: no`, no compression). Event ids are `"<bootId>:<tick>"` so a restart that resumes from an older snapshot cannot confuse replay.
  - `hello` `{bootId, tick, speed, protocolVersion, worldHash, viewers, role}`
  - `snapshot` (id = current tick): clock, speed, every live entity as `static` `{id, kind: villager|monster|corpse, name, look, maxHp, hostile, voice}` + `dyn`, roster rows, chronicle tail (120), graves keyed `plot:characterId`, alarm, LLM status, recent voices
  - `tick` (one per sim tick): `{t, add: [static+dyn], del: [ids], d: [{id, ...changed dyn fields}], ev: [events]}`. `dyn` = `{p: [x*100, z*100] as ints, f: facing*100, a: [anim, animSeq], hp: rounded, v: visible, mh, oh, act: activityId|null, z: asleep}`; the "at ease" hand rule is applied server-side; diffs are computed on the quantised values so an idle villager contributes nothing. Expected 4 KB/s per viewer by day, up to 14 KB/s in a night wave, under 1 KB/s while everyone sleeps. `speech` events carry the full `Utterance` shape (`src/speech/speechBus.ts:7-22` minus `id`) so the speech bus needs no state. Ambient audio stays client-side (it depends on the camera) and takes its counts (`awake`, people eating/drinking, smithing, relaxing) from `status`.
  - `status` (on change, at most 1 Hz): `BrainStatus` (`brain.ts:30-40`), viewers, speed, population, monsters, alarm, clock part, ambient counts
  - `detail`: `CharacterDetail` (`src/ui/store.ts:34-57`) with *rendered strings* (status line, class label, plan rows with `current`, rung labels) because the client no longer has the sim helpers; pushed for the character this viewer watches
  - `brainlog`: `BrainLogRecord`s, admin role only
  - `world_reset`: sent on new world or import; the client reloads the page (cheapest correct way to rebuild `WorldView`)
  - Reconnect: `Last-Event-ID` with the same `bootId` within a 100-tick ring buffer replays the gap; anything else gets a fresh `snapshot`. A non-2xx closes `EventSource`; the client then calls `/api/me` and goes to `/login`. `protocolVersion` mismatch reloads once (sessionStorage guard).
- `POST /api/watch {viewer, id|null}`: which character's `detail` this viewer receives; keyed by `(session, viewer)` with a TTL so it survives reconnects.
- `GET /api/characters/{id}` (first paint of the inspector), `GET /api/chronicle?before=<tick>&limit=` (history from SQLite), `GET /api/me`, `POST /api/logout`, `GET /healthz` (fails when the last tick is older than 5 s).
- Admin: `POST /api/admin/speed`, `GET/PUT /api/admin/settings/llm` (key write-only, returned masked), `POST /api/admin/llm/test`, `GET /api/admin/llm/models`, `GET /api/admin/brain-log`, `GET/POST/DELETE /api/admin/allowlist`, `GET /api/admin/attempts` (refused logins, with an Allow button), `POST /api/admin/world/new {seed?}`, `POST /api/admin/world/import`, `GET /api/admin/world/export`, `GET /api/admin/health` (tick jitter p50/p99, backlog drops, viewers, DB size, snapshot age, LLM usage).
- Auth: `GET /auth/{provider}/start?return_to=`, `GET|POST /auth/{provider}/callback`, `GET /no-access`.
- Static: the Vite build with SPA fallback, `index.html` no-cache, hashed assets and `public/assets` immutable; gzip/brotli for static only.

### Client-side playback

`src/net/stream.ts` (EventSource, reconnect, ids), `src/net/viewWorld.ts` (entity map holding exactly the `ActorSnapshot` fields plus names, maxHp, hostile), `src/net/api.ts` (typed fetch). A jitter buffer keeps `playTick` two ticks behind the newest frame, advances it by `dt * 20 * speed` each render frame, and on each whole tick applies that tick's deltas, calls `actor.view.sync()` for **every** live actor (as `scene.ts:120-141` does today, so `prev`/`next` never go stale), then dispatches the frame's events to overlay, fx, speech bus and audio. `alpha = frac(playTick)`. Empty buffer: hold `alpha` at 1. Backlog over four frames: apply all but the last two at once (speech and chronicle still dispatched, fx dropped). Depth drift: play at 1.05x or 0.95x for a second. Hidden tab for over 30 s: close the stream, resync with a snapshot on return.

### Persistence (SQLite)

Pragmas: WAL, `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=5000`, `auto_vacuum=INCREMENTAL`, `journal_size_limit=64MB`. Tables:

- `schema_migrations(version, applied_at)`
- `worlds(id, name, seed, created_at, is_active)` with a unique partial index on `is_active = 1`
- `snapshots(id, world_id, tick, game_day, reason, tier, save_version, encoding, size_raw, body BLOB, created_at)`: zstd-compressed `SimState` JSON plus a small sidecar of brain state. Tiers and retention: 0 interval (30 s, keep 20), 1 save_point (keep 24 h), 2 first save of each game day (keep 14 days), 3 first tier-2 after 00:00 UTC each real day (keep forever). Estimated 105 MB after 30 days, 140 MB after a year.
- `commands(id, world_id, tick, seq, source, body, created_at)`: every applied command, kept 48 h; snapshot + commands replays any recent tick exactly.
- `chronicle(id, world_id, tick, kind, text, ids)`: append-only full history, batched one transaction per second.
- `brain_log(id, world_id, tick, created_at, character_id, kind, status, latency_ms, prompt_tokens, completion_tokens, summary, prompt BLOB, response BLOB, commands)`: full prompt/response for the newest 5,000 rows, metadata 30 days.
- `brain_usage(day, kind, requests, failures, tokens)`: the daily cap survives restarts.
- `settings(key, value, updated_at)`: `llm` (without the key), `llm.api_key` (never serialised to clients), `llm.capabilities` (learned 400-adaptations, so the dance does not repeat on each boot), `sim` (speed).
- `users(id, email, display_name, role, created_at, last_login_at)`, `identities(provider, subject, user_id, email_at_link, created_at)`, `allowlist(email, role, added_by, added_at, note)`, `sessions(id = sha256(token), user_id, created_at, expires_at, last_seen_at, user_agent)`, `auth_attempts(id, provider, email, outcome, at)`.

In-flight OIDC flows live in memory with a 10-minute TTL. Boot loads the newest snapshot of the active world or creates one from a random seed. Import accepts the browser export `{version, savedAt, state}` through the same serde path (`#[serde(default)]` on optional fields; `Intent` modelled as `IntentAction` flattened with `source/say/thought`); export emits the same envelope. Backup: nightly `VACUUM INTO /data/backup/latest.db`, plus Litestream to an S3-compatible bucket (Tigris on Fly, or R2) run as the container entrypoint (`litestream replicate -exec`), recommended and configured in phase 6.

### Authentication and access

- Crates: `oauth2` (code flow, PKCE, state) + `jsonwebtoken` (JWKS verification, Apple's ES256 client secret). Not `openidconnect` (heavy, strict parsing trips on Apple's string-typed `email_verified`). One crypto backend (`aws-lc-rs`) across rustls, reqwest and jsonwebtoken. Providers are an enum with `authorize_url()` and `exchange() -> Identity {provider, subject, email, email_verified, name}`; enabled by `AUTH_PROVIDERS=google,apple`.
- Flow: `/auth/{p}/start` stores `{state, nonce, pkce, return_to}` under a `flowId` and sets `__Secure-auth_flow=<flowId>; Path=/auth/; Secure; HttpOnly; SameSite=None; Max-Age=600` (None because Apple's `response_mode=form_post` callback is a cross-site POST). The callback checks state, exchanges the code with the verifier, verifies the id_token (JWKS by `kid` with refetch on unknown, `iss`, `aud`, `exp`, `nonce`, lenient `email_verified`), then resolves: existing `identities(provider, subject)` → that user; else lowercase e-mail on `allowlist` → create user with that role and link the identity; else record an `auth_attempts` row and redirect to `/no-access?email=` (the admin panel lists attempts with an Allow button, which also handles Apple private-relay addresses).
- Session: 32 random bytes, base64url in `__Host-session; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=30d` (sliding, 90-day absolute cap), sha256 in `sessions`. Plain cookie name when `PUBLIC_ORIGIN` is `http://` in local dev. Removing an e-mail from the allowlist revokes its sessions; SSE handlers re-check the session every 60 s.
- Apple: Services ID as `client_id`, `scope=name email`, client secret JWT `{iss: TeamID, sub: ServicesID, aud: https://appleid.apple.com}` signed ES256 with the `.p8`, minted for 30 days and re-minted when under a day remains (Apple caps it at 6 months). The e-mail claim comes on every sign-in; the name only on the first. Apple refuses `localhost` return URLs, so Apple is tested only on the deployed domain. Google: standard discovery, `scope=openid email profile`, both issuer spellings accepted.
- CSRF for mutating routes: `SameSite=Lax` plus middleware requiring `Sec-Fetch-Site ∈ {same-origin, none}`, `Origin == PUBLIC_ORIGIN` when present, and `Content-Type: application/json`. Per-IP rate limit on `/auth/*`. `ADMIN_EMAILS` seeds the allowlist with role `admin` at boot (idempotent).
- Development: `AUTH_MODE=token` (a shared bearer token) for local work and the first deployment; never deploy with auth off.

### Brain port notes

- One-to-one port of `brain.ts`, `governor.ts`, `queue.ts` (BinaryHeap by priority then expiry, per-key replacement, 60 s sliding RPM window, `Semaphore` for concurrency, circuit breaker, `CancellationToken` cancel by key prefix), `client.ts` (both wire protocols, the 400-adaptation order, warm-up timeout, `reasoning_effort`, truncation retry with `tokenScale`), `parse.ts` (fuzzed with `cargo fuzz`), `validate.ts`, `prompts.ts`, `retrieval.ts`.
- Transport behind a `trait LlmTransport` so tests use a fake; `tools/mock-llm.mjs` stays for end-to-end runs.
- New: `maxRequestsPerDay` and `maxTokensPerDay`, spread through the day (budget = cap × fraction of day elapsed + a small burst) so the village does not go mute in the evening; LLM disabled on a fresh deploy; a kill switch and 24 h counters in the admin panel.
- Restart behaviour: treat `c.planSource == 'llm' && c.planDay == today` (already in sim state) as "planned", so a deploy does not re-ask every villager for a plan; `turns` are rebuilt from `conversation.lines`.
- Brain log rows also record the commands each answer produced (a gap in today's log).

### Frontend changes

- `WorldView.sync` takes the `ViewWorld`; graves are keyed `plot:characterId` (fixes the full-yard quirk); the at-ease rule and `CLASSES`/`MONSTERS` lookups go away (maxHp, name, hostile come in `static`).
- `SpeechBus.handle(utterances)` becomes a pure consumer; `audio/ambient.ts` takes counts from `status` and keeps its world-cell maths.
- `ui/store.ts` signals fill from `snapshot`, `tick`, `status`, `detail`; no sim imports. Speed buttons render for admins only. Settings splits into Audio (per viewer, `localStorage`) and Admin (LLM, allowlist, attempts, world, speed, health). New `Login.tsx`, `NoAccess.tsx`, routing by `/api/me`.
- `tests/architecture.test.ts` rewritten: `core ← world ← {render, audio}`, `net ← generated`, `three` only in `render`, nothing imports `sim`/`brain`/`persist`.

## Porting rules for fidelity

Bit-exact parity between the TS and Rust sims is **not** an acceptance criterion: V8 and JavaScriptCore already differ from each other in `Math.hypot`, `sin`, `cos`, `atan2`, and Rust's std calls the platform libm. Acceptance is (a) the Rust sim is deterministic with itself across x86_64 and aarch64, (b) the ported invariant tests pass, (c) soak statistics match the TS sim within ±25% over 10 seeds × 30 days, (d) the world golden fixtures match exactly. A per-tick trace diff (`tick, rng, positions to 1e-6, intent kinds`) is the porting tool: the first divergence pinpoints a bug, and `rng` is the canary since a different number of draws is always a logic error.

Rules, each a known divergence source:

- `core::jsmath`: `js_round(x) = (x + 0.5).floor()` (JS rounds half toward +∞); `js_hypot` as V8 does it (scale by max, Kahan sum); `sin/cos/atan2/exp/pow` from the `libm` crate so results are identical on every machine.
- mulberry32 in `u32` with `wrapping_add`/`wrapping_mul`, `(t ^ (t >> 14)) as f64 / 4294967296.0`; `hashString`/`chemistry` as FNV-1a over UTF-16 code units with `wrapping_mul(16777619)`.
- A* (`src/world/pathfind.ts`): `cost: Vec<f32>`; compute tentative `g` in f64 from widened f32 values, store back as f32, heap key is the unrounded f64; port the heap's `<=`/`<` choices, `nearestWalkable`'s ring order and `smooth()`'s 40-cell look-ahead verbatim, because tie-breaks decide paths.
- `IndexMap` (with `shift_remove`, never `swap_remove`) for `characters`, `monsters`, `conversations`, `dead`, `slots`, `relationships`; stable `sort_by` everywhere, comparators mirrored exactly.
- String lengths and caps (`240`, `600`, `200`, `speechTicks`) in UTF-16 code units via `encode_utf16()`, never splitting a surrogate pair.
- `structuredClone` → `Clone`; `atob` tile heights decoded once at startup; `Number(id.slice(1))` and `charCodeAt(len-1) % 3` reproduced.
- `[profile.dev.package.sim] opt-level = 3` so the 200-day soak (5.76 M ticks) stays fast in tests.

## Phases

Each phase ends with tests green and something runnable. Tracks B (frontend) and C (auth, LLM client) run in parallel with the sim port.

**Phase 0: artefacts and scaffold (while the TS code is still the trusted reference).**
- `tools/dump-world.ts` writes `server/crates/world/tests/golden/{map,ground,navgrid,places,scatter,paths}.json` (terrain and masks per hex, `groundAt` on a 0.5-unit grid, nav costs as f32 bit patterns, all places with doors/slots/obstacles/grave plots/spawn points, scatter in RNG order, raw and smoothed paths for 300 door/slot pairs). `tools/soak.ts --json` and a `--trace` flag for the per-tick trace. One real browser export committed as `server/crates/sim/tests/fixtures/export-day-N.json`. Tag the commit `ts-sim-reference`.
- `server/` workspace, config from env, `/healthz`, static serving, `tracing` JSON logs, Vite dev proxy for `/api` and `/auth`.
- Hand-write the wire types in Rust first (`api/wire.rs`), export with ts-rs, commit `src/generated/api.ts`.
- `tools/assets/build-assets.mjs` also emits `server/data/asset-manifest.json` and `server/data/tile-heights.json`; regenerate and commit.
- Deploy a hello-world SSE ticker behind the real platform proxy and confirm: headers flush immediately, HTTP/2 to the browser, no buffering, keep-alive survives the idle timeout, behaviour across a deploy.
- CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` (x86_64 and aarch64), `pnpm typecheck`, `pnpm test`, ts-rs output diff.

**Phase 1: core and world in Rust.** Port `src/core/*` and `src/world/*` against the golden fixtures (exact for categorical data and nav costs, 1e-9 for coordinates, exact paths). Port `hex.test.ts` and `world.test.ts`.

**Phase 2: sim in Rust.** Port `state`, `create`, `data/*`, `util`, `describe`, `commands`, `events`, `sim` and the twelve systems in tick order, under the porting rules above and with `serde(rename_all = "camelCase")` so the export fixture deserialises. Port `sim.test.ts` (determinism through a JSON round trip, 3-day invariants, 200-day soak as `#[ignore]` plus a 20-day CI variant) and `drama.test.ts`. `cargo run --bin soak -- 30 1337 --log|--json|--trace`. Gate: trace parity with the TS sim for at least one game day on a fixed seed (ratchet the constant up as bugs are fixed), soak statistics within ±25%.

**Track B (parallel with 1-2): thin client against a Node reference server.** `tools/ref-server.ts` wraps the existing TS `Sim` with the delta encoder and SSE on Node `http` (a day of work). Build `src/net/`, the jitter buffer, the `WorldView`/speech/audio/HUD rewrite against it. Switching to Rust later is a URL change, and the reference server doubles as a protocol oracle (same frames from both backends for the same command stream).

**Track C (parallel from phase 0): auth and the LLM client.** Auth touches nothing in the sim: providers, sessions, allowlist, middleware, login and no-access pages. `brain/llm/{client,queue,parse}` have no sim dependency either; `client.test.ts` and `parse.test.ts` port one to one.

**Phase 3: runtime, store and the live milestone.** World task, digest task, broadcast fan-out, SSE handler, SQLite store (snapshots, commands, chronicle, settings), autosave, shutdown snapshot, resume on boot, admin speed and world endpoints. Point the track-B client at it. Deploy to the platform behind `AUTH_MODE=token`. Milestone: two browsers watch the same world from different cameras with bubbles, labels, effects, inspector and chronicle; kill and restart the server and the world resumes within one tick.

**Phase 4: brain.** Prompts, validate, retrieval, governor, orchestrator; golden prompt tests ported from `brain.test.ts`; admin LLM settings, probe and model list; `brainlog` stream; daily caps; `cargo run --bin probe`. Milestone: the deployed server runs against `tools/mock-llm.mjs`, then a real endpoint, with the decision log visible to the admin.

**Phase 5: real login.** Switch the deployment from token auth to Google and Apple; allowlist and attempts in the admin panel; `ADMIN_EMAILS` bootstrap. Milestone: a non-allowlisted Google account is refused and shows up in attempts; the admin allows it; it watches; the admin changes speed and everyone sees it.

**Phase 6: deployment hardening.** Multi-stage `Dockerfile` with `cargo-chef` layering (node for the web build, rust builder, `debian:bookworm-slim` runtime), `fly.toml` (shared-cpu-1x, 512 MB, 3 GB volume at `/data`, `auto_stop_machines = "off"`, `min_machines_running = 1`, `kill_timeout = 30`), secrets (`SESSION_SECRET`, `GOOGLE_*`, `APPLE_*`, optional `LLM_API_KEY` seed), Litestream entrypoint, nightly `VACUUM INTO`, `/api/admin/health`, README and the design spec updated with the server design and wire protocol.

**Phase 7: cleanup.** Delete `src/sim`, `src/brain`, `src/persist`, `src/app`, `tools/soak.ts`, `tools/llm-probe.ts` (keep `dump-world.ts` runnable from the `ts-sim-reference` tag), the `/llm-proxy` dev proxy, `idb` and `fake-indexeddb`; new architecture test; `tools/mock-llm.mjs` stays.

## Risks and mitigations

- **SSE through the platform proxy** (buffering, idle timeouts, HTTP/1.1 to the browser): proven in phase 0 before anything depends on it.
- **Deploy restarts stop the world for 10-30 s**: shutdown snapshot, resume, `bootId` ids, client reconnect with snapshot fallback, `kill_timeout = 30`, Litestream handles shutdown ordering.
- **Floating-point drift between TS and Rust**: not a goal; self-determinism across architectures via `libm`, trace diff as the debugging tool, statistical parity as the gate.
- **LLM spend around the clock** (today's cadence is roughly 20k requests a day): daily caps spread through the day, disabled on a fresh deploy, kill switch, 24 h counters.
- **Old-save compatibility**: the committed export fixture is a permanent test; `#[serde(default)]` on fields added later; `SAVE_VERSION` migrations in Rust.
- **Protocol drift between cached bundles and a new server**: `protocolVersion` with a single guarded reload; `index.html` no-cache.
- **A panic in the world task takes the world down**: every command is validated server-side (admin import is a second producer now), brain jobs are isolated tasks, parse fallbacks are fuzzed.
- **Lock held across an await** (deadlocks the tick): `clippy::await_holding_lock = deny`.
- **Apple cannot be tested locally**: Google is tested on `localhost`, Apple only on the deployed staging URL; the provider enum keeps Apple optional so Google alone works.

## Resource budget

Typical RSS 50-80 MB (binary and runtime about 15 MB, state 2-4 MB, world 1 MB, SQLite cache 8 MB, TLS roots and HTTP client 5 MB, zstd 1 MB; Litestream adds about 40 MB in the same container). CPU under 2% of a shared vCPU with 20 viewers (ticks 0.1%, frame encoding 0.1%, digest 0.4%, SSE 0.4%, snapshots 10 ms every 30 s). Per viewer about 100 KB memory and 2-15 KB/s egress. Database about 180 MB after 30 days and 430 MB after a year with the retention above.

## Verification

- `cd server && cargo test` (unit, golden world, 20-day soak, tick-loop tests using `tokio::time::pause()`/`advance()` for bounded catch-up and `clock_skip`) and `cargo test -- --ignored` for the 200-day soak with the assertions from `src/sim/sim.test.ts:60-95`. Run in CI on x86_64 and aarch64; the determinism test must agree across both.
- `pnpm test` and `pnpm typecheck` for the client, including the rewritten architecture rules; ts-rs output diff in CI.
- Parity: `bun tools/soak.ts 30 <seed> --json` versus `cargo run --bin soak -- 30 <seed> --json` over 10 seeds, deltas within ±25%; `--trace` diff for the ratcheted number of identical ticks.
- Local end to end: `node tools/mock-llm.mjs`, `cargo run` with `AUTH_MODE=token`, `pnpm dev`, two browser windows: bubbles, effects, inspector, chronicle, brain log; SIGTERM and restart resumes within one tick; a Playwright smoke test (Chromium is installed) opens two pages and asserts both receive the same tick ids and the same roster.
- Reconnect: devtools offline for 3 s (ring replay) and for 30 s (snapshot fallback); hidden tab for a minute resyncs on return.
- Auth: Google against `http://localhost:5173`; Apple on the deployed domain; unlisted account lands on `/no-access` and appears in attempts; after Allow the next login works; `curl /api/stream` without a cookie gets 401; a cross-site form POST to an admin route is rejected.
- Import: export from the current browser build, import through the admin panel, villagers, graves and chronicle match; export from the server re-imports.
- Resources: `docker stats` on the container (target under 80 MB RSS with a few viewers) and bytes per second per viewer from the SSE stream (target under 10 KB/s by day at 20 Hz).
