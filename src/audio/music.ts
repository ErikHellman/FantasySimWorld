import type { MusicMood, Sting } from './mood';
import { MUSIC_TRACKS, STING_TRACKS, type MusicTrack } from './musicTracks';

const BASE = import.meta.env.BASE_URL;
const trackUrl = (id: string): string => `${BASE}audio/music/${id}.mp3`;

/** Seconds. */
const FADE = 4;
const FADE_INTO_BATTLE = 1.5;
/** start the next track this long before the current one ends */
const OVERLAP = 4;
/** a change of time of day waits for the track to end, but not longer than this */
const TIME_OF_DAY_WAIT = 90;
const STING_DUCK = 0.3;

const QUIET_MOODS: ReadonlySet<MusicMood> = new Set(['day', 'evening', 'night']);

interface Deck {
  element: HTMLAudioElement;
  gain: GainNode;
  track: MusicTrack | null;
  /** the deck is fading out; its element is paused when the fade ends */
  stopAt: number;
}

/**
 * Streams the background music through two `<audio>` decks that crossfade:
 * between moods, and from one track to the next within a mood. Tracks are not
 * decoded into memory like the ambience. Missing files are skipped.
 */
export class MusicPlayer {
  private context: AudioContext | null = null;
  private decks: Deck[] = [];
  private active: Deck | null = null;
  private sting: Deck | null = null;
  private mood: MusicMood | null = null;
  /** a pending change of time of day, and when it has waited long enough */
  private switchBy = Infinity;
  private enabled = true;
  private paused = false;
  private readonly next = new Map<MusicMood, number>();
  private readonly missing = new Set<string>();
  private found: ReadonlySet<string> = new Set();
  /** called when it is known which tracks exist */
  onFound: ((found: ReadonlySet<string>) => void) | null = null;

  /** Hooks the player into the engine's graph; called once the AudioContext exists. */
  attach(context: AudioContext, output: AudioNode): void {
    this.context = context;
    const deck = (): Deck => {
      const element = new Audio();
      element.preload = 'auto';
      const gain = context.createGain();
      gain.gain.value = 0;
      context.createMediaElementSource(element).connect(gain).connect(output);
      const created: Deck = { element, gain, track: null, stopAt: Infinity };
      element.addEventListener('ended', () => this.onEnded(created));
      element.addEventListener('error', () => this.onError(created));
      return created;
    };
    this.decks = [deck(), deck()];
    this.sting = deck();
    this.sting.gain.gain.value = 1;
    void this.probe();
    this.play(FADE);
  }

  /** Which tracks are there: HEAD requests, since a missing file is answered with the HTML page. */
  private async probe(): Promise<void> {
    const ids = [...MUSIC_TRACKS.map((track) => track.id), ...Object.values(STING_TRACKS)];
    const found = new Set<string>();
    await Promise.all(
      ids.map(async (id) => {
        try {
          const response = await fetch(trackUrl(id), { method: 'HEAD' });
          if (response.ok && !(response.headers.get('content-type') ?? '').includes('text/html')) found.add(id);
          else this.missing.add(id);
        } catch {
          this.missing.add(id);
        }
      }),
    );
    this.found = found;
    this.onFound?.(found);
  }

  get loaded(): ReadonlySet<string> {
    return this.found;
  }

  /** For the debug handle and tests in the browser. */
  get state(): { mood: MusicMood | null; track: string | null; enabled: boolean } {
    return { mood: this.mood, track: this.active?.track?.id ?? null, enabled: this.enabled };
  }

  setMood(mood: MusicMood): void {
    if (mood === this.mood) return;
    const previous = this.mood;
    this.mood = mood;
    if (!this.context || !this.enabled) return;
    // A new hour of the day can wait for the current piece to finish; anything else changes now.
    if (previous && QUIET_MOODS.has(previous) && QUIET_MOODS.has(mood) && this.active?.track) {
      this.switchBy = this.context.currentTime + TIME_OF_DAY_WAIT;
      return;
    }
    this.play(mood === 'battle' ? FADE_INTO_BATTLE : FADE);
  }

  /** Call a few times a second: moves on to the next track before the current one ends. */
  update(): void {
    const context = this.context;
    if (!context) return;
    const now = context.currentTime;
    // Faded-out decks are stopped even when music has just been switched off.
    for (const deck of this.decks) {
      if (now >= deck.stopAt) this.stop(deck);
    }
    if (!this.enabled || this.paused) return;
    const active = this.active;
    if (now >= this.switchBy) this.play(FADE);
    else if (active?.track && !active.element.paused && active.element.duration - active.element.currentTime < OVERLAP) this.play(OVERLAP);
    else if (!active && this.mood) this.play(FADE);
  }

  /** Plays a sting over the music, which ducks under it. */
  playSting(sting: Sting): void {
    const deck = this.sting;
    const id = STING_TRACKS[sting];
    if (!deck || !this.context || !this.enabled || this.paused || this.missing.has(id)) return;
    deck.element.src = trackUrl(id);
    void deck.element.play().catch(() => undefined);
    const active = this.active;
    if (active?.track) this.ramp(active, active.track.gain * STING_DUCK, 0.6);
  }

  setEnabled(enabled: boolean): void {
    if (enabled === this.enabled) return;
    this.enabled = enabled;
    if (!this.context) return;
    if (enabled) {
      this.play(FADE);
      return;
    }
    for (const deck of this.decks) {
      if (deck.track) this.fadeOut(deck, 1);
    }
    this.active = null;
  }

  setPaused(paused: boolean): void {
    if (paused === this.paused) return;
    this.paused = paused;
    for (const deck of [...this.decks, ...(this.sting ? [this.sting] : [])]) {
      if (paused) deck.element.pause();
      else if (deck.track || (deck === this.sting && deck.element.src && !deck.element.ended)) void deck.element.play().catch(() => undefined);
    }
  }

  /** Crossfades to the next track for the current mood (or to silence if there is none). */
  private play(fade: number): void {
    const context = this.context;
    if (!context || !this.enabled || !this.mood) return;
    this.switchBy = Infinity;
    const track = this.pick(this.mood);
    const outgoing = this.active;
    if (outgoing?.track) this.fadeOut(outgoing, fade);
    if (!track) {
      this.active = null;
      return;
    }
    const deck = this.decks.find((candidate) => candidate !== outgoing);
    if (!deck) return;
    deck.track = track;
    deck.stopAt = Infinity;
    deck.element.src = trackUrl(track.id);
    deck.gain.gain.cancelScheduledValues(context.currentTime);
    deck.gain.gain.setValueAtTime(0, context.currentTime);
    this.ramp(deck, track.gain, fade);
    this.active = deck;
    if (!this.paused) void deck.element.play().catch(() => undefined);
  }

  /** The next track in the mood's playlist, skipping missing files and not repeating the current one if there is a choice. */
  private pick(mood: MusicMood): MusicTrack | null {
    const list = MUSIC_TRACKS.filter((track) => track.moods.includes(mood) && !this.missing.has(track.id));
    if (list.length === 0) return null;
    const current = this.active?.track?.id;
    let index = (this.next.get(mood) ?? Math.floor(Math.random() * list.length)) % list.length;
    if (list.length > 1 && list[index]?.id === current) index = (index + 1) % list.length;
    this.next.set(mood, index + 1);
    return list[index] ?? null;
  }

  private ramp(deck: Deck, to: number, seconds: number): void {
    const context = this.context;
    if (!context) return;
    const now = context.currentTime;
    deck.gain.gain.cancelScheduledValues(now);
    deck.gain.gain.setValueAtTime(deck.gain.gain.value, now);
    deck.gain.gain.linearRampToValueAtTime(to, now + seconds);
  }

  private fadeOut(deck: Deck, seconds: number): void {
    this.ramp(deck, 0, seconds);
    deck.stopAt = (this.context?.currentTime ?? 0) + seconds + 0.1;
  }

  private stop(deck: Deck): void {
    deck.stopAt = Infinity;
    deck.track = null;
    deck.element.pause();
    // Drop the source so nothing more is downloaded.
    deck.element.removeAttribute('src');
    deck.element.load();
  }

  private onEnded(deck: Deck): void {
    if (deck === this.sting) {
      const active = this.active;
      if (active?.track) this.ramp(active, active.track.gain, 2);
      return;
    }
    if (deck === this.active) this.play(1);
  }

  private onError(deck: Deck): void {
    if (!deck.track) return;
    this.missing.add(deck.track.id);
    const wasActive = deck === this.active;
    this.stop(deck);
    if (wasActive) {
      this.active = null;
      this.play(1);
    }
  }
}
