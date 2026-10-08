import * as THREE from 'three/webgpu';
import { rotate } from '../core/math2d';
import type { Ceremony, Grave } from '../generated/api';
import { getPlace } from '../world/places';
import type { Assets } from './assets';

/**
 * Where on the green the couple stand, in the green's own frame: south of the well, clear of the benches.
 * The sim's `WEDDING_SPOT` in `server/crates/sim/src/systems/ceremonies.rs` is the same point.
 */
const WEDDING_SPOT = { x: 0.5, z: 5.2 };
/** What dresses the green for a wedding: [bundle, model, right, forward, turn, scale], from the couple's spot. */
const WEDDING_DRESSING: [string, string, number, number, number, number][] = [
  ['halloween', 'arch', 0, -1.3, 0, 1.6],
  ['dungeon', 'banner_red', -3.4, -1.0, 0, 1.3],
  ['dungeon', 'banner_yellow', 3.4, -1.0, 0, 1.3],
  ['custom', 'flowers_mixed', -2.2, 1.4, 0.4, 1.5],
  ['custom', 'flowers_red', 2.4, 1.6, -0.6, 1.5],
  ['custom', 'flowers_violet', -3.6, 3.2, 1.1, 1.4],
  ['custom', 'flowers_yellow', 3.6, 3.4, 0.2, 1.4],
];
/** Candles set inside the graveyard's gate for a funeral, in the graveyard's own frame. */
const FUNERAL_CANDLES: [string, string, number, number, number][] = [
  ['halloween', 'plaque_candles', -1.9, 4.2, 0],
  ['halloween', 'shrine_candles', 1.9, 4.2, 0],
];
/** The coffin lies at the foot of the grave, in the plot's own frame. */
const COFFIN_OFFSET = { x: 0, z: 1.3 };

/**
 * Dresses the place of a ceremony while the village is there: an arch, banners and flowers on the green for a
 * wedding; a coffin at each grave and candles by the gate for a funeral. Built once, shown when the ceremony
 * reaches its gathering, and hidden when it is over.
 */
export class CeremonyDressing {
  readonly group = new THREE.Group();
  private readonly wedding = new THREE.Group();
  private readonly candles = new THREE.Group();
  private readonly coffins = new THREE.Group();
  private coffin: THREE.Object3D | null = null;

  constructor(private readonly assets: Assets, private readonly heightAt: (x: number, z: number) => number) {
    this.group.name = 'ceremony';
    const green = getPlace('green');
    const spot = rotate(WEDDING_SPOT, green.facing);
    const centre = { x: green.origin.x + spot.x, z: green.origin.z + spot.z };
    for (const [bundle, model, right, forward, turn, scale] of WEDDING_DRESSING) {
      this.put(this.wedding, bundle, model, centre, green.facing, right, forward, turn, scale);
    }
    const yard = getPlace('graveyard');
    for (const [bundle, model, right, forward, turn] of FUNERAL_CANDLES) {
      this.put(this.candles, bundle, model, yard.origin, yard.facing, right, forward, turn, 1);
    }
    if (assets.hasModel('halloween', 'coffin')) this.coffin = assets.model('halloween', 'coffin');
    this.wedding.visible = this.candles.visible = this.coffins.visible = false;
    this.group.add(this.wedding, this.candles, this.coffins);
  }

  /** Shows the dressing for the ceremony's place and phase, or none. */
  show(pending: Ceremony | null, graves: ReadonlyMap<string, Grave>): void {
    const ceremony = pending !== null && pending.phase !== 'announced' && pending.phase !== 'over' ? pending : null;
    this.wedding.visible = ceremony?.kind === 'wedding';
    this.candles.visible = ceremony?.kind === 'funeral';
    this.coffins.visible = this.candles.visible;
    this.coffins.clear();
    if (!ceremony || ceremony.kind !== 'funeral' || !this.coffin) return;
    const yard = getPlace('graveyard');
    for (const grave of graves.values()) {
      if (!ceremony.ids.includes(grave.characterId)) continue;
      const plot = yard.gravePlots[grave.plot];
      if (!plot) continue;
      const at = rotate(COFFIN_OFFSET, plot.facing);
      const coffin = this.coffin.clone();
      const [x, z] = [plot.pos.x + at.x, plot.pos.z + at.z];
      coffin.position.set(x, this.heightAt(x, z), z);
      coffin.rotation.y = plot.facing;
      coffin.traverse((child) => {
        const mesh = child as THREE.Mesh;
        if (mesh.isMesh) mesh.castShadow = true;
      });
      this.coffins.add(coffin);
    }
  }

  private put(
    into: THREE.Group, bundle: string, model: string, origin: { x: number; z: number }, facing: number,
    right: number, forward: number, turn: number, scale: number,
  ): void {
    if (!this.assets.hasModel(bundle, model)) return;
    const [sin, cos] = [Math.sin(facing), Math.cos(facing)];
    const x = origin.x + right * cos + forward * sin;
    const z = origin.z - right * sin + forward * cos;
    const object = this.assets.model(bundle, model).clone();
    object.position.set(x, this.heightAt(x, z), z);
    object.rotation.y = facing + turn;
    object.scale.setScalar(scale);
    object.traverse((child) => {
      const mesh = child as THREE.Mesh;
      if (mesh.isMesh) mesh.castShadow = mesh.receiveShadow = true;
    });
    into.add(object);
  }
}
