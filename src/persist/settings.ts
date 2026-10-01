export type WireProtocol = 'auto' | 'tools' | 'json';
export type ConversationMode = 'auto' | 'batched' | 'turns';

export interface LlmSettings {
  enabled: boolean;
  /** OpenAI-compatible base URL, e.g. https://api.openai.com/v1 */
  baseUrl: string;
  apiKey: string;
  model: string;
  /** requests in flight at once; 1 suits a local model */
  concurrency: number;
  protocol: WireProtocol;
  conversations: ConversationMode;
  temperature: number;
  maxTokens: number;
  /** extra request headers, one "Name: value" per line */
  headers: string;
  /** hard cap on requests per real minute (0 = no cap) */
  requestsPerMinute: number;
}

export interface AudioSettings {
  muted: boolean;
  master: number;
  ambient: number;
  effects: number;
}

export interface Settings {
  llm: LlmSettings;
  audio: AudioSettings;
}

export const DEFAULT_SETTINGS: Settings = {
  llm: {
    enabled: false,
    baseUrl: '',
    apiKey: '',
    model: '',
    concurrency: 2,
    protocol: 'auto',
    conversations: 'auto',
    temperature: 0.8,
    maxTokens: 500,
    headers: '',
    requestsPerMinute: 0,
  },
  audio: { muted: false, master: 0.8, ambient: 0.7, effects: 0.8 },
};

export interface EndpointPreset {
  label: string;
  baseUrl: string;
  concurrency: number;
  headers: string;
  note: string;
}

/** Starting points for the endpoint form. The model is picked from the server's own list. */
export const ENDPOINT_PRESETS: EndpointPreset[] = [
  { label: 'Ollama (local)', baseUrl: 'http://localhost:11434/v1', concurrency: 1, headers: '', note: 'No key needed. Open the app from localhost so Ollama accepts the requests, and use a model that supports tools.' },
  { label: 'LM Studio (local)', baseUrl: 'http://localhost:1234/v1', concurrency: 1, headers: '', note: 'No key needed. Turn on CORS in LM Studio\'s server settings.' },
  { label: 'OpenAI', baseUrl: 'https://api.openai.com/v1', concurrency: 4, headers: '', note: 'Needs an API key.' },
  { label: 'OpenRouter', baseUrl: 'https://openrouter.ai/api/v1', concurrency: 4, headers: '', note: 'Needs an API key.' },
  { label: 'Anthropic', baseUrl: 'https://api.anthropic.com/v1', concurrency: 4, headers: 'anthropic-dangerous-direct-browser-access: true', note: 'Needs an API key. Uses Anthropic\'s OpenAI-compatible endpoint.' },
];

/** Fills in anything missing from stored settings (older saves, new fields). */
export function normalizeSettings(stored: unknown): Settings {
  const value = (stored ?? {}) as Partial<Settings>;
  return {
    llm: { ...DEFAULT_SETTINGS.llm, ...(value.llm ?? {}) },
    audio: { ...DEFAULT_SETTINGS.audio, ...(value.audio ?? {}) },
  };
}

export function parseHeaders(text: string): Record<string, string> {
  const headers: Record<string, string> = {};
  for (const line of text.split('\n')) {
    const index = line.indexOf(':');
    if (index <= 0) continue;
    const name = line.slice(0, index).trim();
    const value = line.slice(index + 1).trim();
    if (name && value) headers[name] = value;
  }
  return headers;
}
