// Slice 1 demo: terrain plus a line-up of animated characters, to check the render stack.
import * as THREE from 'three/webgpu';
import { hexCenter } from './world/hex';
import { Assets } from './render/assets';
import { CameraRig } from './render/camera';
import { ActorView } from './render/characters';
import { Lighting } from './render/lighting';
import { backendFromUrl, createRenderer, fitRenderer } from './render/renderer';
import { SceneryBuilder } from './render/scenery';
import { Terrain } from './render/terrain';
import { buildDebugOverlay } from './render/debug';
import { places } from './world/places';
import { scatter } from './world/scatter';
import type { AnimId } from './sim/data/anims';

async function start(): Promise<void> {
  const canvas = document.getElementById('world') as HTMLCanvasElement;
  const { renderer, backend } = await createRenderer(canvas, backendFromUrl());
  const assets = await Assets.load();
  const scene = new THREE.Scene();
  const lighting = new Lighting(scene);
  const builder = new SceneryBuilder();
  const terrain = new Terrain(assets, builder);
  for (const model of [...places.flatMap((place) => place.models), ...scatter.models]) {
    builder.add(assets.model(model.bundle, model.model), {
      x: model.x, y: model.y + terrain.heightAt(model.x, model.z), z: model.z, rotation: model.rotation, scale: model.scale,
    });
  }
  if (new URLSearchParams(location.search).get('debug') === 'nav') scene.add(buildDebugOverlay());
  scene.add(builder.build());

  const rig = new CameraRig(canvas);
  const centre = hexCenter({ col: 7, row: 7 });
  rig.lookAt(centre.x, centre.z, 60);

  const cast: Array<[string, string | null, string | null, AnimId]> = [
    ['Knight', 'sword_1handed', 'shield_badge', 'walk'],
    ['Barbarian', 'axe_2handed', null, 'attack_2h_chop'],
    ['Mage', 'staff', 'spellbook_open', 'cast_long'],
    ['Ranger', null, 'bow_withString', 'bow_idle'],
    ['Rogue', 'dagger', 'dagger', 'idle'],
    ['Rogue_Hooded', 'crossbow_1handed', null, 'run'],
    ['Druid', 'druid_staff', null, 'sit_floor'],
    ['Engineer', 'engineer_Wrench', null, 'hammer'],
    ['Skeleton_Warrior', 'Skeleton_Blade', 'Skeleton_Shield_Large_A', 'skeleton_idle'],
    ['Skeleton_Mage', 'Skeleton_Staff', null, 'skeleton_walk'],
    ['Necromancer', 'Skeleton_Scythe', null, 'magic_summon'],
    ['Paladin', 'paladin_hammer', 'paladin_shield', 'wave'],
    ['Witch', 'Broom', null, 'idle'],
    ['Farmer_A', 'farmer_pitchfork', null, 'dig'],
  ];
  const actors = cast.map(([model, mainHand, offHand, anim], index) => {
    const actor = new ActorView(assets, { model, texture: null, hide: [], mainHand, offHand });
    actor.sync({ x: centre.x - 20 + index * 3.2, z: centre.z + 0.5, facing: 0, anim, animSeq: 0, visible: true });
    scene.add(actor.root);
    return actor;
  });

  let last = performance.now();
  let hour = 10;
  (window as unknown as { __debug: unknown }).__debug = { renderer, scene, rig, backend, setHour: (h: number) => (hour = h) };
  renderer.setAnimationLoop(() => {
    const now = performance.now();
    const dt = Math.min(0.1, (now - last) / 1000);
    last = now;
    if (!fitRenderer(renderer, rig)) return;
    rig.update(dt);
    lighting.update(hour, rig.target, rig.distance);
    for (const actor of actors) actor.update(dt, 1, (x, z) => terrain.heightAt(x, z));
    renderer.render(scene, rig.camera);
  });
  console.info(`Fantasy Sim World: ${backend} backend, ${builder.count} scenery instances`);
}

start().catch((error) => {
  console.error(error);
  document.body.insertAdjacentHTML('beforeend', `<pre style="position:fixed;top:8px;left:8px;color:#f88">${String(error)}</pre>`);
});
