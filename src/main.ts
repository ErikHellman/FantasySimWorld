import { GameLoop } from './app/loop';
import { Assets } from './render/assets';
import { backendFromUrl, createRenderer, fitRenderer } from './render/renderer';
import { WorldView } from './render/scene';
import { createState } from './sim/create';
import { Sim } from './sim/sim';

async function start(): Promise<void> {
  const canvas = document.getElementById('world') as HTMLCanvasElement;
  const overlay = document.getElementById('overlay') as HTMLElement;
  const { renderer, backend } = await createRenderer(canvas, backendFromUrl());
  const assets = await Assets.load();

  const sim = new Sim(createState());
  const view = new WorldView(assets, canvas, overlay);
  view.sync(sim.state, []);
  view.rig.onClick = (event) => {
    view.selectedId = view.pick(event.clientX, event.clientY);
    view.follow(view.selectedId);
  };

  const loop = new GameLoop(sim, {
    onTick: (events) => view.sync(sim.state, events),
    onFrame: (realDt, simDt, alpha) => {
      if (!fitRenderer(renderer, view.rig)) return;
      view.frame(realDt, simDt, alpha, sim.state);
      renderer.render(view.scene, view.rig.camera);
    },
  });
  renderer.setAnimationLoop(() => loop.frame());

  (window as unknown as { __debug: unknown }).__debug = { sim, view, loop, renderer, backend };
  console.info(`Fantasy Sim World: ${backend} backend`);
}

start().catch((error) => {
  console.error(error);
  document.body.insertAdjacentHTML('beforeend', '<pre id="fatal" style="position:fixed;top:8px;left:8px;color:#f88"></pre>');
  (document.getElementById('fatal') as HTMLElement).textContent = String(error);
});
