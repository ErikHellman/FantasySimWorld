// Headless run of the simulation with the local routine brain only.
//   bun tools/soak.ts [days] [seed] [--log]
import { TICKS_PER_DAY, formatClock, gameTime } from '../src/core/time';
import { createState } from '../src/sim/create';
import { Sim } from '../src/sim/sim';
import { living } from '../src/sim/util';

const days = Number(process.argv[2] ?? 10);
const seed = Number(process.argv[3] ?? 1337);
const log = process.argv.includes('--log');

const sim = new Sim(createState(seed));
const counts: Record<string, number> = {};
const started = performance.now();
let chronicleSeen = sim.state.chronicle.length;

for (let tick = 0; tick < days * TICKS_PER_DAY; tick++) {
  for (const event of sim.step()) {
    const key = event.type === 'conversation_started' ? `conversation:${event.tone}` : event.type;
    counts[key] = (counts[key] ?? 0) + 1;
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

const elapsed = (performance.now() - started) / 1000;
const state = sim.state;
console.log(`\n${days} days in ${elapsed.toFixed(1)} s (${Math.round((days * TICKS_PER_DAY) / elapsed)} ticks/s)`);
console.log('events', counts);
console.log(`alive ${state.order.length}, dead ${Object.keys(state.dead).length}, arrivals ${state.director.arrivals}`);
for (const [id, record] of Object.entries(state.dead)) console.log(`  † ${record.name}, day ${record.day}: ${record.cause}`);
const kinds: Record<string, number> = {};
for (const entry of state.chronicle) kinds[entry.kind] = (kinds[entry.kind] ?? 0) + 1;
console.log('chronicle kinds (last 600)', kinds);
for (const c of living(state)) {
  const labels: Record<string, number> = {};
  for (const id of Object.keys(c.relationships)) {
    if (!state.characters[id]) continue;
    const label = c.relationships[id]!.label;
    labels[label] = (labels[label] ?? 0) + 1;
  }
  console.log(
    `${c.profile.name.padEnd(20)} hp ${Math.round(c.hp).toString().padStart(3)} hunger ${Math.round(c.needs.hunger).toString().padStart(3)} energy ${Math.round(c.needs.energy).toString().padStart(3)} social ${Math.round(c.needs.social).toString().padStart(3)} kills ${c.kills} mem ${c.memories.length} food ${c.inventory.food} potion ${c.inventory.potion} gift ${c.inventory.gift}${c.partnerId ? ' ♥' + state.characters[c.partnerId]?.profile.name : ''} ${JSON.stringify(labels)}`,
  );
}
