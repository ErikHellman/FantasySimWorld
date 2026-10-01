import { formatClock, gameTime, isNight } from '../core/time';
import type { LlmSettings } from '../persist/settings';
import type { SimEvent } from '../sim/events';
import type { Sim } from '../sim/sim';
import type { Character, ConversationLine, Id, Intent, SimState } from '../sim/state';
import { firstName, getCharacter, isFighting, living, nameOf } from '../sim/util';
import { Governor } from './governor';
import { LlmClient } from './llm/client';
import { JobQueue } from './llm/queue';
import { LlmError, type LlmResponse } from './llm/types';
import {
  conversationPrompt, decidePrompt, mercyPrompt, planPrompt, promptText, reactPrompt, reflectPrompt, replyPrompt, type Prompt,
} from './prompts';
import { feeling, interpretConversation, interpretDecision, interpretPlan, resolvePerson, spoken } from './tools/validate';

export type RequestKind = 'decide' | 'react' | 'plan_day' | 'converse' | 'reply' | 'mercy' | 'reflect';

export interface BrainLogRecord {
  time: string;
  character: string;
  kind: RequestKind;
  status: 'ok' | 'fallback' | 'error' | 'stale';
  latencyMs: number;
  tokens: number;
  summary: string;
  prompt: string;
  response: string;
}

export interface BrainStatus {
  state: 'off' | 'ready' | 'busy' | 'error';
  inFlight: number;
  queued: number;
  requests: number;
  failures: number;
  tokens: number;
  latencyMs: number;
  protocol: string;
  message: string;
}

export interface BrainHooks {
  /** current simulation speed (0 = paused) */
  speed(): number;
  log(record: BrainLogRecord): void;
  status(status: BrainStatus): void;
}

type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

interface Mind {
  lastDecideAt: number;
  lastReactAt: number;
  plannedDay: number;
  reflectedDay: number;
  /** after a failed plan or reflection, do not ask again before this time */
  retryAt: number;
}

const RETRY_AFTER_MS = 60_000;

const SCAN_INTERVAL_MS = 400;
const MAX_TURN_LINES = 6;

const PRIORITY: Record<RequestKind, number> = { mercy: 0, react: 1, reply: 2, converse: 2, decide: 4, plan_day: 5, reflect: 8 };

/**
 * Connects the simulation to a language model. It watches for moments that
 * deserve a decision, asks the model through a budgeted queue, checks the
 * answer against the world as it is by then, and sends the result to the sim
 * as commands. When the model is slow, wrong or absent, nothing here blocks:
 * the sim's own routines and local lines carry on.
 */
export class Brain {
  private client: LlmClient | null = null;
  private queue: JobQueue | null = null;
  private settings: LlmSettings | null = null;
  private readonly governor = new Governor();
  private readonly minds = new Map<Id, Mind>();
  /** conversation id -> transcript as the brain knows it (turn-by-turn mode) */
  private readonly turns = new Map<Id, ConversationLine[]>();
  private lastScan = -Infinity;
  private tokens = 0;
  private lastError = '';

  constructor(
    private readonly sim: Sim,
    private readonly hooks: BrainHooks,
    private readonly fetchImpl?: FetchLike,
    /** starting request budget; the default suits a model of unknown speed */
    private readonly initialBudget = 3,
  ) {}

  get enabled(): boolean {
    return this.client !== null;
  }

  /** True when the model has more waiting than it can clear soon; the app slows the world to 1x meanwhile. */
  get backlogged(): boolean {
    const queue = this.queue;
    return queue !== null && queue.waiting > queue.concurrency * 2;
  }

  /** Applies new settings; pass a disabled or incomplete configuration to fall back to local minds. */
  configure(settings: LlmSettings): void {
    this.queue?.clear();
    this.turns.clear();
    this.lastError = '';
    const usable = settings.enabled && settings.baseUrl.trim() !== '' && settings.model.trim() !== '';
    if (!usable) {
      this.client = null;
      this.queue = null;
      this.settings = null;
      this.sim.enqueue({ type: 'tuning', tuning: { chatter: 0.5, scriptWaitTicks: 0, mercyWaitTicks: 40 } });
      this.publish();
      return;
    }
    this.settings = settings;
    this.client = new LlmClient(settings, this.fetchImpl);
    this.queue = new JobQueue(Math.max(1, settings.concurrency), settings.requestsPerMinute);
    this.queue.onChange = () => this.publish();
    this.governor.reset(this.initialBudget);
    this.applyTuning();
    this.publish();
  }

  private mind(id: Id): Mind {
    let mind = this.minds.get(id);
    if (!mind) {
      mind = { lastDecideAt: -Infinity, lastReactAt: -Infinity, plannedDay: 0, reflectedDay: 0, retryAt: 0 };
      this.minds.set(id, mind);
    }
    return mind;
  }

  private get state(): SimState {
    return this.sim.state;
  }

  /** Prompt size in tokens: smaller when requests are served one at a time by a local model. */
  private get promptBudget(): number {
    return (this.settings?.concurrency ?? 1) <= 1 ? 900 : 1500;
  }

  private publish(): void {
    const queue = this.queue;
    if (!queue || !this.client) {
      this.hooks.status({ state: 'off', inFlight: 0, queued: 0, requests: 0, failures: 0, tokens: this.tokens, latencyMs: 0, protocol: '', message: '' });
      return;
    }
    const busy = queue.inFlight + queue.waiting > 0;
    this.hooks.status({
      state: queue.circuitOpen ? 'error' : busy ? 'busy' : 'ready',
      inFlight: queue.inFlight,
      queued: queue.waiting,
      requests: queue.completed + queue.failed,
      failures: queue.failed,
      tokens: this.tokens,
      latencyMs: queue.latencyMs,
      protocol: this.client.protocol === 'tools' ? 'native tool calls' : 'JSON output',
      message: queue.circuitOpen ? `Paused after repeated failures; villagers are on their routines. ${this.lastError}` : this.lastError,
    });
  }

  private applyTuning(): void {
    const queue = this.queue;
    if (!queue) return;
    const speed = Math.max(1, this.hooks.speed());
    const latency = (queue.latencyMs || 5000) / 1000;
    // How long a conversation or a yielded opponent waits for the model, in real seconds, converted to ticks.
    const waitSeconds = Math.min(28, Math.max(7, latency * 2 + 4));
    this.sim.enqueue({
      type: 'tuning',
      tuning: {
        chatter: this.governor.chatter(),
        scriptWaitTicks: Math.round(waitSeconds * 20 * speed),
        mercyWaitTicks: Math.round(waitSeconds * 20 * speed),
      },
    });
  }

  private record(kind: RequestKind, c: Character | null, status: BrainLogRecord['status'], summary: string, prompt: Prompt | null, response: LlmResponse | null, error?: unknown): void {
    const time = gameTime(this.state.tick);
    if (response) this.tokens += response.promptTokens + response.completionTokens;
    if (error) this.lastError = error instanceof Error ? error.message : String(error);
    else if (status === 'ok') this.lastError = '';
    this.hooks.log({
      time: `D${time.day} ${formatClock(time.minuteOfDay)}`,
      character: c ? firstName(c) : '—',
      kind,
      status,
      latencyMs: response?.latencyMs ?? 0,
      tokens: response ? response.promptTokens + response.completionTokens : 0,
      summary,
      prompt: prompt ? promptText(prompt) : '',
      response: response?.raw ?? (error instanceof LlmError ? `${error.message}${error.raw ? `\n\n${error.raw}` : ''}` : error ? String(error) : ''),
    });
  }

  /**
   * Runs one request through the queue. `build` is called when the request is
   * actually sent, so the prompt describes the world at that moment; returning
   * null from it skips the request. `apply` gets the reply and returns a
   * one-line summary, or throws to mark the reply unusable.
   */
  private request(options: {
    kind: RequestKind;
    key: string;
    character: Id | null;
    ttlMs: number;
    build: () => Prompt | null;
    apply: (response: LlmResponse) => { status: BrainLogRecord['status']; summary: string };
    onFail?: () => void;
  }): void {
    const queue = this.queue;
    const client = this.client;
    if (!queue || !client) {
      options.onFail?.();
      return;
    }
    this.governor.spend();
    queue.enqueue({
      key: options.key,
      priority: PRIORITY[options.kind],
      expiresAt: performance.now() + options.ttlMs,
      onDrop: () => options.onFail?.(),
      run: async (signal) => {
        const prompt = options.build();
        const c = getCharacter(this.state, options.character);
        if (!prompt) {
          options.onFail?.();
          return;
        }
        try {
          const response = await client.complete({ messages: prompt.messages, tools: prompt.tools, maxCalls: prompt.maxCalls, signal });
          const result = options.apply(response);
          this.record(options.kind, c, result.status, result.summary, prompt, response);
          if (result.status !== 'ok') options.onFail?.();
        } catch (error) {
          options.onFail?.();
          if (!(error instanceof LlmError && error.kind === 'aborted')) {
            this.record(options.kind, c, 'error', error instanceof Error ? error.message : String(error), prompt, null, error);
          }
          throw error;
        }
      },
    });
  }

  /** Per sim tick: react to events that need the model at once. */
  handle(events: readonly SimEvent[]): void {
    for (const event of events) {
      if (event.type === 'conversation_started') this.onConversation(event.convoId);
      else if (event.type === 'conversation_ended') {
        this.turns.delete(event.convoId);
        this.queue?.cancel(`v:${event.convoId}`);
      } else if (event.type === 'mercy_request') this.onMercy(event.attackerId, event.victimId);
      else if (event.type === 'death' && !event.monster) {
        this.queue?.cancel(`c:${event.id}`);
        this.minds.delete(event.id);
      }
    }
  }

  /**
   * Per rendered frame: keep the queue moving and, a few times a second, look
   * for characters with something to decide. `force` scans at once.
   */
  update(force = false): void {
    const queue = this.queue;
    if (!queue) return;
    const paused = this.hooks.speed() === 0;
    queue.setPaused(paused);
    queue.pump();
    const now = performance.now();
    if (paused || (!force && now - this.lastScan < SCAN_INTERVAL_MS)) return;
    this.lastScan = now;
    this.governor.refill(queue.capacity, now);
    if (this.governor.shouldRetune(now)) this.applyTuning();
    this.scan(now);
  }

  private scan(now: number): void {
    const queue = this.queue;
    if (!queue) return;
    const state = this.state;
    const time = gameTime(state.tick);
    const deciders: Array<{ c: Character; mind: Mind; urgency: number }> = [];
    for (const c of living(state)) {
      const key = `c:${c.id}`;
      if (queue.has(key) || c.convoId || c.combat.yielded || c.combat.mercyUntilTick > 0) continue;
      const mind = this.mind(c.id);
      const urgency = c.stimuli.reduce((max, stimulus) => Math.max(max, stimulus.urgency), -1);

      // Danger: one quick fight-or-flee call, while instinct has already acted.
      if (isFighting(c)) {
        const fresh = c.stimuli.some((stimulus) => (stimulus.kind === 'threat' || stimulus.kind === 'attacked' || stimulus.kind === 'alarm') && state.tick - stimulus.tick < 200);
        if (fresh && c.intent?.source === 'reflex' && now - mind.lastReactAt > 30_000 && this.governor.allow(0)) {
          mind.lastReactAt = now;
          this.requestReact(c);
        }
        continue;
      }

      if (c.asleep) {
        const night = time.hour < 12 ? time.day - 1 : time.day;
        if (isNight(time.hour) && mind.reflectedDay !== night && c.lastReflectionDay !== night && c.memories.length > 4 && now >= mind.retryAt && this.governor.allow(2)) {
          mind.reflectedDay = night;
          this.requestReflect(c, night);
        }
        continue;
      }

      // Each morning, once: a plan for the day in the character's own words.
      if (mind.plannedDay !== time.day && time.hour >= 4.5 && time.hour < 11 && c.planDay === time.day && now >= mind.retryAt && this.governor.allow(1)) {
        mind.plannedDay = time.day;
        this.requestPlan(c);
        continue;
      }

      if (urgency < 0) continue;
      const cooldown = urgency >= 2 ? 6_000 : urgency >= 1 ? 18_000 : 40_000 * this.governor.patience();
      if (now - mind.lastDecideAt >= cooldown) deciders.push({ c, mind, urgency });
    }

    // Whoever has waited longest for a decision goes first, so a slow model is shared fairly
    // rather than spent on the same few villagers.
    deciders.sort((a, b) => b.urgency - a.urgency || (a.mind.lastDecideAt < b.mind.lastDecideAt ? -1 : a.mind.lastDecideAt > b.mind.lastDecideAt ? 1 : 0));
    for (const { c, mind, urgency } of deciders) {
      if (!this.governor.allow(urgency >= 1 ? 1 : 2)) break;
      mind.lastDecideAt = now;
      this.requestDecide(c);
    }
  }

  private requestDecide(c: Character): void {
    let epoch = -1;
    const id = c.id;
    this.request({
      kind: 'decide',
      key: `c:${id}`,
      character: id,
      ttlMs: 30_000,
      build: () => {
        const self = getCharacter(this.state, id);
        if (!self || self.convoId || self.asleep || isFighting(self) || self.combat.yielded) return null;
        epoch = self.epoch;
        const prompt = decidePrompt(this.state, self, this.promptBudget);
        this.sim.enqueue({ type: 'clear_stimuli', id, throughTick: this.state.tick });
        return prompt;
      },
      apply: (response) => {
        const self = getCharacter(this.state, id);
        if (!self || self.epoch !== epoch) return { status: 'stale', summary: 'the situation changed before the answer came' };
        const decision = interpretDecision(this.state, self, response.calls);
        const steps = decision.intents.map((intent) => describeIntent(this.state, intent)).join(' → ');
        const problems = decision.problems.length > 0 ? ` (dropped: ${decision.problems.join('; ')})` : '';
        if (decision.intents.length > 0) {
          this.sim.enqueue({ type: 'set_intents', id, epoch, intents: decision.intents });
          return { status: 'ok', summary: `${steps}${problems}` };
        }
        if (decision.thought) this.sim.enqueue({ type: 'thought', id, text: decision.thought });
        if (decision.keep) return { status: 'ok', summary: `carries on${decision.thought ? `: ${decision.thought}` : ''}` };
        return { status: 'fallback', summary: `nothing usable${problems}` };
      },
    });
  }

  private requestReact(c: Character): void {
    let epoch = -1;
    const id = c.id;
    this.request({
      kind: 'react',
      key: `c:${id}`,
      character: id,
      ttlMs: 9_000,
      build: () => {
        const self = getCharacter(this.state, id);
        if (!self || !isFighting(self) || self.combat.yielded) return null;
        epoch = self.epoch;
        const prompt = reactPrompt(this.state, self);
        this.sim.enqueue({ type: 'clear_stimuli', id, throughTick: this.state.tick });
        return prompt;
      },
      apply: (response) => {
        const self = getCharacter(this.state, id);
        if (!self || self.epoch !== epoch) return { status: 'stale', summary: 'the fight moved on before the answer came' };
        const decision = interpretDecision(this.state, self, response.calls);
        const intent = decision.intents[0];
        if (!intent) return { status: 'fallback', summary: `nothing usable (${decision.problems.join('; ')})` };
        this.sim.enqueue({ type: 'set_intents', id, epoch, intents: [intent] });
        return { status: 'ok', summary: `${intent.kind}${intent.say ? `: “${intent.say}”` : ''}` };
      },
    });
  }

  private requestPlan(c: Character): void {
    const id = c.id;
    this.request({
      kind: 'plan_day',
      key: `c:${id}`,
      character: id,
      ttlMs: 90_000,
      onFail: () => {
        // Try again later this morning, but not at once.
        const mind = this.mind(id);
        mind.plannedDay = 0;
        mind.retryAt = performance.now() + RETRY_AFTER_MS;
      },
      build: () => {
        const self = getCharacter(this.state, id);
        return self && !self.asleep && !isFighting(self) ? planPrompt(this.state, self, this.promptBudget) : null;
      },
      apply: (response) => {
        const plan = interpretPlan(response.calls);
        if (!plan) return { status: 'fallback', summary: 'the plan could not be read' };
        this.sim.enqueue({ type: 'set_plan', id, blocks: plan.blocks, thought: plan.thought });
        return { status: 'ok', summary: plan.blocks.map((block) => `${formatClock(block.hour * 60)} ${block.activity}`).join(', ') };
      },
    });
  }

  private requestReflect(c: Character, night: number): void {
    const id = c.id;
    this.request({
      kind: 'reflect',
      key: `c:${id}`,
      character: id,
      ttlMs: 240_000,
      build: () => {
        const self = getCharacter(this.state, id);
        return self && self.asleep ? reflectPrompt(this.state, self, this.promptBudget) : null;
      },
      onFail: () => {
        // Let it be tried again later the same night, but not at once.
        const mind = this.mind(id);
        mind.reflectedDay = 0;
        mind.retryAt = performance.now() + RETRY_AFTER_MS;
      },
      apply: (response) => {
        const call = response.calls.find((entry) => entry.name === 'reflection');
        const summary = typeof call?.args.summary === 'string' ? call.args.summary : '';
        if (!call || !summary.trim()) return { status: 'fallback', summary: 'the reflection could not be read' };
        const insights = Array.isArray(call.args.insights) ? call.args.insights.filter((entry): entry is string => typeof entry === 'string') : [];
        const relationships: Array<{ id: Id; summary: string }> = [];
        if (Array.isArray(call.args.relationships)) {
          for (const entry of call.args.relationships) {
            const item = entry as { person?: unknown; opinion?: unknown };
            const person = resolvePerson(this.state, item?.person, id);
            if (person && typeof item.opinion === 'string') relationships.push({ id: person.id, summary: item.opinion });
          }
        }
        this.sim.enqueue({ type: 'reflection', id, summary, insights, relationships });
        this.mind(id).reflectedDay = night;
        return { status: 'ok', summary: summary.slice(0, 140) };
      },
    });
  }

  private conversationMode(): 'batched' | 'turns' {
    const preference = this.settings?.conversations ?? 'auto';
    if (preference !== 'auto') return preference;
    return this.queue && this.queue.capacity >= 1.2 ? 'turns' : 'batched';
  }

  private onConversation(convoId: Id): void {
    if (!this.queue || !this.governor.allow(1)) {
      if (this.queue) this.sim.enqueue({ type: 'conversation_local', convoId });
      return;
    }
    if (this.conversationMode() === 'turns') {
      const convo = this.state.conversations[convoId];
      if (!convo) return;
      this.turns.set(convoId, convo.lines.map((line) => ({ ...line })));
      // Whoever has not spoken yet goes first: the initiator if there is no opening line.
      this.requestReply(convoId, convo.lines.length === 0 ? convo.a : convo.b);
      return;
    }
    const useLocal = (): void => this.sim.enqueue({ type: 'conversation_local', convoId });
    this.request({
      kind: 'converse',
      key: `v:${convoId}`,
      character: this.state.conversations[convoId]?.a ?? null,
      ttlMs: 20_000,
      onFail: useLocal,
      build: () => {
        const convo = this.state.conversations[convoId];
        const a = getCharacter(this.state, convo?.a);
        const b = getCharacter(this.state, convo?.b);
        return convo && !convo.closed && a && b ? conversationPrompt(this.state, convo, a, b, this.promptBudget) : null;
      },
      apply: (response) => {
        const convo = this.state.conversations[convoId];
        const a = getCharacter(this.state, convo?.a);
        const b = getCharacter(this.state, convo?.b);
        if (!convo || convo.closed || !a || !b) return { status: 'stale', summary: 'the conversation was already over' };
        const script = interpretConversation(response.calls, a, b, convo.lines.length === 0);
        if (!script) return { status: 'fallback', summary: 'the lines could not be read' };
        this.sim.enqueue({ type: 'conversation_lines', convoId, lines: script.lines, topic: script.topic, feelings: script.feelings, closed: true });
        return { status: 'ok', summary: `${firstName(a)} & ${firstName(b)} on ${script.topic || 'this and that'} (${script.lines.length + convo.lines.length} lines)` };
      },
    });
  }

  /** Turn-by-turn: ask one speaker for their next line, then the other, until someone ends it. */
  private requestReply(convoId: Id, speakerId: Id): void {
    const useLocal = (): void => {
      this.turns.delete(convoId);
      this.sim.enqueue({ type: 'conversation_local', convoId });
    };
    this.request({
      kind: 'reply',
      key: `v:${convoId}`,
      character: speakerId,
      ttlMs: 20_000,
      onFail: useLocal,
      build: () => {
        const convo = this.state.conversations[convoId];
        const lines = this.turns.get(convoId);
        const self = getCharacter(this.state, speakerId);
        const other = getCharacter(this.state, convo ? (convo.a === speakerId ? convo.b : convo.a) : null);
        return convo && !convo.closed && lines && self && other ? replyPrompt(this.state, convo, lines, self, other, this.promptBudget) : null;
      },
      apply: (response) => {
        const convo = this.state.conversations[convoId];
        const lines = this.turns.get(convoId);
        if (!convo || convo.closed || !lines) return { status: 'stale', summary: 'the conversation was already over' };
        const call = response.calls.find((entry) => entry.name === 'reply');
        const text = spoken(call?.args.text);
        if (!call || !text) return { status: 'fallback', summary: 'the reply could not be read' };
        lines.push({ speakerId, text });
        const closed = call.args.end === true || lines.length >= MAX_TURN_LINES;
        const felt = feeling(call.args.feeling);
        this.sim.enqueue({ type: 'conversation_lines', convoId, lines: [{ speakerId, text }], feelings: felt ? { [speakerId]: felt } : undefined, closed, topic: convo.topic || undefined });
        if (closed) this.turns.delete(convoId);
        else {
          const next = convo.a === speakerId ? convo.b : convo.a;
          // Ask for the answer straight away (once this request has been cleared from the queue),
          // so it is ready by the time this line has been spoken.
          setTimeout(() => this.requestReply(convoId, next), 0);
        }
        return { status: 'ok', summary: `“${text}”${closed ? ' (ends)' : ''}` };
      },
    });
  }

  private onMercy(attackerId: Id, victimId: Id): void {
    this.request({
      kind: 'mercy',
      key: `c:${attackerId}`,
      character: attackerId,
      ttlMs: 15_000,
      build: () => {
        const attacker = getCharacter(this.state, attackerId);
        const victim = getCharacter(this.state, victimId);
        return attacker && victim && attacker.combat.mercyUntilTick > 0 ? mercyPrompt(this.state, attacker, victim) : null;
      },
      apply: (response) => {
        const call = response.calls.find((entry) => entry.name === 'spare' || entry.name === 'finish');
        if (!call) return { status: 'fallback', summary: 'no choice was made; mercy by default' };
        const say = spoken(call.args.say);
        if (typeof call.args.thought === 'string') this.sim.enqueue({ type: 'thought', id: attackerId, text: call.args.thought });
        this.sim.enqueue({ type: 'mercy', attackerId, spare: call.name === 'spare', say });
        return { status: 'ok', summary: `${call.name}${say ? `: “${say}”` : ''}` };
      },
    });
  }
}

function describeIntent(state: SimState, intent: Intent): string {
  switch (intent.kind) {
    case 'go_to': return `go to ${intent.placeId}`;
    case 'activity': return intent.activity;
    case 'talk': return `talk to ${nameOf(state, intent.targetId)} (${intent.tone}): “${intent.opener}”`;
    case 'give': return `give ${intent.item} to ${nameOf(state, intent.targetId)}`;
    case 'fight': return intent.mode === 'monster' ? 'fight' : `${intent.mode} ${nameOf(state, intent.targetId)}`;
    case 'flee': return 'flee';
    case 'wait': return 'wait';
  }
}
