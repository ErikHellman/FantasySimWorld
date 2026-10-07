import * as THREE from 'three/webgpu';
import type { House, WorldObject } from '../generated/api';
import { buildingPlaces, type Place, type PlacedModel } from '../world/places';
import type { Assets } from './assets';
import type { Lamp } from './lighting';
import type { QualitySettings } from './quality';

/** The burnt shell that stands where a building was. */
const RUIN_MODEL = 'building_destroyed';
/** What is left where a building was taken down to its footings: a bare plot and a heap of rubble. */
const PLOT_MODEL = 'building_dirt';
const RUBBLE_MODEL = 'rubble_half';
/**
 * Timber that gathers beside a ruin as it is rebuilt: [model, right, forward, turn], in the building's own frame.
 * It lies to the sides, clear of the door where the builders stand.
 */
const STACKS: [string, number, number, number][] = [
  ['Wood_Planks_Stack_Small', -5.2, 1.6, 0.3],
  ['Wood_Log_Stack', 5.1, 0.4, -0.4],
  ['Wood_Planks_Stack_Medium', -5.0, -1.4, 1.5],
  ['Stone_Bricks_Stack_Medium', 5.3, -2.0, 0.9],
];
/** The height a building's flames reach when it is all alight, in metres. */
const FLAME_REACH = 7;

interface Structure {
  placeId: string;
  x: number;
  y: number;
  z: number;
  building: THREE.Object3D;
  ruin: THREE.Object3D;
  plot: THREE.Object3D;
  rubble: THREE.Object3D;
  stacks: THREE.Object3D[];
}

interface Flame {
  mesh: THREE.Mesh;
  /** where it sits around the building, and how far up its walls */
  dx: number;
  dz: number;
  lift: number;
  size: number;
  phase: number;
}

interface Puff {
  mesh: THREE.Mesh;
  dx: number;
  dz: number;
  phase: number;
}

/** A repeatable scatter: the same flames in the same places every time. */
function spread(index: number, salt: number): number {
  const x = Math.sin(index * 127.1 + salt * 311.7) * 43758.5453;
  return x - Math.floor(x);
}

/** Whether a place's model is its building (the first model of a place that has one); the rest is ordinary scenery. */
export function isBuilding(place: Place, model: PlacedModel): boolean {
  return buildingPlaces.has(place.id) && place.models[0] === model;
}

/**
 * The village's buildings, kept as one object each (unlike the rest of the scenery, which is merged) so that
 * one can burn, stand as a ruin, be taken down to its footings and be built again. One fire is drawn at a
 * time, as the sim allows only one.
 */
export class Structures {
  readonly group = new THREE.Group();
  private readonly structures = new Map<string, Structure>();
  private readonly flames: Flame[] = [];
  private readonly puffs: Puff[] = [];
  private readonly fire = new THREE.Group();
  private burning: { structure: Structure; level: number } | null = null;
  private time = 0;

  constructor(assets: Assets, places: readonly Place[], heightAt: (x: number, z: number) => number, quality: QualitySettings) {
    this.group.name = 'structures';
    const put = (source: THREE.Object3D, x: number, y: number, z: number, rotation: number, scale: number): THREE.Object3D => {
      const object = source.clone();
      object.position.set(x, y, z);
      object.rotation.y = rotation;
      object.scale.setScalar(scale);
      object.traverse((child) => {
        const mesh = child as THREE.Mesh;
        if (mesh.isMesh) mesh.castShadow = mesh.receiveShadow = true;
      });
      object.updateMatrixWorld(true);
      object.traverse((child) => (child.matrixAutoUpdate = false));
      this.group.add(object);
      return object;
    };
    for (const place of places) {
      const model = place.models.find((entry) => isBuilding(place, entry));
      if (!model) continue;
      const y = model.y + heightAt(model.x, model.z);
      const building = put(assets.model(model.bundle, model.model), model.x, y, model.z, model.rotation, model.scale);
      const ruin = put(assets.model('hex', RUIN_MODEL), model.x, y, model.z, model.rotation, model.scale);
      ruin.visible = false;
      const plot = put(assets.model('hex', PLOT_MODEL), model.x, y + 0.02, model.z, model.rotation, model.scale);
      plot.visible = false;
      const rubble = put(assets.model('dungeon', RUBBLE_MODEL), model.x, y, model.z, model.rotation + 0.6, 0.7);
      rubble.visible = false;
      const [sin, cos] = [Math.sin(place.facing), Math.cos(place.facing)];
      const stacks = STACKS.filter(([name]) => assets.hasModel('resource', name)).map(([name, right, forward, turn]) => {
        const x = place.origin.x + right * cos + forward * sin;
        const z = place.origin.z - right * sin + forward * cos;
        const stack = put(assets.model('resource', name), x, heightAt(x, z), z, place.facing + turn, 1);
        stack.visible = false;
        return stack;
      });
      this.structures.set(place.id, { placeId: place.id, x: model.x, y, z: model.z, building, ruin, plot, rubble, stacks });
    }

    const flameGeometry = new THREE.ConeGeometry(0.5, 1.6, 6).translate(0, 0.8, 0);
    const flameMaterials = [0xffd24a, 0xff8a1e, 0xff5a1a].map(
      (color) => new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0.85, depthWrite: false, blending: THREE.AdditiveBlending }),
    );
    for (let i = 0; i < quality.fireFlames; i++) {
      const mesh = new THREE.Mesh(flameGeometry, flameMaterials[i % flameMaterials.length]);
      const angle = spread(i, 1) * Math.PI * 2;
      const radius = 1.4 + spread(i, 2) * 2;
      this.flames.push({ mesh, dx: Math.cos(angle) * radius, dz: Math.sin(angle) * radius, lift: spread(i, 3), size: 0.8 + spread(i, 4) * 0.9, phase: spread(i, 5) * 6.28 });
      this.fire.add(mesh);
    }
    const puffGeometry = new THREE.SphereGeometry(1, 8, 6);
    for (let i = 0; i < quality.fireSmoke; i++) {
      const material = new THREE.MeshBasicMaterial({ color: 0x2b2622, transparent: true, opacity: 0.4, depthWrite: false });
      const mesh = new THREE.Mesh(puffGeometry, material);
      this.puffs.push({ mesh, dx: (spread(i, 6) - 0.5) * 3, dz: (spread(i, 7) - 0.5) * 3, phase: i / Math.max(1, quality.fireSmoke) });
      this.fire.add(mesh);
    }
    this.fire.visible = false;
    this.group.add(this.fire);
  }

  /**
   * Brings every building in line with what the server says: the homes' fires and ruins from `houses`, and
   * whatever was done to any building from `objects` (`b:<place>`). A building mentioned in neither stands whole.
   */
  show(houses: ReadonlyMap<string, House>, objects: ReadonlyMap<string, WorldObject>): void {
    this.burning = null;
    for (const structure of this.structures.values()) {
      const house = houses.get(structure.placeId);
      const object = objects.get(`b:${structure.placeId}`);
      const stage = object?.stage ?? 'whole';
      const ruined = house?.phase === 'ruin' || stage === 'ruin';
      const down = stage === 'dismantled' || stage === 'raising';
      structure.building.visible = !ruined && !down;
      structure.ruin.visible = ruined;
      structure.plot.visible = down;
      structure.rubble.visible = stage === 'dismantled';
      // The timber arrives stack by stack, the last of it when the work is nearly done.
      const built = house?.phase === 'ruin' ? house.built : stage === 'raising' ? object?.progress ?? 0 : -1;
      structure.stacks.forEach((stack, index) => (stack.visible = built >= 0 && built >= (index * 100) / structure.stacks.length + 5));
      if (house?.phase === 'burning') this.burning = { structure, level: Math.max(0.15, house.fire / 100) };
    }
    this.fire.visible = this.burning !== null;
    if (this.burning) this.fire.position.set(this.burning.structure.x, this.burning.structure.y, this.burning.structure.z);
  }

  /** Per rendered frame: flames lick and smoke rises over the burning building. */
  update(dt: number): void {
    if (!this.burning) return;
    this.time += dt;
    const level = this.burning.level;
    for (const flame of this.flames) {
      const flicker = 0.75 + 0.25 * Math.sin(this.time * 9 + flame.phase) + 0.15 * Math.sin(this.time * 23 + flame.phase * 3);
      const size = flame.size * (0.5 + level * 1.3);
      flame.mesh.position.set(flame.dx, 0.4 + flame.lift * level * FLAME_REACH * 0.7, flame.dz);
      flame.mesh.scale.set(size, size * (1 + level) * flicker, size);
    }
    for (const puff of this.puffs) {
      const age = (this.time * 0.22 + puff.phase) % 1;
      const size = (0.6 + age * 2.4) * (0.5 + level);
      puff.mesh.position.set(puff.dx + age * 2.5, 3 + level * FLAME_REACH * 0.6 + age * 12, puff.dz + age * 1.2);
      puff.mesh.scale.setScalar(size);
      (puff.mesh.material as THREE.MeshBasicMaterial).opacity = 0.45 * (1 - age) * Math.min(1, age * 6);
    }
  }

  /** The light the fire throws, flickering, or null when nothing burns. */
  light(): Lamp | null {
    if (!this.burning) return null;
    const { structure, level } = this.burning;
    const flicker = 0.85 + 0.15 * Math.sin(this.time * 11) + 0.08 * Math.sin(this.time * 29);
    return { x: structure.x, y: structure.y + 4, z: structure.z, power: (1 + level * 2.5) * flicker };
  }
}
