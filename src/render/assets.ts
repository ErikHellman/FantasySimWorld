import * as THREE from 'three/webgpu';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { assetManifest } from '../generated/assetManifest';

const MODEL_PREFIX = 'M__';
const BASE = import.meta.env.BASE_URL;

export interface CharacterTemplate {
  scene: THREE.Object3D;
  texture: string | null;
  height: number;
}

/**
 * Everything loaded from public/assets. Models are templates: clone them (or
 * instance their geometry), never add them to the scene directly.
 */
export class Assets {
  private readonly models = new Map<string, THREE.Object3D>();
  private readonly characters = new Map<string, CharacterTemplate>();
  private readonly materials = new Map<string, THREE.Material>();
  private readonly textureLoader = new THREE.TextureLoader();
  readonly clips = new Map<string, THREE.AnimationClip>();

  static async load(onProgress?: (done: number, total: number) => void): Promise<Assets> {
    const assets = new Assets();
    const loader = new GLTFLoader();
    const bundleIds = Object.keys(assetManifest.bundles);
    const characterIds = Object.keys(assetManifest.characters);
    const total = bundleIds.length + characterIds.length + 1;
    let done = 0;
    const tick = () => onProgress?.(++done, total);

    await Promise.all([
      ...bundleIds.map(async (bundleId) => {
        const gltf = await loader.loadAsync(BASE + assetManifest.bundles[bundleId]!.url);
        for (const child of [...gltf.scene.children]) {
          if (!child.name.startsWith(MODEL_PREFIX)) continue;
          child.removeFromParent();
          assets.applyMaterials(child);
          child.updateMatrixWorld(true);
          assets.models.set(`${bundleId}/${child.name.slice(MODEL_PREFIX.length)}`, child);
        }
        tick();
      }),
      ...characterIds.map(async (id) => {
        const entry = assetManifest.characters[id]!;
        const gltf = await loader.loadAsync(BASE + entry.url);
        assets.applyMaterials(gltf.scene);
        assets.characters.set(id, { scene: gltf.scene, texture: entry.texture, height: entry.height });
        tick();
      }),
      (async () => {
        const gltf = await loader.loadAsync(BASE + assetManifest.animations.url);
        for (const clip of gltf.animations) assets.clips.set(clip.name, clip);
        tick();
      })(),
    ]);
    return assets;
  }

  /** One shared material per texture id, so instancing and batching work across a pack. */
  material(textureId: string): THREE.Material {
    let material = this.materials.get(textureId);
    if (!material) {
      const url = assetManifest.textures[textureId];
      if (!url) throw new Error(`Unknown texture ${textureId}`);
      const map = this.textureLoader.load(BASE + url);
      map.flipY = false;
      map.colorSpace = THREE.SRGBColorSpace;
      material = new THREE.MeshLambertMaterial({ map });
      material.name = textureId;
      this.materials.set(textureId, material);
    }
    return material;
  }

  /**
   * Untextured materials (the props made in Blender) are given the same simple
   * shading as everything else; glowing ones are left alone.
   */
  private flat(material: THREE.Material): THREE.Material {
    const standard = material as THREE.MeshStandardMaterial;
    if (!standard.isMeshStandardMaterial || standard.emissive.getHex() !== 0 || standard.transparent) return material;
    const key = `flat:${standard.color.getHexString()}`;
    let flat = this.materials.get(key);
    if (!flat) {
      flat = new THREE.MeshLambertMaterial({ color: standard.color });
      flat.name = key;
      this.materials.set(key, flat);
    }
    return flat;
  }

  /** Swaps the stripped glTF materials (tagged with extras.tex) for the shared ones. */
  applyMaterials(root: THREE.Object3D, override?: (textureId: string) => string): void {
    root.traverse((object) => {
      const mesh = object as THREE.Mesh;
      if (!mesh.isMesh) return;
      const swap = (material: THREE.Material): THREE.Material => {
        const tex = (material.userData as { tex?: string }).tex ?? material.name;
        if (!tex || !assetManifest.textures[tex]) return this.flat(material);
        return this.material(override ? override(tex) : tex);
      };
      mesh.material = Array.isArray(mesh.material) ? mesh.material.map(swap) : swap(mesh.material);
    });
  }

  model(bundle: string, id: string): THREE.Object3D {
    const model = this.models.get(`${bundle}/${id}`);
    if (!model) throw new Error(`Unknown model ${bundle}/${id}`);
    return model;
  }

  hasModel(bundle: string, id: string): boolean {
    return this.models.has(`${bundle}/${id}`);
  }

  character(id: string): CharacterTemplate {
    const template = this.characters.get(id);
    if (!template) throw new Error(`Unknown character model ${id}`);
    return template;
  }

  clip(name: string): THREE.AnimationClip {
    const clip = this.clips.get(name);
    if (!clip) throw new Error(`Unknown animation clip ${name}`);
    return clip;
  }
}
