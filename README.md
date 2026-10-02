# Fantasy Sim World

A 3D world in the KayKit style. A dozen fantasy villagers follow their days, talk, make friends and enemies, fall in love, fight the dead that rise from the graveyard at night and, if pushed far enough, each other. The world runs on a server, around the clock; everyone who signs in watches the same village from their own free top-down camera.

Each villager has a name, a profile, a personality, relationships and a memory that grows as the simulation runs. A language model makes their decisions through tool calls on any OpenAI-compatible endpoint; walking, working and fighting are simulated by the server and never wait for the model. With no model connected, villagers run on built-in routines, so the world is alive either way.

The design is in [docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md](docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md).
The plan for moving the simulation, the language model and all state to a Rust server with authenticated viewers is in [docs/superpowers/specs/2026-10-02-backend-migration-plan.md](docs/superpowers/specs/2026-10-02-backend-migration-plan.md).

## Run

```bash
pnpm install
```

The browser only draws the world; a server runs it. Until the Rust server (below) runs the simulation, use the reference server, which runs the TypeScript simulation behind the same protocol:

```bash
bun tools/ref-server.ts
```

```bash
pnpm dev
```

Open the printed URL in a browser with WebGPU or WebGL 2. `pnpm dev` forwards `/api` and `/auth` to `http://localhost:8080` (`SERVER_URL` changes it); the reference server listens there (`PORT` changes it). Every tab watches the same world.

- Drag to pan, scroll to zoom, right-drag or Q / E to turn, WASD or arrows to move.
- Click a villager to follow them and open the inspector (profile, needs, plan, relationships, memories).
- Admins set the speed for everyone: Space pauses; 1, 2 and 3 set 1×, 2× and 4×.
- `?renderer=webgl` or `?renderer=webgpu` forces a render backend; `?debug=nav` shows the navigation grid and activity spots; `?render=off` follows the stream without drawing (for headless tests, where software WebGL is too slow).

The reference server has no sign-in and keeps nothing: every visitor is an admin (`REF_ROLE=viewer` makes them viewers, `REF_ROLE=anonymous` refuses everyone, to see the login page), and a restart starts a new world (`SEED=…` picks it, `IMPORT=save.json` starts from an export, `INITIAL_SPEED=0` starts paused). The admin panel (🛠) has the speed, export, import and "new world", the language model settings, the allowlist and refused sign-ins, and the server's health.

## Connecting a language model

Open **🛠 Admin**, pick a preset or enter a base URL, save, choose a model ("List models" asks the endpoint with the saved settings), and press **Test connection**. The server makes the requests; the key is stored on the server, shown only as its last characters, and never part of an export. With the reference server the model can also be set from the environment:

```bash
LLM_BASE_URL=http://localhost:11434/v1 LLM_MODEL=<model> bun tools/ref-server.ts
```

- **Local models (Ollama, LM Studio)**: run them where the server can reach them, use a model that supports tools, and set "Requests at once" to 1.
- **Reasoning models**: leave "Model reasoning" on Off. A model that thinks before every answer is several times slower and can spend its whole output budget on thinking.

How the model is used:

- It is asked only at decision points: on waking (a plan for the day), when something notable happens, when a plan block ends, in danger (fight or flee), in conversations, when a beaten opponent yields (spare or finish), and once a night to reflect.
- Tool calls are sent natively. If a model answers in prose instead of calling its tool, the client switches to JSON-schema output, which local servers enforce with a grammar. Tested with `qwen/qwen3.6-35b-a3b` in LM Studio: 85% usable answers natively, 20 of 20 as JSON output, about 6 seconds each with reasoning off.
- A governor measures the endpoint's speed and keeps demand below it, so a slow local model gets fewer, more important requests. "Max requests / minute" is a hard cap on top of that.
- Every answer is checked against the world as it is by then. Stale, invalid or missing answers fall back to the routines.
- The 🧠 pill in the top bar opens the decision log for admins: each prompt, the raw reply, what was done with it, latency and tokens.

To measure a model before letting it run the village:

```bash
bun tools/llm-probe.ts http://localhost:11434/v1 <model> 20
```

To try the whole path with no model at all, run the mock endpoint and point the server at it:

```bash
node tools/mock-llm.mjs
LLM_BASE_URL=http://localhost:8787/v1 LLM_MODEL=mock bun tools/ref-server.ts
```

## Sound

The game has an ambient mixer but ships no audio. Put your own files in `public/audio/`, named after the slots listed in [public/audio/README.md](public/audio/README.md); missing slots are silent, and Settings shows which were found.

Character speech is not implemented yet, but the seam is in place: every spoken line arrives from the server complete, with the speaker's voice profile, position and duration, and passes through `SpeechBus` ([src/speech/speechBus.ts](src/speech/speechBus.ts)), so a text-to-speech sink can be added in the browser without touching the server.

## Tests and tools

```bash
pnpm test
```

```bash
pnpm build
```

- `pnpm test` runs the unit tests, including a save/load determinism check, a 200-day headless run, the reference server's encoder and digest, the client's stream playback, and the architecture rules (nothing the browser loads reaches the simulation, the brain or the old browser saves).
- `bun tools/soak.ts 30 1337 --log` runs the world headlessly for 30 days and prints its chronicle. `--json` prints the end-of-run statistics as JSON; `--trace` prints one line per tick (RNG state, positions, intents) for diffing two implementations.

## The server (in progress)

The simulation is moving to a Rust server that runs one shared world for several viewers ([the plan](docs/superpowers/specs/2026-10-02-backend-migration-plan.md)). The browser is already the thin viewer the plan describes: it follows the server's event stream (`src/net`) and draws it. The Rust workspace is under `server/`; its Phase 0 binary streams empty frames with token sign-in, and the simulation is being ported to it. Until then `tools/ref-server.ts` serves the TypeScript simulation over the same wire protocol (`src/generated/api.ts`), so switching servers is only a change of `SERVER_URL`. The reference server's encoder and digest (`tools/ref-server/encode.ts`, `digest.ts`) are what the Rust ones are ported from.

```bash
cd server
AUTH_TOKEN=dev-token-0123456789 LOG_FORMAT=pretty cargo run
```

With the server running, `pnpm dev` forwards `/api` and `/auth` to it (`SERVER_URL` overrides `http://localhost:8080`); open `/auth/token` once to sign in (the login page links to it). Settings come from the environment: `AUTH_TOKEN` (required, 16+ characters), `PORT` or `BIND_ADDR`, `PUBLIC_ORIGIN`, `STATIC_DIR` (the web build, default `dist`), `DATA_DIR` (the SQLite database `world.db` and `backup/latest.db`, default `data`), `INITIAL_SPEED` (0, 1, 2 or 4) and `LOG_FORMAT=pretty`.

Google and Apple sign-in replace the token with `AUTH_MODE=oidc` and `AUTH_PROVIDERS=google,apple` (either alone works). Google needs `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` (redirect URI `<PUBLIC_ORIGIN>/auth/google/callback`; `http://localhost` works for testing). Apple needs `APPLE_TEAM_ID`, `APPLE_KEY_ID`, `APPLE_SERVICES_ID` and `APPLE_PRIVATE_KEY` (the contents of the `.p8` file; `\n` may stand for line breaks), with return URL `<PUBLIC_ORIGIN>/auth/apple/callback`, and only works on a real https domain. Only e-mails on the allowlist get in: `ADMIN_EMAILS=a@example.com,b@example.com` puts admins on it at every start, and admins manage the rest under `/api/admin/allowlist` (refused sign-ins are listed under `/api/admin/attempts`). `TRUST_PROXY=true` takes client addresses from `Fly-Client-IP` / `X-Forwarded-For` for the sign-in rate limit; set it only behind a proxy that sets them. There is no mode without sign-in.

- `cd server && cargo test` runs the server tests, including a check that `src/generated/api.ts` matches the Rust wire types. `cargo run --bin export-api` rewrites it.
- `bun tools/dump-world.ts` writes the world layer's output to `server/crates/world/tests/golden/`, the reference the Rust world must reproduce.
- `bun tools/make-export-fixture.ts` runs the world with the brain against the mock endpoint and writes a save export into `server/crates/sim/tests/fixtures/`, which the Rust sim must keep reading.
- `bun tools/assets/export-server-data.ts` writes `server/data/*.json` (model bounds, clip durations, tile heights) from `src/generated`; `pnpm assets` writes them too.
- `AUTH_TOKEN=… node tools/sse-probe.mjs <url> [seconds]` checks that the event stream reaches a client through a hosting proxy unbuffered. `Dockerfile` and `fly.toml` deploy the server to Fly.io for that check.

## How it is put together

| Folder | What it holds |
| --- | --- |
| `src/core` | Seeded RNG, 2D maths, game time |
| `src/world` | The hex map, places and activity spots, scatter, navigation grid, pathfinding |
| `src/net` | The event stream, the world as this viewer knows it, the jitter buffer, REST calls |
| `src/render` | three.js scene, actors, effects, labels, camera, lighting, animation table |
| `src/ui` | The HUD, admin panel, login and no-access pages (Preact) |
| `src/audio`, `src/speech` | Ambient mixer and the speech bus |
| `src/generated` | Wire types (from the Rust server), asset manifest, tile heights |
| `src/sim`, `src/brain`, `src/persist`, `src/app` | The TypeScript simulation, LLM brain, browser saves and game loop: no longer loaded by the browser, kept as the reference for the Rust port and run by the reference server |
| `tools` | Reference server, asset build, Blender prop generator, headless soak, LLM probe, mock endpoint, server fixtures and stream probe |
| `server` | The Rust server (Cargo workspace: `core`, `world`, `sim`, `brain`, `server`), being built |

The sim depends only on `core` and `world` (and the animation table in `render/anims.ts`); the brain talks to it through commands applied at tick boundaries, so a run is reproducible for a given command stream. The browser depends on `core`, `world`, `net` and `generated`, never on the sim.

## Assets

The 3D models are from the [KayKit](https://kaylousberg.com) collection by Kay Lousberg (CC0). The processed subset the game uses is committed under `public/assets/`. To rebuild it from the original collection:

```bash
KAYKIT_DIR="/path/to/The Complete KayKit Collection v7" pnpm assets
```

A few props the collection lacks at character scale (a wooden fence, wildflowers, mushrooms, a notice board) are generated with Blender and bundled by the same command:

```bash
/Applications/Blender.app/Contents/MacOS/Blender -b -P tools/blender/make_props.py
```
