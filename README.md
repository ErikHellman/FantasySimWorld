# Fantasy Sim World

A 3D world in the KayKit style. A dozen fantasy villagers follow their days, talk, make friends and enemies, fall in love, fight the dead that rise from the graveyard at night and, if pushed far enough, each other. The world runs on a server, around the clock; everyone who signs in watches the same village from their own free top-down camera.

Each villager has a name, a profile, a personality, relationships and a memory that grows as the simulation runs. A language model makes their decisions through tool calls on any OpenAI-compatible endpoint; walking, working and fighting are simulated by the server and never wait for the model. With no model connected, villagers run on built-in routines, so the world is alive either way.

![The village by day, with the villager list, the chronicle and the inspector open (an earlier build)](docs/screenshots/village-day.jpg)

The design is in [docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md](docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md).
The move of the simulation, the language model and all state to a Rust server with authenticated viewers, and what was measured along the way, is in [docs/superpowers/specs/2026-10-02-backend-migration-plan.md](docs/superpowers/specs/2026-10-02-backend-migration-plan.md).

## Status

The server migration is complete. The world, the language-model brain and every piece of state live in the Rust server (`server/`); the browser is a viewer that draws the server's event stream (`src/`). What works today:

- The village simulation at 20 ticks a second, with routines, needs, conversations, relationships, night attacks, fights, deaths, graves and newcomers. A world resumes where it was after a restart.
- House fires: when not one villager is awake, the dead set a home alight. Everyone is woken to carry water from the well or the lake; a house they cannot save burns down and is rebuilt over several days, while the people who lived there sleep rough by the campfire ([the design](docs/superpowers/specs/2026-10-03-house-fire-design.md)).
- Five billboards around the village with a new slogan each morning. Villagers sometimes stop to read one and say what they think, and admins can put their own text up ([the design](docs/superpowers/specs/2026-10-03-billboards-design.md)).
- The language model on the server, with a request queue, a speed governor, daily caps, a decision log for admins and a fallback to routines.
- Sign-in with a shared token, or with Google, Apple, Microsoft, LinkedIn, GitHub or Discord accounts and an allowlist; local development without sign-in.
- Ambient sound, effects and adaptive music.
- A container image with Litestream backups and Fly.io settings.

Not yet done or not yet tried:

- **The real deployment**: `fly deploy`, the event-stream check through Fly's proxy and `docker build` itself have not been run ([docs/deploy.md](docs/deploy.md) has the steps).
- **Sign-in with real accounts**: every provider is tested against fakes only ([docs/sign-in.md](docs/sign-in.md) links a guide per provider, with the console steps and the check).
- **Models**: one real model has been tried (`qwen/qwen3.6-35b-a3b` in LM Studio). No hosted endpoint has been tested yet.
- **Later**: spoken lines (text to speech; the seam is in place), viewers acting on the world, orc raiders and large bosses, block and dodge, embeddings for memory, and newcomer profiles written by the model.

## What you need

- **Node.js 22** and **pnpm 10** (`corepack enable` picks the version in `package.json`), for the web client.
- **Rust 1.88 or newer** (stable), for the server.
- A browser with WebGPU or WebGL 2.
- Only for some tools: [Bun](https://bun.sh) (`pnpm audio` and the `.ts` tools), ffmpeg and bsdtar (`pnpm audio`), Blender (generated props), and the KayKit collection (rebuilding the models).

## Run

```bash
pnpm install
pnpm build                      # the web client, into dist/
cd server
AUTH_TOKEN=dev-token-0123456789 PUBLIC_ORIGIN=http://localhost:8080 STATIC_DIR=../dist LOG_FORMAT=pretty cargo run --release
```

Open <http://localhost:8080/auth/token>, paste the token, and you are watching (as an admin) a world that keeps running while the server does. Every tab and every person watches the same world. The world, its history and the settings live in `server/data/world.db`; stop the server with Ctrl-C and it resumes where it was.

`PUBLIC_ORIGIN` is the address in the browser. It defaults to `http://localhost:5173`, the dev server's, and the CSRF check refuses changes (speed, admin actions) from any other origin, so set it whenever you browse the server directly.

For work on the client, run `pnpm dev` next to the server: it serves the client with hot reload on <http://localhost:5173> and forwards `/api` and `/auth` to `http://localhost:8080` (`SERVER_URL` changes it). Start the server without `PUBLIC_ORIGIN` (or with the dev server's address) and open `/auth/token` on the dev server's port once to sign in.

To skip signing in while developing, start the server with `AUTH_MODE=off` instead of `AUTH_TOKEN`: every browser on this machine is an admin. The server then listens on `127.0.0.1` only and refuses to start with a non-local `BIND_ADDR` or `PUBLIC_ORIGIN`. With `pnpm dev` that is all it takes; when the browser talks to the server directly on :8080, set `PUBLIC_ORIGIN` to match:

```bash
AUTH_MODE=off PUBLIC_ORIGIN=http://localhost:8080 STATIC_DIR=../dist LOG_FORMAT=pretty cargo run --release
```

Neither the server nor Vite reads a `.env` file. To keep your settings in one (it is git-ignored), export it into the shell and start everything from that shell:

```bash
set -a; . ./.env; set +a
```

### In the browser

- Drag to pan, scroll to zoom, right-drag or Q / E to turn, WASD or arrows to move.
- Shift or Option (Alt) + scroll, R / F, or right-drag up and down tilts the view, from low over the ground to straight down.
- Click a villager to follow them and open the inspector (profile, needs, plan, relationships, memories).
- On a touch screen, one finger pans, pinching zooms, twisting two fingers turns and dragging two fingers up or down tilts.
- On a phone (either side under 600 or so pixels, portrait or landscape) the screen is simpler: a tab bar opens the villagers, the chronicle, the chat, the followed villager's page or the ☰ menu, one at a time. Tablets and opened foldables get the desktop layout.
- The ✦ pill in the top bar shows your credits and opens what they buy (see [Credits](#credits)).
- Admins set the speed for everyone: Space pauses; 1, 2 and 3 set 1×, 2× and 4×.
- ❓ Help in the top bar, or the ? or H key, lists the controls and explains what is on the screen and how the village works.
- `?renderer=webgl` or `?renderer=webgpu` forces a render backend; `?quality=low` or `?quality=high` forces the picture's quality (touch devices and machines with little memory get `low` by themselves: fewer pixels, a smaller shadow map, fewer lamps, thinner grass and 30 frames a second); `?debug=nav` shows the navigation grid and activity spots; `?render=off` follows the stream without drawing (for headless tests, where software WebGL is too slow).

The admin panel (🛠) has the speed, export, import and "new world", the text on the billboards, the language model's configuration (read-only) and a connection test, the allowlist and refused sign-ins, and the server's health. The 🧠 pill in the top bar opens the model's decision log.

### Server settings

All settings come from environment variables, read at start:

| Variable | Default | What it is |
| --- | --- | --- |
| `AUTH_MODE` | `token` | `token`, `oidc` (Google, Apple, Microsoft, LinkedIn, GitHub or Discord accounts, [docs/sign-in.md](docs/sign-in.md)) or `off` (local development only) |
| `AUTH_TOKEN` | none | The shared secret for `AUTH_MODE=token`, 16 characters or more |
| `AUTH_PROVIDERS`, `ADMIN_EMAILS` | none | Which sign-in providers are on, and the first admins ([docs/sign-in.md](docs/sign-in.md)) |
| `GOOGLE_*`, `APPLE_*`, `MICROSOFT_*`, `LINKEDIN_*`, `GITHUB_*`, `DISCORD_*` | none | Each provider's client ID and secret; one guide per provider in [docs/sign-in/](docs/sign-in/) |
| `PORT` or `BIND_ADDR` | `0.0.0.0:8080` (`127.0.0.1:8080` with `AUTH_MODE=off`) | Where the server listens; `BIND_ADDR` wins when both are set |
| `PUBLIC_ORIGIN` | `http://localhost:5173` | The address in the browser; sign-in redirects and the CSRF check use it |
| `STATIC_DIR` | `dist` | The web build the server serves |
| `DATA_DIR` | `data` | Where `world.db` and its backups live |
| `INITIAL_SPEED` | `1` | Speed of a new world: `0` (paused), `1`, `2` or `4` |
| `CREDITS_PER_MINUTE` | `2` | Credits a viewer earns for a minute of watching with the tab in front; `0` turns credits off |
| `CREDITS_CAP` | `400` | The most credits watching and prayer can build up |
| `TRUST_PROXY` | `false` | Take the client's address from `Fly-Client-IP` / `X-Forwarded-For`; only behind a proxy that sets them |
| `LOG_FORMAT` | JSON lines | `pretty` for humans |
| `LLM_*` | off | The language model ([below](#connecting-a-language-model)) |

A value the server cannot use stops it at start with a message naming the variable. Deploying to Fly.io, backups and health are in [docs/deploy.md](docs/deploy.md).

## Connecting a language model

The language model is configured only with environment variables on the server. Nothing about it can be set or changed from a browser. The server reads them at start, so a change takes effect on the next restart. Without `LLM_BASE_URL` and `LLM_MODEL` the model is off and the villagers follow their built-in routines. A value the server cannot use (a number out of range, an unknown choice, or only one of the two) stops it at start with a message naming the variable.

| Variable | Default | What it is |
| --- | --- | --- |
| `LLM_BASE_URL` | none | An OpenAI-compatible chat completions endpoint that supports tool calling, e.g. `https://api.openai.com/v1`. Set it together with `LLM_MODEL`. |
| `LLM_MODEL` | none | The model's name at that endpoint |
| `LLM_API_KEY` | none | The endpoint's key, sent as `Authorization: Bearer`. Local models need none. |
| `LLM_CONCURRENCY` | `2` | Requests at once, 1 to 16 |
| `LLM_REQUESTS_PER_MINUTE` | `40` | A hard cap per real minute; `0` is no cap |
| `LLM_MAX_REQUESTS_PER_DAY` | `0` | Requests per UTC day, spread through the day; `0` is no cap |
| `LLM_MAX_TOKENS_PER_DAY` | `0` | Prompt plus completion tokens per UTC day, spread through the day; `0` is no cap |
| `LLM_PROTOCOL` | `auto` | How tool calls travel: `tools` (native), `json` (JSON-schema output) or `auto` (detect) |
| `LLM_CONVERSATIONS` | `auto` | `batched` (one request per conversation), `turns` (one per line) or `auto` (chosen by speed) |
| `LLM_REASONING` | `off` | For models that think before answering: `off`, `low` or `default` (the model's own) |
| `LLM_TEMPERATURE` | `0.8` | 0 to 2 |
| `LLM_MAX_TOKENS` | `600` | Output tokens per answer, 100 to 4000 |
| `LLM_HEADERS` | none | Extra request headers, one `Name: value` per line (or separated by a literal `\n`) |

The server makes every request. The key and header values never reach a browser and are never part of an export. **🛠 Admin → Language model** shows the configuration the server is running with (the key only as its last four characters) and has **Test connection**, which asks the model for one real decision.

```bash
# A hosted model (on Fly.io: fly secrets set LLM_API_KEY=… LLM_BASE_URL=… LLM_MODEL=…)
LLM_BASE_URL=https://api.openai.com/v1 LLM_MODEL=<model> LLM_API_KEY=sk-… \
  AUTH_TOKEN=dev-token-0123456789 PUBLIC_ORIGIN=http://localhost:8080 STATIC_DIR=../dist cargo run --release

# Ollama or LM Studio, where the server can reach it (LM Studio is http://localhost:1234/v1)
LLM_BASE_URL=http://localhost:11434/v1 LLM_MODEL=<model> LLM_CONCURRENCY=1 \
  AUTH_TOKEN=dev-token-0123456789 PUBLIC_ORIGIN=http://localhost:8080 STATIC_DIR=../dist cargo run --release
```

- **Local models (Ollama, LM Studio)**: use a model that supports tools and set `LLM_CONCURRENCY=1`.
- **Reasoning models**: leave `LLM_REASONING` at `off`. A model that thinks before every answer is several times slower and can spend its whole output budget on thinking.
- **Daily caps**: `LLM_MAX_REQUESTS_PER_DAY` and `LLM_MAX_TOKENS_PER_DAY` are spread evenly through the day so the village does not go quiet in the evening. A world that runs around the clock asks a fast model about 20,000 times a day.
- **Upgrading from a version with model settings in the admin panel**: the settings and key it saved are deleted from the database on the first start, so set the `LLM_*` variables before deploying.

How the model is used:

- It is asked only at decision points: on waking (a plan for the day), when something notable happens, when a plan block ends, in danger (fight or flee), in conversations, when a beaten opponent yields (spare or finish), and once a night to reflect.
- Tool calls are sent natively. If a model answers in prose instead of calling its tool, the client switches to JSON-schema output, which local servers enforce with a grammar. Tested with `qwen/qwen3.6-35b-a3b` in LM Studio: 85% usable answers natively, 20 of 20 as JSON output, about 6 seconds each with reasoning off.
- A governor measures the endpoint's speed and keeps demand below it, so a slow local model gets fewer, more important requests. `LLM_REQUESTS_PER_MINUTE` is a hard cap on top of that. While the model has a backlog, a world running faster than 1× holds at 1×.
- Every answer is checked against the world as it is by then. Stale, invalid or missing answers fall back to the routines.
- The 🧠 pill in the top bar opens the decision log for admins: each prompt, the raw reply, the commands it produced, latency and tokens.

To measure a model before letting it run the village (real prompts from a world a few days in):

```bash
cd server && cargo run --release -p fsw-brain --bin probe -- http://localhost:11434/v1 <model> 20
```

To try the whole path with no model at all, run the mock endpoint (`MOCK_DELAY` sets its latency in milliseconds, `MOCK_MODE=text` makes it answer with JSON in the text):

```bash
node tools/mock-llm.mjs
```

and start the server against it:

```bash
cd server && LLM_BASE_URL=http://localhost:8787/v1 LLM_MODEL=mock AUTH_TOKEN=dev-token-0123456789 PUBLIC_ORIGIN=http://localhost:8080 STATIC_DIR=../dist cargo run --release
```

## Chat

Everyone watching shares a chat along the bottom of the screen. It can be expanded, collapsed to one line showing the latest message, or hidden behind a 💬 button that counts what you missed.

- Each person gets a made-up name when they connect ("Mossy Wren"). Settings changes it, as long as nobody else in the chat has it (ignoring case). The chosen name is kept in this browser and asked for on each connect; if someone else has it by then, you get a made-up one.
- The chat says when someone joins and when they leave. A reload, or a tab that was hidden for a while, drops the connection for a moment, so someone only counts as gone after five minutes away; back sooner, and neither is mentioned. Until then their name stays theirs.
- Signed in with an account (Google, GitHub and the rest), all your tabs share one name. With a shared token or sign-in off, everyone is the same account, so each tab is its own person.
- The server keeps the last 50 messages, in memory, and sends them to everyone who connects. They survive a new world but not a server restart.
- A message is at most 500 characters, text only. It may use **bold**, *italic* (or _italic_), ~~strikethrough~~, `code`, fenced code blocks, links (`[text](https://…)` or a bare `https://…` address; nothing but http and https) and emoji, from your keyboard or the 😀 picker. Nothing in a message is ever treated as HTML.
- Each person may send about one message a second, in bursts of up to five.
- Every message passes through `filter` in [server/crates/server/src/chat.rs](server/crates/server/src/chat.rs) before anyone sees it. It allows everything for now; it is where moderation goes.

## Credits

People watching earn credits and spend them on things that happen in the village.

- **Earning.** A browser tab that is on screen and has the focus tells the server so every 30 seconds, and each report that arrives on time earns half a minute's worth (`CREDITS_PER_MINUTE`, 2 by default, up to `CREDITS_CAP`). A hidden or unfocused tab earns nothing, and neither do extra tabs or reports sent faster: the server counts at most one every 25 seconds per person. A new account starts with 10.
- **Spending.** The ✦ pill opens the list. Gifts (food 5, a healing potion 8, a small gift 10) go into the pack of the villager you have selected. One of the dead (20 to 50) rises where your view is centred, whatever the hour, and crumbles after three hours of the village's day. A feast (80) and a healing (100) are for everyone. A raid (150) and a necromancer (250) come from the graveyard. The server refuses, and takes nothing for, what cannot happen: a full pack, a spot nothing can stand on, too many of the dead abroad already, a paused world. One person may buy three things in a row, then one every five seconds.
- **The villagers know who did it.** To them you are one of the powers that watch the valley, known by your chat name. The chat and the chronicle say what you did; villagers remember it, thank you or plead with you on the spot, pray to you at the chapel and the shrine, and with a language model connected they speak of you in their own words. A new chat name is a new power to them. Names end up in saves and exports with the rest of the chronicle.
- **Prayer pays.** When three different villagers have spoken your name in one day of the village, you earn 15 credits, once that day, whether or not you are watching.
- **Where it is kept.** With provider sign-in the balance belongs to the account and lives in the database, with a record of each purchase kept for 90 days. With token sign-in or sign-in off there are no accounts, so each tab has its own balance, in memory, gone after an hour away or a server restart.
- The offers and their prices are the `OFFERS` table in [server/crates/server/src/credits.rs](server/crates/server/src/credits.rs); what each does in the village is in [server/crates/sim/src/systems/viewer_events.rs](server/crates/sim/src/systems/viewer_events.rs).

## Sound

The game ships ambient loops, effects and background music, all CC0, public domain or CC BY ([credits](public/audio/CREDITS.md)). The ambience follows the camera, the time of day and what the villagers are doing. The music follows what you are watching, from calm day, evening and night pieces to tension when the alarm is up or monsters are near, battle when a fight is in view, and grief after a death, with short victory and defeat stings. Settings turns ambience, effects and music on or off, and sets their volumes, in this browser only.

The files are built by `pnpm audio` (`tools/audio/build-audio.ts`, which needs Bun, ffmpeg and bsdtar) from the sources listed in `tools/audio/sources.ts`. To swap a sound, change its entry there and rebuild, or drop your own file into `public/audio/` under the slot's name ([public/audio/README.md](public/audio/README.md)). Only CC0, public-domain or CC BY sounds go in, each credited in `public/audio/CREDITS.md`.

Character speech is not implemented yet, but the seam is in place: every spoken line arrives from the server complete, with the speaker's voice profile, position and duration, and passes through `SpeechBus` ([src/speech/speechBus.ts](src/speech/speechBus.ts)), so a text-to-speech sink can be added in the browser without touching the server.

## Tests and tools

What CI runs ([.github/workflows/ci.yml](.github/workflows/ci.yml)) on every push and pull request:

```bash
pnpm typecheck && pnpm test     # the client
cd server && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked
```

- `pnpm test` covers the client's stream playback, the view of the world it builds from the stream, the world layer, the audio mood and settings, and the architecture rules (the browser never reaches the simulation, the brain or saves; three.js stays in `src/render`).
- `cargo test` covers the Rust world against golden fixtures from the TypeScript version, the sim (determinism through a save round trip, invariants over several days, a 20-day run), old browser saves still loading, the prompts byte for byte, the brain against a fake model, sign-in against a fake OpenID provider, the store, and the HTTP routes.
- `cargo test -- --ignored` adds the 200-day soak and, with `node tools/mock-llm.mjs` running, a real HTTP round trip to the mock model.
- Two committed files are written by the server and checked by its tests. In `server/`, after changing a type in `server/crates/server/src/api/wire.rs`, run `cargo run --bin export-api` (rewrites `src/generated/api.ts`); after changing the stream encoder, run `cargo run --bin export-frames` (rewrites `tests/fixtures/server-frames.json`, which the client's tests replay).

Other tools:

- `cargo run --release -p fsw-sim --bin soak -- 30 1337 --log` runs the world headlessly for 30 days (seed 1337) and prints its chronicle; `--json` prints statistics, `--trace` one line per tick.
- `AUTH_TOKEN=… node tools/sse-probe.mjs <url> [seconds]` checks that the event stream reaches a client through a hosting proxy unbuffered.
- `bun tools/assets/export-server-data.ts` writes `server/data/*.json` (model bounds, clip durations, tile heights) from `src/generated`; `pnpm assets` writes them too.
- `bun tools/dump-world.ts [outDir]` writes the TypeScript world layer's output as the golden fixtures the Rust world is checked against.

## How it is put together

| Folder | What it holds |
| --- | --- |
| `src/core` | Seeded RNG, 2D maths, game time |
| `src/world` | The hex map, places and activity spots, scatter, navigation grid, pathfinding (terrain, scenery and camera bounds in the browser) |
| `src/net` | The event stream, the world as this viewer knows it, the jitter buffer, REST calls |
| `src/render` | three.js scene, actors, effects, labels, camera, lighting, animation table |
| `src/ui` | The HUD, chat, admin panel, login and no-access pages (Preact and signals) |
| `src/audio`, `src/speech` | Ambient mixer, music and the speech bus |
| `src/generated` | Wire types (from the Rust server), asset manifest, tile heights; never edited by hand |
| `tests` | Client tests that span layers: architecture rules, stream replay, server data |
| `tools` | Asset and audio builds, Blender prop generator, mock model endpoint, stream probe, world dump for the golden fixtures |
| `server` | The Rust server, a Cargo workspace: `core` and `world` (shared maths and the static world), `sim` (the simulation), `brain` (the language model: client, queue, governor, prompts, validation), `server` (HTTP, event stream, chat, sign-in, SQLite store) |
| `deploy`, `Dockerfile`, `fly.toml` | The container image, Litestream and Fly.io settings ([docs/deploy.md](docs/deploy.md)) |

On the server the sim depends only on `core` and `world`; the brain reads the world and talks to the sim through commands applied at tick boundaries, so a run is reproducible for a given command stream. The browser depends on `core`, `world`, `net` and `generated`, never on the sim.

The TypeScript simulation, brain, browser saves and reference server that the Rust port was checked against are kept at the `ts-sim-reference` tag (`git checkout ts-sim-reference`), with the tools that compare the two (`bun tools/soak.ts … --trace`, `python3 tools/soak-parity.py`, and the fixture writers). Comments in the Rust crates that name `src/sim/…` or `src/brain/…` files refer to that tag.

## Assets

The 3D models are from the [KayKit](https://kaylousberg.com) collection by Kay Lousberg (CC0). The processed subset the game uses is committed under `public/assets/`. To rebuild it from the original collection:

```bash
KAYKIT_DIR="/path/to/The Complete KayKit Collection v7" pnpm assets
```

The build packs the models with meshopt compression, shrinks the palette textures to 256 pixels a side, and gives every URL in the manifest a hash of its file (`?v=…`), because the server lets browsers keep `/assets` for a year.

Characters come on two skeletons: most use KayKit's medium rig, and the Greymane brothers use `Barbarian_Large` on the large rig. Each rig gets its own clip file in `public/assets/animations/`. The large rig has no work, rest or ranged clips of its own, so the build copies those over from the medium rig, adjusted to the longer bones (`rigs` in `tools/assets/asset-list.mjs`). The sim times actions by the medium rig's clips, and the client stretches the large rig's one-off clips, such as attacks, to finish at the same moment.

A few props the collection lacks at character scale (a wooden fence, wildflowers, mushrooms, a notice board) are generated with Blender and bundled by the same command:

```bash
/Applications/Blender.app/Contents/MacOS/Blender -b -P tools/blender/make_props.py
```

## Working on the code

[AGENTS.md](AGENTS.md) has the rules a change has to keep (for people and coding agents alike), with more for the server in [server/AGENTS.md](server/AGENTS.md) and for the web client in [src/AGENTS.md](src/AGENTS.md).
