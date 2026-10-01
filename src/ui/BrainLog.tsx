import { useState } from 'preact/hooks';
import { ui } from './store';

/** Every language-model request: what was asked, what came back, what was done with it. */
export function BrainLog() {
  const [openId, setOpenId] = useState<number | null>(null);
  const llm = ui.llm.value;
  const entries = ui.brainLog.value;
  return (
    <div class="modal" onClick={() => (ui.panel.value = 'none')}>
      <section class="panel dialog wide" onClick={(event) => event.stopPropagation()}>
        <button class="close" title="Close" onClick={() => (ui.panel.value = 'none')}>×</button>
        <h2>Decision log</h2>
        <p class="muted">
          {llm.state === 'off'
            ? 'No language model is connected; villagers are following their built-in routines.'
            : `${llm.requests} requests · ${llm.failures} failed · ${llm.tokens.toLocaleString()} tokens · about ${Math.round(llm.latencyMs)} ms each · ${llm.protocol || 'protocol not yet chosen'}`}
          {llm.message && <><br />{llm.message}</>}
        </p>
        <ol class="brainlog">
          {entries.length === 0 && <li class="muted">Nothing yet.</li>}
          {entries.map((entry) => (
            <li class={`brain ${entry.status}`} key={entry.id}>
              <button class="brain-head" onClick={() => setOpenId(openId === entry.id ? null : entry.id)}>
                <span class="brain-time">{entry.time}</span>
                <span class="brain-who">{entry.character}</span>
                <span class="badge">{entry.kind}</span>
                <span class={`badge ${entry.status === 'ok' ? 'good' : 'bad'}`}>{entry.status}</span>
                <span class="brain-summary">{entry.summary}</span>
                <span class="muted">{Math.round(entry.latencyMs)} ms{entry.tokens ? ` · ${entry.tokens} tok` : ''}</span>
              </button>
              {openId === entry.id && (
                <div class="brain-body">
                  <h4>Prompt</h4>
                  <pre>{entry.prompt}</pre>
                  <h4>Response</h4>
                  <pre>{entry.response}</pre>
                </div>
              )}
            </li>
          ))}
        </ol>
      </section>
    </div>
  );
}
