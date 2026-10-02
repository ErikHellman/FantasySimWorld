import { signal } from '@preact/signals';
import { DEFAULT_AUDIO, type AudioSettings } from '../audio/engine';
import { formatClock, gameTime, partOfDay } from '../core/time';
import type {
  BrainLogRecord, CharacterDetail, ChronicleEntry, Grave, LlmStatus, Me, RosterEntry, Snapshot, Status, Utterance,
} from '../generated/api';
import type { ConnectionState } from '../net/stream';

export type { CharacterDetail, RelationRow, RosterEntry } from '../generated/api';

export interface AudioSlotStatus {
  id: string;
  label: string;
  loaded: boolean;
}

/** A spoken line in the Voices feed. */
export interface VoiceLine {
  id: number;
  tick: number;
  speaker: string;
  speakerId: string;
  to: string | null;
  text: string;
  loud: boolean;
}

const VOICES_KEPT = 80;
const CHRONICLE_KEPT = 400;
const BRAIN_LOG_KEPT = 150;

export const ui = {
  loading: signal<string | null>('Loading the valley…'),
  started: signal(false),
  backend: signal(''),
  /** who is watching; null until /api/me answers */
  me: signal<Me | null>(null),
  connection: signal<ConnectionState>('connecting'),
  clock: signal({ day: 1, time: '06:00', part: 'early morning' }),
  /** the world's speed as the server runs it (0 = paused) */
  speed: signal(1),
  viewers: signal(0),
  population: signal(0),
  monsters: signal(0),
  alarm: signal(false),
  roster: signal<RosterEntry[]>([]),
  selectedId: signal<string | null>(null),
  detail: signal<CharacterDetail | null>(null),
  chronicle: signal<ChronicleEntry[]>([]),
  /** older chronicle entries exist on the server */
  chronicleMore: signal(true),
  graves: signal<Grave[]>([]),
  panel: signal<'none' | 'settings' | 'admin' | 'brain' | 'graves'>('none'),
  audio: signal<AudioSettings>(DEFAULT_AUDIO),
  llm: signal<LlmStatus>({ state: 'off', inFlight: 0, queued: 0, requests: 0, failures: 0, tokens: 0, latencyMs: 0, protocol: '', message: '' }),
  brainLog: signal<BrainLogRecord[]>([]),
  audioSlots: signal<AudioSlotStatus[]>([]),
  notice: signal<string | null>(null),
  /** recent spoken lines, newest first */
  voices: signal<VoiceLine[]>([]),
  /** the world is being held at 1x while the language model catches up */
  throttled: signal(false),
};

export const isAdmin = (): boolean => ui.me.value?.role === 'admin';

/** Things the UI can ask the app to do; filled in by main.ts. */
export interface UiActions {
  start(): void;
  setSpeed(speed: number): void;
  select(id: string | null): void;
  applyAudio(settings: AudioSettings): void;
}

export const actions: UiActions = {
  start: () => {},
  setSpeed: () => {},
  select: () => {},
  applyAudio: () => {},
};

let voiceId = 0;
const voiceLine = (u: Utterance): VoiceLine => ({
  id: ++voiceId, tick: u.tick, speaker: u.speakerName, speakerId: u.speakerId, to: u.toName, text: u.text, loud: u.loud,
});

/** Everything the HUD shows, from a fresh snapshot. */
export function fromSnapshot(snapshot: Snapshot): void {
  ui.speed.value = snapshot.speed;
  ui.roster.value = snapshot.roster;
  ui.population.value = snapshot.roster.length;
  ui.chronicle.value = snapshot.chronicle;
  ui.chronicleMore.value = true;
  ui.graves.value = snapshot.graves;
  ui.alarm.value = snapshot.alarm;
  ui.llm.value = snapshot.llm;
  ui.voices.value = snapshot.voices.map(voiceLine);
  setClock(snapshot.tick);
}

export function fromStatus(status: Status): void {
  ui.llm.value = status.llm;
  ui.viewers.value = status.viewers;
  ui.speed.value = status.speed;
  ui.throttled.value = status.throttled;
  ui.population.value = status.population;
  ui.monsters.value = status.monsters;
  ui.alarm.value = status.alarm;
}

export function addChronicle(entry: ChronicleEntry): void {
  ui.chronicle.value = [...ui.chronicle.value, entry].slice(-CHRONICLE_KEPT);
}

/** A new grave takes its plot from whoever lay there before (a full graveyard reuses its oldest plot). */
export function addGrave(grave: Grave): void {
  ui.graves.value = [...ui.graves.value.filter((existing) => existing.plot !== grave.plot), grave];
}

export function addVoice(utterance: Utterance): void {
  ui.voices.value = [voiceLine(utterance), ...ui.voices.value].slice(0, VOICES_KEPT);
}

export function addBrainLog(records: readonly BrainLogRecord[]): void {
  const byId = new Map(ui.brainLog.value.map((record) => [record.id, record]));
  for (const record of records) byId.set(record.id, record);
  ui.brainLog.value = [...byId.values()].sort((a, b) => b.id - a.id).slice(0, BRAIN_LOG_KEPT);
}

export function setClock(tick: number): void {
  const time = gameTime(tick);
  const next = { day: time.day, time: formatClock(time.minuteOfDay), part: partOfDay(time.hour) };
  const clock = ui.clock.value;
  if (clock.day !== next.day || clock.time !== next.time || clock.part !== next.part) ui.clock.value = next;
}

/** Audio settings live in this browser only. */
const AUDIO_KEY = 'fsw.audio';

export function loadAudioSettings(): AudioSettings {
  try {
    const stored = localStorage.getItem(AUDIO_KEY);
    return stored ? { ...DEFAULT_AUDIO, ...(JSON.parse(stored) as Partial<AudioSettings>) } : DEFAULT_AUDIO;
  } catch {
    return DEFAULT_AUDIO;
  }
}

export function saveAudioSettings(settings: AudioSettings): void {
  try {
    localStorage.setItem(AUDIO_KEY, JSON.stringify(settings));
  } catch {
    // storage blocked: the settings last until the page closes
  }
}
