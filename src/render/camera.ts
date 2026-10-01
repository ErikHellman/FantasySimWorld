import * as THREE from 'three/webgpu';
import { clamp } from '../core/math2d';
import { worldMap } from '../world/map';

const MIN_DISTANCE = 14;
const MAX_DISTANCE = 190;
const MIN_PITCH = THREE.MathUtils.degToRad(35);
const MAX_PITCH = THREE.MathUtils.degToRad(82);
const KEY_PAN_SPEED = 1.1; // fraction of the camera distance per second
const KEY_ROTATE_SPEED = 1.6;

/**
 * Top-down camera: drag to pan, wheel to zoom, right-drag or Q/E to rotate,
 * WASD / arrows to pan. Optionally follows a moving point.
 */
export class CameraRig {
  readonly camera = new THREE.PerspectiveCamera(32, 1, 1, 900);
  readonly target = new THREE.Vector3();
  distance = 70;
  yaw = 0;
  pitch = THREE.MathUtils.degToRad(58);

  private goalTarget = new THREE.Vector3();
  private goalDistance = this.distance;
  private goalYaw = 0;
  private goalPitch = this.pitch;
  private readonly keys = new Set<string>();
  private drag: { button: number; x: number; y: number; moved: number } | null = null;
  private follow: (() => { x: number; z: number } | null) | null = null;
  private readonly raycaster = new THREE.Raycaster();
  private readonly ground = new THREE.Plane(new THREE.Vector3(0, 1, 0), 0);
  /** called when the user clicks without dragging */
  onClick: ((event: PointerEvent) => void) | null = null;

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

  /** Follow a moving point until the user pans; pass null to stop. */
  setFollow(getPosition: (() => { x: number; z: number } | null) | null): void {
    this.follow = getPosition;
  }

  get isFollowing(): boolean {
    return this.follow !== null;
  }

  resize(width: number, height: number): void {
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
  }

  private groundPoint(clientX: number, clientY: number): THREE.Vector3 | null {
    const rect = this.element.getBoundingClientRect();
    const ndc = new THREE.Vector2(((clientX - rect.left) / rect.width) * 2 - 1, -((clientY - rect.top) / rect.height) * 2 + 1);
    this.raycaster.setFromCamera(ndc, this.camera);
    return this.raycaster.ray.intersectPlane(this.ground, new THREE.Vector3());
  }

  private readonly onPointerDown = (event: PointerEvent): void => {
    this.element.setPointerCapture(event.pointerId);
    this.drag = { button: event.button, x: event.clientX, y: event.clientY, moved: 0 };
  };

  private readonly onPointerMove = (event: PointerEvent): void => {
    const drag = this.drag;
    if (!drag) return;
    const dx = event.clientX - drag.x;
    const dy = event.clientY - drag.y;
    drag.moved += Math.abs(dx) + Math.abs(dy);
    if (drag.button === 0 && !event.shiftKey) {
      const before = this.groundPoint(drag.x, drag.y);
      const after = this.groundPoint(event.clientX, event.clientY);
      if (before && after) {
        this.goalTarget.x += before.x - after.x;
        this.goalTarget.z += before.z - after.z;
        this.target.copy(this.goalTarget);
        this.follow = null;
        this.place();
      }
    } else {
      this.goalYaw -= dx * 0.006;
      this.goalPitch = clamp(this.goalPitch + dy * 0.004, MIN_PITCH, MAX_PITCH);
    }
    drag.x = event.clientX;
    drag.y = event.clientY;
  };

  private readonly onPointerUp = (event: PointerEvent): void => {
    const drag = this.drag;
    this.drag = null;
    if (drag && drag.button === 0 && drag.moved < 5) this.onClick?.(event);
  };

  private readonly onWheel = (event: WheelEvent): void => {
    event.preventDefault();
    const factor = Math.exp(clamp(event.deltaY, -120, 120) * 0.0016);
    this.goalDistance = clamp(this.goalDistance * factor, MIN_DISTANCE, MAX_DISTANCE);
  };

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    const element = event.target as HTMLElement | null;
    if (element && (element.tagName === 'INPUT' || element.tagName === 'TEXTAREA' || element.tagName === 'SELECT')) return;
    this.keys.add(event.code);
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

    const followed = this.follow?.();
    if (this.follow && !followed) this.follow = null;
    if (followed) this.goalTarget.set(followed.x, 0, followed.z);

    this.goalTarget.x = clamp(this.goalTarget.x, worldMap.minX, worldMap.maxX);
    this.goalTarget.z = clamp(this.goalTarget.z, worldMap.minZ, worldMap.maxZ);

    const k = 1 - Math.exp(-dt * 10);
    this.target.lerp(this.goalTarget, k);
    this.distance += (this.goalDistance - this.distance) * k;
    this.yaw += (this.goalYaw - this.yaw) * k;
    this.pitch += (this.goalPitch - this.pitch) * k;
    this.place();
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
