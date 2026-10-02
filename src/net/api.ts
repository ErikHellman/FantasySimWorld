import type {
  AllowlistAdd, AllowlistEntry, ApiError, AuthAttempt, BrainLogRecord, CharacterDetail, ChroniclePage, HealthReport,
  LlmSettingsUpdate, LlmSettingsView, LlmTestResult, Me, ModelList, NewWorldRequest, SpeedRequest, WatchRequest,
} from '../generated/api';

/** A failed request: the HTTP status and the server's `ApiError` message. */
export class HttpError extends Error {
  constructor(readonly status: number, message: string) {
    super(message);
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const init: RequestInit = { method, credentials: 'same-origin', headers: { Accept: 'application/json' } };
  if (method !== 'GET') {
    // The server only accepts JSON on mutating routes (part of its CSRF defence), so always send it.
    init.headers = { ...init.headers, 'Content-Type': 'application/json' };
    init.body = JSON.stringify(body ?? {});
  }
  const response = await fetch(path, init);
  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      message = ((await response.json()) as ApiError).error || message;
    } catch {
      // not JSON
    }
    throw new HttpError(response.status, message);
  }
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

/** Typed calls to the server's REST routes. */
export const api = {
  me: () => request<Me>('GET', '/api/me'),
  logout: () => request<void>('POST', '/api/logout'),
  watch: (body: WatchRequest) => request<void>('POST', '/api/watch', body),
  character: (id: string) => request<CharacterDetail>('GET', `/api/characters/${encodeURIComponent(id)}`),
  chronicle: (before: number, limit = 100) => request<ChroniclePage>('GET', `/api/chronicle?before=${before}&limit=${limit}`),
  admin: {
    speed: (body: SpeedRequest) => request<void>('POST', '/api/admin/speed', body),
    llmSettings: () => request<LlmSettingsView>('GET', '/api/admin/settings/llm'),
    saveLlmSettings: (body: LlmSettingsUpdate) => request<LlmSettingsView>('PUT', '/api/admin/settings/llm', body),
    /** tests the draft when given (the stored key unless the draft has one), else the stored settings */
    testLlm: (draft?: LlmSettingsUpdate) => request<LlmTestResult>('POST', '/api/admin/llm/test', draft ?? {}),
    models: () => request<ModelList>('GET', '/api/admin/llm/models'),
    brainLog: () => request<BrainLogRecord[]>('GET', '/api/admin/brain-log'),
    allowlist: () => request<AllowlistEntry[]>('GET', '/api/admin/allowlist'),
    allow: (body: AllowlistAdd) => request<AllowlistEntry>('POST', '/api/admin/allowlist', body),
    disallow: (email: string) => request<void>('DELETE', `/api/admin/allowlist?email=${encodeURIComponent(email)}`),
    attempts: () => request<AuthAttempt[]>('GET', '/api/admin/attempts'),
    newWorld: (body: NewWorldRequest) => request<void>('POST', '/api/admin/world/new', body),
    /** the browser export envelope {version, savedAt, state}, passed through as parsed JSON */
    importWorld: (save: unknown) => request<void>('POST', '/api/admin/world/import', save),
    exportUrl: '/api/admin/world/export',
    health: () => request<HealthReport>('GET', '/api/admin/health'),
  },
};
