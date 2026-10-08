import * as THREE from 'three/webgpu';
import type { WireEvent } from '../generated/api';
import type { ViewEntity, ViewWorld } from '../net/viewWorld';
import { billboards, getPlace, places, type PlacedModel } from '../world/places';
import { GROUND_COVER, scatter } from '../world/scatter';
import type { Assets } from './assets';
import { Billboards } from './billboards';
import { CameraRig } from './camera';
import { CeremonyDressing } from './ceremony';
import { ActorView } from './characters';
import { buildDebugOverlay } from './debug';
import { Effects } from './fx';
import { Lighting } from './lighting';
import { LootMarks } from './loot';
import { Overlay, type LabelState } from './overlay';
import type { QualitySettings } from './quality';
import { Marks } from './marks';
import { PartCache, SceneryBuilder } from './scenery';
import { Structures, isBuilding } from './structures';
import { Terrain } from './terrain';
import { Vegetation, isVegetation } from './vegetation';

const GRAVE_MODELS = ['gravestone', 'gravemarker_A', 'gravemarker_B'];
/** Props that give light at night, and how high above their base the light sits. */
const LAMP_HEIGHTS: Record<string, number> = { post_lantern: 2.7, lantern_standing: 1.1, lantern_hanging: 0.4, torch_lit: 1.2, Campfire_Logs: 0.9 };
const SEA_LEVEL = -1.55;

interface Actor {
  view: ActorView;
  /** the entity as last synced (the view world updates it in place) */
  entity: ViewEntity;
}

/** The 3D view of the world: built once, then reconciled with the view world every tick. */
export class WorldView {
  readonly scene = new THREE.Scene();
  readonly rig: CameraRig;
  readonly lighting: Lighting;
  readonly terrain: Terrain;
  readonly effects: Effects;
  readonly overlay: Overlay;
  private readonly actors = new Map<string, Actor>();
  private readonly graves = new Map<string, THREE.Object3D>();
  private gravesVersion = -1;
  private readonly billboards: Billboards;
  private billboardsVersion = -1;
  private readonly structures: Structures;
  private housesVersion = -1;
  private readonly dressing: CeremonyDressing;
  private ceremonyVersion = -1;
  private readonly vegetation: Vegetation;
  private readonly marks: Marks;
  private readonly loot: LootMarks;
  private lootVersion = -1;
  private readonly builder: SceneryBuilder;
  private worldObjectsVersion = -1;
  private readonly actorGroup = new THREE.Group();
  private readonly heightAt = (x: number, z: number): number => this.terrain.heightAt(x, z);
  private readonly projected = new THREE.Vector3();
  private readonly frustum = new THREE.Frustum();
  private readonly viewProjection = new THREE.Matrix4();
  selectedId: string | null = null;

  constructor(private readonly assets: Assets, private readonly canvas: HTMLCanvasElement, overlayElement: HTMLElement, quality: QualitySettings) {
    this.rig = new CameraRig(canvas);
    this.lighting = new Lighting(this.scene, quality);
    this.overlay = new Overlay(overlayElement);
    this.effects = new Effects(assets);

    const parts = new PartCache();
    const builder = new SceneryBuilder(parts);
    this.builder = builder;
    this.terrain = new Terrain(assets, builder);
    this.marks = new Marks(assets, builder, this.heightAt, quality);
    this.loot = new LootMarks(assets, this.heightAt);
    const place = (model: PlacedModel, castShadow: boolean): number =>
      builder.add(assets.model(model.bundle, model.model), {
        x: model.x, y: model.y + this.terrain.heightAt(model.x, model.z), z: model.z, rotation: model.rotation, scale: model.scale, castShadow,
      });
    // The buildings are left out here: `Structures` keeps each as its own object, because one can burn down or be
    // taken apart. The props stay merged, but `Marks` remembers each one's handle so it can be hidden when it is.
    for (const entry of places) {
      entry.models.forEach((model, index) => {
        if (isBuilding(entry, model)) return;
        this.marks.prop(entry.id, index, place(model, true));
      });
    }
    // Grass, flowers and the like are most of the valley's triangles; their shadows are specks nobody would miss.
    // A small machine draws only some of them, the same ones every time.
    // Trees and rocks are left out too: `Vegetation` draws them as instances, because the sim can fell and mine them.
    let cover = 0;
    for (const model of scatter.models) {
      if (isVegetation(model)) continue;
      if (!GROUND_COVER.has(model.model)) place(model, true);
      else if (cover++ % quality.groundCoverStep === 0) {
        const handle = place(model, false);
        if (model.model.startsWith('Grass')) this.marks.grassOn(model, handle);
      }
    }
    this.vegetation = new Vegetation(assets, scatter.models, this.heightAt, quality, parts);
    this.billboards = new Billboards(billboards, this.heightAt, quality.billboardPixels);
    this.structures = new Structures(assets, places, this.heightAt, quality);
    this.dressing = new CeremonyDressing(assets, this.heightAt);
    this.scene.add(builder.build(), this.vegetation.group, this.structures.group, this.marks.group, this.loot.group, this.dressing.group, this.billboards.group, this.actorGroup, this.effects.group);
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

  /** Brings the view in line with the world after a tick, and plays the tick's events. */
  sync(world: ViewWorld, events: readonly WireEvent[]): void {
    for (const entity of world.entities.values()) {
      let actor = this.actors.get(entity.id);
      if (!actor) {
        const view = new ActorView(this.assets, entity.look);
        this.actorGroup.add(view.root);
        actor = { view, entity };
        this.actors.set(entity.id, actor);
      }
      actor.entity = entity;
      actor.view.sync(entity);
    }
    for (const [id, actor] of this.actors) {
      if (!world.entities.has(id)) {
        actor.view.dispose();
        this.actors.delete(id);
      }
    }
    if (this.selectedId && !world.entities.has(this.selectedId)) this.selectedId = null;

    this.syncGraves(world);
    if (this.billboardsVersion !== world.billboardsVersion) {
      this.billboardsVersion = world.billboardsVersion;
      this.billboards.show(world.billboards);
    }
    this.vegetation.setTick(world.tick);
    const changed = this.housesVersion !== world.housesVersion || this.worldObjectsVersion !== world.worldObjectsVersion;
    if (changed) {
      this.housesVersion = world.housesVersion;
      this.worldObjectsVersion = world.worldObjectsVersion;
      this.structures.show(world.houses, world.worldObjects);
      this.vegetation.show(world.worldObjects);
      this.marks.show(world.worldObjects);
    }
    if (this.ceremonyVersion !== world.ceremonyVersion) {
      this.ceremonyVersion = world.ceremonyVersion;
      this.dressing.show(world.ceremony, world.graves);
    }
    if (this.lootVersion !== world.lootVersion) {
      this.lootVersion = world.lootVersion;
      this.loot.show(world.loot);
    }
    // A prop or grass hidden in a baked chunk needs that chunk baked again: one chunk per tick, at most.
    this.builder.flush();

    for (const event of events) {
      if (event.type === 'speech') continue; // spoken lines reach the overlay through the speech bus
      if (event.type === 'emote') this.overlay.emote(event.id, event.emote);
      else this.effects.handle(event, this.heightAt, (id) => world.isMonster(id));
    }
  }

  /** Graves are keyed `plot:characterId`, so a reused plot gets its new stone. */
  private syncGraves(world: ViewWorld): void {
    if (this.gravesVersion === world.gravesVersion) return;
    this.gravesVersion = world.gravesVersion;
    const yard = getPlace('graveyard');
    for (const [key, object] of this.graves) {
      if (!world.graves.has(key)) {
        object.removeFromParent();
        this.graves.delete(key);
      }
    }
    for (const [key, grave] of world.graves) {
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
  frame(dt: number, simDt: number, alpha: number, hour: number): void {
    this.rig.update(dt);
    this.structures.update(dt);
    this.vegetation.update(dt, alpha);
    this.lighting.setFire(this.structures.light());
    this.lighting.update(hour, this.rig.target, this.rig.distance);
    this.effects.update(simDt);

    const camera = this.rig.camera;
    this.viewProjection.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.frustum.setFromProjectionMatrix(this.viewProjection, camera.coordinateSystem, camera.reversedDepth);

    const labels: LabelState[] = [];
    for (const [id, actor] of this.actors) {
      actor.view.update(simDt, alpha, this.heightAt, this.frustum);
      const entity = actor.entity;
      if (!entity.name) continue; // corpses have no label
      const position = actor.view.root.position;
      labels.push({
        id,
        name: entity.name,
        x: position.x,
        y: position.y + actor.view.height + 0.35,
        z: position.z,
        visible: entity.visible,
        hp: entity.hp / entity.maxHp,
        hurt: entity.hp < entity.maxHp * 0.97,
        hostile: entity.hostile,
        selected: id === this.selectedId,
        asleep: entity.asleep,
      });
    }
    this.overlay.update(labels, this.rig.camera, this.rig.distance, this.canvas.clientWidth, this.canvas.clientHeight);
  }

  /**
   * The villager or monster nearest to a screen point, if any is within `radius` pixels of it and so counts as
   * clicked. A body has no card and cannot be picked.
   */
  pick(clientX: number, clientY: number, radius = 46): string | null {
    const rect = this.canvas.getBoundingClientRect();
    let best: string | null = null;
    let bestDistance = radius;
    for (const [id, actor] of this.actors) {
      if (actor.entity.kind === 'corpse' || !actor.entity.visible) continue;
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
