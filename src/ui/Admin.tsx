import { useEffect, useState } from 'preact/hooks';
import type {
  AllowlistEntry, AuthAttempt, HealthReport, LlmSettingsView, Role,
} from '../generated/api';
import { api } from '../net/api';
import { actions, ui } from './store';

const SPEEDS = [
  { value: 0, label: 'Paused' },
  { value: 1, label: '1×' },
  { value: 2, label: '2×' },
  { value: 4, label: '4×' },
];

const errorText = (error: unknown): string => (error instanceof Error ? error.message : String(error));

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

const PROTOCOLS: Record<LlmSettingsView['protocol'], string> = { auto: 'detect', tools: 'native tool calling', json: 'JSON schema output' };
const CONVERSATIONS: Record<LlmSettingsView['conversations'], string> = { auto: 'chosen by speed', batched: 'one call per conversation', turns: 'turn by turn' };
const REASONING: Record<LlmSettingsView['reasoning'], string> = { off: 'off', low: 'low', default: 'model default' };
const cap = (value: number): string => (value === 0 ? 'no cap' : value.toLocaleString());

/** Read-only: the model is set with LLM_* environment variables on the server, never from here. */
function LlmSection() {
  const [view, setView] = useState<LlmSettingsView | null>(null);
  const [message, setMessage] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.admin.llmSettings().then(setView, (error) => setMessage(errorText(error)));
  }, []);
  if (!view) return <><h3>Language model</h3><p class="muted">{message || 'Loading…'}</p></>;

  const test = async (): Promise<void> => {
    setBusy(true);
    setMessage('Asking the model to make a decision…');
    try {
      setMessage((await api.admin.testLlm()).message);
    } catch (error) {
      setMessage(errorText(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <h3>Language model</h3>
      <p class="muted">
        Set on the server with the <code>LLM_*</code> environment variables (see the README) and read at start. Nothing here
        can change them.
      </p>
      {view.enabled ? (
        <ul class="slots">
          <li>{view.model} at {view.baseUrl}</li>
          <li>API key: {view.apiKeySet ? `set (${view.apiKeyHint ?? '…'})` : 'none'}{view.headers && ` · extra headers: ${view.headers.split('\n').join(', ')}`}</li>
          <li>{view.concurrency} at once · per minute {cap(view.requestsPerMinute)} · per day {cap(view.maxRequestsPerDay)} requests, {cap(view.maxTokensPerDay)} tokens</li>
          <li>Tool calls {PROTOCOLS[view.protocol]} · conversations {CONVERSATIONS[view.conversations]} · reasoning {REASONING[view.reasoning]}</li>
          <li>Temperature {view.temperature} · up to {view.maxTokens} output tokens</li>
        </ul>
      ) : (
        <p>Off: the villagers follow their built-in routines. Set <code>LLM_BASE_URL</code> and <code>LLM_MODEL</code> on the server and restart it to turn the model on.</p>
      )}
      <div class="row">
        <button onClick={() => void test()} disabled={busy || !view.enabled}>Test connection</button>
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
