import type {
  AllowlistAdd, AllowlistEntry, ApiError, AuthAttempt, BillboardRequest, BrainLogRecord, BuyRequest, CharacterDetail, ChatPost, ChatRename, ChatYou,
  ChroniclePage, CreditGrant, CreditViewer, CreditsBalance, CreditsState, GuestAnswer, GuestRequest, HealthReport,
  LlmSettingsView, LlmTestResult, Me, NewWorldRequest, SignInOptions, SpeedRequest,
  PresenceRequest, WatchRequest,
} from '../generated/api';

/** A failed request: the HTTP status, the server's `ApiError` message, and its `Retry-After`, if it sent one. */
export class HttpError extends Error {
  constructor(readonly status: number, message: string, readonly retryAfterSeconds: number | null = null) {
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
    const retryAfter = Number(response.headers.get('Retry-After'));
    throw new HttpError(response.status, message, retryAfter > 0 ? retryAfter : null);
  }
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

/** Typed calls to the server's REST routes. */
export const api = {
  /** which sign-in buttons to offer; needs no session */
  providers: () => request<SignInOptions>('GET', '/auth/providers'),
  /** a guest seat (setting the guest cookie), or a place in line; ask again no sooner than the answer says */
  guest: (body: GuestRequest) => request<GuestAnswer>('POST', '/auth/guest', body),
  me: () => request<Me>('GET', '/api/me'),
  logout: () => request<void>('POST', '/api/logout'),
  watch: (body: WatchRequest) => request<void>('POST', '/api/watch', body),
  character: (id: string) => request<CharacterDetail>('GET', `/api/characters/${encodeURIComponent(id)}`),
  /** says something in the viewers' chat; it comes back on the stream */
  chat: (body: ChatPost) => request<void>('POST', '/api/chat', body),
  /** takes a chat name, if nobody connected has it */
  chatName: (body: ChatRename) => request<ChatYou>('POST', '/api/chat/name', body),
  /** this viewer's credits and what they buy; `viewer` is the tab's id */
  credits: (viewer: string) => request<CreditsState>('GET', `/api/credits?viewer=${encodeURIComponent(viewer)}`),
  /** reports that this tab is being watched, which earns credits */
  presence: (body: PresenceRequest) => request<CreditsBalance>('POST', '/api/presence', body),
  /** spends credits on something in the world; rejects with the server's reason when it cannot happen */
  buy: (body: BuyRequest) => request<CreditsBalance>('POST', '/api/credits/buy', body),
  chronicle: (before: number, limit = 100) => request<ChroniclePage>('GET', `/api/chronicle?before=${before}&limit=${limit}`),
  admin: {
    speed: (body: SpeedRequest) => request<void>('POST', '/api/admin/speed', body),
    /** puts text up on a billboard, or without text gives it back to the daily slogans; the change comes back on the stream */
    billboard: (body: BillboardRequest) => request<void>('POST', '/api/admin/billboards', body),
    /** everyone with the world open, and their credits */
    creditViewers: () => request<CreditViewer[]>('GET', '/api/admin/credits'),
    /** gives credits to one of them by chat name, or to all with `name: null`; answers with the list as it is now */
    grantCredits: (body: CreditGrant) => request<CreditViewer[]>('POST', '/api/admin/credits/grant', body),
    llmSettings: () => request<LlmSettingsView>('GET', '/api/admin/settings/llm'),
    /** asks the model the server was started with for one decision */
    testLlm: () => request<LlmTestResult>('POST', '/api/admin/llm/test'),
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
