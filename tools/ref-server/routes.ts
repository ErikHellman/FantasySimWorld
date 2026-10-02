// The REST surface and the stream endpoint, on plain node:http. Bodies use the
// types in src/generated/api.ts; every error is an `ApiError` ({error}).
import { randomUUID } from 'node:crypto';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { LlmClient } from '../../src/brain/llm/client';
import { probe } from '../../src/brain/llm/probe';
import { gameTime } from '../../src/core/time';
import type {
  AllowlistAdd, AllowlistEntry, ApiError, AuthAttempt, HealthReport, LlmSettingsUpdate, LlmTestResult, Me, ModelList,
  NewWorldRequest, Role, SpeedRequest, WatchRequest,
} from '../../src/generated/api';
import { migrate } from '../../src/persist/migrations';
import { createState } from '../../src/sim/create';
import { SAVE_VERSION } from '../../src/sim/state';
import { detailOf } from './digest';
import type { LlmConfig, WorldHost } from './host';

/** Who the caller is. There is no sign-in here: REF_ROLE picks admin (default), viewer, or anonymous (every /api call is 401). */
export interface Identity {
  me: Me | null;
}

export function identityFromEnv(role: string | undefined): Identity {
  if (role === 'anonymous') return { me: null };
  const r: Role = role === 'viewer' ? 'viewer' : 'admin';
  return { me: { email: r === 'admin' ? 'dev@localhost' : 'viewer@localhost', displayName: r === 'admin' ? 'Developer' : 'Viewer', role: r } };
}

class HttpError extends Error {
  constructor(readonly status: number, message: string) {
    super(message);
  }
}

const MAX_BODY = 32 * 1024 * 1024;

function json(res: ServerResponse, status: number, body: unknown): void {
  const text = JSON.stringify(body);
  res.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
  res.end(text);
}

function noContent(res: ServerResponse): void {
  res.writeHead(204);
  res.end();
}

async function readJson<T>(req: IncomingMessage): Promise<T> {
  // Mutating requests must be JSON (the real server's CSRF rule); keep the client honest.
  if (!(req.headers['content-type'] ?? '').startsWith('application/json')) throw new HttpError(415, 'Send the body as application/json.');
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of req) {
    size += (chunk as Buffer).length;
    if (size > MAX_BODY) throw new HttpError(413, 'The body is too large.');
    chunks.push(chunk as Buffer);
  }
  const text = Buffer.concat(chunks).toString('utf8');
  if (!text.trim()) return {} as T;
  try {
    return JSON.parse(text) as T;
  } catch {
    throw new HttpError(400, 'The body is not valid JSON.');
  }
}

/** In-memory stand-ins for the auth tables, so the admin panel has something to show. */
const allowlist: AllowlistEntry[] = [
  { email: 'dev@localhost', role: 'admin', addedBy: 'ADMIN_EMAILS', addedAt: Date.now(), note: 'reference server' },
];
const attempts: AuthAttempt[] = [
  { id: 2, provider: 'google', email: 'curious.neighbour@example.com', outcome: 'not_allowed', at: Date.now() - 3_600_000 },
  { id: 1, provider: 'apple', email: 'x7q2@privaterelay.appleid.com', outcome: 'not_allowed', at: Date.now() - 7_200_000 },
];

const percentile = (values: readonly number[], p: number): number => {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return Math.round((sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] as number) * 100) / 100;
};

function llmConfigFrom(update: LlmSettingsUpdate, current: LlmConfig): LlmConfig {
  const positive = (value: unknown, fallback: number): number => (typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : fallback);
  return {
    enabled: Boolean(update.enabled),
    baseUrl: String(update.baseUrl ?? '').trim(),
    model: String(update.model ?? '').trim(),
    apiKey: update.apiKey === undefined ? current.apiKey : String(update.apiKey).trim(),
    concurrency: Math.max(1, Math.min(16, Math.round(positive(update.concurrency, current.concurrency)))),
    protocol: ['auto', 'tools', 'json'].includes(update.protocol) ? update.protocol : current.protocol,
    conversations: ['auto', 'batched', 'turns'].includes(update.conversations) ? update.conversations : current.conversations,
    reasoning: ['off', 'low', 'default'].includes(update.reasoning) ? update.reasoning : current.reasoning,
    temperature: Math.min(2, positive(update.temperature, current.temperature)),
    maxTokens: Math.max(100, Math.round(positive(update.maxTokens, current.maxTokens))),
    headers: String(update.headers ?? ''),
    requestsPerMinute: Math.round(positive(update.requestsPerMinute, current.requestsPerMinute)),
    maxRequestsPerDay: Math.round(positive(update.maxRequestsPerDay, current.maxRequestsPerDay)),
    maxTokensPerDay: Math.round(positive(update.maxTokensPerDay, current.maxTokensPerDay)),
  };
}

export function createHandler(host: WorldHost, identity: Identity) {
  const route = async (req: IncomingMessage, res: ServerResponse): Promise<void> => {
    const url = new URL(req.url ?? '/', 'http://localhost');
    const path = url.pathname;
    const method = req.method ?? 'GET';

    if (path === '/healthz') {
      // Fails when the loop has stopped waking (ticks may legitimately stop while paused).
      const ok = Date.now() - host.lastWakeAt < 5000;
      res.writeHead(ok ? 200 : 503, { 'Content-Type': 'text/plain' });
      res.end(ok ? 'ok' : 'stalled');
      return;
    }

    // Stand-ins for the sign-in pages: there is nothing to sign in to, so go straight back.
    if (path.startsWith('/auth/')) {
      const back = url.searchParams.get('return_to') ?? '/';
      if (path === '/auth/token') {
        res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
        res.end('<!doctype html><meta charset="utf-8"><title>Sign in</title><p>The reference server needs no sign-in. <a href="/">Back to the valley</a></p>');
        return;
      }
      res.writeHead(302, { Location: back.startsWith('/') && !back.startsWith('//') ? back : '/' });
      res.end();
      return;
    }

    if (!path.startsWith('/api/')) throw new HttpError(404, 'Not found.');
    const me = identity.me;
    if (!me) throw new HttpError(401, 'Not signed in.');
    const admin = (): void => {
      if (me.role !== 'admin') throw new HttpError(403, 'Only admins can do that.');
    };

    if (path === '/api/stream' && method === 'GET') {
      res.writeHead(200, {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
        'X-Accel-Buffering': 'no',
        Connection: 'keep-alive',
      });
      res.flushHeaders();
      req.socket.setNoDelay(true);
      const lastEventId = req.headers['last-event-id'];
      host.connect(req, res, url.searchParams.get('viewer') || randomUUID(), me.role, Array.isArray(lastEventId) ? lastEventId[0] : lastEventId);
      return;
    }
    if (path === '/api/me' && method === 'GET') return json(res, 200, me);
    if (path === '/api/logout' && method === 'POST') return noContent(res);
    if (path === '/api/watch' && method === 'POST') {
      const body = await readJson<WatchRequest>(req);
      if (typeof body.viewer !== 'string' || !body.viewer) throw new HttpError(400, 'viewer is required.');
      host.watch(body.viewer, typeof body.id === 'string' ? body.id : null);
      return noContent(res);
    }
    const character = /^\/api\/characters\/([^/]+)$/.exec(path);
    if (character && method === 'GET') {
      const detail = detailOf(host.sim.state, decodeURIComponent(character[1] as string));
      if (!detail) throw new HttpError(404, 'No such villager.');
      return json(res, 200, detail);
    }
    if (path === '/api/chronicle' && method === 'GET') {
      const before = Number(url.searchParams.get('before') ?? Number.MAX_SAFE_INTEGER);
      const limit = Math.max(1, Math.min(500, Number(url.searchParams.get('limit') ?? 100) || 100));
      return json(res, 200, host.chroniclePage(Number.isFinite(before) ? before : Number.MAX_SAFE_INTEGER, limit));
    }

    // ---- admin ----
    if (path.startsWith('/api/admin/')) admin();
    if (path === '/api/admin/speed' && method === 'POST') {
      const body = await readJson<SpeedRequest>(req);
      if (![0, 1, 2, 4].includes(body.speed)) throw new HttpError(400, 'speed must be 0, 1, 2 or 4.');
      host.speed = body.speed;
      return noContent(res);
    }
    if (path === '/api/admin/settings/llm') {
      if (method === 'GET') return json(res, 200, host.llmView());
      if (method === 'PUT') {
        host.configureLlm(llmConfigFrom(await readJson<LlmSettingsUpdate>(req), host.llmConfig));
        return json(res, 200, host.llmView());
      }
    }
    if (path === '/api/admin/llm/test' && method === 'POST') {
      // Tests the draft in the body when there is one (the stored key unless a new one is given), else the stored settings.
      const body = await readJson<Partial<LlmSettingsUpdate>>(req);
      const config = body.baseUrl ? llmConfigFrom(body as LlmSettingsUpdate, host.llmConfig) : host.llmConfig;
      if (!config.baseUrl || !config.model) return json(res, 200, { ok: false, message: 'Set a base URL and a model first.' } satisfies LlmTestResult);
      const result = await probe(config);
      return json(res, 200, { ok: result.ok, message: result.message } satisfies LlmTestResult);
    }
    if (path === '/api/admin/llm/models' && method === 'GET') {
      if (!host.llmConfig.baseUrl) throw new HttpError(400, 'Save a base URL first.');
      try {
        return json(res, 200, { models: await new LlmClient(host.llmConfig).listModels() } satisfies ModelList);
      } catch (error) {
        throw new HttpError(502, error instanceof Error ? error.message : String(error));
      }
    }
    if (path === '/api/admin/brain-log' && method === 'GET') {
      const limit = Math.max(1, Math.min(500, Number(url.searchParams.get('limit') ?? 150) || 150));
      return json(res, 200, host.brainLog.slice(0, limit));
    }
    if (path === '/api/admin/allowlist') {
      if (method === 'GET') return json(res, 200, allowlist);
      if (method === 'POST') {
        const body = await readJson<AllowlistAdd>(req);
        const email = String(body.email ?? '').trim().toLowerCase();
        if (!/^[^@\s]+@[^@\s]+$/.test(email)) throw new HttpError(400, 'That is not an e-mail address.');
        const entry: AllowlistEntry = { email, role: body.role === 'admin' ? 'admin' : 'viewer', addedBy: me.email, addedAt: Date.now(), note: String(body.note ?? '') };
        const index = allowlist.findIndex((row) => row.email === email);
        if (index >= 0) allowlist[index] = entry;
        else allowlist.push(entry);
        return json(res, 201, entry);
      }
      if (method === 'DELETE') {
        // The address goes in the query (?email=); a JSON body {email} is accepted too.
        const email = (url.searchParams.get('email') ?? (await readJson<{ email?: string }>(req).catch(() => ({} as { email?: string }))).email ?? '').trim().toLowerCase();
        const index = allowlist.findIndex((row) => row.email === email);
        if (index < 0) throw new HttpError(404, 'That address is not on the list.');
        allowlist.splice(index, 1);
        return noContent(res);
      }
    }
    if (path === '/api/admin/attempts' && method === 'GET') return json(res, 200, attempts);
    if (path === '/api/admin/world/new' && method === 'POST') {
      const body = await readJson<NewWorldRequest>(req);
      const seed = typeof body.seed === 'number' && Number.isInteger(body.seed) && body.seed >= 0 ? body.seed : Math.floor(Math.random() * 2 ** 31);
      noContent(res);
      host.load(createState(seed), 'new_world');
      return;
    }
    if (path === '/api/admin/world/import' && method === 'POST') {
      const save = migrate(await readJson<unknown>(req));
      if (!save) throw new HttpError(400, 'That file is not a save this version can read.');
      noContent(res);
      host.load(save.state, 'import');
      return;
    }
    if (path === '/api/admin/world/export' && method === 'GET') {
      const state = host.sim.state;
      res.writeHead(200, {
        'Content-Type': 'application/json',
        'Content-Disposition': `attachment; filename="fantasy-sim-world-day${gameTime(state.tick).day}.json"`,
        'Cache-Control': 'no-store',
      });
      res.end(JSON.stringify({ version: SAVE_VERSION, savedAt: Date.now(), state }));
      return;
    }
    if (path === '/api/admin/health' && method === 'GET') {
      const report: HealthReport = {
        tick: host.sim.state.tick,
        tickJitterP50Ms: percentile(host.jitter, 0.5),
        tickJitterP99Ms: percentile(host.jitter, 0.99),
        clockSkips: host.clockSkips,
        backlogDrops: host.backlogDrops,
        viewers: host.viewers.size,
        dbBytes: 0,
        snapshotAgeSeconds: null,
        llm: { requests24h: host.llm.requests, failures24h: host.llm.failures, tokens24h: host.llm.tokens },
      };
      return json(res, 200, report);
    }
    throw new HttpError(404, 'Not found.');
  };

  return (req: IncomingMessage, res: ServerResponse): void => {
    route(req, res).catch((error: unknown) => {
      const status = error instanceof HttpError ? error.status : 500;
      if (status === 500) console.error(error);
      if (res.headersSent) {
        res.end();
        return;
      }
      json(res, status, { error: error instanceof Error ? error.message : String(error) } satisfies ApiError);
    });
  };
}
