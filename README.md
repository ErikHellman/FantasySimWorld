# Fantasy Sim World

A 3D world in the KayKit style. A dozen fantasy villagers follow their days, talk, make friends and enemies, fall in love, fight the dead that rise from the graveyard at night and, if pushed far enough, each other. The world runs on a server, around the clock; everyone who signs in watches the same village from their own free top-down camera.

Each villager has a name, a profile, a personality, relationships and a memory that grows as the simulation runs. A language model makes their decisions through tool calls on any OpenAI-compatible endpoint; walking, working and fighting are simulated by the server and never wait for the model. With no model connected, villagers run on built-in routines, so the world is alive either way.

The design is in [docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md](docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md).
The plan for moving the simulation, the language model and all state to a Rust server with authenticated viewers is in [docs/superpowers/specs/2026-10-02-backend-migration-plan.md](docs/superpowers/specs/2026-10-02-backend-migration-plan.md).

## Run

```bash
pnpm install
pnpm build                      # the web client, into dist/
cd server
AUTH_TOKEN=dev-token-0123456789 STATIC_DIR=../dist LOG_FORMAT=pretty cargo run --release
```

Open <http://localhost:8080/auth/token>, paste the token, and you are watching (as an admin) a world that keeps running while the server does. Every tab and every person watches the same world. The world, its history and the settings live in `server/data/world.db`; stop the server with Ctrl-C and it resumes where it was.

For work on the client, run `pnpm dev` next to the server: it serves the client with hot reload and forwards `/api` and `/auth` to `http://localhost:8080` (`SERVER_URL` changes it). Open `/auth/token` on the dev server's port once to sign in.

In the browser (WebGPU or WebGL 2):

- Drag to pan, scroll to zoom, right-drag or Q / E to turn, WASD or arrows to move.
- Click a villager to follow them and open the inspector (profile, needs, plan, relationships, memories).
- Admins set the speed for everyone: Space pauses; 1, 2 and 3 set 1×, 2× and 4×.
- `?renderer=webgl` or `?renderer=webgpu` forces a render backend; `?debug=nav` shows the navigation grid and activity spots; `?render=off` follows the stream without drawing (for headless tests, where software WebGL is too slow).

The admin panel (🛠) has the speed, export, import and "new world", the language model settings, the allowlist and refused sign-ins, and the server's health.

Server settings come from the environment: `AUTH_TOKEN` (16+ characters) or Google and Apple sign-in ([docs/sign-in.md](docs/sign-in.md)), `PORT` or `BIND_ADDR`, `PUBLIC_ORIGIN` (the address in the browser; sign-in and the CSRF check use it), `STATIC_DIR` (the web build, default `dist`), `DATA_DIR` (default `data`), `INITIAL_SPEED` (0, 1, 2 or 4 for a new world), `LLM_API_KEY` (seeds the model key once) and `LOG_FORMAT=pretty`. Deploying to Fly.io, backups and health are in [docs/deploy.md](docs/deploy.md).

## Connecting a language model

Open **🛠 Admin → Language model**, pick a preset or enter a base URL, choose a model ("List models" asks that endpoint), and press **Test connection**, then **Save** with **Enabled** on. The server makes every request; the key is stored on the server, shown only as its last characters, never sent to a browser and never part of an export. A fresh server starts with the model off.

- **Local models (Ollama, LM Studio)**: run them where the server can reach them, use a model that supports tools, and set "Requests at once" to 1.
- **Reasoning models**: leave "Model reasoning" on Off. A model that thinks before every answer is several times slower and can spend its whole output budget on thinking.
- **Daily caps**: requests and tokens per day, spread evenly through the day so the village does not go quiet in the evening. A world that runs around the clock asks a fast model about 20,000 times a day.

How the model is used:

- It is asked only at decision points: on waking (a plan for the day), when something notable happens, when a plan block ends, in danger (fight or flee), in conversations, when a beaten opponent yields (spare or finish), and once a night to reflect.
- Tool calls are sent natively. If a model answers in prose instead of calling its tool, the client switches to JSON-schema output, which local servers enforce with a grammar. Tested with `qwen/qwen3.6-35b-a3b` in LM Studio: 85% usable answers natively, 20 of 20 as JSON output, about 6 seconds each with reasoning off.
- A governor measures the endpoint's speed and keeps demand below it, so a slow local model gets fewer, more important requests. "Max requests / minute" is a hard cap on top of that. While the model has a backlog, a world running faster than 1× holds at 1×.
- Every answer is checked against the world as it is by then. Stale, invalid or missing answers fall back to the routines.
- The 🧠 pill in the top bar opens the decision log for admins: each prompt, the raw reply, the commands it produced, latency and tokens.

To measure a model before letting it run the village (real prompts from a world a few days in):

```bash
cd server && cargo run --release -p fsw-brain --bin probe -- http://localhost:11434/v1 <model> 20
```

To try the whole path with no model at all, run the mock endpoint and use `http://localhost:8787/v1` (model `mock`) as the base URL:

```bash
node tools/mock-llm.mjs
```

## Sound

The game has an ambient mixer but ships no audio. Put your own files in `public/audio/`, named after the slots listed in [public/audio/README.md](public/audio/README.md); missing slots are silent, and Settings shows which were found.

Character speech is not implemented yet, but the seam is in place: every spoken line arrives from the server complete, with the speaker's voice profile, position and duration, and passes through `SpeechBus` ([src/speech/speechBus.ts](src/speech/speechBus.ts)), so a text-to-speech sink can be added in the browser without touching the server.

## Tests and tools

```bash
pnpm typecheck && pnpm test     # the client
cd server && cargo test         # the server
```

- `pnpm test` covers the client's stream playback, the view of the world it builds from the stream, the world layer, and the architecture rules (the browser never reaches the simulation, the brain or saves).
- `cargo test` covers the Rust world against golden fixtures from the TypeScript version, the sim (determinism through a save round trip, invariants over several days, a 20-day run; `cargo test -- --ignored` adds the 200-day soak), old browser saves still loading, the prompts byte for byte, the brain against a fake model, sign-in against a fake OpenID provider, the store, and the HTTP routes. It also checks that `src/generated/api.ts` matches the Rust wire types (`cargo run --bin export-api` rewrites it).
- `cargo run --release -p fsw-sim --bin soak -- 30 1337 --log` runs the world headlessly for 30 days and prints its chronicle; `--json` prints statistics, `--trace` one line per tick.
- `AUTH_TOKEN=… node tools/sse-probe.mjs <url> [seconds]` checks that the event stream reaches a client through a hosting proxy unbuffered.
- `bun tools/assets/export-server-data.ts` writes `server/data/*.json` (model bounds, clip durations, tile heights) from `src/generated`; `pnpm assets` writes them too.

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
| `tools` | Asset build, Blender prop generator, mock model endpoint, stream probe, world dump for the golden fixtures |
| `server` | The Rust server, a Cargo workspace: `core` and `world` (shared maths and the static world), `sim` (the simulation), `brain` (the language model: client, queue, governor, prompts, validation), `server` (HTTP, event stream, sign-in, SQLite store) |
| `deploy`, `Dockerfile`, `fly.toml` | The container image, Litestream and Fly.io settings ([docs/deploy.md](docs/deploy.md)) |

On the server the sim depends only on `core` and `world`; the brain reads the world and talks to the sim through commands applied at tick boundaries, so a run is reproducible for a given command stream. The browser depends on `core`, `world`, `net` and `generated`, never on the sim.

The TypeScript simulation, brain, browser saves and reference server that the Rust port was checked against are kept at the `ts-sim-reference` tag (`git checkout ts-sim-reference`), with the tools that compare the two (`bun tools/soak.ts … --trace`, `python3 tools/soak-parity.py`, and the fixture writers).

## Assets

The 3D models are from the [KayKit](https://kaylousberg.com) collection by Kay Lousberg (CC0). The processed subset the game uses is committed under `public/assets/`. To rebuild it from the original collection:

```bash
KAYKIT_DIR="/path/to/The Complete KayKit Collection v7" pnpm assets
```

A few props the collection lacks at character scale (a wooden fence, wildflowers, mushrooms, a notice board) are generated with Blender and bundled by the same command:

```bash
/Applications/Blender.app/Contents/MacOS/Blender -b -P tools/blender/make_props.py
```
