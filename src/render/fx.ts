import * as THREE from 'three/webgpu';
import type { WireEvent } from '../generated/api';
import type { Assets } from './assets';

interface Projectile {
  object: THREE.Object3D;
  from: THREE.Vector3;
  to: THREE.Vector3;
  age: number;
  life: number;
  arc: number;
}

interface Burst {
  mesh: THREE.Mesh;
  age: number;
  life: number;
  grow: number;
}

/** One puff of the dragon's breath, poured from its mouth onto the ground. */
interface Flame {
  mesh: THREE.Mesh;
  from: THREE.Vector3;
  to: THREE.Vector3;
  /** seconds before it leaves the mouth */
  delay: number;
  age: number;
}

const MAX_EFFECTS = 40;
/** Puffs in one breath, the gap between them, how long each takes to reach the ground, and how long it burns there. */
const BREATH_PUFFS = 20;
const PUFF_GAP = 0.035;
const PUFF_FLIGHT = 0.4;
const PUFF_LINGER = 0.8;

/** Short-lived visuals for one-shot events: arrows, spell bolts, impact flashes. */
export class Effects {
  readonly group = new THREE.Group();
  private readonly projectiles: Projectile[] = [];
  private readonly bursts: Burst[] = [];
  private readonly flames: Flame[] = [];
  private readonly puffGeometry = new THREE.IcosahedronGeometry(0.8, 0);
  private readonly boltGeometry = new THREE.SphereGeometry(0.22, 10, 8);
  private readonly ringGeometry = new THREE.RingGeometry(0.35, 0.5, 20).rotateX(-Math.PI / 2);
  private readonly materials = {
    magic: new THREE.MeshBasicMaterial({ color: 0x9fe8ff }),
    dark: new THREE.MeshBasicMaterial({ color: 0x8dff9a }),
    hit: new THREE.MeshBasicMaterial({ color: 0xfff1b0, transparent: true, depthWrite: false }),
    blood: new THREE.MeshBasicMaterial({ color: 0xff6a5c, transparent: true, depthWrite: false }),
    dust: new THREE.MeshBasicMaterial({ color: 0xcdbb94, transparent: true, depthWrite: false }),
    // The same three colours as a burning house.
    flames: [0xffd24a, 0xff8a1e, 0xff5a1a].map(
      (color) => new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0.9, depthWrite: false, blending: THREE.AdditiveBlending }),
    ),
    scorch: new THREE.MeshBasicMaterial({ color: 0xff8a1e, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }),
  };

  constructor(private readonly assets: Assets) {
    this.group.name = 'effects';
  }

  private burst(x: number, y: number, z: number, material: THREE.Material, grow: number, life: number): void {
    if (this.bursts.length >= MAX_EFFECTS) return;
    const mesh = new THREE.Mesh(this.ringGeometry, material.clone());
    mesh.position.set(x, y, z);
    this.group.add(mesh);
    this.bursts.push({ mesh, age: 0, life, grow });
  }

  /** A stream of fire from the dragon's mouth down to `to`, and a ring of flame where it lands. */
  private breathe(from: THREE.Vector3, to: THREE.Vector3): void {
    if (this.flames.length > MAX_EFFECTS * 2) return;
    for (let i = 0; i < BREATH_PUFFS; i++) {
      const mesh = new THREE.Mesh(this.puffGeometry, this.materials.flames[i % 3]);
      mesh.visible = false;
      this.group.add(mesh);
      // Each puff lands a little off the line, so the fire spreads over the ground.
      const spread = new THREE.Vector3(Math.sin(i * 2.4) * 1.8, 0, Math.cos(i * 3.1) * 1.8);
      this.flames.push({ mesh, from, to: to.clone().add(spread), delay: i * PUFF_GAP, age: 0 });
    }
    this.burst(to.x, to.y + 0.1, to.z, this.materials.scorch, 9, 0.9);
  }

  /**
   * `mouthOf` says where a breathing monster's mouth is now (null when it is not drawn); the breath falls from there,
   * or from its middle when that is not known.
   */
  handle(
    event: WireEvent,
    heightAt: (x: number, z: number) => number,
    isMonster: (id: string) => boolean,
    mouthOf: (id: string) => THREE.Vector3 | null,
  ): void {
    if (event.type === 'attack' && event.ranged === 'fire') {
      const ground = new THREE.Vector3(event.to.x, heightAt(event.to.x, event.to.z) + 0.3, event.to.z);
      const mouth = mouthOf(event.attackerId) ?? new THREE.Vector3(event.from.x, heightAt(event.from.x, event.from.z) + 8, event.from.z);
      this.breathe(mouth, ground);
    } else if (event.type === 'attack' && event.ranged && this.projectiles.length < MAX_EFFECTS) {
      const from = new THREE.Vector3(event.from.x, heightAt(event.from.x, event.from.z) + 1.3, event.from.z);
      const to = new THREE.Vector3(event.to.x, heightAt(event.to.x, event.to.z) + 1.1, event.to.z);
      let object: THREE.Object3D;
      if (event.ranged === 'magic') {
        object = new THREE.Mesh(this.boltGeometry, isMonster(event.attackerId) ? this.materials.dark : this.materials.magic);
      } else if (event.ranged === 'food') {
        // A child's apple, lobbed at someone's ear.
        object = this.assets.model('resource', 'Food_Apple_Red').clone();
      } else {
        object = this.assets.model('weapons', event.ranged === 'arrow' ? 'arrow_bow' : 'arrow_crossbow').clone();
      }
      const distance = from.distanceTo(to);
      this.group.add(object);
      this.projectiles.push({ object, from, to, age: 0, life: Math.max(0.12, distance / 38), arc: event.ranged === 'magic' ? 0 : distance * 0.04 });
    } else if (event.type === 'hit') {
      const y = heightAt(event.pos.x, event.pos.z);
      this.burst(event.pos.x, y + 1.2, event.pos.z, isMonster(event.targetId) ? this.materials.hit : this.materials.blood, 2.2, 0.28);
    } else if (event.type === 'monster_spawn') {
      const y = heightAt(event.pos.x, event.pos.z);
      this.burst(event.pos.x, y + 0.08, event.pos.z, this.materials.dust, 4.5, 1.4);
    }
  }

  update(dt: number): void {
    for (let i = this.flames.length - 1; i >= 0; i--) {
      const f = this.flames[i] as Flame;
      f.age += dt;
      const t = (f.age - f.delay) / PUFF_FLIGHT;
      f.mesh.visible = t > 0;
      if (t <= 0) continue;
      if (t < 1) {
        f.mesh.position.lerpVectors(f.from, f.to, t);
        f.mesh.scale.setScalar(0.5 + t * 1.6);
      } else {
        // On the ground it billows up and burns out.
        const burn = (t - 1) * (PUFF_FLIGHT / PUFF_LINGER);
        f.mesh.position.copy(f.to);
        f.mesh.position.y += burn * 1.5;
        f.mesh.scale.setScalar(2.1 * Math.max(0, 1 - burn));
        if (burn >= 1) {
          f.mesh.removeFromParent();
          this.flames.splice(i, 1);
        }
      }
    }
    for (let i = this.projectiles.length - 1; i >= 0; i--) {
      const p = this.projectiles[i] as Projectile;
      p.age += dt;
      const t = Math.min(1, p.age / p.life);
      p.object.position.lerpVectors(p.from, p.to, t);
      p.object.position.y += Math.sin(t * Math.PI) * p.arc;
      p.object.lookAt(p.to);
      if (t >= 1) {
        p.object.removeFromParent();
        this.projectiles.splice(i, 1);
      }
    }
    for (let i = this.bursts.length - 1; i >= 0; i--) {
      const b = this.bursts[i] as Burst;
      b.age += dt;
      const t = Math.min(1, b.age / b.life);
      b.mesh.scale.setScalar(1 + t * b.grow);
      (b.mesh.material as THREE.MeshBasicMaterial).opacity = 1 - t;
      if (t >= 1) {
        b.mesh.removeFromParent();
        (b.mesh.material as THREE.Material).dispose();
        this.bursts.splice(i, 1);
      }
    }
  }
}
