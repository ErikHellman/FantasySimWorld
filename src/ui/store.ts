import { signal } from '@preact/signals';
import { DEFAULT_AUDIO, type AudioSettings } from '../audio/engine';
import { formatClock, gameTime, partOfDay } from '../core/time';
import type {
  Billboard, BrainLogRecord, CharacterDetail, ChatMessage, ChatWelcome, ChronicleEntry, CreditOffer, CreditsState, Grave, LlmStatus, Me,
  RosterEntry,
  Snapshot, Status, Utterance,
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
/** as many as the server keeps */
const CHAT_KEPT = 50;

/** How much of the chat strip is showing. */
export type ChatView = 'expanded' | 'collapsed' | 'hidden';
const CHAT_VIEWS: readonly ChatView[] = ['expanded', 'collapsed', 'hidden'];

/** The pane open on a phone, where only one shows at a time; the desktop layout ignores it. */
export type Sheet = 'none' | 'villagers' | 'feed' | 'chat' | 'character' | 'menu';

/** A window over the whole HUD: settings, help and the like. */
export type Panel = 'none' | 'settings' | 'admin' | 'brain' | 'graves' | 'help' | 'shop';

/** What this browser remembers about the chat. */
export interface ChatSettings {
  view: ChatView;
  /** the name this browser asked for; claimed again after each connect, if it is free */
  name?: string;
}

export const ui = {
  loading: signal<string | null>('Loading the valley…'),
  started: signal(false),
  backend: signal(''),
  /** the picture's quality tier, `high` or `low` */
  quality: signal(''),
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
  /** what each billboard shows, in map order (the admin panel edits them) */
  billboards: signal<Billboard[]>([]),
  panel: signal<Panel>('none'),
  audio: signal<AudioSettings>(DEFAULT_AUDIO),
  llm: signal<LlmStatus>({ state: 'off', inFlight: 0, queued: 0, requests: 0, failures: 0, tokens: 0, latencyMs: 0, protocol: '', message: '' }),
  brainLog: signal<BrainLogRecord[]>([]),
  audioSlots: signal<AudioSlotStatus[]>([]),
  notice: signal<string | null>(null),
  /** recent spoken lines, newest first */
  voices: signal<VoiceLine[]>([]),
  /** the world is being held at 1x while the language model catches up */
  throttled: signal(false),
  /** the viewers' chat, oldest first */
  chat: signal<ChatMessage[]>([]),
  /** this viewer's chat name; null until the stream says (or when they can only read) */
  chatName: signal<string | null>(null),
  /** the longest message the server accepts, in UTF-16 code units */
  chatMax: signal(500),
  chatView: signal<ChatView>('collapsed'),
  /** messages that arrived while the chat was not open */
  chatUnread: signal(0),
  /** this viewer's credits and what they buy; null until the server says, and `perMinute` 0 when credits are off */
  credits: signal<CreditsState | null>(null),
  /** the screen is phone-sized and gets the phone layout (see layout.ts) */
  mobile: signal(false),
  sheet: signal<Sheet>('none'),
  /** pixels the phone's tab bar or open pane covers along the right and bottom edges; the camera centres on the rest */
  covered: signal({ right: 0, bottom: 0 }),
};

export const isAdmin = (): boolean => ui.me.value?.role === 'admin';

/** Things the UI can ask the app to do; filled in by main.ts. */
export interface UiActions {
  start(): void;
  setSpeed(speed: number): void;
  select(id: string | null): void;
  applyAudio(settings: AudioSettings): void;
  /** rejects with an `HttpError` the chat shows */
  sendChat(text: string): Promise<void>;
  /** resolves to the name as the server stored it */
  renameChat(name: string): Promise<string>;
  /** spends credits on `offer`, aimed at the selected villager or where the camera looks; rejects with an `HttpError` the shop shows */
  buy(offer: CreditOffer): Promise<void>;
}

export const actions: UiActions = {
  start: () => {},
  setSpeed: () => {},
  select: () => {},
  applyAudio: () => {},
  sendChat: () => Promise.resolve(),
  renameChat: (name) => Promise.resolve(name),
  buy: () => Promise.resolve(),
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
  ui.billboards.value = snapshot.billboards;
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

export function setBillboard(board: Billboard): void {
  ui.billboards.value = ui.billboards.value.map((existing) => (existing.id === board.id ? board : existing));
}

export function addVoice(utterance: Utterance): void {
  ui.voices.value = [voiceLine(utterance), ...ui.voices.value].slice(0, VOICES_KEPT);
}

export function addBrainLog(records: readonly BrainLogRecord[]): void {
  const byId = new Map(ui.brainLog.value.map((record) => [record.id, record]));
  for (const record of records) byId.set(record.id, record);
  ui.brainLog.value = [...byId.values()].sort((a, b) => b.id - a.id).slice(0, BRAIN_LOG_KEPT);
}

export function fromChatWelcome(welcome: ChatWelcome): void {
  ui.chat.value = welcome.history.slice(-CHAT_KEPT);
  ui.chatName.value = welcome.you;
  ui.chatMax.value = welcome.maxLength;
}

export function addChat(message: ChatMessage): void {
  const last = ui.chat.value.at(-1);
  if (last && last.id >= message.id) return; // already in the welcome's history
  ui.chat.value = [...ui.chat.value, message].slice(-CHAT_KEPT);
  // Comings and goings are there to read, not to call anyone back to the chat.
  if (message.kind === 'say' && !chatIsOpen()) ui.chatUnread.value++;
}

/** Whether credits are on and known, so the pill and the shop have something to show. */
export const creditsOn = (): boolean => (ui.credits.value?.perMinute ?? 0) > 0;

export function setBalance(balance: number): void {
  const credits = ui.credits.value;
  if (credits && credits.balance !== balance) ui.credits.value = { ...credits, balance };
}

/** Why `offer` cannot be bought right now, or null when it can. */
export function offerBlocked(offer: CreditOffer, balance: number, selectedId: string | null): string | null {
  if (offer.target === 'character' && !selectedId) return 'Select a villager first.';
  if (balance < offer.cost) return `You need ${offer.cost - balance} more.`;
  return null;
}

/** Whether the chat's history is on screen: the chat sheet on a phone, the expanded strip on a wider screen. */
const chatIsOpen = (): boolean => (ui.mobile.value ? ui.sheet.value === 'chat' : ui.chatView.value === 'expanded');

/** Opens a window, or closes it if it is already open; on a phone it takes the place of the open sheet. */
export function togglePanel(panel: Panel): void {
  ui.panel.value = ui.panel.value === panel ? 'none' : panel;
  if (ui.panel.value !== 'none') ui.sheet.value = 'none';
}

/** Opens a phone pane, or closes it if it is already open; any other pane or window closes. */
export function toggleSheet(sheet: Sheet): void {
  ui.sheet.value = ui.sheet.value === sheet ? 'none' : sheet;
  ui.panel.value = 'none';
  if (ui.sheet.value === 'chat') ui.chatUnread.value = 0;
}

export function closeSheet(): void {
  ui.sheet.value = 'none';
}

export function setChatView(view: ChatView): void {
  ui.chatView.value = view;
  if (view === 'expanded') ui.chatUnread.value = 0;
  saveChatSettings({ ...loadChatSettings(), view });
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

/** The chat's view and chosen name live in this browser only. */
const CHAT_KEY = 'fsw.chat';

export function loadChatSettings(): ChatSettings {
  try {
    const stored = JSON.parse(localStorage.getItem(CHAT_KEY) ?? '{}') as Partial<ChatSettings>;
    const view = CHAT_VIEWS.includes(stored.view as ChatView) ? (stored.view as ChatView) : 'collapsed';
    return typeof stored.name === 'string' && stored.name ? { view, name: stored.name } : { view };
  } catch {
    return { view: 'collapsed' };
  }
}

export function saveChatSettings(settings: ChatSettings): void {
  try {
    localStorage.setItem(CHAT_KEY, JSON.stringify(settings));
  } catch {
    // storage blocked: the settings last until the page closes
  }
}
