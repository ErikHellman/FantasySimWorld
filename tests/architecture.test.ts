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
 * The simulation, the brain and the saves live on the server (server/crates);
 * the TypeScript versions are kept at the `ts-sim-reference` tag and must not
 * come back. three.js stays inside render/.
 */
const SRC = join(__dirname, '..', 'src');
const SERVER_ONLY = ['sim', 'brain', 'persist', 'app'];
const ALLOWED: Record<string, string[]> = {
  core: ['core', 'generated'],
  world: ['core', 'world', 'generated'],
  generated: ['core', 'generated'],
  net: ['net', 'generated'],
  render: ['core', 'world', 'render', 'generated', 'net'],
  audio: ['core', 'world', 'audio', 'generated'],
  speech: ['core', 'speech', 'generated'],
  ui: ['core', 'ui', 'net', 'audio', 'generated'],
  'main.ts': ['core', 'world', 'render', 'audio', 'speech', 'net', 'ui', 'generated'],
};
/** Packages the client no longer needs: saves and the model's key live on the server. */
const GONE_PACKAGES = ['idb', 'fake-indexeddb'];

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
      if (!allowed) {
        problems.push(`${posix(path)} is in no known layer`);
        continue;
      }
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

  it('has no simulation, brain, saves or game loop of its own', () => {
    expect(SERVER_ONLY.filter((dir) => existsSync(join(SRC, dir)))).toEqual([]);
    const pkg = JSON.parse(readFileSync(join(__dirname, '..', 'package.json'), 'utf8')) as Record<string, Record<string, string> | undefined>;
    const deps = { ...pkg.dependencies, ...pkg.devDependencies };
    expect(GONE_PACKAGES.filter((name) => name in deps)).toEqual([]);
    const problems = all.filter((path) => imports(path).some((spec) => GONE_PACKAGES.includes(spec))).map(posix);
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
