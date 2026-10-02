// The two generated files the world layer depends on (model bounds, clip
// durations, tile heights), as JSON for the Rust server to embed with
// include_str!. Written by build-assets.mjs, or from the committed TypeScript
// copies with `bun tools/assets/export-server-data.ts` when the KayKit
// collection is not at hand.
import fs from 'node:fs';
import path from 'node:path';

export function writeServerData(repo, manifest, heights) {
  const dir = path.join(repo, 'server/data');
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'asset-manifest.json'), `${JSON.stringify(manifest, null, 1)}\n`);
  fs.writeFileSync(path.join(dir, 'tile-heights.json'), `${JSON.stringify(heights, null, 1)}\n`);
}
