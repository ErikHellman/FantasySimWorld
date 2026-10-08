import * as THREE from 'three/webgpu';
import type { Loot } from '../generated/api';
import { hashString } from '../core/rng';
import type { Assets } from './assets';

/** What each kind of loot is drawn as: a stack of coins, a lump of gold, a gilded chest. */
const LOOT_MODELS: Record<Loot['kind'], [string, string, number]> = {
  coins: ['resource', 'Money_Coins_Stack_Small', 1.0],
  trinket: ['resource', 'Gold_Nuggets', 0.8],
  relic: ['dungeon', 'chest_gold', 0.6],
};

/**
 * What the dead leave on the ground: drawn where the server says until it is picked up or blows away.
 * Loot is not part of the map, so nothing here touches the merged scenery.
 */
export class LootMarks {
  readonly group = new THREE.Group();
  private readonly drawn = new Map<string, THREE.Object3D>();

  constructor(private readonly assets: Assets, private readonly heightAt: (x: number, z: number) => number) {
    this.group.name = 'loot';
  }

  /** Brings the ground in line with the server's loot. */
  show(loot: ReadonlyMap<string, Loot>): void {
    for (const [id, object] of this.drawn) {
      if (!loot.has(id)) {
        object.removeFromParent();
        this.drawn.delete(id);
      }
    }
    for (const [id, item] of loot) {
      if (this.drawn.has(id)) continue;
      const [bundle, model, scale] = LOOT_MODELS[item.kind];
      if (!this.assets.hasModel(bundle, model)) continue;
      const object = this.assets.model(bundle, model).clone();
      object.position.set(item.pos.x, this.heightAt(item.pos.x, item.pos.z), item.pos.z);
      object.rotation.y = (hashString(id) % 12) * 0.5;
      object.scale.setScalar(scale);
      object.traverse((child) => {
        const mesh = child as THREE.Mesh;
        if (mesh.isMesh) mesh.castShadow = true;
      });
      this.group.add(object);
      this.drawn.set(id, object);
    }
  }
}
