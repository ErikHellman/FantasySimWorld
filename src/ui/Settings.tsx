import { useState } from 'preact/hooks';
import { ENDPOINT_PRESETS, type LlmSettings, type Settings } from '../persist/settings';
import { actions, ui } from './store';

export function SettingsPanel() {
  const [draft, setDraft] = useState<Settings>(() => structuredClone(ui.settings.value));
  const [test, setTest] = useState<{ busy: boolean; message: string }>({ busy: false, message: '' });
  const [models, setModels] = useState<string[]>([]);
  const [note, setNote] = useState('');
  const llm = draft.llm;

  const setLlm = (patch: Partial<LlmSettings>): void => setDraft({ ...draft, llm: { ...draft.llm, ...patch } });
  const setAudio = (patch: Partial<Settings['audio']>): void => {
    const next = { ...draft, audio: { ...draft.audio, ...patch } };
    setDraft(next);
    actions.applySettings({ ...ui.settings.value, audio: next.audio });
  };

  const runTest = async (): Promise<void> => {
    setTest({ busy: true, message: 'Asking the model to make a decision…' });
    setTest({ busy: false, message: await actions.testConnection(draft) });
  };

  const fetchModels = async (): Promise<void> => {
    setTest({ busy: true, message: 'Fetching the model list…' });
    const list = await actions.listModels(draft);
    setModels(list);
    setTest({ busy: false, message: list.length > 0 ? `${list.length} models available.` : 'The server did not return a model list. Type the model name by hand.' });
    if (list.length > 0 && !llm.model) setLlm({ model: list[0] as string });
  };

  const save = (): void => {
    actions.applySettings(draft);
    ui.panel.value = 'none';
  };

  return (
    <div class="modal" onClick={() => (ui.panel.value = 'none')}>
      <section class="panel dialog" onClick={(event) => event.stopPropagation()}>
        <button class="close" title="Close" onClick={() => (ui.panel.value = 'none')}>×</button>
        <h2>Settings</h2>

        <h3>Language model</h3>
        <p class="muted">
          Any OpenAI-compatible chat completions endpoint that supports tool calling. Requests go straight from this browser
          to the endpoint; the key is stored only in this browser.
        </p>
        <label class="check">
          <input type="checkbox" checked={llm.enabled} onChange={(event) => setLlm({ enabled: event.currentTarget.checked })} />
          Let a language model make the villagers' decisions
        </label>
        <div class="presets">
          {ENDPOINT_PRESETS.map((preset) => (
            <button
              class={llm.baseUrl === preset.baseUrl ? 'on' : ''}
              onClick={() => {
                setLlm({ baseUrl: preset.baseUrl, concurrency: preset.concurrency, headers: preset.headers, model: llm.baseUrl === preset.baseUrl ? llm.model : '' });
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
          <input type="text" value={llm.baseUrl} placeholder="https://…/v1" onInput={(event) => setLlm({ baseUrl: event.currentTarget.value.trim() })} />
        </label>
        <label>
          API key
          <input type="password" value={llm.apiKey} placeholder="not needed for local models" autocomplete="off" onInput={(event) => setLlm({ apiKey: event.currentTarget.value.trim() })} />
        </label>
        <label>
          Model
          <span class="row">
            <input type="text" list="model-list" value={llm.model} placeholder="model name" onInput={(event) => setLlm({ model: event.currentTarget.value.trim() })} />
            <button onClick={() => void fetchModels()} disabled={!llm.baseUrl || test.busy}>List models</button>
          </span>
          <datalist id="model-list">{models.map((model) => <option value={model} />)}</datalist>
        </label>
        <div class="grid">
          <label>
            Requests at once
            <input type="number" min="1" max="16" value={llm.concurrency} onInput={(event) => setLlm({ concurrency: Math.max(1, Math.min(16, Number(event.currentTarget.value) || 1)) })} />
          </label>
          <label>
            Max requests / minute
            <input type="number" min="0" max="600" value={llm.requestsPerMinute} title="0 means no cap" onInput={(event) => setLlm({ requestsPerMinute: Math.max(0, Number(event.currentTarget.value) || 0) })} />
          </label>
          <label>
            Tool calls
            <select value={llm.protocol} onChange={(event) => setLlm({ protocol: event.currentTarget.value as LlmSettings['protocol'] })}>
              <option value="auto">Detect</option>
              <option value="tools">Native tool calling</option>
              <option value="json">JSON schema output</option>
            </select>
          </label>
          <label>
            Conversations
            <select value={llm.conversations} onChange={(event) => setLlm({ conversations: event.currentTarget.value as LlmSettings['conversations'] })}>
              <option value="auto">Choose by speed</option>
              <option value="batched">One call per conversation</option>
              <option value="turns">Turn by turn</option>
            </select>
          </label>
          <label>
            Model reasoning
            <select value={llm.reasoning} title="Models that think before answering are much slower. Off asks them not to." onChange={(event) => setLlm({ reasoning: event.currentTarget.value as LlmSettings['reasoning'] })}>
              <option value="off">Off (fastest)</option>
              <option value="low">Low</option>
              <option value="default">Model default</option>
            </select>
          </label>
          <label>
            Temperature
            <input type="number" min="0" max="2" step="0.1" value={llm.temperature} onInput={(event) => setLlm({ temperature: Number(event.currentTarget.value) })} />
          </label>
          <label>
            Max output tokens
            <input type="number" min="100" max="4000" step="50" value={llm.maxTokens} onInput={(event) => setLlm({ maxTokens: Math.max(100, Number(event.currentTarget.value) || 500) })} />
          </label>
        </div>
        <label>
          Extra headers <span class="muted">(one “Name: value” per line)</span>
          <textarea rows={2} value={llm.headers} onInput={(event) => setLlm({ headers: event.currentTarget.value })} />
        </label>
        <div class="row">
          <button onClick={() => void runTest()} disabled={!llm.baseUrl || !llm.model || test.busy}>Test connection</button>
          <span class="test-result">{test.message}</span>
        </div>

        <h3>Sound</h3>
        <label class="check">
          <input type="checkbox" checked={draft.audio.muted} onChange={(event) => setAudio({ muted: event.currentTarget.checked })} />
          Mute
        </label>
        <div class="grid">
          <label>Master<input type="range" min="0" max="1" step="0.05" value={draft.audio.master} onInput={(event) => setAudio({ master: Number(event.currentTarget.value) })} /></label>
          <label>Ambience<input type="range" min="0" max="1" step="0.05" value={draft.audio.ambient} onInput={(event) => setAudio({ ambient: Number(event.currentTarget.value) })} /></label>
          <label>Effects<input type="range" min="0" max="1" step="0.05" value={draft.audio.effects} onInput={(event) => setAudio({ effects: Number(event.currentTarget.value) })} /></label>
        </div>
        <details>
          <summary>
            Sound files: {ui.audioSlots.value.filter((slot) => slot.loaded).length} of {ui.audioSlots.value.length} found
          </summary>
          <p class="muted">Put audio files in <code>public/audio/</code> named after the slot (for example <code>birds_day.ogg</code>). Missing slots are silent.</p>
          <ul class="slots">
            {ui.audioSlots.value.map((slot) => (
              <li class={slot.loaded ? 'ok' : ''}>{slot.loaded ? '●' : '○'} <code>{slot.id}</code> <span class="muted">{slot.label}</span></li>
            ))}
          </ul>
        </details>

        <h3>World</h3>
        <p class="muted">
          {ui.canSave.value
            ? ui.savedAt.value ? `Saved in this browser at ${new Date(ui.savedAt.value).toLocaleTimeString()}.` : 'The world is saved in this browser automatically.'
            : 'This world is open in another tab, so this tab does not save.'}
        </p>
        <div class="row">
          <button onClick={() => actions.exportSave()}>Export save</button>
          <label class="file-button">
            Import save
            <input type="file" accept="application/json,.json" onChange={(event) => {
              const file = event.currentTarget.files?.[0];
              if (file) actions.importSave(file);
            }} />
          </label>
          <button class="danger" onClick={() => {
            if (confirm('Start a new world? The current one will be lost.')) actions.resetWorld();
          }}>New world</button>
        </div>

        <footer>
          <button onClick={() => (ui.panel.value = 'none')}>Cancel</button>
          <button class="primary" onClick={save}>Save settings</button>
        </footer>
      </section>
    </div>
  );
}
