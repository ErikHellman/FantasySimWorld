// The running world: the TypeScript Sim and Brain on a wall clock, the delta
// encoder, the replay ring and the connected viewers. Single-threaded, so a
// connection set up between two wakes always sees a consistent world.
import { randomBytes } from 'node:crypto';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { performance } from 'node:perf_hooks';
import { Brain, type BrainLogRecord as TsBrainLogRecord } from '../../src/brain/brain';
import type {
  BrainLogRecord, ChronicleEntry, Hello, LlmSettingsView, LlmStatus, Role, Snapshot, Utterance,
} from '../../src/generated/api';
import { PROTOCOL_VERSION } from '../../src/generated/api';
import { DEFAULT_SETTINGS, type LlmSettings } from '../../src/persist/settings';
import { Sim } from '../../src/sim/sim';
import type { SimState } from '../../src/sim/state';
import { worldHash } from '../../src/world/navgrid';
import { TickClock } from './clock';
import { chroniclePage, chronicleTail, detailOf, isAlarm, rosterOf, statusOf } from './digest';
import { DeltaTracker, chronicleEntry, graveOf } from './encode';
import { TickRing, eventId, parseEventId, sseEvent } from './sse';

const DIGEST_MS = 250;
const STATUS_MIN_MS = 1000;
const KEEP_ALIVE_MS = 15_000;
const WATCH_TTL_MS = 10 * 60_000;
/** A viewer whose socket has this much unsent data is too slow and is dropped. */
const MAX_BUFFERED = 4 * 1024 * 1024;
const VOICES_KEPT = 80;
const BRAIN_LOG_KEPT = 500;

export interface Viewer {
  res: ServerResponse;
  viewer: string;
  role: Role;
  /** the last `detail` sent, as JSON, so an unchanged one is not sent again */
  lastDetail: string;
}

/** The brain's settings plus the daily caps the Rust brain adds (stored here, not enforced). */
export type LlmConfig = LlmSettings & { maxRequestsPerDay: number; maxTokensPerDay: number };

const OFF: LlmStatus = { state: 'off', inFlight: 0, queued: 0, requests: 0, failures: 0, tokens: 0, latencyMs: 0, protocol: '', message: '' };

export class WorldHost {
  bootId = randomBytes(4).toString('hex');
  readonly hash = worldHash();
  speed: number;
  sim!: Sim;
  private brain!: Brain;
  private tracker!: DeltaTracker;
  private readonly ring = new TickRing(100);
  private readonly clock = new TickClock(performance.now());
  readonly viewers = new Set<Viewer>();
  /** viewer id -> watched character */
  private readonly watches = new Map<string, { id: string | null; at: number }>();
  /** the whole chronicle since the world began (the sim keeps only the last 600) */
  chronicle: ChronicleEntry[] = [];
  private voices: Utterance[] = [];
  llm: LlmStatus = OFF;
  brainLog: BrainLogRecord[] = [];
  private brainLogId = 0;
  llmConfig: LlmConfig;
  throttled = false;
  private lastStatus = '';
  private lastRoster = '';
  private lastStatusAt = 0;
  private lastDigestAt = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  lastTickAt = Date.now();
  lastWakeAt = Date.now();
  /** how late each wake was, in ms, newest last */
  readonly jitter: number[] = [];
  backlogDrops = 0;

  constructor(state: SimState, speed: number, llm: LlmConfig) {
    this.speed = speed;
    this.llmConfig = llm;
    this.load(state);
    setInterval(() => this.broadcast(': keep-alive\n\n'), KEEP_ALIVE_MS).unref();
  }

  get clockSkips(): number {
    return this.clock.skips;
  }

  /** Effective speed: held at 1x while the model has a backlog. */
  get playSpeed(): number {
    return this.throttled ? 1 : this.speed;
  }

  /** Replaces the world (start, new world, import). Viewers are told and disconnected; they reload. */
  load(state: SimState, reason?: string): void {
    this.sim = new Sim(state);
    this.tracker = new DeltaTracker(state);
    this.ring.clear();
    this.chronicle = state.chronicle.map(chronicleEntry);
    this.voices = [];
    this.brain = new Brain(this.sim, {
      speed: () => this.playSpeed,
      status: (status) => (this.llm = status),
      log: (record) => this.logBrain(record),
    });
    this.brain.configure(this.llmConfig);
    if (reason) {
      // A new boot id, so no viewer tries to replay ticks of the old world into the new one.
      this.bootId = randomBytes(4).toString('hex');
      this.broadcast(sseEvent('world_reset', { reason }));
      for (const viewer of this.viewers) viewer.res.end();
      this.viewers.clear();
    }
  }

  configureLlm(config: LlmConfig): void {
    this.llmConfig = config;
    this.brain.configure(config);
  }

  start(): void {
    let deadline = performance.now();
    const loop = (): void => {
      const now = performance.now();
      // Timers may fire a little early; a tick must not run before its deadline.
      if (now < deadline) {
        this.timer = setTimeout(loop, deadline - now);
        return;
      }
      this.jitter.push(now - deadline);
      if (this.jitter.length > 1200) this.jitter.shift();
      this.wake(now);
      deadline = this.clock.nextWake(performance.now());
      this.timer = setTimeout(loop, Math.max(0, deadline - performance.now()));
    };
    this.timer = setTimeout(loop, 0);
  }

  stop(): void {
    if (this.timer) clearTimeout(this.timer);
  }

  private wake(now: number): void {
    this.lastWakeAt = Date.now();
    const ticks = this.clock.due(now, this.playSpeed);
    for (let i = 0; i < ticks; i++) this.step();
    this.brain.update();
    this.throttled = this.speed > 1 && this.brain.backlogged;
    if (now - this.lastDigestAt >= DIGEST_MS) {
      this.lastDigestAt = now;
      this.digest(now);
    }
  }

  /** One sim tick, encoded once and sent to everyone. */
  step(): void {
    const events = this.sim.step();
    const state = this.sim.state;
    const frame = this.tracker.frame(state, events);
    for (const event of frame.ev ?? []) {
      if (event.type === 'chronicle') this.chronicle.push(event.entry);
      else if (event.type === 'speech') {
        const { type: _type, ...utterance } = event;
        this.voices = [utterance, ...this.voices].slice(0, VOICES_KEPT);
      }
    }
    this.brain.handle(events);
    const text = sseEvent('tick', frame, eventId(this.bootId, frame.t));
    this.ring.push(frame.t, text);
    this.broadcast(text);
    this.lastTickAt = Date.now();
  }

  private logBrain(record: TsBrainLogRecord): void {
    // The TypeScript brain does not split tokens or list commands; the Rust brain does.
    const entry: BrainLogRecord = {
      id: ++this.brainLogId,
      tick: this.sim.state.tick,
      at: Date.now(),
      character: record.character,
      kind: record.kind,
      status: record.status,
      latencyMs: Math.round(record.latencyMs),
      promptTokens: record.tokens,
      completionTokens: 0,
      summary: record.summary,
      prompt: record.prompt,
      response: record.response,
      commands: [],
    };
    this.brainLog = [entry, ...this.brainLog].slice(0, BRAIN_LOG_KEPT);
    const text = sseEvent('brainlog', entry);
    for (const viewer of this.viewers) if (viewer.role === 'admin') this.write(viewer, text);
  }

  // ---- viewers -------------------------------------------------------------

  private write(viewer: Viewer, text: string): void {
    if (viewer.res.writableEnded) {
      this.viewers.delete(viewer);
      return;
    }
    if (viewer.res.writableLength > MAX_BUFFERED) {
      this.backlogDrops++;
      viewer.res.destroy();
      this.viewers.delete(viewer);
      return;
    }
    viewer.res.write(text);
  }

  broadcast(text: string): void {
    for (const viewer of this.viewers) this.write(viewer, text);
  }

  hello(role: Role): Hello {
    return {
      bootId: this.bootId, tick: this.sim.state.tick, speed: this.speed, protocolVersion: PROTOCOL_VERSION,
      worldHash: this.hash, viewers: this.viewers.size, role,
    };
  }

  snapshot(): Snapshot {
    const state = this.sim.state;
    return {
      tick: state.tick,
      speed: this.speed,
      entities: this.tracker.entities(),
      roster: rosterOf(state),
      chronicle: chronicleTail(state),
      graves: state.graves.map(graveOf),
      alarm: isAlarm(state),
      llm: this.llm,
      voices: this.voices,
    };
  }

  /** Opens a stream: hello, then a replay of what was missed or a snapshot, then the current status and detail. */
  connect(req: IncomingMessage, res: ServerResponse, viewerId: string, role: Role, lastEventId: string | undefined): void {
    const viewer: Viewer = { res, viewer: viewerId, role, lastDetail: '' };
    res.write('retry: 500\n\n');
    this.viewers.add(viewer);
    res.write(sseEvent('hello', this.hello(role)));
    const last = parseEventId(lastEventId);
    const state = this.sim.state;
    const replay = last && last.bootId === this.bootId ? this.ring.since(last.tick, state.tick) : null;
    if (replay) for (const text of replay) res.write(text);
    else res.write(sseEvent('snapshot', this.snapshot(), eventId(this.bootId, state.tick)));
    res.write(sseEvent('status', this.status()));
    res.write(sseEvent('roster', rosterOf(state)));
    this.touchWatch(viewerId);
    this.sendDetail(viewer);
    // Bun's node:http reports a closed connection on the request, Node on both.
    const gone = (): void => {
      this.viewers.delete(viewer);
    };
    req.on('close', gone);
    res.on('close', gone);
  }

  private status() {
    return statusOf(this.sim.state, { llm: this.llm, viewers: this.viewers.size, speed: this.speed, throttled: this.throttled });
  }

  private touchWatch(viewerId: string): void {
    const watch = this.watches.get(viewerId);
    if (watch) watch.at = Date.now();
  }

  watch(viewerId: string, id: string | null): void {
    this.watches.set(viewerId, { id, at: Date.now() });
    for (const viewer of this.viewers) {
      if (viewer.viewer !== viewerId) continue;
      viewer.lastDetail = '';
      this.sendDetail(viewer);
    }
  }

  private sendDetail(viewer: Viewer, cache?: Map<string, string | null>): void {
    const id = this.watches.get(viewer.viewer)?.id;
    if (!id) return;
    let text = cache?.get(id);
    if (text === undefined) {
      const detail = detailOf(this.sim.state, id);
      text = detail ? JSON.stringify(detail) : null;
      cache?.set(id, text);
    }
    if (text === null || text === viewer.lastDetail) return;
    viewer.lastDetail = text;
    this.write(viewer, `event: detail\ndata: ${text}\n\n`);
  }

  /** Four times a second: status and roster (at most once a second, when changed) and each watched character's detail. */
  private digest(now: number): void {
    const state = this.sim.state;
    if (now - this.lastStatusAt >= STATUS_MIN_MS) {
      const status = JSON.stringify(this.status());
      const roster = JSON.stringify(rosterOf(state));
      if (status !== this.lastStatus || roster !== this.lastRoster) this.lastStatusAt = now;
      if (status !== this.lastStatus) {
        this.lastStatus = status;
        this.broadcast(`event: status\ndata: ${status}\n\n`);
      }
      if (roster !== this.lastRoster) {
        this.lastRoster = roster;
        this.broadcast(`event: roster\ndata: ${roster}\n\n`);
      }
    }
    // One computation per watched character, however many viewers watch it.
    const cache = new Map<string, string | null>();
    for (const viewer of this.viewers) {
      this.touchWatch(viewer.viewer);
      this.sendDetail(viewer, cache);
    }
    const stale = Date.now() - WATCH_TTL_MS;
    for (const [viewerId, watch] of this.watches) if (watch.at < stale) this.watches.delete(viewerId);
  }

  chroniclePage(before: number, limit: number) {
    return chroniclePage(this.chronicle, before, limit);
  }

  llmView(): LlmSettingsView {
    const c = this.llmConfig;
    return {
      enabled: c.enabled, baseUrl: c.baseUrl, model: c.model, apiKeySet: c.apiKey !== '', apiKeyHint: c.apiKey ? `…${c.apiKey.slice(-4)}` : null,
      concurrency: c.concurrency, protocol: c.protocol, conversations: c.conversations, reasoning: c.reasoning, temperature: c.temperature,
      maxTokens: c.maxTokens, headers: c.headers, requestsPerMinute: c.requestsPerMinute, maxRequestsPerDay: c.maxRequestsPerDay, maxTokensPerDay: c.maxTokensPerDay,
    };
  }
}

/** LLM settings from the environment: on when LLM_BASE_URL and LLM_MODEL are set. */
export function llmFromEnv(env: NodeJS.ProcessEnv): LlmConfig {
  const baseUrl = env.LLM_BASE_URL ?? '';
  const model = env.LLM_MODEL ?? '';
  return {
    ...DEFAULT_SETTINGS.llm,
    enabled: baseUrl !== '' && model !== '',
    baseUrl,
    model,
    apiKey: env.LLM_API_KEY ?? '',
    concurrency: Number(env.LLM_CONCURRENCY ?? DEFAULT_SETTINGS.llm.concurrency),
    maxRequestsPerDay: 0,
    maxTokensPerDay: 0,
  };
}

