# AGENTS.md: the Rust server

The repository-wide rules are in [../AGENTS.md](../AGENTS.md). This file is about the Cargo workspace in `server/`. Run every `cargo` command from here.

```bash
cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked
```

## Crates

| Crate (directory) | What it holds | Depends on |
| --- | --- | --- |
| `fsw-core` (`crates/core`) | `jsmath`, the seeded RNG, game time, 2D maths | nothing of ours |
| `fsw-world` (`crates/world`) | The static world: hex map, places (with their building pads), scatter, nav grid, A* | core |
| `fsw-sim` (`crates/sim`) | The 20 Hz simulation: state, commands, events, the systems, the nav overlay for a world that changes | core, world |
| `fsw-brain` (`crates/brain`) | The model client, queue, governor, prompts, retrieval, tools, validation | core, world, sim |
| `fsw-server` (`crates/server`) | HTTP (axum), the event stream, guest seats and the admin's token, the SQLite store, the tasks that run it all | all of the above |
| `fsw-testing` (`crates/testing`) | The hand-written cast of the TypeScript game, for tests only (`create_test_state`, `admit`) | sim, world |

Keep that order. The sim must not depend on the brain or the server, and the brain must not drive the sim except through `Command`s. The crates are named `fsw-*` because a crate called `core` would shadow Rust's own.

Binaries: `fsw-server` (the default), `soak` (`-p fsw-sim`, a headless run: `cargo run --release -p fsw-sim --bin soak -- <days> <seed> [--log|--json|--trace]`), `probe` (`-p fsw-brain`, measures a model endpoint), `export-api` and `export-frames` (rewrite the committed files the client tests read).

## How the server runs

- **The world task** (`world_task.rs`) is the only thing that advances the sim. It wakes every 50 ms, runs the ticks owed (at most 12 a wake, and skips ahead past 10 s owed), applies queued commands at tick boundaries, encodes each tick once (`encode.rs`) into a 100-tick replay ring and a broadcast channel, and asks the store for snapshots (every 30 s, after save points, on shutdown).
- **The digest task** (`digest.rs`) computes the roster, the status line and the inspector's detail for watched villagers, at most four times a second, once per villager however many people watch.
- **The brain task** (`brain_task.rs`) feeds the brain each tick's events and returns its decisions as commands through the world's input queue. Its decision log goes to the store and to admins.
- **The casting task** (`casting_task.rs`) founds a new world: one request to the model per villager (`fsw_brain::casting`), by a client of its own outside the brain's queue, each answer sent to the sim as `Command::Settle`; without a model, or after three failures running, it sends `Command::FoundVillage` and the sim invents the rest from `data/names.rs`. It answers the sim's `NewcomerWanted` event the same way (`NewcomerDraft` or `NewcomerLocal`).
- **The store** (`store/`) has one thread that owns the only write connection; everything else sends it `StoreOp`s. Reads open a short-lived connection in `spawn_blocking`.
- **Config** (`config.rs`) reads every environment variable once at start and refuses values it cannot use. A new setting gets a parse with a clear error and a test there, a row in the README's settings table, and a line in `docs/deploy.md` if it matters in production.

## Keeping the sim deterministic

A given seed and command stream must produce the same world on every machine (x86_64 and aarch64). The port from TypeScript set these rules; new code follows them too:

- Use `fsw_core::jsmath` (`sin`, `cos`, `atan2`, `exp`, `pow`, `log`, `js_hypot`, `js_round`), which call `libm`, never `f64::sin` and friends from std, whose results depend on the platform.
- Draw randomness only from the sim's RNG through the `Ctx` helpers. No `rand`, no clocks, no `HashMap` iteration order in anything that affects the world.
- Ordered maps are `IndexMap`, removed from with `shift_remove`, never `swap_remove`. Sorts are stable (`sort_by`).
- Text lengths and caps count UTF-16 code units (`encode_utf16()`) and never split a surrogate pair, as the saves and prompts expect.
- Systems reach characters by id through `Ctx` (`ctx.c(id)`). `rel()` creates relationship records and is for the sim only; readers outside it use `rel_or_default` and `lethal_unlocked_read`, which never write.
- `Sim::step` runs commands, then the day roll-over (relationships, ceremonies, then children's birthdays and births), then the systems in a fixed order (weather, director, routine, quirks, children, intents, music, conversations, monsters, ghosts, animals, fire, ceremonies, world, economy, perception, blocking, separation, needs). Changing that order changes every world.
- The weather's schedule is a hash of the seed and the tick (`systems::weather::roll`), never the RNG, so a seed has the same weather whatever its villagers do.
- `SimState.objects` (what differs from the map) is changed only through `put_object` and `drop_object` in `systems/world.rs`, which refresh `SimState.nav`, the walkability overlay. The overlay is a cache: `#[serde(skip)]`, equal to every other overlay, rebuilt on load, and `None` while nothing has changed, so an untouched world keeps the static grid. Every path and walkability check in the sim goes through it (`nav::*`, `NavOverlay`), never through `fsw_world::navgrid` directly.

The tests that hold this in place: `crates/sim/tests/sim.rs` (determinism through a save round trip, invariants, a 20-day run, the 200-day soak under `--ignored`), `drama.rs`, and `save_fixtures.rs`.

## Formats that must not drift

- **Saves**: `SimState` serialises to the exact JSON shape of the old browser saves, so `crates/sim/tests/fixtures/export-day-*.json` must keep loading, and server exports stay importable. A breaking change needs a new `SAVE_VERSION` (`crates/sim/src/state.rs`) and a way to read the old one. Fields added since (`objects`, `stockpiles`, `projects`, `carried`, `economy`, `loot`, `gold`, a character's `child`, `animals`, `fauna`, a character's `mount`, `war_band` and `plundered`, a monster's `errand`, …) are `#[serde(default, skip_serializing_if = …)]`, so they vanish from a save when empty; follow that for new ones.
- **Wire types** live in `crates/server/src/api/wire.rs` (camelCase on the wire, exported with ts-rs). After a change, run `cargo run --bin export-api` and commit `src/generated/api.ts` with it; a test fails otherwise. Bump `PROTOCOL_VERSION` when an older client cannot read the new shape.
- **Stream frames**: after changing `encode.rs`, run `cargo run --bin export-frames` and commit `tests/fixtures/server-frames.json` (at the repository root), which the client's tests replay.
- **The world**: `crates/world/tests/golden/*.json` come from the TypeScript world (`bun tools/dump-world.ts`, from the repository root). Nav costs and paths must match exactly. A layout change goes into `src/world` and `crates/world` together.
- **Prompts**: `crates/brain/tests/prompts.rs` checks every prompt word for word against `crates/brain/tests/fixtures/prompts.json`. A deliberate prompt change updates the expected text in that fixture in the same commit.
- **Embedded data**: `server/data/*.json` (asset bounds, clip durations, tile heights) are compiled in with `include_str!`, generated from `src/generated` (see the root AGENTS.md). The hand-written cast of the TypeScript game lives in `crates/testing/data/cast.json` and is compiled into the test helper crate only; tests that know villagers by name start from `fsw_testing::create_test_state(seed)`, which gives the world `create_state(seed)` always gave.
- **The database**: `store/migrations.rs` holds numbered migrations applied once each. Add the next number; never edit one that has shipped. Times are milliseconds since the Unix epoch.

## The language model

- Everything about the model comes from `LLM_*` variables (`config.rs`, `crates/brain/src/settings.rs`). There is no route that changes it: `GET /api/admin/settings/llm` shows it (the key as a hint only) and `POST /api/admin/llm/test` tries it.
- The brain must never block the world. Answers are validated against the world as it is when they arrive (`validate.rs`) and anything stale or invalid falls back to the routines.
- Casting (`casting.rs`) is the one path that calls the model outside the queue: the founding of a world and its newcomers, with a closed tool schema, every text cleaned and cut to size, first names checked against the living, the dead and the old cast, and no murderous or occultist quirks from outside.
- Tests use a fake transport (`crates/brain/tests/brain.rs`). For an end-to-end run use `node tools/mock-llm.mjs` (port 8787) and `cargo test -p fsw-brain --test mock_llm -- --ignored`. Do not point tests or servers at a real endpoint unless asked.

## Access and HTTP

- Routes are in `crates/server/src/api/router.rs`: `/auth/*` (the admin's token form and the guest seats, rate-limited), `/api/*` (CSRF check, then the viewer: an admin by token or a guest by seat), `/api/admin/*` (admins only), `/healthz`, and the web build as static files.
- Tests: `crates/server/tests/http.rs` for the routes, the token, the seats and the line. Add a test there for a new route or a change to who may call one.
- Run the server for a manual check with a scratch `DATA_DIR` so the developer's `data/world.db` stays untouched.
