import * as THREE from 'three/webgpu';

interface Keyframe {
  hour: number;
  sun: number;
  sunIntensity: number;
  sky: number;
  ground: number;
  ambient: number;
  background: number;
}

/** Light colours through the day; values are interpolated between neighbours. */
const KEYFRAMES: Keyframe[] = [
  { hour: 0, sun: 0x8fa8ff, sunIntensity: 0.55, sky: 0x3d4f94, ground: 0x141a30, ambient: 0.62, background: 0x10162c },
  { hour: 4.6, sun: 0x8fa8ff, sunIntensity: 0.55, sky: 0x3d4f94, ground: 0x141a30, ambient: 0.62, background: 0x10162c },
  { hour: 5.6, sun: 0xffb98a, sunIntensity: 1.7, sky: 0xe0c0b0, ground: 0x5f504a, ambient: 1.05, background: 0xf0c0a0 },
  { hour: 7, sun: 0xfff1d6, sunIntensity: 2.7, sky: 0xcfe6ff, ground: 0x6f7a5c, ambient: 1.25, background: 0x9fd4f2 },
  { hour: 12, sun: 0xffffff, sunIntensity: 3.0, sky: 0xd6ecff, ground: 0x7a8562, ambient: 1.35, background: 0x8fcdf0 },
  { hour: 17.5, sun: 0xffe9c4, sunIntensity: 2.6, sky: 0xcfe2f5, ground: 0x6f7a5c, ambient: 1.2, background: 0x9bcfee },
  { hour: 19.6, sun: 0xff9a5c, sunIntensity: 1.5, sky: 0xd9a58f, ground: 0x54433f, ambient: 0.98, background: 0xe59a6e },
  { hour: 21, sun: 0x8fa8ff, sunIntensity: 0.6, sky: 0x4254a0, ground: 0x161c34, ambient: 0.66, background: 0x18203c },
  { hour: 24, sun: 0x8fa8ff, sunIntensity: 0.55, sky: 0x3d4f94, ground: 0x141a30, ambient: 0.62, background: 0x10162c },
];

const SHADOW_RANGE = 70;
/** Point lights are a fixed pool (changing the number of lights recompiles shaders); they move to the lamps nearest the camera. */
const LAMP_POOL = 6;
const LAMP_COLOR = 0xffb45e;

export interface Lamp {
  x: number;
  y: number;
  z: number;
  /** relative brightness */
  power: number;
}

export class Lighting {
  readonly sun = new THREE.DirectionalLight(0xffffff, 3);
  readonly hemisphere = new THREE.HemisphereLight(0xd6ecff, 0x7a8562, 1.3);
  readonly background = new THREE.Color(0x8fcdf0);
  /** 0 in full daylight, 1 at night; used for lamps and windows */
  night = 0;
  private readonly lampLights: THREE.PointLight[] = [];
  private lamps: Lamp[] = [];
  private readonly a = new THREE.Color();
  private readonly b = new THREE.Color();
  private readonly offset = new THREE.Vector3();

  constructor(scene: THREE.Scene) {
    this.sun.castShadow = true;
    this.sun.shadow.mapSize.set(2048, 2048);
    const camera = this.sun.shadow.camera;
    camera.left = -SHADOW_RANGE;
    camera.right = SHADOW_RANGE;
    camera.top = SHADOW_RANGE;
    camera.bottom = -SHADOW_RANGE;
    camera.near = 1;
    camera.far = 400;
    camera.updateProjectionMatrix();
    this.sun.shadow.bias = -0.0005;
    this.sun.shadow.normalBias = 0.08;
    scene.add(this.sun, this.sun.target, this.hemisphere);
    scene.background = this.background;
    for (let i = 0; i < LAMP_POOL; i++) {
      const light = new THREE.PointLight(LAMP_COLOR, 0, 26, 1.6);
      this.lampLights.push(light);
      scene.add(light);
    }
  }

  /** Where the lanterns, torches and fires are. */
  setLamps(lamps: Lamp[]): void {
    this.lamps = lamps;
  }

  private placeLamps(focus: THREE.Vector3): void {
    if (this.night <= 0.02) {
      for (const light of this.lampLights) light.intensity = 0;
      return;
    }
    const nearest = [...this.lamps]
      .sort((p, q) => (p.x - focus.x) ** 2 + (p.z - focus.z) ** 2 - ((q.x - focus.x) ** 2 + (q.z - focus.z) ** 2))
      .slice(0, LAMP_POOL);
    this.lampLights.forEach((light, index) => {
      const lamp = nearest[index];
      if (!lamp) {
        light.intensity = 0;
        return;
      }
      light.position.set(lamp.x, lamp.y, lamp.z);
      light.intensity = this.night * 45 * lamp.power;
    });
  }

  private mix(from: number, to: number, t: number, out: THREE.Color): THREE.Color {
    return out.copy(this.a.setHex(from)).lerp(this.b.setHex(to), t);
  }

  /** Sets colours and sun direction for an hour of the day, keeping the shadow box around `focus`. */
  update(hour: number, focus: THREE.Vector3, viewDistance: number): void {
    let i = 0;
    while (i < KEYFRAMES.length - 2 && hour >= (KEYFRAMES[i + 1] as Keyframe).hour) i++;
    const from = KEYFRAMES[i] as Keyframe;
    const to = KEYFRAMES[i + 1] as Keyframe;
    const t = THREE.MathUtils.clamp((hour - from.hour) / (to.hour - from.hour), 0, 1);

    this.mix(from.sun, to.sun, t, this.sun.color);
    this.sun.intensity = THREE.MathUtils.lerp(from.sunIntensity, to.sunIntensity, t);
    this.mix(from.sky, to.sky, t, this.hemisphere.color);
    this.mix(from.ground, to.ground, t, this.hemisphere.groundColor);
    this.hemisphere.intensity = THREE.MathUtils.lerp(from.ambient, to.ambient, t);
    this.mix(from.background, to.background, t, this.background);
    this.night = THREE.MathUtils.clamp(1 - (this.sun.intensity - 0.6) / 1.2, 0, 1);

    // The sun (or moon) sweeps east to west; at night the moon takes a fixed high angle.
    const dayT = THREE.MathUtils.clamp((hour - 5.5) / 15, 0, 1);
    const isDay = hour > 5.5 && hour < 20.5;
    const azimuth = isDay ? THREE.MathUtils.lerp(-1.1, 1.1, dayT) : 0.6;
    const elevation = isDay ? 0.5 + Math.sin(dayT * Math.PI) * 0.55 : 0.95;
    this.offset.set(Math.sin(azimuth) * Math.cos(elevation), Math.sin(elevation), 0.35 * Math.cos(elevation) + 0.2).normalize();

    // Snap the shadow box to whole texels so shadows do not shimmer as the camera pans.
    const range = Math.min(SHADOW_RANGE, Math.max(30, viewDistance * 0.75));
    const camera = this.sun.shadow.camera;
    if (camera.right !== range) {
      camera.left = -range;
      camera.right = range;
      camera.top = range;
      camera.bottom = -range;
      camera.updateProjectionMatrix();
    }
    const texel = (range * 2) / this.sun.shadow.mapSize.x;
    const fx = Math.round(focus.x / texel) * texel;
    const fz = Math.round(focus.z / texel) * texel;
    this.sun.target.position.set(fx, 0, fz);
    this.sun.position.set(fx, 0, fz).addScaledVector(this.offset, 180);
    this.sun.target.updateMatrixWorld();
    this.placeLamps(focus);
  }
}
