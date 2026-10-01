/** Shape of src/generated/assetManifest.ts (written by tools/assets/build-assets.mjs). */
export interface AssetManifest {
  /** texture id -> url */
  textures: Record<string, string>;
  bundles: Record<string, BundleEntry>;
  characters: Record<string, CharacterAssetEntry>;
  animations: { url: string; clips: Record<string, number> };
}

export interface BundleEntry {
  url: string;
  /** model id -> bounds as [minX, minY, minZ, maxX, maxY, maxZ] in model units */
  models: Record<string, { bounds: number[] }>;
}

export interface CharacterAssetEntry {
  url: string;
  texture: string | null;
  height: number;
}
