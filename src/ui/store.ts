import { signal } from '@preact/signals';
import { DEFAULT_AUDIO, type AudioSettings } from '../audio/engine';
import { formatClock, gameTime, partOfDay } from '../core/time';
import type {
  Billboard, BrainLogRecord, Ceremony, ChatMessage, ChatWelcome, ChronicleEntry, Detail, Economy, Founding, Grave, GuestEndReason, House, LlmStatus, Me,
  RosterEntry,
  SiteDetail, Snapshot, Status, Stockpile, Utterance, Weather, WorldObject,
} from '../generated/api';
import type { GuestWait } from '../net/guestQueue';
import type { ConnectionState } from '../net/stream';

export type { CharacterDetail, Detail, GhostDetail, MonsterDetail, RelationRow, RosterEntry, SiteDetail } from '../generated/api';

/** Whether a selectable id names a site (a building `b:<place>` or something built `n:<n>`) rather than a villager. */
export const isSiteId = (id: string): boolean => id.startsWith('b:') || id.startsWith('n:');

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
export type Panel = 'none' | 'settings' | 'admin' | 'brain' | 'graves' | 'help' | 'tech';

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
  /** waiting for a seat: this page's place in line (null before the server answers) and any trouble asking; null once seated */
  line: signal<{ wait: GuestWait | null; problem: string | null } | null>(null),
  /** why this guest's stream is over, once it is; the launch screen says so and offers to come back */
  ended: signal<GuestEndReason | null>(null),
  connection: signal<ConnectionState>('connecting'),
  clock: signal({ day: 1, time: '06:00', part: 'early morning' }),
  /** the world's speed as the server runs it (0 = paused) */
  speed: signal(1),
  viewers: signal(0),
  population: signal(0),
  monsters: signal(0),
  alarm: signal(false),
  /** a new world still being founded: how many villagers have arrived; null once it is (or for a world that always was) */
  founding: signal<Founding | null>(null),
  roster: signal<RosterEntry[]>([]),
  /** the villager or monster being followed, or the site being looked at (see `isSiteId`) */
  selectedId: signal<string | null>(null),
  /** the inspector for whoever is selected: a villager, or one of the dead */
  detail: signal<Detail | null>(null),
  /** the page of the building or built thing being looked at, once the server has sent it */
  site: signal<SiteDetail | null>(null),
  chronicle: signal<ChronicleEntry[]>([]),
  /** older chronicle entries exist on the server */
  chronicleMore: signal(true),
  graves: signal<Grave[]>([]),
  /** what each billboard shows, in map order (the admin panel edits them) */
  billboards: signal<Billboard[]>([]),
  /** the homes that are burning or in ruins */
  houses: signal<House[]>([]),
  /** the wedding or funeral the village has arranged, if any */
  ceremony: signal<Ceremony | null>(null),
  /** the materials the village has put by, for the places that hold any */
  stockpiles: signal<Stockpile[]>([]),
  /** the village chest and what the market's stalls hold */
  economy: signal<Economy>({ purse: 0, food: 0, potion: 0, gift: 0, trinket: 0, relic: 0 }),
  /** what the sky is doing */
  weather: signal<Weather>({ kind: 'clear', intensity: 0, wind: 0, windDir: 0 }),
  /** the world's things that are alight right now (buildings as `b:<place>`, props as `p:<place>:<index>`) */
  fires: signal<WorldObject[]>([]),
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
  /** the screen is phone-sized and gets the phone layout (see layout.ts) */
  mobile: signal(false),
  sheet: signal<Sheet>('none'),
  /** pixels the phone's tab bar or open pane covers along the right and bottom edges; the camera centres on the rest */
  covered: signal({ right: 0, bottom: 0 }),
};

export const isAdmin = (): boolean => ui.me.value?.role === 'admin';
/** Watching with a guest seat, as everyone but the admin does. */
export const isGuest = (): boolean => ui.me.value?.role === 'guest';

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
}

export const actions: UiActions = {
  start: () => {},
  setSpeed: () => {},
  select: () => {},
  applyAudio: () => {},
  sendChat: () => Promise.resolve(),
  renameChat: (name) => Promise.resolve(name),
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
  ui.houses.value = snapshot.houses;
  ui.ceremony.value = snapshot.ceremony;
  ui.stockpiles.value = snapshot.stockpiles;
  ui.economy.value = snapshot.economy;
  ui.weather.value = snapshot.weather;
  ui.fires.value = snapshot.worldObjects.filter((object) => object.stage === 'burning');
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
  ui.founding.value = status.founding;
}

export function addChronicle(entry: ChronicleEntry): void {
  ui.chronicle.value = [...ui.chronicle.value, entry].slice(-CHRONICLE_KEPT);
}

/** A new grave takes its plot from whoever lay there before (a full graveyard reuses its oldest plot). */
export function addGrave(grave: Grave): void {
  ui.graves.value = [...ui.graves.value.filter((existing) => existing.plot !== grave.plot), grave];
}

/** A house caught fire, burned on, fell in ruins, was worked on or stands whole again. */
export function setHouse(house: House): void {
  const others = ui.houses.value.filter((entry) => entry.id !== house.id);
  ui.houses.value = house.phase === 'intact' ? others : [...others, house];
}

/** A wedding or funeral was arranged, moved on, or is over. */
export function setCeremony(ceremony: Ceremony): void {
  ui.ceremony.value = ceremony.phase === 'over' ? null : ceremony;
}

/** A stockpile filled or emptied; one with nothing left is dropped. */
export function setStockpile(pile: Stockpile): void {
  const others = ui.stockpiles.value.filter((entry) => entry.place !== pile.place);
  ui.stockpiles.value = pile.wood + pile.stone + pile.ore === 0 ? others : [...others, pile];
}

/** Coin moved through the village chest, or goods on or off the market's stalls. */
export function setEconomy(economy: Economy): void {
  ui.economy.value = economy;
}

/** The weather changed, or came on harder or eased. */
export function setWeather(weather: Weather): void {
  ui.weather.value = weather;
}

/** A thing of the world caught fire or stopped burning. */
export function setWorldObject(object: WorldObject): void {
  const others = ui.fires.value.filter((entry) => entry.id !== object.id);
  ui.fires.value = object.stage === 'burning' ? [...others, object] : others;
}

/** Everything the village has put by, over every stockpile. */
export function villageStores(): { wood: number; stone: number; ore: number } {
  const all = { wood: 0, stone: 0, ore: 0 };
  for (const pile of ui.stockpiles.value) {
    all.wood += pile.wood;
    all.stone += pile.stone;
    all.ore += pile.ore;
  }
  return all;
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
