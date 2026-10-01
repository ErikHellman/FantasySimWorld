import { h, render } from 'preact';
import { GameLoop } from './app/loop';
import { ambientLevels, playEffects } from './audio/ambient';
import { AudioEngine } from './audio/engine';
import { AUDIO_SLOTS } from './audio/slots';
import { Brain } from './brain/brain';
import { LlmClient } from './brain/llm/client';
import { probe } from './brain/llm/probe';
import { Autosaver } from './persist/autosave';
import { clearSaves, loadSave, loadSettings, saveSettings, writeSave } from './persist/db';
import { downloadSave, readSaveFile } from './persist/exportImport';
import { Assets } from './render/assets';
import { backendFromUrl, createRenderer, fitRenderer } from './render/renderer';
import { WorldView } from './render/scene';
import { gameTime } from './core/time';
import { createState } from './sim/create';
import { Sim } from './sim/sim';
import { SpeechBus } from './speech/speechBus';
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

  // Speech: every spoken line goes through the bus; bubbles and the transcript are its sinks today.
  const speech = new SpeechBus();
  speech.add({ speak: (u) => view.overlay.say(u.speakerId, u.text, Math.max(2.6, u.durationMs / 1000), u.loud) });
  speech.add({
    speak: (u) => {
      ui.voices.value = [{ id: u.id, tick: u.tick, speaker: u.speakerName, speakerId: u.speakerId, to: u.toName, text: u.text, loud: u.loud }, ...ui.voices.value].slice(0, 80);
    },
  });

  const audio = new AudioEngine();
  audio.apply(settings.audio);
  const publishSlots = (loaded: ReadonlySet<string>): void => {
    ui.audioSlots.value = AUDIO_SLOTS.map((slot) => ({ id: slot.id, label: slot.label, loaded: loaded.has(slot.id) }));
  };
  publishSlots(new Set());
  audio.onLoaded = publishSlots;
  const listener = (): { x: number; z: number; distance: number } => ({ x: view.rig.target.x, z: view.rig.target.z, distance: view.rig.distance });

  let uiTimer = 0;
  let audioTimer = 0;
  let wantedSpeed = 1;
  let logId = 0;
  const brain = new Brain(sim, {
    speed: () => loop.speed,
    status: (status) => (ui.llm.value = status),
    log: (record) => (ui.brainLog.value = [{ id: ++logId, ...record }, ...ui.brainLog.value].slice(0, 150)),
  });
  const loop: GameLoop = new GameLoop(sim, {
    onTick: (events) => {
      view.sync(sim.state, events);
      autosaver.handle(events);
      brain.handle(events);
      speech.handle(events, sim.state);
      playEffects(audio, events, sim.state, listener());
    },
    onFrame: (realDt, simDt, alpha) => {
      brain.update();
      // Hold the world at 1x while the language model has a backlog, so nobody waits long for an answer.
      const throttled = wantedSpeed > 1 && brain.backlogged;
      if (ui.throttled.value !== throttled) ui.throttled.value = throttled;
      if (ui.started.value) loop.speed = throttled ? 1 : wantedSpeed;
      audioTimer += realDt;
      if (audioTimer >= 0.5) {
        audioTimer = 0;
        audio.mix(ambientLevels(sim.state, gameTime(sim.state.tick).hour, listener()));
      }
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
  brain.configure(settings.llm);

  const select = (id: string | null): void => {
    ui.selectedId.value = id;
    view.selectedId = id;
    view.follow(id);
    refreshUi(sim.state);
  };
  view.rig.onClick = (event) => select(view.pick(event.clientX, event.clientY));

  actions.start = () => {
    ui.started.value = true;
    wantedSpeed = ui.speed.value;
    loop.speed = wantedSpeed;
    void audio.start();
  };
  actions.setSpeed = (speed) => {
    ui.speed.value = speed;
    wantedSpeed = speed;
    if (ui.started.value) loop.speed = speed;
    audio.setPaused(speed === 0);
    speech.setRate(speed);
  };
  actions.select = select;
  actions.applySettings = (next) => {
    const llmChanged = JSON.stringify(next.llm) !== JSON.stringify(ui.settings.value.llm);
    ui.settings.value = next;
    void saveSettings(next);
    audio.apply(next.audio);
    if (llmChanged) brain.configure(next.llm);
  };
  actions.testConnection = async (candidate) => (await probe(candidate.llm)).message;
  actions.listModels = async (candidate) => {
    try {
      return await new LlmClient(candidate.llm).listModels();
    } catch (error) {
      console.warn('Could not list models', error);
      return [];
    }
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
      brain.handle(events);
      speech.handle(events, sim.state);
    }
    refreshUi(sim.state);
    return sim.state.tick;
  };
  (window as unknown as { __debug: unknown }).__debug = { sim, view, loop, renderer, ui, actions, advance, brain };
  console.info(`Fantasy Sim World: ${backend} backend${save ? ', resumed from save' : ', new world'}`);
}

start().catch((error) => {
  console.error(error);
  ui.loading.value = `Could not start: ${String(error)}`;
});
