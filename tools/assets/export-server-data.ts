// Writes server/data/*.json from the committed src/generated files.
//   bun tools/assets/export-server-data.ts
import { join } from 'node:path';
import { assetManifest } from '../../src/generated/assetManifest';
import { BRIDGE_GRID, BRIDGE_HEIGHTS, NO_GROUND, TILE_GRID, TILE_HEIGHTS } from '../../src/generated/tileHeights';
// @ts-expect-error plain JavaScript module shared with build-assets.mjs
import { writeServerData } from './server-data.mjs';

writeServerData(join(import.meta.dirname, '../..'), assetManifest, {
  TILE_GRID,
  BRIDGE_GRID,
  NO_GROUND,
  BRIDGE_HEIGHTS,
  TILE_HEIGHTS,
});
console.log('wrote server/data/asset-manifest.json and server/data/tile-heights.json');
