import * as THREE from 'three/webgpu';
import { clone as cloneSkinned } from 'three/addons/utils/SkeletonUtils.js';
import { lerp, lerpAngle } from '../core/math2d';
import type { Look } from '../generated/api';
import { TIMING_RIG } from '../core/assetTypes';
import { ANIMS, type AnimDef, type AnimId, clipSeconds } from './anims';
import type { Assets } from './assets';

const FADE_SECONDS = 0.18;
/**
 * How far outside the view a character is still drawn, in metres: enough for
 * a swung weapon and for the shadow it throws into the picture at a low sun.
 */
const CULL_MARGIN = 6;

export type ActorLook = Look;

/** What the view needs from an entity each tick. */
export interface ActorSnapshot {
  x: number;
  z: number;
  facing: number;
  /** an AnimId; anything else plays as idle */
  anim: string;
  animSeq: number;
  visible: boolean;
  /** what is in each hand right now ("bundle/model", or a weapons model id) */
  mainHand: string | null;
  offHand: string | null;
}

/**
 * One animated character or monster. Positions are interpolated between the
 * last two ticks; animation state is reconciled from the stream every tick.
 */
export class ActorView {
  readonly root = new THREE.Group();
  readonly height: number;
  /** which clip set this skeleton plays */
  private readonly rig: string;
  private readonly mixer: THREE.AnimationMixer;
  private readonly actions = new Map<string, THREE.AnimationAction>();
  private current: THREE.AnimationAction | null = null;
  private anim: string | null = null;
  private animSeq = -1;
  private prev = { x: 0, z: 0, facing: 0 };
  private next = { x: 0, z: 0, facing: 0 };
  private fresh = true;
  private readonly hands: { r: THREE.Object3D | null; l: THREE.Object3D | null };
  private held: { r: THREE.Object3D | null; l: THREE.Object3D | null } = { r: null, l: null };
  private heldId: { r: string | null; l: string | null } = { r: null, l: null };
  private readonly model: THREE.Object3D;
  private skeleton: THREE.Skeleton | null = null;
  private readonly bounds = new THREE.Sphere();
  /** animation time that passed while out of view, owed to the mixer */
  private unplayed = 0;

  constructor(private readonly assets: Assets, look: ActorLook) {
    const template = assets.character(look.model);
    this.height = template.height;
    this.rig = template.rig;
    const model = cloneSkinned(template.scene);
    model.scale.setScalar(template.scale);
    if (look.texture && template.texture) {
      const from = template.texture;
      const to = look.texture;
      assets.applyMaterials(model, (tex) => (tex === from ? to : tex));
    }
    model.traverse((object) => {
      const mesh = object as THREE.Mesh;
      if (mesh.isMesh) {
        mesh.castShadow = true;
        mesh.receiveShadow = false;
        mesh.frustumCulled = false; // skinned bounds are the bind pose; update() culls the whole model instead
        if (look.hide.includes(mesh.name)) mesh.visible = false;
      }
      // Cloning gives every part of the body its own skeleton over the same bones; one is enough to update and upload.
      const skinned = object as THREE.SkinnedMesh;
      if (skinned.isSkinnedMesh) {
        if (!this.skeleton) this.skeleton = skinned.skeleton;
        else skinned.bind(this.skeleton, skinned.bindMatrix);
      }
    });
    this.model = model;
    this.root.add(model);
    this.mixer = new THREE.AnimationMixer(model);
    this.hands = { r: model.getObjectByName('handslotr') ?? null, l: model.getObjectByName('handslotl') ?? null };
  }

  /** Puts a weapon or tool model in a hand (null empties it). */
  hold(hand: 'r' | 'l', modelId: string | null): void {
    if (this.heldId[hand] === modelId) return;
    this.heldId[hand] = modelId;
    const slot = this.hands[hand];
    const existing = this.held[hand];
    if (existing) existing.removeFromParent();
    this.held[hand] = null;
    if (!slot || !modelId) return;
    const [bundle, id] = modelId.includes('/') ? (modelId.split('/') as [string, string]) : ['weapons', modelId];
    const item = this.assets.model(bundle, id).clone();
    item.traverse((object) => {
      const mesh = object as THREE.Mesh;
      if (mesh.isMesh) mesh.castShadow = true;
    });
    slot.add(item);
    this.held[hand] = item;
  }

  private action(def: AnimDef): THREE.AnimationAction {
    let action = this.actions.get(def.clip);
    if (!action) {
      action = this.mixer.clipAction(this.assets.clip(this.rig, def.clip));
      this.actions.set(def.clip, action);
    }
    return action;
  }

  /**
   * Playback speed. The sim times one-shot animations (attacks, hits) by the
   * timing rig's clips, so another rig's version is stretched to end on time.
   */
  private timeScale(def: AnimDef): number {
    const speed = def.speed ?? 1;
    if (def.loop || this.rig === TIMING_RIG) return speed;
    return speed * (clipSeconds(this.rig, def.clip) / clipSeconds(TIMING_RIG, def.clip));
  }

  private play(anim: string, immediate: boolean): void {
    const def: AnimDef = ANIMS[anim as AnimId] ?? ANIMS.idle;
    const action = this.action(def);
    action.reset();
    action.setLoop(def.loop ? THREE.LoopRepeat : THREE.LoopOnce, def.loop ? Infinity : 1);
    action.clampWhenFinished = !def.loop;
    action.timeScale = this.timeScale(def);
    action.enabled = true;
    action.setEffectiveWeight(1);
    if (def.loop) action.time = Math.random() * action.getClip().duration; // desynchronise crowds
    if (this.current && this.current !== action && !immediate) {
      action.crossFadeFrom(this.current, FADE_SECONDS, false);
    } else if (this.current && this.current !== action) {
      this.current.stop();
    }
    action.play();
    this.current = action;
  }

  /** Called once per tick with the entity's new state. */
  sync(snapshot: ActorSnapshot): void {
    this.prev = this.fresh ? { x: snapshot.x, z: snapshot.z, facing: snapshot.facing } : this.next;
    this.next = { x: snapshot.x, z: snapshot.z, facing: snapshot.facing };
    this.root.visible = snapshot.visible;
    this.hold('r', snapshot.mainHand);
    this.hold('l', snapshot.offHand);
    if (snapshot.anim !== this.anim || snapshot.animSeq !== this.animSeq) {
      this.play(snapshot.anim, this.fresh);
      this.unplayed = 0;
      this.anim = snapshot.anim;
      this.animSeq = snapshot.animSeq;
    }
    this.fresh = false;
  }

  /** Called every frame; alpha is how far we are between the last two ticks. */
  update(dt: number, alpha: number, heightAt: (x: number, z: number) => number, frustum: THREE.Frustum): void {
    if (!this.root.visible) return;
    const x = lerp(this.prev.x, this.next.x, alpha);
    const z = lerp(this.prev.z, this.next.z, alpha);
    const y = heightAt(x, z);
    this.root.position.set(x, y, z);
    this.root.rotation.y = lerpAngle(this.prev.facing, this.next.facing, alpha);
    // Out of view, neither the body nor its animation is worth the work; the clip catches up when it comes back.
    this.bounds.center.set(x, y + this.height / 2, z);
    this.bounds.radius = this.height + CULL_MARGIN;
    this.model.visible = frustum.intersectsSphere(this.bounds);
    if (!this.model.visible) {
      this.unplayed += dt;
      return;
    }
    this.mixer.update(dt + this.unplayed);
    this.unplayed = 0;
  }

  dispose(): void {
    this.mixer.stopAllAction();
    this.mixer.uncacheRoot(this.model);
    this.skeleton?.dispose();
    this.root.removeFromParent();
  }
}
