import {
  PROTOCOL_VERSION, type BrainLogRecord, type CharacterDetail, type ChatMessage, type ChatWelcome, type ChatYou, type Hello,
  type RosterEntry, type Snapshot, type Status, type TickFrame, type WorldReset,
} from '../generated/api';
import { HttpError, api } from './api';

export type ConnectionState = 'connecting' | 'live' | 'down';

export interface StreamHandlers {
  hello(hello: Hello): void;
  snapshot(snapshot: Snapshot): void;
  tick(frame: TickFrame): void;
  status(status: Status): void;
  /** the roster, when it changes (at most once a second) */
  roster(roster: RosterEntry[]): void;
  detail(detail: CharacterDetail): void;
  brainlog(record: BrainLogRecord): void;
  worldReset(reset: WorldReset): void;
  /** the chat history and this viewer's chat name; replaces what came before */
  chatWelcome(welcome: ChatWelcome): void;
  chat(message: ChatMessage): void;
  /** this viewer's chat name changed (here or in another tab of the same account) */
  chatYou(you: ChatYou): void;
  /** the server refused the stream and `/api/me` says this browser is not signed in (401) or not allowed (403) */
  refused(status: 401 | 403): void;
  connection(state: ConnectionState): void;
}

const RELOAD_GUARD = 'fsw.protocolReload';
/** a tab hidden this long drops its stream and resyncs from a snapshot on return */
const HIDDEN_CLOSE_MS = 30_000;
/** a running world that sends no tick for this long has lost us somewhere (a proxy that kept a dead stream open) */
const SILENT_MS = 10_000;

/** A random id for this tab (kept across reloads of the same tab), so the server knows whose inspector is whose. */
export function viewerId(): string {
  try {
    const stored = sessionStorage.getItem('fsw.viewer');
    if (stored) return stored;
    const id = crypto.randomUUID();
    sessionStorage.setItem('fsw.viewer', id);
    return id;
  } catch {
    return crypto.randomUUID();
  }
}

/**
 * The event stream from the server. The browser reconnects by itself after a
 * dropped connection, sending the last event id so the server can replay the
 * gap; when the server refuses (a non-2xx answer closes the EventSource), this
 * asks `/api/me` whether we are still signed in, and otherwise tries again
 * with a fresh connection (and so a fresh snapshot).
 */
export class Stream {
  private source: EventSource | null = null;
  private lastTick = -1;
  private retryTimer = 0;
  private retryDelay = 1000;
  private hiddenTimer = 0;
  /** closed because the tab was hidden; reopen when it is shown */
  private suspended = false;
  /** the world's speed as last reported; a paused world is silent by design */
  private speed = 1;
  private lastTickAt = Date.now();

  /** `chatName` is the chat name this browser would like, asked for on each connect. */
  constructor(
    private readonly viewer: string,
    private readonly handlers: StreamHandlers,
    private readonly chatName: () => string | undefined = () => undefined,
  ) {
    document.addEventListener('visibilitychange', () => this.onVisibility());
    window.setInterval(() => {
      if (!this.source || this.source.readyState !== EventSource.OPEN || this.speed === 0) return;
      if (Date.now() - this.lastTickAt < SILENT_MS) return;
      console.warn('The stream has gone quiet; reconnecting');
      this.lastTick = -1;
      this.open();
    }, SILENT_MS / 2);
  }

  open(): void {
    this.close();
    this.lastTickAt = Date.now();
    this.handlers.connection('connecting');
    const name = this.chatName();
    const asking = name ? `&name=${encodeURIComponent(name)}` : '';
    const source = new EventSource(`/api/stream?viewer=${encodeURIComponent(this.viewer)}${asking}`);
    this.source = source;
    const on = <T>(name: string, handle: (data: T) => void): void => {
      source.addEventListener(name, (event) => {
        if (this.source !== source) return;
        handle(JSON.parse((event as MessageEvent<string>).data) as T);
      });
    };
    on<Hello>('hello', (hello) => {
      this.retryDelay = 1000;
      this.handlers.connection('live');
      if (hello.protocolVersion !== PROTOCOL_VERSION) {
        this.protocolMismatch(hello.protocolVersion);
        return;
      }
      this.speed = hello.speed;
      try {
        sessionStorage.removeItem(RELOAD_GUARD);
      } catch {
        // storage blocked: nothing to clear
      }
      this.handlers.hello(hello);
    });
    on<Snapshot>('snapshot', (snapshot) => {
      this.lastTick = snapshot.tick;
      this.lastTickAt = Date.now();
      this.speed = snapshot.speed;
      this.handlers.snapshot(snapshot);
    });
    on<TickFrame>('tick', (frame) => {
      this.lastTickAt = Date.now();
      if (this.lastTick >= 0 && frame.t !== this.lastTick + 1) {
        if (frame.t <= this.lastTick) return; // already seen
        // A hole the server did not fill: start over from a snapshot.
        console.warn(`Stream skipped from tick ${this.lastTick} to ${frame.t}; resyncing`);
        this.lastTick = -1;
        this.open();
        return;
      }
      this.lastTick = frame.t;
      this.handlers.tick(frame);
    });
    on<Status>('status', (status) => {
      this.speed = status.speed;
      this.handlers.status(status);
    });
    on<RosterEntry[]>('roster', (roster) => this.handlers.roster(roster));
    on<CharacterDetail>('detail', (detail) => this.handlers.detail(detail));
    on<BrainLogRecord>('brainlog', (record) => this.handlers.brainlog(record));
    on<ChatWelcome>('chat_welcome', (welcome) => this.handlers.chatWelcome(welcome));
    on<ChatMessage>('chat', (message) => this.handlers.chat(message));
    on<ChatYou>('chat_you', (you) => this.handlers.chatYou(you));
    on<WorldReset>('world_reset', (reset) => {
      this.close();
      this.handlers.worldReset(reset);
    });
    source.onerror = () => {
      if (this.source !== source) return;
      if (source.readyState === EventSource.CONNECTING) {
        this.handlers.connection('connecting'); // the browser is retrying with Last-Event-ID
        return;
      }
      this.handlers.connection('down');
      void this.afterRefusal();
    };
  }

  close(): void {
    clearTimeout(this.retryTimer);
    this.source?.close();
    this.source = null;
  }

  private async afterRefusal(): Promise<void> {
    this.close();
    try {
      await api.me();
    } catch (error) {
      if (error instanceof HttpError && (error.status === 401 || error.status === 403)) {
        this.handlers.refused(error.status);
        return;
      }
    }
    // Server down or restarting: try again, backing off to ten seconds.
    this.retryTimer = window.setTimeout(() => this.open(), this.retryDelay);
    this.retryDelay = Math.min(10_000, this.retryDelay * 2);
  }

  private protocolMismatch(version: number): void {
    this.close();
    let reloaded = false;
    try {
      reloaded = sessionStorage.getItem(RELOAD_GUARD) !== null;
      sessionStorage.setItem(RELOAD_GUARD, String(version));
    } catch {
      reloaded = true; // cannot guard, so do not risk a reload loop
    }
    if (!reloaded) location.reload();
    else this.handlers.connection('down');
    if (reloaded) console.error(`The server speaks protocol ${version}, this page ${PROTOCOL_VERSION}; reloading did not help.`);
  }

  private onVisibility(): void {
    if (document.hidden) {
      this.hiddenTimer = window.setTimeout(() => {
        if (!this.source) return;
        this.suspended = true;
        this.close();
      }, HIDDEN_CLOSE_MS);
      return;
    }
    clearTimeout(this.hiddenTimer);
    if (this.suspended) {
      this.suspended = false;
      this.lastTick = -1;
      this.open();
    }
  }
}
