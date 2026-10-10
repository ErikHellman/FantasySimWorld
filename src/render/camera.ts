import * as THREE from 'three/webgpu';
import { clamp } from '../core/math2d';
import { worldMap } from '../world/map';
import { twoFinger, type Finger, type FingerPair } from './gestures';

const FOV = 32; // vertical, in degrees
const MIN_DISTANCE = 14;
const MAX_DISTANCE = 190;
// Pitch is the camera's angle above the horizon. The low cap keeps it above the ground; the high cap stops just
// short of straight down, where lookAt with a +Y up vector has no defined yaw and the view would flip.
const MIN_PITCH = THREE.MathUtils.degToRad(15);
const MAX_PITCH = THREE.MathUtils.degToRad(89.5);
const KEY_PAN_SPEED = 1.1; // fraction of the camera distance per second
const KEY_ROTATE_SPEED = 1.6;
const KEY_TILT_SPEED = 0.9; // radians per second
const WHEEL_TILT_SPEED = 0.0013; // radians per wheel delta unit, about 9° per mouse-wheel notch
const DRAG_TILT_SPEED = 0.004; // radians per pixel, for right-drag and two-finger drags alike
/** The keys that move the camera, and so count as the viewer taking it. */
const CAMERA_KEYS = new Set(['KeyW', 'KeyA', 'KeyS', 'KeyD', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'KeyQ', 'KeyE', 'KeyR', 'KeyF']);

/**
 * Top-down camera: drag to pan, wheel to zoom, right-drag or Q/E to rotate,
 * Shift/Alt + wheel, right-drag up and down or R/F to tilt, WASD / arrows to pan.
 * On a touch screen: one finger pans, two fingers pinch to zoom, twist to rotate and drag up and down to tilt.
 * Optionally follows a moving point.
 */
/** A point the camera follows, and how high above the ground it is. */
export interface Followed {
  x: number;
  z: number;
  lift?: number;
}

export class CameraRig {
  readonly camera = new THREE.PerspectiveCamera(FOV, 1, 1, 900);
  readonly target = new THREE.Vector3();
  distance = 70;
  yaw = 0;
  pitch = THREE.MathUtils.degToRad(58);

  private goalTarget = new THREE.Vector3();
  private goalDistance = this.distance;
  private goalYaw = 0;
  private goalPitch = this.pitch;
  private readonly keys = new Set<string>();
  /** where each finger or mouse button that is down was last seen */
  private readonly pointers = new Map<number, Finger>();
  /** the press in progress; `multi` once a second finger joined it, which rules it out as a click */
  private drag: { button: number; moved: number; multi: boolean } | null = null;
  /** the first two fingers as the camera last read them; null until a second finger is down */
  private pair: FingerPair | null = null;
  private follow: (() => Followed | null) | null = null;
  private readonly raycaster = new THREE.Raycaster();
  private readonly ground = new THREE.Plane(new THREE.Vector3(0, 1, 0), 0);
  private size = { width: 1, height: 1 };
  /** pixels the HUD covers along the right and bottom edges, eased towards `goalInset` */
  private inset = { right: 0, bottom: 0 };
  private goalInset = { right: 0, bottom: 0 };
  /** called when the user clicks without dragging */
  onClick: ((event: PointerEvent) => void) | null = null;
  /** `performance.now()` when the viewer last touched the camera; the Auto camera waits for them to leave it alone */
  lastInputAt = -Infinity;

  constructor(private readonly element: HTMLElement) {
    element.addEventListener('pointerdown', this.onPointerDown);
    element.addEventListener('pointermove', this.onPointerMove);
    element.addEventListener('pointerup', this.onPointerUp);
    element.addEventListener('pointercancel', this.onPointerUp);
    element.addEventListener('wheel', this.onWheel, { passive: false });
    element.addEventListener('contextmenu', (event) => event.preventDefault());
    window.addEventListener('keydown', this.onKeyDown);
    window.addEventListener('keyup', (event) => this.keys.delete(event.code));
    window.addEventListener('blur', () => this.keys.clear());
  }

  lookAt(x: number, z: number, distance?: number): void {
    this.goalTarget.set(x, 0, z);
    this.target.copy(this.goalTarget);
    if (distance !== undefined) this.distance = this.goalDistance = distance;
  }

  /** Moves the view smoothly to a point on the ground, as a pan would, and stops following. */
  glideTo(x: number, z: number): void {
    this.follow = null;
    this.goalTarget.set(x, 0, z);
  }

  /** Follow a moving point until the user pans; pass null to stop. */
  setFollow(getPosition: (() => Followed | null) | null): void {
    this.follow = getPosition;
  }

  get isFollowing(): boolean {
    return this.follow !== null;
  }

  resize(width: number, height: number): void {
    this.size = { width, height };
    this.project();
  }

  /**
   * Tells the camera how much of the canvas the HUD covers along its right and bottom edges, as a phone's open pane
   * does. The view then centres on the part still showing, at the same scale, so a followed villager stays in sight.
   */
  setInset(right: number, bottom: number): void {
    this.goalInset = { right, bottom };
  }

  /** Renders the canvas as the top-left part of a larger frame whose centre is the middle of the uncovered area. */
  private project(): void {
    const { width, height } = this.size;
    const { right, bottom } = this.inset;
    const fullWidth = width + right;
    const fullHeight = height + bottom;
    this.camera.aspect = fullWidth / fullHeight;
    // A wider field over the taller frame keeps the canvas' own pixels at the scale they have with nothing covered.
    this.camera.fov = THREE.MathUtils.radToDeg(2 * Math.atan(Math.tan(THREE.MathUtils.degToRad(FOV / 2)) * (fullHeight / height)));
    if (right > 0 || bottom > 0) this.camera.setViewOffset(fullWidth, fullHeight, right, bottom, width, height);
    else this.camera.clearViewOffset();
    this.camera.updateProjectionMatrix();
  }

  /** The ray from the camera through a point on the page (in client pixels), for picking what is under it. */
  rayAt(clientX: number, clientY: number): THREE.Raycaster {
    const rect = this.element.getBoundingClientRect();
    const ndc = new THREE.Vector2(((clientX - rect.left) / rect.width) * 2 - 1, -((clientY - rect.top) / rect.height) * 2 + 1);
    this.raycaster.setFromCamera(ndc, this.camera);
    return this.raycaster;
  }

  private groundPoint(clientX: number, clientY: number): THREE.Vector3 | null {
    return this.rayAt(clientX, clientY).ray.intersectPlane(this.ground, new THREE.Vector3());
  }

  private readonly onPointerDown = (event: PointerEvent): void => {
    this.lastInputAt = performance.now();
    this.element.setPointerCapture(event.pointerId);
    this.pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (this.pointers.size === 1) this.drag = { button: event.button, moved: 0, multi: false };
    else if (this.drag) this.drag.multi = true;
    this.pair = null;
  };

  private readonly onPointerMove = (event: PointerEvent): void => {
    const drag = this.drag;
    const last = this.pointers.get(event.pointerId);
    if (!drag || !last) return;
    const next = { x: event.clientX, y: event.clientY };
    const dx = next.x - last.x;
    const dy = next.y - last.y;
    drag.moved += Math.abs(dx) + Math.abs(dy);
    this.pointers.set(event.pointerId, next);
    // Fingers report one at a time, so two-finger gestures are read once per frame in update(), when both have moved.
    if (this.pointers.size > 1) return;
    if (drag.button === 0 && !event.shiftKey) {
      const before = this.groundPoint(last.x, last.y);
      const after = this.groundPoint(next.x, next.y);
      if (before && after) {
        this.goalTarget.x += before.x - after.x;
        this.goalTarget.z += before.z - after.z;
        this.target.copy(this.goalTarget);
        this.follow = null;
        this.place();
      }
    } else {
      this.goalYaw -= dx * 0.006;
      this.tilt(dy * DRAG_TILT_SPEED);
    }
  };

  private readonly onPointerUp = (event: PointerEvent): void => {
    if (!this.pointers.delete(event.pointerId)) return;
    this.pair = null;
    // A finger left on the screen after a pinch carries on panning from where it is now, with no jump.
    if (this.pointers.size > 0) return;
    const drag = this.drag;
    this.drag = null;
    if (drag && drag.button === 0 && !drag.multi && drag.moved < 5) this.onClick?.(event);
  };

  private readonly onWheel = (event: WheelEvent): void => {
    event.preventDefault();
    this.lastInputAt = performance.now();
    if (event.shiftKey || event.altKey) {
      // macOS browsers turn Shift + wheel into horizontal scrolling, so the delta may arrive on x instead of y.
      const delta = event.deltaY || event.deltaX;
      this.tilt(-clamp(delta, -120, 120) * WHEEL_TILT_SPEED);
      return;
    }
    const factor = Math.exp(clamp(event.deltaY, -120, 120) * 0.0016);
    this.goalDistance = clamp(this.goalDistance * factor, MIN_DISTANCE, MAX_DISTANCE);
  };

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    const element = event.target as HTMLElement | null;
    if (element && (element.tagName === 'INPUT' || element.tagName === 'TEXTAREA' || element.tagName === 'SELECT')) return;
    this.keys.add(event.code);
    if (CAMERA_KEYS.has(event.code)) this.lastInputAt = performance.now();
  };

  update(dt: number): void {
    const forward = (this.keys.has('KeyW') || this.keys.has('ArrowUp') ? 1 : 0) - (this.keys.has('KeyS') || this.keys.has('ArrowDown') ? 1 : 0);
    const right = (this.keys.has('KeyD') || this.keys.has('ArrowRight') ? 1 : 0) - (this.keys.has('KeyA') || this.keys.has('ArrowLeft') ? 1 : 0);
    if (forward || right) {
      const speed = this.distance * KEY_PAN_SPEED * dt;
      const sin = Math.sin(this.yaw);
      const cos = Math.cos(this.yaw);
      this.goalTarget.x += (right * cos - forward * sin) * speed;
      this.goalTarget.z += (-right * sin - forward * cos) * speed;
      this.follow = null;
    }
    const turn = (this.keys.has('KeyE') ? 1 : 0) - (this.keys.has('KeyQ') ? 1 : 0);
    if (turn) this.goalYaw += turn * KEY_ROTATE_SPEED * dt;
    const raise = (this.keys.has('KeyR') ? 1 : 0) - (this.keys.has('KeyF') ? 1 : 0);
    if (raise) this.tilt(raise * KEY_TILT_SPEED * dt);
    this.readFingers();

    const followed = this.follow?.();
    if (this.follow && !followed) this.follow = null;
    // Something in the air (the dragon) is looked at up there, not at its shadow; the view settles back to the
    // ground when it lands or the follow ends.
    if (followed) this.goalTarget.set(followed.x, followed.lift ?? 0, followed.z);
    else this.goalTarget.y = 0;

    this.goalTarget.x = clamp(this.goalTarget.x, worldMap.minX, worldMap.maxX);
    this.goalTarget.z = clamp(this.goalTarget.z, worldMap.minZ, worldMap.maxZ);

    const k = 1 - Math.exp(-dt * 10);
    if (this.inset.right !== this.goalInset.right || this.inset.bottom !== this.goalInset.bottom) {
      const ease = (from: number, to: number): number => (Math.abs(to - from) < 0.5 ? to : from + (to - from) * k);
      this.inset = { right: ease(this.inset.right, this.goalInset.right), bottom: ease(this.inset.bottom, this.goalInset.bottom) };
      this.project();
    }
    this.target.lerp(this.goalTarget, k);
    this.distance += (this.goalDistance - this.distance) * k;
    this.yaw += (this.goalYaw - this.yaw) * k;
    this.pitch += (this.goalPitch - this.pitch) * k;
    this.place();
  }

  /** Pinch zooms, twist turns and two fingers dragged up or down tilt; none of them stops following. */
  private readFingers(): void {
    if (this.pointers.size < 2) return;
    const [a, b] = this.pointers.values();
    const now: FingerPair = [a!, b!];
    if (this.pair) {
      const move = twoFinger(this.pair, now);
      this.goalDistance = clamp(this.goalDistance / move.scale, MIN_DISTANCE, MAX_DISTANCE);
      this.goalYaw += move.twist;
      this.tilt(move.lift * DRAG_TILT_SPEED);
    }
    this.pair = now;
  }

  /** Tilts towards looking straight down (positive) or towards the horizon (negative), within the caps. */
  private tilt(delta: number): void {
    this.goalPitch = clamp(this.goalPitch + delta, MIN_PITCH, MAX_PITCH);
  }

  private place(): void {
    const horizontal = Math.cos(this.pitch) * this.distance;
    this.camera.position.set(
      this.target.x + Math.sin(this.yaw) * horizontal,
      this.target.y + Math.sin(this.pitch) * this.distance,
      this.target.z + Math.cos(this.yaw) * horizontal,
    );
    this.camera.lookAt(this.target);
    this.camera.updateMatrixWorld();
  }
}
