import * as THREE from 'three/webgpu';
import { clamp, lerp, lerpAngle } from '../core/math2d';
import { hashString } from '../core/rng';
import type { Look } from '../generated/api';
import type { Assets } from './assets';

/** The animals are the Blender-made models in this bundle (`tools/blender/make_animals.py`). */
const BUNDLE = 'animals';
/** How far outside the view an animal is still drawn, in metres. */
const CULL_MARGIN = 3;
/** How high a bird flies, and how fast it climbs or comes down, in metres and metres a second. */
const FLIGHT_HEIGHT = 4.5;
const CLIMB = 3.5;
const TICK_SECONDS = 0.05;
/**
 * The server moves an animal every fourth tick, a stride at a time (a ridden horse every tick): the view eases from
 * one place to the next over the ticks between, up to this many.
 */
const STRIDE = 4;
/** Below this height an animal is small enough to go without a shadow when shadows must be saved. */
const SMALL = 0.8;

/** The joints an animal's model may have; each is an empty at its joint, turned to move the part under it. */
const JOINTS = [
  'body', 'head', 'tail', 'ear_l', 'ear_r', 'leg_fl', 'leg_fr', 'leg_bl', 'leg_br', 'leg_l', 'leg_r',
  'wing_l', 'wing_r', 'foot_l', 'foot_r', 'saddle', 'seat',
] as const;
type JointName = (typeof JOINTS)[number];
const JOINT_SET: ReadonlySet<string> = new Set(JOINTS);

/** What the view needs from an animal each tick. */
export interface AnimalSnapshot {
  x: number;
  z: number;
  facing: number;
  anim: string;
  visible: boolean;
  /** who rides it, for a horse */
  mount: string | null;
}

/** One model's joints, with their rest poses to return to each frame. */
class Joints {
  private readonly joints = new Map<JointName, { object: THREE.Object3D; rotation: THREE.Euler; position: THREE.Vector3 }>();
  /** how high the body's joint sits above the ground at rest */
  readonly bodyHeight: number;

  constructor(readonly model: THREE.Object3D) {
    // Every animal in the bundle has a `body`, so the loader numbers the repeats (`body_3`); the number is dropped here.
    model.traverse((object) => {
      const name = object.name.replace(/_\d+$/, '') as JointName;
      if (!JOINT_SET.has(name) || this.joints.has(name)) return;
      this.joints.set(name, { object, rotation: object.rotation.clone(), position: object.position.clone() });
    });
    this.bodyHeight = this.joints.get('body')?.position.y ?? 0.3;
  }

  get(name: JointName): THREE.Object3D | null {
    return this.joints.get(name)?.object ?? null;
  }

  reset(): void {
    for (const { object, rotation, position } of this.joints.values()) {
      object.rotation.copy(rotation);
      object.position.copy(position);
    }
    this.model.position.y = 0;
  }

  turn(name: JointName, x: number, y = 0, z = 0): void {
    const object = this.get(name);
    if (!object) return;
    object.rotation.x += x;
    object.rotation.y += y;
    object.rotation.z += z;
  }
}

/**
 * One of the valley's animals. Positions are interpolated between the last two ticks like a character's; the
 * animations are not clips but a little motion of the model's parts, worked out each frame from what the server says
 * the animal is doing and how fast it is going.
 */
export class AnimalView {
  readonly root = new THREE.Group();
  readonly height: number;
  private readonly standing: Joints;
  /** a bird's model with its wings spread, shown while it is in the air */
  private readonly flying: Joints | null;
  private readonly bounds = new THREE.Sphere();
  private prev = { x: 0, z: 0, facing: 0 };
  private next = { x: 0, z: 0, facing: 0 };
  private fresh = true;
  /** ticks since the animal last moved, and how many the step to `next` is spread over */
  private ticksSince = 0;
  private span = 1;
  private anim = 'idle';
  /** paces a second, from the last two ticks */
  private speed = 0;
  private lift = 0;
  private time = 0;
  /** so a flock does not move as one */
  private readonly phase: number;
  /** dogs wag */
  private readonly wags: boolean;

  constructor(assets: Assets, look: Look, id: string, smallShadows: boolean) {
    const standing = assets.model(BUNDLE, look.model).clone();
    this.standing = new Joints(standing);
    const flyingId = `${look.model}_flying`;
    this.flying = assets.hasModel(BUNDLE, flyingId) ? new Joints(assets.model(BUNDLE, flyingId).clone()) : null;
    this.height = new THREE.Box3().setFromObject(standing).max.y;
    const shadows = smallShadows || this.height >= SMALL;
    for (const joints of [this.standing, this.flying]) {
      if (!joints) continue;
      joints.model.traverse((object) => {
        const mesh = object as THREE.Mesh;
        if (mesh.isMesh) mesh.castShadow = shadows;
      });
      this.root.add(joints.model);
    }
    if (this.flying) this.flying.model.visible = false;
    this.phase = (hashString(id) % 1000) / 100;
    this.wags = look.model.startsWith('dog');
  }

  /** Where a rider's hips go, for a horse. */
  seat(): THREE.Object3D | null {
    return this.standing.get('seat');
  }

  /** Called once per tick with the animal's new state. */
  sync(snapshot: AnimalSnapshot): void {
    this.ticksSince++;
    const moved = snapshot.x !== this.next.x || snapshot.z !== this.next.z || snapshot.facing !== this.next.facing;
    if (this.fresh || moved) {
      this.prev = this.fresh ? { x: snapshot.x, z: snapshot.z, facing: snapshot.facing } : this.next;
      this.next = { x: snapshot.x, z: snapshot.z, facing: snapshot.facing };
      this.span = clamp(this.ticksSince, 1, STRIDE);
      this.speed = Math.hypot(this.next.x - this.prev.x, this.next.z - this.prev.z) / (this.span * TICK_SECONDS);
      this.ticksSince = 0;
    } else if (this.ticksSince >= this.span) {
      this.speed = 0;
    }
    this.root.visible = snapshot.visible;
    this.anim = snapshot.anim;
    const saddle = this.standing.get('saddle');
    if (saddle) saddle.visible = snapshot.mount !== null;
    this.fresh = false;
  }

  /** Called every frame; alpha is how far we are between the last two ticks. `move` is false to leave the parts still. */
  update(dt: number, alpha: number, heightAt: (x: number, z: number) => number, frustum: THREE.Frustum, move: boolean): void {
    if (!this.root.visible) return;
    const k = Math.min(1, (this.ticksSince + alpha) / this.span);
    const x = lerp(this.prev.x, this.next.x, k);
    const z = lerp(this.prev.z, this.next.z, k);
    const aloft = this.anim === 'fly';
    const target = aloft ? FLIGHT_HEIGHT : 0;
    this.lift += clamp(target - this.lift, -CLIMB * dt, CLIMB * dt);
    const y = heightAt(x, z);
    this.root.position.set(x, y + this.lift, z);
    this.root.rotation.y = lerpAngle(this.prev.facing, this.next.facing, k);

    const flying = this.flying !== null && (aloft || this.lift > 0.3);
    this.standing.model.visible = !flying;
    if (this.flying) this.flying.model.visible = flying;
    this.bounds.center.set(x, y + this.lift + this.height / 2, z);
    this.bounds.radius = this.height + CULL_MARGIN;
    if (!move || !frustum.intersectsSphere(this.bounds)) return;
    this.time += dt;
    const joints = flying && this.flying ? this.flying : this.standing;
    joints.reset();
    this.pose(joints, this.time + this.phase);
  }

  private pose(j: Joints, t: number): void {
    const sway = Math.sin(t * 1.3);
    // A ridden horse stands still while its rider does.
    const anim = this.anim === 'ridden' && this.speed < 0.3 ? 'idle' : this.anim;
    switch (anim) {
      case 'walk':
      case 'run':
      case 'ridden':
        this.gait(j, t, this.anim === 'walk' ? 0.45 : 0.75);
        j.turn('tail', 0, sway * 0.25);
        break;
      case 'graze':
        j.turn('head', 0.65 + Math.sin(t * 2.1) * 0.08);
        j.turn('tail', 0, sway * 0.3);
        break;
      case 'peck': {
        // Quick dips with pauses between.
        const dip = Math.max(0, Math.sin(t * 6)) ** 3;
        j.turn('head', dip * 0.9, Math.sin(t * 0.7) * 0.3);
        break;
      }
      case 'sleep': {
        j.model.position.y = -j.bodyHeight * 0.55;
        for (const leg of ['leg_fl', 'leg_fr'] as const) j.turn(leg, -1.35);
        for (const leg of ['leg_bl', 'leg_br'] as const) j.turn(leg, 1.35);
        j.turn('head', 0.35 + Math.sin(t * 0.8) * 0.03);
        break;
      }
      case 'sit':
        j.model.position.y = -j.bodyHeight * 0.2;
        j.turn('body', -0.5);
        j.turn('head', 0.4);
        j.turn('leg_fl', 0.5);
        j.turn('leg_fr', 0.5);
        j.turn('leg_bl', -1.1);
        j.turn('leg_br', -1.1);
        this.wag(j, t);
        break;
      case 'fly': {
        const flap = Math.sin(t * 14) * 0.6;
        j.turn('wing_l', 0, 0, flap);
        j.turn('wing_r', 0, 0, -flap);
        break;
      }
      default:
        j.turn('head', 0, Math.sin(t * 0.5) * 0.35);
        this.wag(j, t);
    }
  }

  /** Legs swing in diagonal pairs (or by turns, for two), quicker the faster the animal goes, and the body bobs. */
  private gait(j: Joints, t: number, stride: number): void {
    const step = t * clamp(this.speed * 3.2, 5, 16);
    const swing = Math.sin(step) * stride;
    j.turn('leg_fl', swing);
    j.turn('leg_br', swing);
    j.turn('leg_fr', -swing);
    j.turn('leg_bl', -swing);
    j.turn('leg_l', swing);
    j.turn('leg_r', -swing);
    const body = j.get('body');
    if (body) body.position.y += Math.abs(Math.sin(step)) * j.bodyHeight * 0.06;
    j.turn('head', Math.sin(step * 2) * 0.05);
  }

  private wag(j: Joints, t: number): void {
    if (this.wags) j.turn('tail', 0, Math.sin(t * 9) * 0.5);
  }

  dispose(): void {
    this.root.removeFromParent();
  }
}
