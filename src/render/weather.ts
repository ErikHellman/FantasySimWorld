import * as THREE from 'three/webgpu';
import type { Weather } from '../generated/api';
import type { QualitySettings } from './quality';

/** How fast rain and hail fall, in metres a second. */
const RAIN_SPEED = 26;
const HAIL_SPEED = 18;
/** How far the wind blows them sideways at full strength, in metres a second. */
const RAIN_DRIFT = 9;
const HAIL_DRIFT = 4;
/** How high above the camera's target drops start, and how far below it they are given up. */
const TOP = 34;
const BOTTOM = -12;
/** How far out from the camera's target drops fall, as a share of the camera's distance, within bounds. */
const REACH = 0.85;
const MIN_REACH = 24;
const MAX_REACH = 110;
/** How quickly what is drawn follows what the server says, per second; the server tells it in steps. */
const FOLLOW = 0.6;
/** Hailstones beside raindrops. */
const HAIL_SHARE = 0.3;

/** What the sky looks like right now: eased toward the server's word, 0..1. */
export interface Sky {
  rain: number;
  hail: number;
  wind: number;
  /** the way the wind blows toward, in radians from +x toward +z */
  windDir: number;
  /** how grey the day is */
  overcast: number;
}

/** How grey each kind of weather makes the day at full strength. */
function overcastOf(weather: Weather): number {
  const strength = weather.intensity / 100;
  switch (weather.kind) {
    case 'rain':
      return 0.85 * strength;
    case 'hail':
      return strength;
    case 'wind':
      return 0.3 * strength;
    default:
      return 0;
  }
}

function approach(value: number, target: number, step: number): number {
  return value < target ? Math.min(target, value + step) : Math.max(target, value - step);
}

interface Drops {
  mesh: THREE.InstancedMesh;
  x: Float32Array;
  y: Float32Array;
  z: Float32Array;
  /** hail only: upward speed after a bounce, and whether it has bounced */
  vy: Float32Array;
  bounced: Uint8Array;
}

/**
 * Rain and hail falling in a box around where the camera looks, slanted by the wind. Each is one instanced mesh
 * whose count follows how hard it is coming down; drops that leave the box come in again at the other side, so
 * the weather stays put in the world while the camera moves through it.
 */
export class WeatherView {
  readonly group = new THREE.Group();
  readonly sky: Sky = { rain: 0, hail: 0, wind: 0, windDir: 0, overcast: 0 };
  private target: Weather = { kind: 'clear', intensity: 0, wind: 0, windDir: 0 };
  private readonly rain: Drops;
  private readonly hail: Drops;
  private readonly basis = new THREE.Matrix4();
  private readonly tilt = new THREE.Quaternion();
  private readonly scale = new THREE.Vector3();
  private readonly axis = new THREE.Vector3();
  private readonly origin = new THREE.Vector3();

  constructor(quality: QualitySettings, private readonly heightAt: (x: number, z: number) => number) {
    this.group.name = 'weather';
    const streak = new THREE.BoxGeometry(1, 1, 1);
    const rainMaterial = new THREE.MeshBasicMaterial({ color: 0xb4c8dc, transparent: true, opacity: 0.38, depthWrite: false });
    this.rain = this.drops(streak, rainMaterial, quality.rainDrops);
    const stone = new THREE.IcosahedronGeometry(1, 0);
    const hailMaterial = new THREE.MeshBasicMaterial({ color: 0xf2f6fa, transparent: true, opacity: 0.9 });
    this.hail = this.drops(stone, hailMaterial, Math.round(quality.rainDrops * HAIL_SHARE));
  }

  private drops(geometry: THREE.BufferGeometry, material: THREE.Material, count: number): Drops {
    const mesh = new THREE.InstancedMesh(geometry, material, Math.max(1, count));
    mesh.count = 0;
    mesh.frustumCulled = false;
    mesh.matrixAutoUpdate = false;
    mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
    this.group.add(mesh);
    return {
      mesh, x: new Float32Array(count), y: new Float32Array(count), z: new Float32Array(count),
      vy: new Float32Array(count), bounced: new Uint8Array(count),
    };
  }

  /** What the server last said of the weather. */
  show(weather: Weather): void {
    this.target = weather;
  }

  /** Per rendered frame (`dt` in real seconds): the sky eases toward the server's word, and the drops fall. */
  update(dt: number, focus: THREE.Vector3, distance: number): void {
    const target = this.target;
    const step = FOLLOW * dt;
    const strength = target.intensity / 100;
    this.sky.rain = approach(this.sky.rain, target.kind === 'rain' ? strength : 0, step);
    this.sky.hail = approach(this.sky.hail, target.kind === 'hail' ? strength : 0, step * 2);
    this.sky.wind = approach(this.sky.wind, target.wind / 100, step);
    this.sky.overcast = approach(this.sky.overcast, overcastOf(target), step * 0.5);
    // The wind turns rather than jumps.
    const turn = Math.atan2(Math.sin((target.windDir * Math.PI) / 180 - this.sky.windDir), Math.cos((target.windDir * Math.PI) / 180 - this.sky.windDir));
    this.sky.windDir += turn * Math.min(1, dt * 0.5);

    const reach = THREE.MathUtils.clamp(distance * REACH, MIN_REACH, MAX_REACH);
    // Far from the camera a thin streak is less than a pixel, so drops grow with the distance they are seen from.
    const size = Math.max(1, distance / 40);
    const windX = Math.cos(this.sky.windDir) * this.sky.wind;
    const windZ = Math.sin(this.sky.windDir) * this.sky.wind;
    // Hail falls through rain too: a shower comes before and after it.
    const rain = Math.max(this.sky.rain, this.sky.hail * 0.5);
    this.fall(this.rain, dt, focus, reach, rain, RAIN_SPEED, windX * RAIN_DRIFT, windZ * RAIN_DRIFT, false);
    this.fall(this.hail, dt, focus, reach, this.sky.hail, HAIL_SPEED, windX * HAIL_DRIFT, windZ * HAIL_DRIFT, true);

    // Every raindrop is the same streak, leaning with the wind; every hailstone the same pellet.
    this.axis.set(windZ, 0, -windX);
    const lean = Math.atan2(Math.hypot(windX * RAIN_DRIFT, windZ * RAIN_DRIFT), RAIN_SPEED);
    if (this.axis.lengthSq() > 1e-6) this.tilt.setFromAxisAngle(this.axis.normalize(), -lean);
    else this.tilt.identity();
    this.write(this.rain, this.basis.compose(this.origin, this.tilt, this.scale.set(0.035 * size, 1.1 * size, 0.035 * size)));
    this.write(this.hail, this.basis.makeScale(0.07 * size, 0.07 * size, 0.07 * size));
  }

  private fall(drops: Drops, dt: number, focus: THREE.Vector3, reach: number, amount: number, speed: number, driftX: number, driftZ: number, bounce: boolean): void {
    const capacity = drops.x.length;
    const count = Math.round(capacity * amount);
    const was = drops.mesh.count;
    // Drops coming on start anywhere in the box, so a shower does not arrive as one sheet.
    for (let i = was; i < count; i++) this.spawn(drops, i, focus, reach, Math.random() * (TOP - BOTTOM) + BOTTOM);
    drops.mesh.count = count;
    const span = reach * 2;
    for (let i = 0; i < count; i++) {
      let x = (drops.x[i] as number) + driftX * dt;
      let z = (drops.z[i] as number) + driftZ * dt;
      // Back in at the far side of the box when the camera has moved on, or the wind has carried it out.
      x = focus.x - reach + ((((x - focus.x + reach) % span) + span) % span);
      z = focus.z - reach + ((((z - focus.z + reach) % span) + span) % span);
      drops.x[i] = x;
      drops.z[i] = z;
      if (!bounce) {
        const y = (drops.y[i] as number) - speed * dt;
        drops.y[i] = y < focus.y + BOTTOM ? focus.y + TOP : y;
        continue;
      }
      const vy = drops.vy[i] as number;
      let y = (drops.y[i] as number) + (vy > 0 ? vy : -speed) * dt;
      if (vy > 0) drops.vy[i] = vy - 30 * dt;
      const ground = this.heightAt(x, z);
      if (y <= ground) {
        if (drops.bounced[i]) {
          this.spawn(drops, i, focus, reach, TOP);
          continue;
        }
        // A hailstone hits the ground and jumps once before it lies still.
        y = ground;
        drops.vy[i] = 3 + (i % 5) * 0.6;
        drops.bounced[i] = 1;
      }
      drops.y[i] = y;
    }
  }

  private spawn(drops: Drops, i: number, focus: THREE.Vector3, reach: number, height: number): void {
    drops.x[i] = focus.x + (Math.random() * 2 - 1) * reach;
    drops.z[i] = focus.z + (Math.random() * 2 - 1) * reach;
    drops.y[i] = focus.y + height;
    drops.vy[i] = 0;
    drops.bounced[i] = 0;
  }

  /** Writes each drop's matrix: the shared basis, moved to where the drop is. */
  private write(drops: Drops, basis: THREE.Matrix4): void {
    const count = drops.mesh.count;
    if (count === 0) return;
    const array = drops.mesh.instanceMatrix.array as Float32Array;
    const e = basis.elements;
    for (let i = 0; i < count; i++) {
      const o = i * 16;
      for (let k = 0; k < 12; k++) array[o + k] = e[k] as number;
      array[o + 12] = drops.x[i] as number;
      array[o + 13] = drops.y[i] as number;
      array[o + 14] = drops.z[i] as number;
      array[o + 15] = 1;
    }
    drops.mesh.instanceMatrix.needsUpdate = true;
  }
}
