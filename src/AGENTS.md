# AGENTS.md: the web client

The repository-wide rules are in [../AGENTS.md](../AGENTS.md). This file is about `src/`, the browser side: a viewer of the world the server runs.

```bash
pnpm typecheck && pnpm test
```

## Layers

`tests/architecture.test.ts` fails when a file imports outside its layer, when a folder appears in `src/` that is in no layer, or when anything reachable from `main.ts` is a simulation, brain or save module. The allowed imports:

| Layer | May import | What it is |
| --- | --- | --- |
| `core` | core, generated | RNG, 2D maths, game time, asset types |
| `world` | core, world, generated | The static map: terrain, places, scatter, nav grid. Geometry only, mirrored by `server/crates/world` |
| `generated` | core, generated | Written by tools; never edited by hand |
| `net` | net, generated | The event stream, playback, the view of the world, REST calls |
| `render` | core, world, render, generated, net | Everything three.js |
| `audio` | core, world, audio, generated | The mixer, ambience, music and mood |
| `speech` | core, speech, generated | `SpeechBus`, the seam for text to speech |
| `ui` | core, ui, net, audio, generated | Preact components and the signals store |
| `main.ts` | everything above | Wires the layers together |

Also enforced: three.js is imported only in `render/`, the folders `sim`, `brain`, `persist` and `app` must not exist, and `idb` / `fake-indexeddb` must not come back. Adding a layer means adding it to the table in the test, deliberately.

## How a frame reaches the screen

1. `net/stream.ts` opens `GET /api/stream` (server-sent events) and reconnects with `Last-Event-ID`.
2. `net/playback.ts` buffers `tick` frames behind a short cushion (scaled by the speed) so motion stays smooth, and applies a backlog at once when it falls behind.
3. `net/viewWorld.ts` keeps the world as this viewer knows it: entities with their static and dynamic fields, decoded from the wire.
4. `render/` draws the `ViewWorld`; `audio/` and `speech/` react to the same events; `ui/store.ts` holds the HUD's state as Preact signals, fed by `status`, `roster`, `detail`, `brainlog` and the chronicle.

The wire types are in `generated/api.ts`, exported from `server/crates/server/src/api/wire.rs`. To change what the server sends, change the Rust side first and run `cargo run --bin export-api` in `server/` (see [../server/AGENTS.md](../server/AGENTS.md)). The client must not compute game state the server does not send; the roster, status and inspector details all come from the server's digest.

## Things that are easy to get wrong

- **`src/world` has a twin** in `server/crates/world`. A change to the map, places or nav grid goes into both and regenerates the golden fixtures with `bun tools/dump-world.ts`; the client warns when the server's `hello.worldHash` differs from its own.
- **A selected id is a villager, one of the dead, a ghost, or a site.** `actions.select` asks `GET /api/characters/{id}`, `GET /api/monsters/{id}` or `GET /api/ghosts/{id}` by `world.isMonster` and `world.isGhost`, and the stream's `detail` event carries a `Detail` tagged by `kind` (`villager`, `monster` or `ghost`); the inspector draws a card for each. `SceneManager.pick` leaves out bodies only. A building (`b:<place>`) or something built (`n:<n>`) is a site (`isSiteId`): it is fetched from `GET /api/sites/{id}` into `ui.site` and drawn by `SitePanel`, and the camera does not follow it.
- **Signed-in state and roles**: the UI shows admin controls from `ui.me.value.role`, but the server is what enforces them. Never rely on the client to keep a viewer from an admin action.
- **The model's settings are not the browser's business**: the admin panel shows them read-only and offers a connection test. Do not add inputs for them.
- **Audio settings** (on/off and volumes) are per browser, in local storage. Sounds are slots (`audio/slots.ts`, `audio/musicTracks.ts`) filled from `public/audio/`; only CC0, public domain or CC BY files, credited in `public/audio/CREDITS.md`.
- **Rendering** goes through `three/webgpu` with a WebGL 2 fallback (`render/renderer.ts`); keep both working. `?renderer=webgl` or `?renderer=webgpu` forces one.
- **Quality tiers** live in `render/quality.ts`: `high` is the full picture, `low` is what phones get, and `?quality=` forces either. A new cost (a light, a bigger texture, a post-processing pass) needs a `low` answer there. Static scenery goes through `SceneryBuilder`, which instances what repeats and bakes the rest into one mesh per material and chunk; do not add static meshes to the scene one by one. Three layers stand outside it because the server can change what they draw: `render/vegetation.ts` (trees and rocks, one instanced pool per model, so one tree can sway, fall or grow back by changing its matrix), `render/structures.ts` (every building, with its ruin, plot, scaffolding, bigger model and fires) and `render/marks.ts` (pieces, ash, pits, mown ground and what the village built). Props and grass stay in the builder, which gives out a handle per thing so one can be hidden; a hidden piece of a baked chunk is baked again, one chunk a tick.

## Checking a change in a browser

- Start the server (see the root AGENTS.md; `AUTH_MODE=off` and a scratch `DATA_DIR` are simplest) and `pnpm dev`, then open <http://localhost:5173>. Vite forwards `/api` and `/auth` to the server.
- The page exposes `window.__debug = { world, view, playback, stream, renderer, ui, actions, audio, mood, stats }`. `__debug.stats()` reports frame times, draw calls, triangles and GPU memory; check it before and after a change to the rendering. The renderer's counts leave out the instanced draws of the shadow pass, so compare shadow casters by counting them in the scene. Read state through it rather than the DOM; Preact can render late.
- A browser tab that is hidden or in the background barely runs `requestAnimationFrame`, so the picture and the camera stand still there even though the stream keeps arriving. Screenshots of a hidden pane can show a stale frame.
- `?render=off` follows the stream without drawing, for headless checks where software WebGL is far too slow; `?debug=nav` draws the navigation grid and activity spots.
