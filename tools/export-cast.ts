// Writes the authored cast (src/sim/data/cast.ts) as server/crates/sim/data/cast.json,
// which the Rust sim embeds. Run again after editing the cast.
//   bun tools/export-cast.ts
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CAST, NEWCOMERS, SEED_BONDS } from '../src/sim/data/cast';

const path = join(import.meta.dirname, '..', 'server/crates/sim/data/cast.json');
writeFileSync(path, `${JSON.stringify({ cast: CAST, seedBonds: SEED_BONDS, newcomers: NEWCOMERS }, null, 1)}\n`);
console.log(`wrote ${path}`);
