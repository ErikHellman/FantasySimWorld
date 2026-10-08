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

const MAX_EFFECTS = 40;

/** Short-lived visuals for one-shot events: arrows, spell bolts, impact flashes. */
export class Effects {
  readonly group = new THREE.Group();
  private readonly projectiles: Projectile[] = [];
  private readonly bursts: Burst[] = [];
  private readonly boltGeometry = new THREE.SphereGeometry(0.22, 10, 8);
  private readonly ringGeometry = new THREE.RingGeometry(0.35, 0.5, 20).rotateX(-Math.PI / 2);
  private readonly materials = {
    magic: new THREE.MeshBasicMaterial({ color: 0x9fe8ff }),
    dark: new THREE.MeshBasicMaterial({ color: 0x8dff9a }),
    hit: new THREE.MeshBasicMaterial({ color: 0xfff1b0, transparent: true, depthWrite: false }),
    blood: new THREE.MeshBasicMaterial({ color: 0xff6a5c, transparent: true, depthWrite: false }),
    dust: new THREE.MeshBasicMaterial({ color: 0xcdbb94, transparent: true, depthWrite: false }),
    spark: new THREE.MeshBasicMaterial({ color: 0xfffbe8, transparent: true, depthWrite: false }),
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

  handle(event: WireEvent, heightAt: (x: number, z: number) => number, isMonster: (id: string) => boolean): void {
    if (event.type === 'attack' && event.ranged && this.projectiles.length < MAX_EFFECTS) {
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
      if (event.outcome === 'blocked') {
        // A quick bright flash where steel met steel.
        this.burst(event.pos.x, y + 1.3, event.pos.z, this.materials.spark, 1.2, 0.16);
      } else if (event.outcome === 'dodged') {
        // A puff of dust where the defender's feet left the ground.
        this.burst(event.pos.x, y + 0.08, event.pos.z, this.materials.dust, 2, 0.5);
      } else {
        this.burst(event.pos.x, y + 1.2, event.pos.z, isMonster(event.targetId) ? this.materials.hit : this.materials.blood, 2.2, 0.28);
      }
    } else if (event.type === 'monster_spawn') {
      const y = heightAt(event.pos.x, event.pos.z);
      this.burst(event.pos.x, y + 0.08, event.pos.z, this.materials.dust, 4.5, 1.4);
    }
  }

  update(dt: number): void {
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
