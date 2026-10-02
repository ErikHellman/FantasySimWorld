import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { assetManifest } from '../src/generated/assetManifest';
import { BRIDGE_GRID, BRIDGE_HEIGHTS, NO_GROUND, TILE_GRID, TILE_HEIGHTS } from '../src/generated/tileHeights';

/** The Rust server embeds server/data/*.json; they must match what the browser's world layer uses. */
const read = (name: string): unknown => JSON.parse(readFileSync(join(__dirname, '..', 'server/data', name), 'utf8'));

describe('server data', () => {
  it('matches the generated asset manifest and tile heights', () => {
    expect(read('asset-manifest.json')).toEqual(assetManifest);
    expect(read('tile-heights.json')).toEqual({ TILE_GRID, BRIDGE_GRID, NO_GROUND, BRIDGE_HEIGHTS, TILE_HEIGHTS });
  });
});
