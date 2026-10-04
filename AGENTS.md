# AGENTS.md

Guidance for coding agents (and people) working in this repository. [README.md](README.md) covers running the game; this file covers the rules a change has to keep. There is more in [server/AGENTS.md](server/AGENTS.md) for the Rust server and [src/AGENTS.md](src/AGENTS.md) for the web client.

## The shape of the project

One shared village runs on a Rust server; browsers only watch it.

- `server/` is a Cargo workspace and owns everything that changes: the simulation (`fsw-sim`), the language-model brain (`fsw-brain`), the SQLite store, sign-in and the event stream (`fsw-server`). `fsw-core` and `fsw-world` hold the maths and the static world.
- `src/` is the web client (TypeScript, three.js, Preact). It draws the server's stream (`GET /api/stream`, server-sent events) and calls a few REST routes. It never simulates, never calls a model and never saves. Viewers reach the world only by spending credits (`POST /api/credits/buy`), which the server checks and turns into a `Command`.
- Every type that crosses the wire is a Rust struct in `server/crates/server/src/api/wire.rs`, exported to `src/generated/api.ts`.
- The TypeScript simulation and brain the Rust port was checked against live only at the `ts-sim-reference` tag. Comments in the crates that name `src/sim/…`, `src/brain/…` or `tools/…` files that no longer exist refer to that tag. Do not bring those folders back to `src/`.

Design and history: [the design](docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md) (read its "Since the server migration" section first) and [the migration plan](docs/superpowers/specs/2026-10-02-backend-migration-plan.md) (its "Progress" section records what was built, measured and left open).

## Commands

Run these before calling a change done; CI runs exactly these on every push.

```bash
pnpm install
pnpm typecheck && pnpm test                     # the client (vitest)
cd server && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked
```

- `cargo test` takes a while the first time: the sim and world crates build at `opt-level = 3` even in debug so the long soak tests stay fast. `cargo test -- --ignored` adds the 200-day soak and a test that needs `node tools/mock-llm.mjs` running.
- When piping test output (`pnpm test | tail`), use `set -o pipefail`, or a failure looks like a pass.
- A server for manual checks: `pnpm build`, then in `server/` `AUTH_MODE=off PUBLIC_ORIGIN=http://localhost:8080 STATIC_DIR=../dist DATA_DIR=<a scratch dir> cargo run`. Use a scratch `DATA_DIR` so you do not touch the developer's `server/data/world.db`. With `pnpm dev` on :5173 next to it, leave `PUBLIC_ORIGIN` unset instead.
- For the model path without a real model: `node tools/mock-llm.mjs`, then start the server with `LLM_BASE_URL=http://localhost:8787/v1 LLM_MODEL=mock`.
- Do not start servers that talk to the developer's own model endpoints (LM Studio, Ollama) unless asked; they may not be running.

## Generated and fixture files

These are written by tools and checked by tests. Regenerate them; never edit them by hand.

| File | Written by | Checked by |
| --- | --- | --- |
| `src/generated/api.ts` | `cargo run --bin export-api` (in `server/`) | a `fsw-server` test |
| `tests/fixtures/server-frames.json` | `cargo run --bin export-frames` (in `server/`) | a `fsw-server` test, and `tests/net.test.ts` replays it |
| `src/generated/assetManifest.ts`, `src/generated/tileHeights.ts`, `server/data/*.json` | `pnpm assets` (needs the KayKit collection); `bun tools/assets/export-server-data.ts` rewrites only `server/data` | `tests/serverData.test.ts` |
| `server/crates/world/tests/golden/*.json` | `bun tools/dump-world.ts` (from the TypeScript world in `src/world`) | `server/crates/world/tests/golden.rs` |
| `public/audio/*` | `pnpm audio` (from `tools/audio/sources.ts`; needs Bun, ffmpeg and bsdtar) | |

Fixtures from the TypeScript reference that cannot be regenerated on this branch: `server/crates/sim/tests/fixtures/export-day-*.json` (browser saves that must keep loading) and `server/crates/brain/tests/fixtures/prompts.json` (the prompts, word for word). Change them only for a deliberate change to the save format or a prompt, and say so in the commit.

## Rules a change has to keep

- **The browser is a viewer.** No simulation, brain, saves or model calls in `src/`. `tests/architecture.test.ts` enforces the layer graph; see [src/AGENTS.md](src/AGENTS.md).
- **The language model is configured on the server only**, through the `LLM_*` environment variables (`server/crates/server/src/config.rs`). Nothing about the model (key, URL, model, tuning) may become settable or visible in full from a browser; the admin panel shows it read-only with a connection test. The key and header values never reach a browser or an export, and `ApiKey` masks the key in `Debug` output.
- **The sim is deterministic.** Only the world task advances it, and the outside world reaches it only through `Command`s applied at tick boundaries. See [server/AGENTS.md](server/AGENTS.md) for the porting rules that keep it reproducible.
- **The world exists twice.** `src/world` (TypeScript, for terrain, scenery and the camera) and `server/crates/world` (Rust, for the sim) must describe the same map. A layout change goes into both, then `bun tools/dump-world.ts` regenerates the golden fixtures and `cargo test` must pass. The client compares the server's `hello.worldHash` with its own and warns when they differ.
- **Wire changes start in Rust.** Change `wire.rs`, run `export-api`, then update the client. Bump `PROTOCOL_VERSION` in `wire.rs` for a change older clients cannot read.
- **The database schema only grows.** Add a new numbered migration in `server/crates/server/src/store/migrations.rs`; never edit one that has shipped.
- **Guests only watch.** Anyone may watch as a guest for 15 minutes without signing in (`server/crates/server/src/guests.rs`). A new `/api` route goes into the `members` group in `api/router.rs`, which answers 403 to guests, unless watching needs it; guests never get a `ChatKey`, so they have no chat and no credits.
- **Security defaults stay safe.** `AUTH_MODE=off` must keep refusing non-loopback addresses and origins; mutating routes keep the CSRF check (JSON bodies, `Origin` equal to `PUBLIC_ORIGIN`); `TRUST_PROXY` stays opt-in.
- **Sound licences**: only CC0, public domain or CC BY (credited in `public/audio/CREDITS.md`). No share-alike or non-commercial sounds.
- **3D models** come from the KayKit collection (CC0), processed by `pnpm assets` into `public/assets/`.

## Style

- Match the code around you: names, comment density and idiom. Comments explain why, in full sentences; doc comments on public items say what a thing is for.
- Rust: edition 2024, `rustfmt.toml` sets a 120-column width. `unsafe` is forbidden and `clippy::await_holding_lock` is denied workspace-wide.
- TypeScript: strict mode with `noUncheckedIndexedAccess`; ES modules with `verbatimModuleSyntax` (`import type` for types); Preact with signals for UI state.
- Docs are plain prose for someone running or changing the game: British spelling, no jargon where a plain word works. Keep [README.md](README.md), [docs/deploy.md](docs/deploy.md) and [docs/sign-in.md](docs/sign-in.md) true when behaviour, settings or commands change.
- Commit subjects are one imperative sentence that says what changed ("Configure the language model from server environment variables only").
- Never commit `.env` or anything with secrets in it; `.env` is git-ignored and nothing loads it automatically.
