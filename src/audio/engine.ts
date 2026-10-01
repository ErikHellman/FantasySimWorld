import type { AudioSettings } from '../persist/settings';
import { AUDIO_EXTENSIONS, AUDIO_SLOTS, type AudioSlot } from './slots';

interface Loop {
  slot: AudioSlot;
  gain: GainNode;
  /** where the gain is heading; smoothed so loops fade rather than jump */
  target: number;
}

const BASE = import.meta.env.BASE_URL;

/**
 * Web Audio playback for the named slots: looping ambience with a gain per
 * loop, and one-shot effects. Created on the first user gesture (browsers do
 * not allow sound before that).
 */
export class AudioEngine {
  private context: AudioContext | null = null;
  private master: GainNode | null = null;
  private ambient: GainNode | null = null;
  private effects: GainNode | null = null;
  private readonly buffers = new Map<string, AudioBuffer>();
  private readonly loops = new Map<string, Loop>();
  private readonly lastPlayed = new Map<string, number>();
  private settings: AudioSettings = { muted: false, master: 0.8, ambient: 0.7, effects: 0.8 };
  /** called when the set of loaded slots changes */
  onLoaded: ((loaded: ReadonlySet<string>) => void) | null = null;

  get loaded(): ReadonlySet<string> {
    return new Set(this.buffers.keys());
  }

  /** Call from a click or key press. Safe to call more than once. */
  async start(): Promise<void> {
    if (this.context) {
      void this.context.resume();
      return;
    }
    const Context = window.AudioContext ?? (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    if (!Context) return;
    const context = new Context();
    this.context = context;
    this.master = context.createGain();
    this.ambient = context.createGain();
    this.effects = context.createGain();
    this.ambient.connect(this.master);
    this.effects.connect(this.master);
    this.master.connect(context.destination);
    this.apply(this.settings);
    await Promise.all(AUDIO_SLOTS.map((slot) => this.load(slot)));
    this.onLoaded?.(this.loaded);
  }

  private async load(slot: AudioSlot): Promise<void> {
    const context = this.context;
    if (!context) return;
    for (const extension of AUDIO_EXTENSIONS) {
      try {
        const response = await fetch(`${BASE}audio/${slot.id}.${extension}`);
        // A dev server answers unknown paths with the HTML page; only accept real audio.
        const type = response.headers.get('content-type') ?? '';
        if (!response.ok || type.includes('text/html')) continue;
        const buffer = await context.decodeAudioData(await response.arrayBuffer());
        this.buffers.set(slot.id, buffer);
        if (slot.kind === 'ambient') this.startLoop(slot, buffer);
        return;
      } catch {
        // Not there, or not decodable: try the next extension.
      }
    }
  }

  private startLoop(slot: AudioSlot, buffer: AudioBuffer): void {
    const context = this.context;
    if (!context || !this.ambient) return;
    const source = context.createBufferSource();
    source.buffer = buffer;
    source.loop = true;
    const gain = context.createGain();
    gain.gain.value = 0;
    source.connect(gain).connect(this.ambient);
    source.start(0, Math.random() * buffer.duration);
    this.loops.set(slot.id, { slot, gain, target: 0 });
  }

  apply(settings: AudioSettings): void {
    this.settings = settings;
    if (!this.context || !this.master || !this.ambient || !this.effects) return;
    const now = this.context.currentTime;
    this.master.gain.setTargetAtTime(settings.muted ? 0 : settings.master, now, 0.1);
    this.ambient.gain.setTargetAtTime(settings.ambient, now, 0.1);
    this.effects.gain.setTargetAtTime(settings.effects, now, 0.1);
  }

  /** Sets how loud each ambient loop should be (0..1); missing ids fade out. */
  mix(levels: Readonly<Record<string, number>>): void {
    const context = this.context;
    if (!context) return;
    for (const [id, loop] of this.loops) {
      const target = Math.max(0, Math.min(1, levels[id] ?? 0)) * loop.slot.gain;
      if (Math.abs(target - loop.target) < 0.01) continue;
      loop.target = target;
      loop.gain.gain.setTargetAtTime(target, context.currentTime, 0.8);
    }
  }

  /** Plays an effect once. `volume` is 0..1; repeats of the same effect are thinned out. */
  play(id: string, volume: number): void {
    const context = this.context;
    const buffer = this.buffers.get(id);
    if (!context || !buffer || !this.effects || volume <= 0.02 || context.state !== 'running') return;
    const now = context.currentTime;
    if (now - (this.lastPlayed.get(id) ?? -1) < 0.08) return;
    this.lastPlayed.set(id, now);
    const slot = AUDIO_SLOTS.find((entry) => entry.id === id);
    const source = context.createBufferSource();
    source.buffer = buffer;
    source.playbackRate.value = 0.92 + Math.random() * 0.16;
    const gain = context.createGain();
    gain.gain.value = Math.min(1, volume) * (slot?.gain ?? 1);
    source.connect(gain).connect(this.effects);
    source.start();
  }

  /** Suspends sound while the simulation is paused. */
  setPaused(paused: boolean): void {
    if (!this.context) return;
    if (paused && this.context.state === 'running') void this.context.suspend();
    else if (!paused && this.context.state === 'suspended') void this.context.resume();
  }
}
