import * as THREE from 'three/webgpu';
import { hashString } from '../core/rng';
import type { WorldObject } from '../generated/api';
import type { PlacedModel } from '../world/places';
import type { Assets } from './assets';
import type { QualitySettings } from './quality';
import type { Part, PartCache } from './scenery';

/** Ticks a felled tree takes to come down (`FALL_TICKS` in the sim). */
const FALL_TICKS = 30;
/** How far over a felled tree ends up, in radians. */
const FALL_ANGLE = (85 * Math.PI) / 180;
/** A tree swayed within this many ticks of the last swing at it (two game minutes). */
const SWAY_TICKS = 40;
/** The stump a felled tree leaves, and its size (the lumber camp's copies are at the hex scale, 7). */
const STUMP = ['hex', 'tree_single_A_cut'] as const;
const STUMP_SCALE = 2.5;
/** How big a sapling and a young tree are against the grown one. */
const SAPLING_SCALE = 0.25;
const YOUNG_SCALE = 0.6;

/** How far the wind bends a tree at full strength, in radians, and how often the trees are posed again while it blows. */
const WIND_LEAN = 0.07;
const WIND_SECONDS = 1 / 12;

const HIDDEN = new THREE.Matrix4().makeScale(0, 0, 0);
const UP = new THREE.Vector3(0, 1, 0);

/** Whether a scatter model is a tree or a rock: the scenery the sim can change. */
export function isVegetation(model: PlacedModel): boolean {
  return model.bundle === 'forest' && (model.model.startsWith('Tree') || model.model.startsWith('Rock'));
}

/** One InstancedMesh per part of a model, every copy of the model in the world a slot in each. */
interface Pool {
  parts: Part[];
  meshes: THREE.InstancedMesh[];
  next: number;
}

interface Item {
  index: number;
  tree: boolean;
  pool: Pool;
  slot: number;
  position: THREE.Vector3;
  rotation: number;
  scale: number;
  /** a slot in the stump pool, trees only */
  stump: number;
}

/** What the server says of a tree or rock that is not as the map has it. */
interface Live {
  item: Item;
  stage: string;
  sinceTick: number;
  progress: number;
}

/**
 * The trees and rocks of the valley, drawn as instances so that one of them
 * can sway under the axe, fall, stand as a stump, grow back, or be mined away,
 * by changing one matrix. The rest of the scenery is merged and never moves.
 */
export class Vegetation {
  readonly group = new THREE.Group();
  private readonly pools = new Map<string, Pool>();
  private readonly items: Item[] = [];
  private readonly stumps: Pool;
  private readonly live = new Map<string, Live>();
  private tick = 0;
  private time = 0;
  private readonly matrix = new THREE.Matrix4();
  private readonly quaternion = new THREE.Quaternion();
  private readonly turn = new THREE.Quaternion();
  private readonly axis = new THREE.Vector3();
  private readonly size = new THREE.Vector3();
  private readonly world = new THREE.Matrix4();
  /** the wind, 0..1, and the way it blows toward (radians from +x toward +z) */
  private wind = 0;
  private windDir = 0;
  private windClock = 0;
  /** some tree is bent by the wind and must be stood up straight once it drops */
  private bent = false;

  constructor(
    assets: Assets,
    models: readonly PlacedModel[],
    heightAt: (x: number, z: number) => number,
    private readonly quality: QualitySettings,
    private readonly cache: PartCache,
  ) {
    this.group.name = 'vegetation';
    const counts = new Map<string, number>();
    let trees = 0;
    for (const [index, model] of models.entries()) {
      if (!isVegetation(model)) continue;
      counts.set(model.model, (counts.get(model.model) ?? 0) + 1);
      if (model.model.startsWith('Tree')) trees++;
      this.items.push({
        index, tree: model.model.startsWith('Tree'), pool: null as unknown as Pool, slot: 0, stump: -1,
        position: new THREE.Vector3(model.x, model.y + heightAt(model.x, model.z), model.z), rotation: model.rotation, scale: model.scale,
      });
    }
    for (const [name, count] of counts) this.pools.set(name, this.pool(assets.model('forest', name), count));
    this.stumps = this.pool(assets.model(...STUMP), Math.max(1, trees));
    for (const item of this.items) {
      const pool = this.pools.get(models[item.index]!.model) as Pool;
      item.pool = pool;
      item.slot = pool.next++;
      if (item.tree) item.stump = this.stumps.next++;
      this.place(item, item.scale, null);
      if (item.tree) this.setAll(this.stumps, item.stump, HIDDEN);
    }
    for (const pool of [...this.pools.values(), this.stumps]) {
      for (const mesh of pool.meshes) {
        mesh.instanceMatrix.needsUpdate = true;
        mesh.computeBoundingSphere();
      }
    }
  }

  private pool(template: THREE.Object3D, count: number): Pool {
    const parts = this.cache.partsOf(template);
    const meshes = parts.map((part) => {
      const mesh = new THREE.InstancedMesh(part.geometry, part.material, count);
      mesh.castShadow = true;
      mesh.receiveShadow = true;
      mesh.matrixAutoUpdate = false;
      // A tree can lean anywhere within its own height of where it stood: culling by the whole pool's bounds is safe.
      mesh.frustumCulled = false;
      this.group.add(mesh);
      return mesh;
    });
    return { parts, meshes, next: 0 };
  }

  private setAll(pool: Pool, slot: number, world: THREE.Matrix4): void {
    pool.parts.forEach((part, i) => {
      const mesh = pool.meshes[i] as THREE.InstancedMesh;
      mesh.setMatrixAt(slot, world.equals(HIDDEN) ? HIDDEN : this.matrix.multiplyMatrices(world, part.local));
      mesh.instanceMatrix.needsUpdate = true;
    });
  }

  /** Puts a tree or rock where it stands, at `scale`, leaning by `lean` (or upright). */
  private place(item: Item, scale: number, lean: THREE.Quaternion | null): void {
    this.quaternion.setFromAxisAngle(UP, item.rotation);
    if (lean) this.quaternion.premultiply(lean);
    this.size.setScalar(scale);
    const world = new THREE.Matrix4().compose(item.position, this.quaternion, this.size);
    this.setAll(item.pool, item.slot, world);
  }

  private hide(item: Item): void {
    this.setAll(item.pool, item.slot, HIDDEN);
  }

  /** The direction a tree leans and falls, fixed for each tree. */
  private leanAxis(id: string): THREE.Vector3 {
    const angle = (hashString(id) % 12) * (Math.PI / 6);
    return this.axis.set(Math.cos(angle), 0, Math.sin(angle));
  }

  /** The wind the trees bend in. */
  setWind(strength: number, direction: number): void {
    this.wind = strength;
    this.windDir = direction;
  }

  /** The sim tick the view has reached; the fall and the sway are timed from it. */
  setTick(tick: number): void {
    this.tick = tick;
  }

  /** Brings every tree and rock in line with what the server says of it; one it does not mention stands as the map has it. */
  show(objects: ReadonlyMap<string, WorldObject>): void {
    const seen = new Set<string>();
    for (const object of objects.values()) {
      if (object.kind !== 'tree' && object.kind !== 'rock') continue;
      const index = Number(object.id.slice(2));
      const item = this.items.find((entry) => entry.index === index);
      if (!item || !object.id.startsWith('s:')) continue;
      seen.add(object.id);
      this.live.set(object.id, { item, stage: object.stage, sinceTick: object.sinceTick, progress: object.progress });
      this.pose(item, object.stage, 0);
    }
    for (const [id, live] of this.live) {
      if (seen.has(id)) continue;
      this.live.delete(id);
      this.pose(live.item, 'intact', 0);
    }
  }

  /** The still pose of a stage; falling and swaying trees are posed in `update`. */
  private pose(item: Item, stage: string, lean: number): void {
    const stump = (shown: boolean): void => {
      if (!item.tree) return;
      if (!shown) return this.setAll(this.stumps, item.stump, HIDDEN);
      this.quaternion.setFromAxisAngle(UP, item.rotation);
      this.size.setScalar(STUMP_SCALE);
      this.setAll(this.stumps, item.stump, new THREE.Matrix4().compose(item.position, this.quaternion, this.size));
    };
    switch (stage) {
      case 'stump':
        this.hide(item);
        stump(true);
        break;
      case 'sapling':
        this.place(item, item.scale * SAPLING_SCALE, null);
        stump(true);
        break;
      case 'young':
        this.place(item, item.scale * YOUNG_SCALE, null);
        stump(false);
        break;
      case 'mined_out':
        this.hide(item);
        break;
      default: {
        // standing, falling, whole, intact: upright or leaning by `lean`
        const tilt = lean === 0 ? null : this.turn.setFromAxisAngle(this.leanAxis(`s:${item.index}`), lean);
        this.place(item, item.scale, tilt);
        stump(false);
      }
    }
  }

  /** Per rendered frame: trees under the axe sway, felled ones come down, and in a wind every tree bends. */
  update(dt: number, alpha: number): void {
    this.time += dt;
    this.blow(dt);
    const now = this.tick + alpha;
    for (const [id, live] of this.live) {
      if (live.stage === 'falling') {
        const t = Math.min(1, Math.max(0, (now - live.sinceTick) / FALL_TICKS));
        this.pose(live.item, 'falling', t * t * FALL_ANGLE);
      } else if (live.stage === 'standing' && this.quality.treeSway) {
        const recent = now - live.sinceTick <= SWAY_TICKS;
        const sway = recent ? Math.sin(this.time * 6 + hashString(id)) * 0.06 * (0.4 + live.progress / 100) : 0;
        this.pose(live.item, 'standing', sway);
      }
    }
  }

  /**
   * Bends every standing tree away from the wind, gusting, a dozen times a second rather than every frame; trees
   * the sim has changed are left to `update`. Only on machines that can afford it.
   */
  private blow(dt: number): void {
    if (!this.quality.windSway || (this.wind < 0.05 && !this.bent)) return;
    this.windClock += dt;
    if (this.windClock < WIND_SECONDS) return;
    this.windClock = 0;
    this.bent = this.wind >= 0.05;
    // A tree leans toward where the wind blows: it turns about the line across the wind.
    this.axis.set(Math.sin(this.windDir), 0, -Math.cos(this.windDir));
    for (const item of this.items) {
      if (!item.tree || this.live.has(`s:${item.index}`)) continue;
      const gust = 0.65 + 0.35 * Math.sin(this.time * 1.7 + item.index * 0.9);
      const lean = this.bent ? WIND_LEAN * this.wind * gust : 0;
      this.quaternion.setFromAxisAngle(UP, item.rotation);
      if (lean !== 0) this.quaternion.premultiply(this.turn.setFromAxisAngle(this.axis, lean));
      this.size.setScalar(item.scale);
      this.setAll(item.pool, item.slot, this.world.compose(item.position, this.quaternion, this.size));
    }
  }
}
