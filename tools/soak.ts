// Headless run of the simulation with the local routine brain only.
//   bun tools/soak.ts [days] [seed] [--log | --json | --trace]
//
//   --log    print the chronicle as it is written
//   --json   print the end-of-run statistics as one JSON object (compared against the Rust sim)
//   --trace  print one line per tick: tick, RNG state, every position to 1e-6 and every intent kind.
//            Diffing two traces finds the first tick where two implementations part ways; the RNG
//            column is the canary, since a different number of draws is always a logic error.
import { TICKS_PER_DAY, formatClock, gameTime } from '../src/core/time';
import { createState } from '../src/sim/create';
import { Sim } from '../src/sim/sim';
import type { SimState } from '../src/sim/state';
import { living } from '../src/sim/util';

const days = Number(process.argv[2] ?? 10);
const seed = Number(process.argv[3] ?? 1337);
const log = process.argv.includes('--log');
const json = process.argv.includes('--json');
const trace = process.argv.includes('--trace');

function traceLine(state: SimState): string {
  const parts = [`${state.tick} ${state.rng}`];
  for (const id of state.order) {
    const c = state.characters[id]!;
    parts.push(`${id}:${c.pos.x.toFixed(6)},${c.pos.z.toFixed(6)},${c.intent?.kind ?? '-'}`);
  }
  for (const [id, m] of Object.entries(state.monsters)) parts.push(`${id}:${m.pos.x.toFixed(6)},${m.pos.z.toFixed(6)},${m.phase}`);
  return parts.join(' ');
}

const sim = new Sim(createState(seed));
const counts: Record<string, number> = {};
const started = performance.now();
let chronicleSeen = sim.state.chronicle.length;
const out: string[] = [];

for (let tick = 0; tick < days * TICKS_PER_DAY; tick++) {
  for (const event of sim.step()) {
    const key = event.type === 'conversation_started' ? `conversation:${event.tone}` : event.type;
    counts[key] = (counts[key] ?? 0) + 1;
  }
  if (trace) {
    out.push(traceLine(sim.state));
    if (out.length >= 2000) process.stdout.write(`${out.splice(0).join('\n')}\n`);
  }
  if (log) {
    for (; chronicleSeen < sim.state.chronicle.length; chronicleSeen++) {
      const entry = sim.state.chronicle[chronicleSeen]!;
      const time = gameTime(entry.tick);
      console.log(`D${time.day} ${formatClock(time.minuteOfDay)} [${entry.kind}] ${entry.text}`);
    }
    if (sim.state.chronicle.length >= 590) chronicleSeen = Math.min(chronicleSeen, sim.state.chronicle.length);
  }
}
if (trace && out.length > 0) process.stdout.write(`${out.join('\n')}\n`);

const elapsed = (performance.now() - started) / 1000;
const state = sim.state;
const kinds: Record<string, number> = {};
for (const entry of state.chronicle) kinds[entry.kind] = (kinds[entry.kind] ?? 0) + 1;
const labelsOf = (id: string): Record<string, number> => {
  const c = state.characters[id]!;
  const labels: Record<string, number> = {};
  for (const other of Object.keys(c.relationships)) {
    if (!state.characters[other]) continue;
    const label = c.relationships[other]!.label;
    labels[label] = (labels[label] ?? 0) + 1;
  }
  return labels;
};

if (trace) {
  // The trace is the whole output.
} else if (json) {
  console.log(
    JSON.stringify({
      days,
      seed,
      tick: state.tick,
      rng: state.rng,
      elapsedSeconds: Number(elapsed.toFixed(2)),
      events: counts,
      alive: state.order.length,
      dead: Object.keys(state.dead).length,
      arrivals: state.director.arrivals,
      deaths: Object.entries(state.dead).map(([id, record]) => ({ id, name: record.name, day: record.day, cause: record.cause })),
      chronicleKinds: kinds,
      characters: living(state).map((c) => ({
        id: c.id,
        name: c.profile.name,
        hp: Math.round(c.hp),
        hunger: Math.round(c.needs.hunger),
        energy: Math.round(c.needs.energy),
        social: Math.round(c.needs.social),
        kills: c.kills,
        memories: c.memories.length,
        inventory: c.inventory,
        partner: c.partnerId,
        labels: labelsOf(c.id),
      })),
    }),
  );
} else {
  printReport();
}

function printReport(): void {
  console.log(`\n${days} days in ${elapsed.toFixed(1)} s (${Math.round((days * TICKS_PER_DAY) / elapsed)} ticks/s)`);
  console.log('events', counts);
  console.log(`alive ${state.order.length}, dead ${Object.keys(state.dead).length}, arrivals ${state.director.arrivals}`);
  for (const [, record] of Object.entries(state.dead)) console.log(`  † ${record.name}, day ${record.day}: ${record.cause}`);
  console.log('chronicle kinds (last 600)', kinds);
  for (const c of living(state)) {
    console.log(
      `${c.profile.name.padEnd(20)} hp ${Math.round(c.hp).toString().padStart(3)} hunger ${Math.round(c.needs.hunger).toString().padStart(3)} energy ${Math.round(c.needs.energy).toString().padStart(3)} social ${Math.round(c.needs.social).toString().padStart(3)} kills ${c.kills} mem ${c.memories.length} food ${c.inventory.food} potion ${c.inventory.potion} gift ${c.inventory.gift}${c.partnerId ? ' ♥' + state.characters[c.partnerId]?.profile.name : ''} ${JSON.stringify(labelsOf(c.id))}`,
    );
  }
}
