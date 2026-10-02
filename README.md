# Fantasy Sim World

A browser-only 3D world in the KayKit style. A dozen fantasy villagers follow their days, talk, make friends and enemies, fall in love, fight the dead that rise from the graveyard at night and, if pushed far enough, each other. You watch from a free top-down camera.

Each villager has a name, a profile, a personality, relationships and a memory that grows as the simulation runs. A language model makes their decisions through tool calls on any OpenAI-compatible endpoint; walking, working and fighting are simulated locally and never wait for the model. With no model connected, villagers run on built-in routines, so the world is alive either way.

The design is in [docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md](docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md).
The plan for moving the simulation, the language model and all state to a Rust server with authenticated viewers is in [docs/superpowers/specs/2026-10-02-backend-migration-plan.md](docs/superpowers/specs/2026-10-02-backend-migration-plan.md).

## Run

```bash
pnpm install
```

```bash
pnpm dev
```

Open the printed URL in a browser with WebGPU or WebGL 2.

- Drag to pan, scroll to zoom, right-drag or Q / E to turn, WASD or arrows to move.
- Click a villager to follow them and open the inspector (profile, needs, plan, relationships, memories).
- Space pauses; 1, 2 and 3 set the speed.
- `?renderer=webgl` or `?renderer=webgpu` forces a render backend; `?debug=nav` shows the navigation grid and activity spots.

The world is saved in the browser (IndexedDB) every 30 seconds and at notable moments, and resumes where it left off. Settings has export, import and "new world".

## Connecting a language model

Open **Settings**, pick a preset or enter a base URL, choose a model ("List models" asks the server), and press **Test connection**. Requests go straight from the browser to the endpoint; the key is stored only in that browser and is never part of an exported save.

- **Local models (Ollama, LM Studio)**: open the app from `localhost` so the server accepts the requests, use a model that supports tools, and set "Requests at once" to 1. If LM Studio refuses the browser's requests, turn on CORS in its server settings.
- **Reasoning models**: leave "Model reasoning" on Off. A model that thinks before every answer is several times slower and can spend its whole output budget on thinking.
- **Cloud endpoints**: any that allow browser requests work directly. For one that does not, start the dev server with a proxy and use `/llm-proxy` as the base URL:

```bash
LLM_PROXY_TARGET=https://your-endpoint.example/v1 pnpm dev
```

How the model is used:

- It is asked only at decision points: on waking (a plan for the day), when something notable happens, when a plan block ends, in danger (fight or flee), in conversations, when a beaten opponent yields (spare or finish), and once a night to reflect.
- Tool calls are sent natively. If a model answers in prose instead of calling its tool, the client switches to JSON-schema output, which local servers enforce with a grammar. Tested with `qwen/qwen3.6-35b-a3b` in LM Studio: 85% usable answers natively, 20 of 20 as JSON output, about 6 seconds each with reasoning off.
- A governor measures the endpoint's speed and keeps demand below it, so a slow local model gets fewer, more important requests. "Max requests / minute" is a hard cap on top of that.
- Every answer is checked against the world as it is by then. Stale, invalid or missing answers fall back to local behaviour.
- The 🧠 pill in the top bar opens the decision log: each prompt, the raw reply, what was done with it, latency and tokens.

To measure a model before letting it run the village:

```bash
bun tools/llm-probe.ts http://localhost:11434/v1 <model> 20
```

To try the whole path with no model at all, run the mock endpoint and point the app at `http://localhost:8787/v1`:

```bash
node tools/mock-llm.mjs
```

## Sound

The game has an ambient mixer but ships no audio. Put your own files in `public/audio/`, named after the slots listed in [public/audio/README.md](public/audio/README.md); missing slots are silent, and Settings shows which were found.

Character speech is not implemented yet, but the seam is in place: every spoken line passes through `SpeechBus` ([src/speech/speechBus.ts](src/speech/speechBus.ts)) with the speaker's voice profile, position and duration, so a text-to-speech sink can be added without touching the simulation.

## Tests and tools

```bash
pnpm test
```

```bash
pnpm build
```

- `pnpm test` runs the unit tests, including a save/load determinism check and a 200-day headless run.
- `bun tools/soak.ts 30 1337 --log` runs the world headlessly for 30 days and prints its chronicle. `--json` prints the end-of-run statistics as JSON; `--trace` prints one line per tick (RNG state, positions, intents) for diffing two implementations.

## The server (in progress)

The simulation is moving to a Rust server that runs one shared world for several viewers ([the plan](docs/superpowers/specs/2026-10-02-backend-migration-plan.md)). Phase 0 is in place: the Cargo workspace under `server/`, a 20 Hz tick loop that streams empty frames, token sign-in, the wire types and the fixtures the port is checked against. The browser game above is unchanged.

```bash
cd server
AUTH_TOKEN=dev-token-0123456789 LOG_FORMAT=pretty cargo run
```

With the server running, `pnpm dev` forwards `/api` and `/auth` to it (`SERVER_URL` overrides `http://localhost:8080`); open `/auth/token` once to sign in. Settings come from the environment: `AUTH_TOKEN` (required, 16+ characters), `PORT` or `BIND_ADDR`, `PUBLIC_ORIGIN`, `STATIC_DIR` (the web build, default `dist`), `DATA_DIR` (the SQLite database `world.db` and `backup/latest.db`, default `data`), `INITIAL_SPEED` (0, 1, 2 or 4) and `LOG_FORMAT=pretty`.

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
| `src/sim` | The simulation: plain JSON state stepped at 20 Hz, with no rendering or network |
| `src/brain` | The LLM client, request queue, governor, prompts, tools and their validation |
| `src/render` | three.js scene, actors, effects, labels, camera, lighting |
| `src/ui` | The HUD (Preact) |
| `src/audio`, `src/speech` | Ambient mixer and the speech bus |
| `src/persist` | IndexedDB saves, settings, export and import |
| `tools` | Asset build, Blender prop generator, headless soak, LLM probe, mock endpoint, server fixtures and stream probe |
| `server` | The Rust server (Cargo workspace: `core`, `world`, `sim`, `brain`, `server`), being built |

The sim depends only on `core` and `world`; the brain talks to it through commands applied at tick boundaries, so a run is reproducible for a given command stream.

## Assets

The 3D models are from the [KayKit](https://kaylousberg.com) collection by Kay Lousberg (CC0). The processed subset the game uses is committed under `public/assets/`. To rebuild it from the original collection:

```bash
KAYKIT_DIR="/path/to/The Complete KayKit Collection v7" pnpm assets
```

A few props the collection lacks at character scale (a wooden fence, wildflowers, mushrooms, a notice board) are generated with Blender and bundled by the same command:

```bash
/Applications/Blender.app/Contents/MacOS/Blender -b -P tools/blender/make_props.py
```
