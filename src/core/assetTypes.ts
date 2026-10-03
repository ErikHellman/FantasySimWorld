/** Shape of src/generated/assetManifest.ts (written by tools/assets/build-assets.mjs). */
export interface AssetManifest {
  /** texture id -> url */
  textures: Record<string, string>;
  bundles: Record<string, BundleEntry>;
  characters: Record<string, CharacterAssetEntry>;
  /** rig id -> its clip file; every rig carries the same clip names */
  rigs: Record<string, RigEntry>;
}

export interface BundleEntry {
  url: string;
  /** model id -> bounds as [minX, minY, minZ, maxX, maxY, maxZ] in model units */
  models: Record<string, { bounds: number[] }>;
}

export interface CharacterAssetEntry {
  url: string;
  texture: string | null;
  /** standing height in world units, after `scale` */
  height: number;
  /** which entry of `rigs` this skeleton plays */
  rig: string;
  /** uniform scale the model is drawn at */
  scale: number;
}

export interface RigEntry {
  url: string;
  /** clip name -> duration in seconds */
  clips: Record<string, number>;
}

/**
 * The rig whose clip lengths the sim times actions by. Other rigs stretch their
 * one-shot clips to match.
 */
export const TIMING_RIG = 'medium';
