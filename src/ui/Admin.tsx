import { useEffect, useState } from 'preact/hooks';
import type {
  AllowlistEntry, AuthAttempt, HealthReport, LlmSettingsUpdate, LlmSettingsView, Role,
} from '../generated/api';
import { api } from '../net/api';
import { actions, ui } from './store';

const SPEEDS = [
  { value: 0, label: 'Paused' },
  { value: 1, label: '1×' },
  { value: 2, label: '2×' },
  { value: 4, label: '4×' },
];

/** Starting points for the endpoint form; the server makes the requests, so no CORS settings are needed. */
const PRESETS = [
  { label: 'Ollama', baseUrl: 'http://localhost:11434/v1', concurrency: 1, note: 'On the server\'s machine. No key needed; use a model that supports tools.' },
  { label: 'LM Studio', baseUrl: 'http://localhost:1234/v1', concurrency: 1, note: 'On the server\'s machine. No key needed.' },
  { label: 'OpenAI', baseUrl: 'https://api.openai.com/v1', concurrency: 4, note: 'Needs an API key.' },
  { label: 'OpenRouter', baseUrl: 'https://openrouter.ai/api/v1', concurrency: 4, note: 'Needs an API key.' },
  { label: 'Anthropic', baseUrl: 'https://api.anthropic.com/v1', concurrency: 4, note: 'Needs an API key. Uses Anthropic\'s OpenAI-compatible endpoint.' },
];

const errorText = (error: unknown): string => (error instanceof Error ? error.message : String(error));

function draftFrom(view: LlmSettingsView): LlmSettingsUpdate {
  const { apiKeySet: _set, apiKeyHint: _hint, ...rest } = view;
  return rest;
}

const formatBytes = (bytes: number): string => (bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`);

function WorldSection() {
  const [health, setHealth] = useState<HealthReport | null>(null);
  const [seed, setSeed] = useState('');
  const [message, setMessage] = useState('');
  const refresh = (): void => {
    api.admin.health().then(setHealth, (error) => setMessage(errorText(error)));
  };
  useEffect(refresh, []);

  const importFile = async (file: File): Promise<void> => {
    let save: unknown;
    try {
      save = JSON.parse(await file.text());
    } catch {
      setMessage('That file is not JSON.');
      return;
    }
    if (!confirm('Replace the running world with this save? Everyone watching will reload.')) return;
    setMessage('Importing…');
    // On success the server sends world_reset and every page, this one included, reloads.
    api.admin.importWorld(save).catch((error) => setMessage(errorText(error)));
  };

  const newWorld = (): void => {
    if (!confirm('Start a new world? The current one will be lost unless you export it first.')) return;
    const value = seed.trim() === '' ? undefined : Number(seed);
    if (value !== undefined && (!Number.isInteger(value) || value < 0)) {
      setMessage('The seed must be a whole number.');
      return;
    }
    api.admin.newWorld(value === undefined ? {} : { seed: value }).catch((error) => setMessage(errorText(error)));
  };

  return (
    <>
      <h3>World</h3>
      <div class="row">
        <span class="muted">Speed for everyone:</span>
        {SPEEDS.map((speed) => (
          <button class={ui.speed.value === speed.value ? 'on' : ''} onClick={() => actions.setSpeed(speed.value)}>{speed.label}</button>
        ))}
      </div>
      <div class="row">
        <a class="file-button" href={api.admin.exportUrl} download>Export save</a>
        <label class="file-button">
          Import save
          <input type="file" accept="application/json,.json" onChange={(event) => {
            const file = event.currentTarget.files?.[0];
            if (file) void importFile(file);
          }} />
        </label>
        <input type="text" placeholder="seed (random)" value={seed} style={{ maxWidth: '130px' }} onInput={(event) => setSeed(event.currentTarget.value)} />
        <button class="danger" onClick={newWorld}>New world</button>
      </div>
      {message && <p class="test-result">{message}</p>}
      <details>
        <summary>Server health</summary>
        {health ? (
          <ul class="slots">
            <li>Tick {health.tick.toLocaleString()} · jitter p50 {health.tickJitterP50Ms} ms, p99 {health.tickJitterP99Ms} ms</li>
            <li>{health.clockSkips} clock skips · {health.backlogDrops} viewers dropped for falling behind · {health.viewers} watching</li>
            <li>Database {formatBytes(health.dbBytes)} · last snapshot {health.snapshotAgeSeconds === null ? 'none' : `${Math.round(health.snapshotAgeSeconds)} s ago`}</li>
            <li>Model, last 24 h: {health.llm.requests24h} requests, {health.llm.failures24h} failed, {health.llm.tokens24h.toLocaleString()} tokens</li>
          </ul>
        ) : <p class="muted">…</p>}
        <button onClick={refresh}>Refresh</button>
      </details>
    </>
  );
}

function LlmSection() {
  const [view, setView] = useState<LlmSettingsView | null>(null);
  const [draft, setDraft] = useState<LlmSettingsUpdate | null>(null);
  const [message, setMessage] = useState('');
  const [busy, setBusy] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [note, setNote] = useState('');

  useEffect(() => {
    api.admin.llmSettings().then((loaded) => {
      setView(loaded);
      setDraft(draftFrom(loaded));
    }, (error) => setMessage(errorText(error)));
  }, []);
  if (!draft || !view) return <><h3>Language model</h3><p class="muted">{message || 'Loading…'}</p></>;

  const set = (patch: Partial<LlmSettingsUpdate>): void => setDraft({ ...draft, ...patch });
  const number = (value: string, min: number, max: number, fallback: number): number => {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? Math.max(min, Math.min(max, parsed)) : fallback;
  };
  const run = async (label: string, work: () => Promise<string>): Promise<void> => {
    setBusy(true);
    setMessage(label);
    try {
      setMessage(await work());
    } catch (error) {
      setMessage(errorText(error));
    } finally {
      setBusy(false);
    }
  };
  const save = (): Promise<void> => run('Saving…', async () => {
    const saved = await api.admin.saveLlmSettings(draft);
    setView(saved);
    setDraft(draftFrom(saved));
    return saved.enabled ? 'Saved. The villagers now think with this model.' : 'Saved. The villagers follow their routines.';
  });
  const test = (): Promise<void> => run('Asking the model to make a decision…', async () => (await api.admin.testLlm(draft)).message);
  const listModels = (): Promise<void> => run('Fetching the model list…', async () => {
    const list = (await api.admin.models()).models;
    setModels(list);
    return list.length > 0 ? `${list.length} models available.` : 'The server returned no models. Type the name by hand.';
  });

  return (
    <>
      <h3>Language model</h3>
      <p class="muted">
        Any OpenAI-compatible chat completions endpoint that supports tool calling. The server makes the requests; the key
        stays on the server and is never shown again.
      </p>
      <label class="check">
        <input type="checkbox" checked={draft.enabled} onChange={(event) => set({ enabled: event.currentTarget.checked })} />
        Let a language model make the villagers' decisions
      </label>
      <div class="presets">
        {PRESETS.map((preset) => (
          <button
            class={draft.baseUrl === preset.baseUrl ? 'on' : ''}
            onClick={() => {
              set({ baseUrl: preset.baseUrl, concurrency: preset.concurrency, model: draft.baseUrl === preset.baseUrl ? draft.model : '' });
              setNote(preset.note);
              setModels([]);
            }}
          >
            {preset.label}
          </button>
        ))}
      </div>
      {note && <p class="muted">{note}</p>}
      <label>
        Base URL
        <input type="text" value={draft.baseUrl} placeholder="https://…/v1" onInput={(event) => set({ baseUrl: event.currentTarget.value.trim() })} />
      </label>
      <label>
        API key
        <span class="row">
          <input
            type="password"
            autocomplete="off"
            value={draft.apiKey ?? ''}
            placeholder={view.apiKeySet ? `stored (${view.apiKeyHint ?? 'set'}); leave empty to keep it` : 'not needed for local models'}
            onInput={(event) => {
              const value = event.currentTarget.value.trim();
              const next = { ...draft };
              if (value) next.apiKey = value;
              else delete next.apiKey;
              setDraft(next);
            }}
          />
          {view.apiKeySet && <button onClick={() => set({ apiKey: '' })} title="Remove the stored key when saving">Clear key</button>}
        </span>
      </label>
      <label>
        Model
        <span class="row">
          <input type="text" list="model-list" value={draft.model} placeholder="model name" onInput={(event) => set({ model: event.currentTarget.value.trim() })} />
          <button onClick={() => void listModels()} disabled={busy} title="Uses the saved base URL and key">List models</button>
        </span>
        <datalist id="model-list">{models.map((model) => <option value={model} />)}</datalist>
      </label>
      <div class="grid">
        <label>
          Requests at once
          <input type="number" min="1" max="16" value={draft.concurrency} onInput={(event) => set({ concurrency: number(event.currentTarget.value, 1, 16, 1) })} />
        </label>
        <label>
          Max requests / minute
          <input type="number" min="0" max="600" value={draft.requestsPerMinute} title="0 means no cap" onInput={(event) => set({ requestsPerMinute: number(event.currentTarget.value, 0, 600, 0) })} />
        </label>
        <label>
          Max requests / day
          <input type="number" min="0" value={draft.maxRequestsPerDay} title="Spread through the day; 0 means no cap" onInput={(event) => set({ maxRequestsPerDay: number(event.currentTarget.value, 0, 10_000_000, 0) })} />
        </label>
        <label>
          Max tokens / day
          <input type="number" min="0" value={draft.maxTokensPerDay} title="Spread through the day; 0 means no cap" onInput={(event) => set({ maxTokensPerDay: number(event.currentTarget.value, 0, 1e12, 0) })} />
        </label>
        <label>
          Tool calls
          <select value={draft.protocol} onChange={(event) => set({ protocol: event.currentTarget.value as LlmSettingsUpdate['protocol'] })}>
            <option value="auto">Detect</option>
            <option value="tools">Native tool calling</option>
            <option value="json">JSON schema output</option>
          </select>
        </label>
        <label>
          Conversations
          <select value={draft.conversations} onChange={(event) => set({ conversations: event.currentTarget.value as LlmSettingsUpdate['conversations'] })}>
            <option value="auto">Choose by speed</option>
            <option value="batched">One call per conversation</option>
            <option value="turns">Turn by turn</option>
          </select>
        </label>
        <label>
          Model reasoning
          <select value={draft.reasoning} title="Models that think before answering are much slower. Off asks them not to." onChange={(event) => set({ reasoning: event.currentTarget.value as LlmSettingsUpdate['reasoning'] })}>
            <option value="off">Off (fastest)</option>
            <option value="low">Low</option>
            <option value="default">Model default</option>
          </select>
        </label>
        <label>
          Temperature
          <input type="number" min="0" max="2" step="0.1" value={draft.temperature} onInput={(event) => set({ temperature: number(event.currentTarget.value, 0, 2, 0.8) })} />
        </label>
        <label>
          Max output tokens
          <input type="number" min="100" max="4000" step="50" value={draft.maxTokens} onInput={(event) => set({ maxTokens: number(event.currentTarget.value, 100, 4000, 600) })} />
        </label>
      </div>
      <label>
        Extra headers <span class="muted">(one “Name: value” per line)</span>
        <textarea rows={2} value={draft.headers} onInput={(event) => set({ headers: event.currentTarget.value })} />
      </label>
      <div class="row">
        <button onClick={() => void test()} disabled={busy || !draft.baseUrl || !draft.model}>Test connection</button>
        <button class="primary" onClick={() => void save()} disabled={busy}>Save model settings</button>
        <span class="test-result">{message}</span>
      </div>
    </>
  );
}

function AccessSection() {
  const [list, setList] = useState<AllowlistEntry[]>([]);
  const [attempts, setAttempts] = useState<AuthAttempt[]>([]);
  const [email, setEmail] = useState('');
  const [role, setRole] = useState<Role>('viewer');
  const [message, setMessage] = useState('');
  const reload = (): void => {
    api.admin.allowlist().then(setList, (error) => setMessage(errorText(error)));
    api.admin.attempts().then(setAttempts, (error) => setMessage(errorText(error)));
  };
  useEffect(reload, []);
  const allow = (address: string, as: Role, note = ''): void => {
    api.admin.allow({ email: address, role: as, note }).then(() => {
      setEmail('');
      setMessage(`${address} may now sign in.`);
      reload();
    }, (error) => setMessage(errorText(error)));
  };
  const remove = (address: string): void => {
    if (!confirm(`Remove ${address}? Their sessions end at once.`)) return;
    api.admin.disallow(address).then(reload, (error) => setMessage(errorText(error)));
  };
  const allowed = new Set(list.map((entry) => entry.email.toLowerCase()));

  return (
    <>
      <h3>Who may watch</h3>
      <ul class="graves">
        {list.map((entry) => (
          <li key={entry.email} class="row">
            <strong>{entry.email}</strong>
            <span class={`badge ${entry.role === 'admin' ? 'good' : ''}`}>{entry.role}</span>
            {entry.note && <span class="muted">{entry.note}</span>}
            <span class="test-result" />
            <button onClick={() => remove(entry.email)}>Remove</button>
          </li>
        ))}
      </ul>
      <div class="row">
        <input type="text" placeholder="e-mail address" value={email} onInput={(event) => setEmail(event.currentTarget.value.trim())} />
        <select value={role} onChange={(event) => setRole(event.currentTarget.value as Role)} style={{ maxWidth: '110px' }}>
          <option value="viewer">viewer</option>
          <option value="admin">admin</option>
        </select>
        <button onClick={() => allow(email, role)} disabled={!email.includes('@')}>Add</button>
      </div>
      {message && <p class="test-result">{message}</p>}
      <details>
        <summary>Refused sign-ins ({attempts.length})</summary>
        <ul class="graves">
          {attempts.length === 0 && <li class="muted">None.</li>}
          {attempts.map((attempt) => (
            <li key={attempt.id} class="row">
              <strong>{attempt.email || '(no e-mail)'}</strong>
              <span class="muted">{attempt.provider} · {new Date(attempt.at).toLocaleString()}</span>
              <span class="test-result" />
              {attempt.email && (allowed.has(attempt.email.toLowerCase())
                ? <span class="badge good">allowed</span>
                : <button onClick={() => allow(attempt.email, 'viewer', `allowed after a ${attempt.provider} sign-in`)}>Allow</button>)}
            </li>
          ))}
        </ul>
      </details>
    </>
  );
}

/** Running the shared world: speed, saves, the language model and who may watch. Admins only. */
export function AdminPanel() {
  return (
    <div class="modal" onClick={() => (ui.panel.value = 'none')}>
      <section class="panel dialog" onClick={(event) => event.stopPropagation()}>
        <button class="close" title="Close" onClick={() => (ui.panel.value = 'none')}>×</button>
        <h2>Admin</h2>
        <WorldSection />
        <LlmSection />
        <AccessSection />
        <footer>
          <button class="primary" onClick={() => (ui.panel.value = 'none')}>Done</button>
        </footer>
      </section>
    </div>
  );
}
