import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * The dependency rule: core <- world <- sim <- brain. The sim must run with no
 * renderer, UI, audio or network, and three.js must stay inside render/.
 */
const SRC = join(__dirname, '..', 'src');
const ALLOWED: Record<string, string[]> = {
  core: ['core', 'generated'],
  world: ['core', 'world', 'generated'],
  sim: ['core', 'world', 'sim', 'generated'],
  brain: ['core', 'world', 'sim', 'brain', 'persist'],
  speech: ['core', 'sim', 'speech'],
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
  return [...source.matchAll(/from\s+'([^']+)'/g)].map((match) => match[1] as string);
}

describe('architecture', () => {
  const all = files(SRC);

  it('keeps each layer to its allowed dependencies', () => {
    const problems: string[] = [];
    for (const path of all) {
      const layer = relative(SRC, path).split(sep)[0] as string;
      const allowed = ALLOWED[layer];
      if (!allowed) continue;
      for (const spec of imports(path)) {
        if (!spec.startsWith('.')) continue;
        const target = relative(SRC, join(path, '..', spec)).split(sep)[0] as string;
        if (!allowed.includes(target)) problems.push(`${relative(SRC, path)} imports ${spec}`);
      }
    }
    expect(problems).toEqual([]);
  });

  it('uses three.js only in the renderer', () => {
    const problems = all
      .filter((path) => relative(SRC, path).split(sep)[0] !== 'render')
      .filter((path) => imports(path).some((spec) => spec === 'three' || spec.startsWith('three/')))
      .map((path) => relative(SRC, path));
    expect(problems).toEqual([]);
  });
});
