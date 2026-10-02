import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, sep } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * The web client is a viewer of a world that runs on the server. Its layers:
 *
 *   core <- world <- {render, audio}      world is geometry only (terrain, scenery, camera bounds)
 *   generated <- net                      the stream, the view world, playback, REST calls
 *   render draws net's ViewWorld; ui shows net's data and calls its REST helpers
 *
 * and nothing the client reaches may import the simulation, the brain, the
 * browser saves or the old game loop (src/sim, src/brain, src/persist,
 * src/app). Those stay in the repository as the reference for the Rust port
 * until phase 7, under their old rule (core <- world <- sim <- brain). three.js
 * stays inside render/.
 */
const SRC = join(__dirname, '..', 'src');
const SERVER_ONLY = ['sim', 'brain', 'persist', 'app'];
const ALLOWED: Record<string, string[]> = {
  core: ['core', 'generated'],
  world: ['core', 'world', 'generated'],
  net: ['net', 'generated'],
  render: ['core', 'world', 'render', 'generated', 'net'],
  audio: ['core', 'world', 'audio', 'generated'],
  speech: ['core', 'speech', 'generated'],
  ui: ['core', 'ui', 'net', 'audio', 'generated'],
  'main.ts': ['core', 'world', 'render', 'audio', 'speech', 'net', 'ui', 'generated'],
  // The reference sim; it reads animation timings from render/anims.ts, which moved there from sim/data.
  sim: ['core', 'world', 'sim', 'generated', 'render/anims.ts'],
  brain: ['core', 'world', 'sim', 'brain', 'persist'],
};

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return files(path);
    return /\.(ts|tsx)$/.test(name) && !name.includes('.test.') ? [path] : [];
  });
}

function imports(path: string): string[] {
  const source = readFileSync(path, 'utf8');
  return [...source.matchAll(/(?:from|import)\s+'([^']+)'/g)].map((match) => match[1] as string);
}

/** The file a relative import points at. */
function resolveImport(from: string, spec: string): string | null {
  const base = join(dirname(from), spec);
  for (const candidate of [base, `${base}.ts`, `${base}.tsx`, join(base, 'index.ts')]) {
    if (existsSync(candidate) && statSync(candidate).isFile()) return candidate;
  }
  return null;
}

const layerOf = (path: string): string => relative(SRC, path).split(sep)[0] as string;
const posix = (path: string): string => relative(SRC, path).split(sep).join('/');

describe('architecture', () => {
  const all = files(SRC);

  it('keeps each layer to its allowed dependencies', () => {
    const problems: string[] = [];
    for (const path of all) {
      const allowed = ALLOWED[layerOf(path)];
      if (!allowed) continue;
      for (const spec of imports(path)) {
        if (!spec.startsWith('.')) continue;
        const target = resolveImport(path, spec);
        if (!target) {
          problems.push(`${posix(path)} imports ${spec}, which does not exist`);
          continue;
        }
        if (!allowed.includes(layerOf(target)) && !allowed.includes(posix(target))) problems.push(`${posix(path)} imports ${spec}`);
      }
    }
    expect(problems).toEqual([]);
  });

  it('never reaches the sim, brain, saves or old loop from the client entry', () => {
    const seen = new Set<string>();
    const queue = [join(SRC, 'main.ts')];
    const problems: string[] = [];
    while (queue.length > 0) {
      const path = queue.pop() as string;
      if (seen.has(path)) continue;
      seen.add(path);
      if (SERVER_ONLY.includes(layerOf(path))) problems.push(posix(path));
      for (const spec of imports(path)) {
        const target = spec.startsWith('.') ? resolveImport(path, spec) : null;
        if (target) queue.push(target);
      }
    }
    expect(problems).toEqual([]);
    expect(seen.size).toBeGreaterThan(20);
  });

  it('uses three.js only in the renderer', () => {
    const problems = all
      .filter((path) => layerOf(path) !== 'render')
      .filter((path) => imports(path).some((spec) => spec === 'three' || spec.startsWith('three/')))
      .map(posix);
    expect(problems).toEqual([]);
  });
});
