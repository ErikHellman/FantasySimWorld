// Runs the world with the LLM brain against tools/mock-llm.mjs, in real time
// at a raised speed, and writes what the browser's "Export" button would write
// ({version, savedAt, state}). The result is committed as the save fixture the
// Rust sim must keep reading (server/crates/sim/tests/fixtures).
//
//   bun tools/make-export-fixture.ts [hours] [seed] [speed] [--until monsters|conversations] [--out file]
//
// Runs at least `hours` game hours from 06:00 on day 1, then (with --until)
// on until monsters are about or a conversation is under way, for at most a
// further game day. The committed fixtures were made with
//   bun tools/make-export-fixture.ts 7 20261002 10 --until conversations
//   bun tools/make-export-fixture.ts 39 20261002 10 --until monsters
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { Brain } from '../src/brain/brain';
import { TICKS_PER_GAME_MINUTE, gameTime } from '../src/core/time';
import { migrate } from '../src/persist/migrations';
import { DEFAULT_SETTINGS } from '../src/persist/settings';
import { createState } from '../src/sim/create';
import { Sim } from '../src/sim/sim';
import { SAVE_VERSION } from '../src/sim/state';

const option = (name: string): string | undefined => {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
};
const hours = Number(process.argv[2] ?? 41);
const seed = Number(process.argv[3] ?? 20261002);
const speed = Number(process.argv[4] ?? 10);
const PORT = 8799;
const FRAME_MS = 25;

const mock = spawn('node', [join(import.meta.dirname, 'mock-llm.mjs'), String(PORT)], {
  env: { ...process.env, MOCK_DELAY: '150' },
  stdio: 'ignore',
});
await new Promise((done) => setTimeout(done, 500));

const sim = new Sim(createState(seed));
const statuses = { requests: 0, failures: 0 };
const brain = new Brain(sim, {
  speed: () => speed,
  status: (status) => Object.assign(statuses, { requests: status.requests, failures: status.failures }),
  log: () => {},
});
brain.configure({ ...DEFAULT_SETTINGS.llm, enabled: true, baseUrl: `http://localhost:${PORT}/v1`, model: 'mock', concurrency: 4, requestsPerMinute: 0 });

const until = option('--until');
const minTick = Math.round(hours * 60 * TICKS_PER_GAME_MINUTE);
const maxTick = until ? minTick + 24 * 60 * TICKS_PER_GAME_MINUTE : minTick;
const done = (): boolean => {
  const state = sim.state;
  if (state.tick >= maxTick) return true;
  if (state.tick < minTick) return false;
  if (until === 'monsters') return Object.values(state.monsters).some((m) => m.phase !== 'rising');
  if (until === 'conversations') return Object.values(state.conversations).some((c) => c.author === 'llm' && c.lines.length > 2);
  return true;
};
let owed = 0;
let lastReport = 0;
while (!done()) {
  owed += (FRAME_MS / 1000) * 20 * speed;
  for (; owed >= 1 && !done(); owed--) brain.handle(sim.step());
  brain.update();
  if (sim.state.tick - lastReport >= 2400) {
    lastReport = sim.state.tick;
    const time = gameTime(sim.state.tick);
    console.log(`day ${time.day} ${time.hour.toFixed(1)}h: ${statuses.requests} requests, ${statuses.failures} failures`);
  }
  await new Promise((done) => setTimeout(done, FRAME_MS));
}
mock.kill();

const state = sim.state;
const document = { version: SAVE_VERSION, savedAt: Date.now(), state };
if (!migrate(JSON.parse(JSON.stringify(document)))) throw new Error('The export does not load back');
const day = gameTime(state.tick).day;
const out = resolve(option('--out') ?? join(import.meta.dirname, '..', `server/crates/sim/tests/fixtures/export-day-${day}.json`));
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify(document));
const llmPlans = Object.values(state.characters).filter((c) => c.planSource === 'llm').length;
console.log(
  `wrote ${out}: tick ${state.tick}, ${state.order.length} alive, ${Object.keys(state.dead).length} dead, ${Object.keys(state.monsters).length} monsters, ` +
    `${Object.keys(state.conversations).length} conversations, ${llmPlans} LLM plans, ${statuses.requests} requests`,
);
process.exit(0);
