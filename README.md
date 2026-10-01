# Fantasy Sim World

A browser-only 3D world in the KayKit style where LLM-driven fantasy characters live in a village: they follow daily routines, talk, form relationships, fight the monsters that rise at night and, if pushed far enough, each other. You watch from a free top-down camera.

The design is in [docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md](docs/superpowers/specs/2026-10-01-fantasy-sim-world-design.md).

## Run

```bash
pnpm install
pnpm dev
```

Open the printed URL in a browser with WebGPU or WebGL 2. Add `?renderer=webgl` or `?renderer=webgpu` to force a backend, and `?debug=nav` to see the navigation grid and activity slots.

```bash
pnpm test        # unit tests
pnpm build       # type-check and build the static bundle into dist/
```

## Assets

The 3D models are from the [KayKit](https://kaylousberg.com) collection by Kay Lousberg (CC0). The processed subset the game uses is committed under `public/assets/`. To rebuild it from the original collection:

```bash
KAYKIT_DIR="/path/to/The Complete KayKit Collection v7" pnpm assets
```
