import * as THREE from 'three/webgpu';
import { gameTime } from '../core/time';
import { CLASSES } from '../sim/data/classes';
import { MONSTERS } from '../sim/data/monsters';
import type { SimEvent } from '../sim/events';
import type { Character, Look, Monster, SimState } from '../sim/state';
import { getPlace, places } from '../world/places';
import { scatter } from '../world/scatter';
import type { Assets } from './assets';
import { CameraRig } from './camera';
import { ActorView } from './characters';
import { buildDebugOverlay } from './debug';
import { Effects } from './fx';
import { Lighting } from './lighting';
import { Overlay, type LabelState } from './overlay';
import { SceneryBuilder } from './scenery';
import { Terrain } from './terrain';

const GRAVE_MODELS = ['gravestone', 'gravemarker_A', 'gravemarker_B'];
/** Props that give light at night, and how high above their base the light sits. */
const LAMP_HEIGHTS: Record<string, number> = { post_lantern: 2.7, lantern_standing: 1.1, lantern_hanging: 0.4, torch_lit: 1.2, Campfire_Logs: 0.9 };
const SEA_LEVEL = -1.55;

interface Actor {
  view: ActorView;
  name: string;
  hostile: boolean;
  maxHp: number;
  hp: number;
  x: number;
  z: number;
  visible: boolean;
  selectable: boolean;
}

/** The 3D view of a SimState: built once, then reconciled with the state every tick. */
export class WorldView {
  readonly scene = new THREE.Scene();
  readonly rig: CameraRig;
  readonly lighting: Lighting;
  readonly terrain: Terrain;
  readonly effects: Effects;
  readonly overlay: Overlay;
  private readonly actors = new Map<string, Actor>();
  private readonly graves = new Map<string, THREE.Object3D>();
  private readonly actorGroup = new THREE.Group();
  private readonly heightAt = (x: number, z: number): number => this.terrain.heightAt(x, z);
  private readonly projected = new THREE.Vector3();
  selectedId: string | null = null;

  constructor(private readonly assets: Assets, private readonly canvas: HTMLCanvasElement, overlayElement: HTMLElement) {
    this.rig = new CameraRig(canvas);
    this.lighting = new Lighting(this.scene);
    this.overlay = new Overlay(overlayElement);
    this.effects = new Effects(assets);

    const builder = new SceneryBuilder();
    this.terrain = new Terrain(assets, builder);
    for (const model of [...places.flatMap((place) => place.models), ...scatter.models]) {
      builder.add(assets.model(model.bundle, model.model), {
        x: model.x, y: model.y + this.terrain.heightAt(model.x, model.z), z: model.z, rotation: model.rotation, scale: model.scale,
      });
    }
    this.scene.add(builder.build(), this.actorGroup, this.effects.group);
    this.lighting.setLamps(places.flatMap((place) => place.models.flatMap((model) => {
      const height = LAMP_HEIGHTS[model.model];
      return height === undefined ? [] : [{ x: model.x, y: model.y + this.terrain.heightAt(model.x, model.z) + height, z: model.z, power: model.model.startsWith('Campfire') ? 1.5 : 1 }];
    })));

    // The valley sits in a sea that runs to the horizon.
    const sea = new THREE.Mesh(new THREE.PlaneGeometry(4000, 4000).rotateX(-Math.PI / 2), new THREE.MeshLambertMaterial({ color: 0x3f8fd6 }));
    sea.position.y = SEA_LEVEL;
    sea.receiveShadow = true;
    this.scene.add(sea);

    if (new URLSearchParams(location.search).get('debug') === 'nav') this.scene.add(buildDebugOverlay());
    const green = getPlace('green');
    this.rig.lookAt(green.origin.x, green.origin.z + 8, 78);
  }

  private ensure(id: string, look: Look, name: string, hostile: boolean, maxHp: number, selectable: boolean): Actor {
    let actor = this.actors.get(id);
    if (!actor) {
      const view = new ActorView(this.assets, look);
      this.actorGroup.add(view.root);
      actor = { view, name, hostile, maxHp, hp: maxHp, x: 0, z: 0, visible: true, selectable };
      this.actors.set(id, actor);
    }
    return actor;
  }

  private syncCharacter(c: Character): void {
    const actor = this.ensure(c.id, c.look, c.profile.name.split(' ')[0] as string, false, CLASSES[c.classId].maxHp, true);
    actor.hp = c.hp;
    actor.x = c.pos.x;
    actor.z = c.pos.z;
    actor.visible = !c.inside;
    // Weapons are put away while sitting or working with a tool.
    const atEase = c.anim === 'sit_chair' || c.anim === 'sit_floor' || c.anim === 'lie';
    actor.view.sync({
      x: c.pos.x, z: c.pos.z, facing: c.facing, anim: c.anim, animSeq: c.animSeq, visible: !c.inside,
      mainHand: c.tool ?? (atEase ? null : c.look.mainHand), offHand: c.tool || atEase ? null : c.look.offHand,
    });
  }

  private syncMonster(m: Monster): void {
    const def = MONSTERS[m.kind];
    const actor = this.ensure(m.id, m.look, def.label, true, def.maxHp, false);
    actor.hp = Math.max(0, m.hp);
    actor.x = m.pos.x;
    actor.z = m.pos.z;
    actor.visible = true;
    actor.view.sync({
      x: m.pos.x, z: m.pos.z, facing: m.facing, anim: m.anim, animSeq: m.animSeq, visible: true,
      mainHand: m.look.mainHand, offHand: m.look.offHand,
    });
  }

  /** Brings the view in line with the state after a sim tick, and plays the tick's events. */
  sync(state: SimState, events: readonly SimEvent[]): void {
    const present = new Set<string>();
    for (const id of state.order) {
      const c = state.characters[id];
      if (!c) continue;
      present.add(id);
      this.syncCharacter(c);
    }
    for (const id in state.monsters) {
      present.add(id);
      this.syncMonster(state.monsters[id] as Monster);
    }
    for (const corpse of state.corpses) {
      const id = `corpse:${corpse.id}`;
      present.add(id);
      const actor = this.ensure(id, corpse.look, '', false, 1, false);
      actor.visible = false; // no label
      actor.view.sync({
        x: corpse.pos.x, z: corpse.pos.z, facing: corpse.facing, anim: corpse.anim, animSeq: 0, visible: true,
        mainHand: corpse.look.mainHand, offHand: corpse.look.offHand,
      });
    }
    for (const [id, actor] of this.actors) {
      if (!present.has(id)) {
        actor.view.dispose();
        this.actors.delete(id);
      }
    }
    if (this.selectedId && !state.characters[this.selectedId]) this.selectedId = null;

    this.syncGraves(state);

    for (const event of events) {
      if (event.type === 'speech') continue; // spoken lines reach the overlay through the speech bus
      if (event.type === 'emote') this.overlay.emote(event.id, event.emote);
      else this.effects.handle(event, this.heightAt, (id) => id in state.monsters);
    }
  }

  private syncGraves(state: SimState): void {
    if (this.graves.size === state.graves.length) return;
    const yard = getPlace('graveyard');
    const wanted = new Set(state.graves.map((grave) => `${grave.plot}:${grave.characterId}`));
    for (const [key, object] of this.graves) {
      if (!wanted.has(key)) {
        object.removeFromParent();
        this.graves.delete(key);
      }
    }
    for (const grave of state.graves) {
      const key = `${grave.plot}:${grave.characterId}`;
      const plot = yard.gravePlots[grave.plot];
      if (this.graves.has(key) || !plot) continue;
      const stone = this.assets.model('halloween', GRAVE_MODELS[grave.plot % GRAVE_MODELS.length] as string).clone();
      stone.position.set(plot.pos.x, this.terrain.heightAt(plot.pos.x, plot.pos.z), plot.pos.z);
      stone.rotation.y = plot.facing;
      stone.traverse((object) => {
        const mesh = object as THREE.Mesh;
        if (mesh.isMesh) mesh.castShadow = true;
      });
      this.scene.add(stone);
      this.graves.set(key, stone);
    }
  }

  /** Per rendered frame: camera, light, interpolated actors, effects and labels. */
  frame(dt: number, simDt: number, alpha: number, state: SimState): void {
    this.rig.update(dt);
    this.lighting.update(gameTime(state.tick).hour, this.rig.target, this.rig.distance);
    this.effects.update(simDt);

    const labels: LabelState[] = [];
    for (const [id, actor] of this.actors) {
      actor.view.update(simDt, alpha, this.heightAt);
      if (!actor.name) continue;
      const position = actor.view.root.position;
      labels.push({
        id,
        name: actor.name,
        x: position.x,
        y: position.y + actor.view.height + 0.35,
        z: position.z,
        visible: actor.visible,
        hp: actor.hp / actor.maxHp,
        hurt: actor.hp < actor.maxHp * 0.97,
        hostile: actor.hostile,
        selected: id === this.selectedId,
      });
    }
    this.overlay.update(labels, this.rig.camera, this.rig.distance, this.canvas.clientWidth, this.canvas.clientHeight);
  }

  /** The character nearest to a screen point, if any is close enough to count as clicked. */
  pick(clientX: number, clientY: number): string | null {
    const rect = this.canvas.getBoundingClientRect();
    let best: string | null = null;
    let bestDistance = 46;
    for (const [id, actor] of this.actors) {
      if (!actor.selectable || !actor.visible) continue;
      const position = actor.view.root.position;
      this.projected.set(position.x, position.y + actor.view.height * 0.5, position.z).project(this.rig.camera);
      if (this.projected.z > 1) continue;
      const sx = rect.left + (this.projected.x * 0.5 + 0.5) * rect.width;
      const sy = rect.top + (-this.projected.y * 0.5 + 0.5) * rect.height;
      const d = Math.hypot(sx - clientX, sy - clientY);
      if (d < bestDistance) {
        bestDistance = d;
        best = id;
      }
    }
    return best;
  }

  /** Follows a character with the camera; null stops following. */
  follow(id: string | null): void {
    this.rig.setFollow(id ? () => {
      const actor = this.actors.get(id);
      return actor ? { x: actor.view.root.position.x, z: actor.view.root.position.z } : null;
    } : null);
  }
}
