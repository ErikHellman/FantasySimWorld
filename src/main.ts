import { h, render } from 'preact';
import { GameLoop } from './app/loop';
import { Autosaver } from './persist/autosave';
import { clearSaves, loadSave, loadSettings, saveSettings, writeSave } from './persist/db';
import { downloadSave, readSaveFile } from './persist/exportImport';
import { Assets } from './render/assets';
import { backendFromUrl, createRenderer, fitRenderer } from './render/renderer';
import { WorldView } from './render/scene';
import { createState } from './sim/create';
import { Sim } from './sim/sim';
import { App } from './ui/App';
import { actions, refreshUi, ui } from './ui/store';

const UI_REFRESH_SECONDS = 0.25;

async function start(): Promise<void> {
  const canvas = document.getElementById('world') as HTMLCanvasElement;
  const overlay = document.getElementById('overlay') as HTMLElement;
  render(h(App, null), document.getElementById('ui') as HTMLElement);

  const [settings, save, { renderer, backend }] = await Promise.all([loadSettings(), loadSave(), createRenderer(canvas, backendFromUrl())]);
  ui.settings.value = settings;
  ui.backend.value = backend;

  const assets = await Assets.load((done, total) => {
    ui.loading.value = `Loading the valley… ${Math.round((done / total) * 100)}%`;
  });

  const sim = new Sim(save?.state ?? createState(Math.floor(Math.random() * 2 ** 31)));
  const view = new WorldView(assets, canvas, overlay);
  view.sync(sim.state, []);
  refreshUi(sim.state);

  const autosaver = new Autosaver(sim);
  autosaver.onSaved = () => (ui.savedAt.value = autosaver.lastSavedAt);
  ui.canSave.value = await autosaver.start();
  if (!ui.canSave.value) ui.notice.value = 'This world is already open in another tab. Changes here will not be saved.';

  let uiTimer = 0;
  const loop = new GameLoop(sim, {
    onTick: (events) => {
      view.sync(sim.state, events);
      autosaver.handle(events);
    },
    onFrame: (realDt, simDt, alpha) => {
      uiTimer += realDt;
      if (uiTimer >= UI_REFRESH_SECONDS) {
        uiTimer = 0;
        refreshUi(sim.state);
      }
      if (!fitRenderer(renderer, view.rig)) return;
      view.frame(realDt, simDt, alpha, sim.state);
      renderer.render(view.scene, view.rig.camera);
    },
  });
  loop.speed = 0;

  const select = (id: string | null): void => {
    ui.selectedId.value = id;
    view.selectedId = id;
    view.follow(id);
    refreshUi(sim.state);
  };
  view.rig.onClick = (event) => select(view.pick(event.clientX, event.clientY));

  actions.start = () => {
    ui.started.value = true;
    loop.speed = ui.speed.value;
  };
  actions.setSpeed = (speed) => {
    ui.speed.value = speed;
    if (ui.started.value) loop.speed = speed;
  };
  actions.select = select;
  actions.applySettings = (next) => {
    ui.settings.value = next;
    void saveSettings(next);
  };
  actions.exportSave = () => downloadSave(sim.state);
  actions.importSave = (file) => {
    void readSaveFile(file).then(async (document_) => {
      if (!document_) {
        ui.notice.value = 'That file is not a save this version can read.';
        return;
      }
      await writeSave(document_.state);
      location.reload();
    });
  };
  actions.resetWorld = () => {
    void clearSaves().then(() => location.reload());
  };

  renderer.setAnimationLoop(() => loop.frame());
  ui.loading.value = null;

  // For poking at the world from the browser console.
  const advance = (ticks: number): number => {
    for (let i = 0; i < ticks; i++) {
      const events = sim.step();
      view.sync(sim.state, events);
      autosaver.handle(events);
    }
    refreshUi(sim.state);
    return sim.state.tick;
  };
  (window as unknown as { __debug: unknown }).__debug = { sim, view, loop, renderer, ui, actions, advance };
  console.info(`Fantasy Sim World: ${backend} backend${save ? ', resumed from save' : ', new world'}`);
}

start().catch((error) => {
  console.error(error);
  ui.loading.value = `Could not start: ${String(error)}`;
});
