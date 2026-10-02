import { h, render } from 'preact';
import { ambientLevels, playEffects } from './audio/ambient';
import { AudioEngine } from './audio/engine';
import { AUDIO_SLOTS } from './audio/slots';
import { gameTime } from './core/time';
import type { AmbientCounts, WireEvent } from './generated/api';
import { HttpError, api } from './net/api';
import { Playback } from './net/playback';
import { Stream, viewerId } from './net/stream';
import { ViewWorld } from './net/viewWorld';
import { Assets } from './render/assets';
import { backendFromUrl, createRenderer, fitRenderer } from './render/renderer';
import { WorldView } from './render/scene';
import { SpeechBus } from './speech/speechBus';
import { App } from './ui/App';
import { Login } from './ui/Login';
import { NoAccess } from './ui/NoAccess';
import {
  actions, addBrainLog, addChronicle, addGrave, addVoice, fromSnapshot, fromStatus, loadAudioSettings, saveAudioSettings, setClock, ui,
} from './ui/store';
import { worldHash } from './world/navgrid';

const UI_REFRESH_SECONDS = 0.25;
const MAX_FRAME_SECONDS = 0.1;

const here = (): string => location.pathname + location.search;
const toLogin = (): void => location.replace(`/login?return_to=${encodeURIComponent(here())}`);

/** The world is drawn from the server's stream: this page only renders, plays sound and shows the HUD. */
async function start(): Promise<void> {
  const root = document.getElementById('ui') as HTMLElement;
  const params = new URLSearchParams(location.search);
  if (location.pathname === '/login') {
    render(h(Login, { returnTo: params.get('return_to') ?? '/', error: params.get('error') }), root);
    return;
  }
  if (location.pathname === '/no-access') {
    render(h(NoAccess, { email: params.get('email') }), root);
    return;
  }

  try {
    ui.me.value = await api.me();
  } catch (error) {
    if (error instanceof HttpError && error.status === 401) return toLogin();
    if (error instanceof HttpError && error.status === 403) return location.replace('/no-access');
    ui.loading.value = 'The server is not answering. Is it running?';
    render(h(App, null), root);
    setTimeout(() => location.reload(), 5000);
    return;
  }

  const canvas = document.getElementById('world') as HTMLCanvasElement;
  const overlay = document.getElementById('overlay') as HTMLElement;
  render(h(App, null), root);

  // `?render=off` keeps everything but the drawing: for headless protocol tests, where software WebGL is far too slow.
  const handle = params.get('render') === 'off' ? null : await createRenderer(canvas, backendFromUrl());
  const backend = handle?.backend ?? 'none';
  ui.backend.value = backend;
  const assets = await Assets.load((done, total) => {
    ui.loading.value = `Loading the valley… ${Math.round((done / total) * 100)}%`;
  });
  ui.loading.value = 'Joining the world…';

  const world = new ViewWorld();
  const view = new WorldView(assets, canvas, overlay);
  const viewer = viewerId();

  // Speech: every spoken line goes through the bus; bubbles and the transcript are its sinks today.
  const speech = new SpeechBus();
  speech.add({ speak: (u) => view.overlay.say(u.speakerId, u.text, Math.max(2.6, u.durationMs / 1000), u.loud) });
  speech.add({ speak: (u) => addVoice(u) });

  const audio = new AudioEngine();
  ui.audio.value = loadAudioSettings();
  audio.apply(ui.audio.value);
  const publishSlots = (loaded: ReadonlySet<string>): void => {
    ui.audioSlots.value = AUDIO_SLOTS.map((slot) => ({ id: slot.id, label: slot.label, loaded: loaded.has(slot.id) }));
  };
  publishSlots(new Set());
  audio.onLoaded = publishSlots;
  const listener = (): { x: number; z: number; distance: number } => ({ x: view.rig.target.x, z: view.rig.target.z, distance: view.rig.distance });
  let ambient: AmbientCounts = { awake: 0, eating: 0, drinking: 0, smithing: 0, relaxing: 0 };

  const select = (id: string | null): void => {
    ui.selectedId.value = id;
    ui.detail.value = null;
    view.selectedId = id;
    view.follow(id);
    // The stream pushes this viewer's detail from now on; fetch it once so the inspector opens at once.
    void api.watch({ viewer, id }).catch((error) => console.warn('watch', error));
    if (id) {
      api.character(id).then(
        (detail) => {
          if (ui.selectedId.value === id) ui.detail.value = detail;
        },
        () => {
          if (ui.selectedId.value === id) select(null);
        },
      );
    }
  };

  /** Effects are left out when a backlog is applied at once; speech and the chronicle are not. */
  const dispatch = (events: readonly WireEvent[], fast: boolean): void => {
    view.sync(world, fast ? [] : events);
    for (const event of events) {
      if (event.type === 'chronicle') addChronicle(event.entry);
      else if (event.type === 'grave') addGrave(event.grave);
    }
    speech.handle(events);
    if (!fast) playEffects(audio, events, (id) => world.isVillager(id), listener());
  };

  const playback = new Playback({
    onTick: (frame, fast) => {
      world.apply(frame);
      dispatch(frame.ev ?? [], fast);
      if (ui.selectedId.value && !world.entities.has(ui.selectedId.value)) select(null);
    },
  });
  const setSpeed = (speed: number, throttled: boolean): void => {
    playback.speed = throttled ? Math.min(1, speed) : speed;
    audio.setPaused(speed === 0);
    speech.setRate(playback.speed);
  };

  const stream = new Stream(viewer, {
    hello: (hello) => {
      ui.viewers.value = hello.viewers;
      setSpeed(hello.speed, false);
      if (hello.worldHash !== worldHash()) {
        console.warn(`The server's world layout (${hello.worldHash}) differs from this page's (${worldHash()})`);
        ui.notice.value = 'This page and the server disagree about the shape of the valley. Try reloading.';
      }
    },
    snapshot: (snapshot) => {
      world.reset(snapshot);
      playback.reset(snapshot.tick);
      fromSnapshot(snapshot);
      setSpeed(snapshot.speed, false);
      view.sync(world, []);
      if (ui.selectedId.value && !world.entities.has(ui.selectedId.value)) select(null);
      if (ui.loading.value) ui.loading.value = null;
    },
    tick: (frame) => playback.push(frame),
    status: (status) => {
      fromStatus(status);
      ambient = status.ambient;
      setSpeed(status.speed, status.throttled);
    },
    roster: (roster) => (ui.roster.value = roster),
    detail: (detail) => {
      if (detail.id === ui.selectedId.value) ui.detail.value = detail;
    },
    brainlog: (record) => addBrainLog([record]),
    worldReset: () => location.reload(),
    refused: (status) => (status === 401 ? toLogin() : location.replace('/no-access')),
    connection: (state) => (ui.connection.value = state),
  });

  view.rig.onClick = (event) => select(view.pick(event.clientX, event.clientY));
  actions.select = select;
  actions.start = () => {
    ui.started.value = true;
    void audio.start();
  };
  actions.setSpeed = (speed) => {
    if (ui.me.value?.role !== 'admin') return;
    ui.speed.value = speed;
    api.admin.speed({ speed }).catch((error) => (ui.notice.value = `Could not change the speed: ${String(error)}`));
  };
  actions.applyAudio = (settings) => {
    ui.audio.value = settings;
    saveAudioSettings(settings);
    audio.apply(settings);
  };

  let last = performance.now();
  let uiTimer = 0;
  let audioTimer = 0;
  const frame = (): void => {
    const now = performance.now();
    const dt = Math.min(MAX_FRAME_SECONDS, (now - last) / 1000);
    last = now;
    const { alpha, simDt } = playback.advance(dt);
    const hour = gameTime(world.tick).hour;
    audioTimer += dt;
    if (audioTimer >= 0.5) {
      audioTimer = 0;
      audio.mix(ambientLevels(ambient, hour, listener()));
    }
    uiTimer += dt;
    if (uiTimer >= UI_REFRESH_SECONDS) {
      uiTimer = 0;
      setClock(world.tick);
    }
    if (!handle) {
      view.rig.resize(canvas.clientWidth || 1, canvas.clientHeight || 1);
      view.frame(dt, simDt, alpha, hour);
      return;
    }
    if (!fitRenderer(handle.renderer, view.rig)) return;
    view.frame(dt, simDt, alpha, hour);
    handle.renderer.render(view.scene, view.rig.camera);
  };
  if (handle) handle.renderer.setAnimationLoop(frame);
  else setInterval(frame, 1000 / 30);
  stream.open();

  // For poking at the world from the browser console.
  (window as unknown as { __debug: unknown }).__debug = { world, view, playback, stream, renderer: handle?.renderer, ui, actions };
  console.info(`Fantasy Sim World: ${backend} backend, watching as ${ui.me.value.role}`);
}

start().catch((error) => {
  console.error(error);
  ui.loading.value = `Could not start: ${String(error)}`;
});
