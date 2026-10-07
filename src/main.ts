import { h, render } from 'preact';
import { ambientLevels, playEffects } from './audio/ambient';
import { AudioEngine } from './audio/engine';
import { MoodTracker } from './audio/mood';
import { MUSIC_TRACKS } from './audio/musicTracks';
import { AUDIO_SLOTS } from './audio/slots';
import { gameTime } from './core/time';
import type { AmbientCounts, GuestEndReason, WireEvent } from './generated/api';
import { HttpError, api } from './net/api';
import { Playback } from './net/playback';
import { Heartbeat, tabIsWatched } from './net/presence';
import { Stream, viewerId } from './net/stream';
import { ViewWorld } from './net/viewWorld';
import { Assets } from './render/assets';
import { pickQuality, qualitySettings, thisDevice } from './render/quality';
import { backendFromUrl, createRenderer, fitRenderer } from './render/renderer';
import { WorldView } from './render/scene';
import { FrameStats } from './render/stats';
import { SpeechBus } from './speech/speechBus';
import { App } from './ui/App';
import { watchLayout } from './ui/layout';
import { Login } from './ui/Login';
import { NoAccess } from './ui/NoAccess';
import {
  actions, addBrainLog, addChat, addChronicle, addGrave, addVoice, closeSheet, fromChatWelcome, fromSnapshot, fromStatus, loadAudioSettings,
  loadChatSettings, saveAudioSettings, saveChatSettings, setBalance, setBillboard, setClock, setHouse, setStockpile, ui,
} from './ui/store';
import { worldHash } from './world/navgrid';

const UI_REFRESH_SECONDS = 0.25;
const MAX_FRAME_SECONDS = 0.1;
/** Frames further apart than this come from a hidden or stalled tab and say nothing about the device's speed. */
const STALLED_FRAME_MS = 250;
/** On a small machine, frames that keep taking longer than this lower the resolution, a step at a time. */
const SLOW_FRAME_MS = 40;
const PIXEL_RATIO_STEP = 0.25;
/** monsters this close to where the camera looks make the music tense */
const MONSTERS_NEAR = 60;
/** A guest's page gives up this long after their pass ends, in case the stream was down when the server ended it. */
const GUEST_END_SLACK_MS = 5_000;

/** The author of the chat line for an admin's gift to everyone (as the server writes it). */
const EVERYONE = 'Everyone watching';

const here = (): string => location.pathname + location.search;
const toLogin = (): void => location.replace(`/login?return_to=${encodeURIComponent(here())}`);
/** Back to the launch screen, saying why the guest's visit ended. */
const guestEnded = (reason: GuestEndReason): void =>
  location.replace(`/login?ended=${reason}&return_to=${encodeURIComponent(here())}`);

/** The world is drawn from the server's stream: this page only renders, plays sound and shows the HUD. */
async function start(): Promise<void> {
  const root = document.getElementById('ui') as HTMLElement;
  watchLayout();
  const params = new URLSearchParams(location.search);
  if (location.pathname === '/login') {
    render(h(Login, { returnTo: params.get('return_to') ?? '/', error: params.get('error'), ended: params.get('ended') }), root);
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

  // A guest watches for a while, then goes back to the launch screen; the server ends their stream on time.
  const guest = ui.me.value.role === 'guest';
  if (guest) {
    const left = (ui.me.value.guestSecondsLeft ?? 0) * 1000;
    ui.guestEndsAt.value = Date.now() + left;
    setTimeout(() => guestEnded('time'), left + GUEST_END_SLACK_MS);
  }

  const canvas = document.getElementById('world') as HTMLCanvasElement;
  const overlay = document.getElementById('overlay') as HTMLElement;
  render(h(App, null), root);

  // `?render=off` keeps everything but the drawing: for headless protocol tests, where software WebGL is far too slow.
  const quality = qualitySettings(pickQuality(thisDevice()), window.devicePixelRatio);
  const handle = params.get('render') === 'off' ? null : await createRenderer(canvas, backendFromUrl(), quality);
  const backend = handle?.backend ?? 'none';
  ui.backend.value = backend;
  ui.quality.value = quality.quality;
  const assets = await Assets.load((done, total) => {
    ui.loading.value = `Loading the valley… ${Math.round((done / total) * 100)}%`;
  });
  ui.loading.value = 'Joining the world…';

  const world = new ViewWorld();
  const view = new WorldView(assets, canvas, overlay, quality);
  const viewer = viewerId();

  // Speech: every spoken line goes through the bus; bubbles and the transcript are its sinks today.
  const speech = new SpeechBus();
  speech.add({ speak: (u) => view.overlay.say(u.speakerId, u.text, Math.max(2.6, u.durationMs / 1000), u.loud) });
  speech.add({ speak: (u) => addVoice(u) });

  const audio = new AudioEngine();
  ui.audio.value = loadAudioSettings();
  ui.chatView.value = loadChatSettings().view;
  audio.apply(ui.audio.value);
  const publishSlots = (): void => {
    const loaded = audio.loaded;
    const music = audio.music.loaded;
    ui.audioSlots.value = [
      ...AUDIO_SLOTS.map((slot) => ({ id: slot.id, label: slot.label, loaded: loaded.has(slot.id) })),
      ...MUSIC_TRACKS.map((track) => ({ id: `music/${track.id}`, label: `${track.title} (${track.moods.join(', ')})`, loaded: music.has(track.id) })),
    ];
  };
  publishSlots();
  audio.onLoaded = publishSlots;
  audio.music.onFound = publishSlots;
  // Background music follows what the viewer is watching: the camera's surroundings and the selected villager.
  const mood = new MoodTracker();
  const seconds = (): number => performance.now() / 1000;
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
      else if (event.type === 'billboard') setBillboard(event.billboard);
      else if (event.type === 'house') setHouse(event.house);
      else if (event.type === 'stockpile') setStockpile(event.stockpile);
    }
    speech.handle(events);
    if (!fast) {
      playEffects(audio, events, (id) => world.isVillager(id), listener());
      mood.noteEvents(events, listener(), ui.selectedId.value, seconds());
    }
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

  // Credits: the server counts them; this page reports that it is being watched and shows the balance.
  let heartbeat: Heartbeat | null = null;
  const loadCredits = (): void => {
    // Guests have no credits; the server would refuse them anyway.
    if (guest) return;
    api.credits(viewer).then(
      (credits) => {
        ui.credits.value = credits;
        if (credits.perMinute > 0 && !heartbeat) {
          heartbeat = new Heartbeat({
            seconds: credits.beatSeconds,
            watched: tabIsWatched,
            report: () => api.presence({ viewer }).then((answer) => answer.balance),
            balance: setBalance,
          });
          heartbeat.start();
        }
      },
      (error) => console.warn('credits', error),
    );
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
    // The welcome means the server knows this tab, which the credits routes need.
    chatWelcome: (welcome) => {
      fromChatWelcome(welcome);
      loadCredits();
    },
    chat: (message) => {
      addChat(message);
      // The village's prayers, or an admin, may just have given this viewer credits.
      if (message.kind === 'event' && (message.author === ui.chatName.value || message.author === EVERYONE)) loadCredits();
    },
    chatYou: (you) => (ui.chatName.value = you.name),
    guestEnd: (end) => guestEnded(end.reason),
    refused: (status) => (status === 401 ? toLogin() : location.replace('/no-access')),
    connection: (state) => (ui.connection.value = state),
    // Each connection asks for the name this browser chose, so the chat says it joined under that name.
  }, () => loadChatSettings().name);
  stream.suspendWhenHidden = !guest;

  view.rig.onClick = (event) => {
    // On a phone, a tap on the world first puts away the open pane, so the view is back without changing who is followed.
    if (ui.mobile.value && ui.sheet.value !== 'none') return closeSheet();
    // A fingertip covers more of the screen than a mouse pointer, so a tap reaches a little further.
    select(view.pick(event.clientX, event.clientY, event.pointerType === 'touch' ? 64 : 46));
  };
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
  actions.sendChat = (text) => api.chat({ viewer, text });
  actions.renameChat = async (name) => {
    const you = await api.chatName({ viewer, name });
    ui.chatName.value = you.name;
    saveChatSettings({ ...loadChatSettings(), name: you.name });
    return you.name;
  };

  actions.buy = async (offer) => {
    const answer = await api.buy({
      viewer,
      offer: offer.id,
      characterId: offer.target === 'character' ? ui.selectedId.value : null,
      x: offer.target === 'point' ? view.rig.target.x : null,
      z: offer.target === 'point' ? view.rig.target.z : null,
    });
    setBalance(answer.balance);
  };

  const burning = (): boolean => ui.houses.value.some((house) => house.phase === 'burning');
  const updateMusic = (hour: number): void => {
    const spot = listener();
    let monstersNear = 0;
    for (const entity of world.entities.values()) {
      if (entity.kind === 'monster' && entity.hp > 0 && Math.hypot(entity.x - spot.x, entity.z - spot.z) < MONSTERS_NEAR) monstersNear++;
    }
    const selectedId = ui.selectedId.value;
    const selectedMood = selectedId ? (ui.roster.value.find((entry) => entry.id === selectedId)?.mood ?? null) : null;
    audio.music.setMood(mood.update({ hour, alarm: ui.alarm.value || burning(), monstersNear, selectedMood }, seconds()));
    const sting = mood.takeSting();
    if (sting) audio.music.playSting(sting);
    audio.music.update();
  };

  const stats = new FrameStats();
  let last = performance.now();
  let lastDrawn = last;
  let uiTimer = 0;
  let audioTimer = 0;
  const frame = (): void => {
    const now = performance.now();
    // A capped frame rate skips the browser's frames that come too soon (with a little slack for its uneven timing).
    if (handle && now - last < quality.frameSeconds * 1000 - 4) return;
    const dt = Math.min(MAX_FRAME_SECONDS, (now - last) / 1000);
    last = now;
    const { alpha, simDt } = playback.advance(dt);
    const hour = gameTime(world.tick).hour;
    audioTimer += dt;
    if (audioTimer >= 0.5) {
      audioTimer = 0;
      audio.mix(ambientLevels(ambient, hour, listener(), [...world.houses.values()]));
      updateMusic(hour);
    }
    uiTimer += dt;
    if (uiTimer >= UI_REFRESH_SECONDS) {
      uiTimer = 0;
      setClock(world.tick);
    }
    view.rig.setInset(ui.covered.value.right, ui.covered.value.bottom);
    if (!handle) {
      view.rig.resize(canvas.clientWidth || 1, canvas.clientHeight || 1);
      view.frame(dt, simDt, alpha, hour);
      return;
    }
    if (!fitRenderer(handle.renderer, view.rig)) return;
    view.frame(dt, simDt, alpha, hour);
    handle.renderer.render(view.scene, view.rig.camera);
    if (now - lastDrawn < STALLED_FRAME_MS) stats.record(now - lastDrawn, performance.now() - now, handle.renderer);
    lastDrawn = now;
    if (quality.adaptive && stats.settled) {
      // Only ever downwards: stepping back up would have the picture pulse between two sizes.
      const ratio = handle.renderer.getPixelRatio();
      if (stats.p95() > SLOW_FRAME_MS && ratio > 1) handle.renderer.setPixelRatio(Math.max(1, ratio - PIXEL_RATIO_STEP));
      stats.restart();
    }
  };
  if (handle) handle.renderer.setAnimationLoop(frame);
  else setInterval(frame, 1000 / 30);
  stream.open();

  // For poking at the world from the browser console.
  (window as unknown as { __debug: unknown }).__debug = {
    world, view, playback, stream, renderer: handle?.renderer, ui, actions, audio, mood, stats: () => stats.report(), heartbeat: () => heartbeat,
  };
  console.info(`Fantasy Sim World: ${backend} backend, ${quality.quality} quality, watching as ${ui.me.value.role}`);
}

start().catch((error) => {
  console.error(error);
  ui.loading.value = `Could not start: ${String(error)}`;
});
