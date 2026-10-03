import { useState } from 'preact/hooks';
import { formatClock, gameTime } from '../core/time';
import { api } from '../net/api';
import { actions, closeSheet, isAdmin, togglePanel, ui } from './store';

// The panes and controls both layouts show: the desktop places them around the edges, a phone one at a time.

const SPEEDS = [
  { value: 0, label: '❚❚', title: 'Pause (space)' },
  { value: 1, label: '1×', title: 'Normal speed (1)' },
  { value: 2, label: '2×', title: 'Double speed (2)' },
  { value: 4, label: '4×', title: 'Four times speed (3)' },
];

// ️ asks for the colour emoji form; without it some of these fall back to plain text glyphs.
const KIND_ICON: Record<string, string> = {
  world: '☀️', social: '💬', romance: '❤️', conflict: '⚡', combat: '⚔️', death: '🪦', arrival: '🚶',
};

const MOOD_ICON: Record<string, string> = { ok: '', fight: '⚔️', sleep: '💤', talk: '💬', hurt: '🩹' };

/** Follows a villager picked from a list; on a phone the pane goes away so they can be watched. */
const follow = (id: string): void => {
  actions.select(id);
  if (ui.mobile.value) closeSheet();
};

/** The admin's speed buttons. */
export function Speeds() {
  return (
    <div class="speeds" role="group" aria-label="Simulation speed">
      {SPEEDS.map((speed) => (
        <button class={ui.speed.value === speed.value ? 'on' : ''} title={speed.title} onClick={() => actions.setSpeed(speed.value)}>
          {speed.label}
        </button>
      ))}
    </div>
  );
}

/** The language model's state; admins open the decision log from it. */
export function LlmPill() {
  const llm = ui.llm.value;
  const admin = isAdmin();
  return (
    <button
      class={`pill llm ${llm.state}`}
      title={llm.message || (admin ? 'Language model status. Click for the decision log.' : 'Language model status')}
      disabled={!admin}
      onClick={() => togglePanel('brain')}
    >
      🧠 {llm.state === 'off' ? 'local minds' : llm.state === 'error' ? 'LLM error' : llm.inFlight + llm.queued > 0 ? `thinking ${llm.inFlight}+${llm.queued}` : 'LLM ready'}
    </button>
  );
}

export function Roster() {
  const selected = ui.selectedId.value;
  // On the desktop a second click on the followed villager lets them go; a phone has the follow chip for that.
  const pick = (id: string): void => (selected === id && !ui.mobile.value ? actions.select(null) : follow(id));
  return (
    <aside class="panel roster">
      <h2>Villagers</h2>
      <ul>
        {ui.roster.value.map((entry) => (
          <li>
            <button class={`roster-row ${selected === entry.id ? 'on' : ''}`} onClick={() => pick(entry.id)}>
              <span class="roster-name">
                {entry.name.split(' ')[0]} {entry.partner && <span class="love">♥</span>} <span class="mood">{MOOD_ICON[entry.mood]}</span>
              </span>
              <span class="roster-status">{entry.status}</span>
              <span class="hpbar"><span style={{ width: `${Math.round(Math.max(0, entry.hp) * 100)}%` }} class={entry.hp < 0.4 ? 'low' : ''} /></span>
            </button>
          </li>
        ))}
      </ul>
    </aside>
  );
}

function Voices() {
  return (
    <ol>
      {ui.voices.value.length === 0 && <li class="muted chron">Nobody has spoken yet.</li>}
      {ui.voices.value.map((line) => {
        const time = gameTime(line.tick);
        return (
          <li class="chron voice" key={line.id}>
            <span class="chron-time">{formatClock(time.minuteOfDay)}</span>
            <span class="chron-icon">{line.loud ? '📣' : '💬'}</span>
            <span class="chron-text">
              <button class="link" onClick={() => follow(line.speakerId)}>{line.speaker}</button>
              {line.to && <span class="muted"> to {line.to}</span>}: {line.text}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

const PAGE = 40;

export function Chronicle() {
  const [tab, setTab] = useState<'chronicle' | 'voices'>('chronicle');
  const [shown, setShown] = useState(PAGE);
  const [busy, setBusy] = useState(false);
  const all = ui.chronicle.value;
  const entries = all.slice(-shown).reverse();
  const earlier = async (): Promise<void> => {
    if (all.length < shown + PAGE && ui.chronicleMore.value) {
      setBusy(true);
      try {
        const page = await api.chronicle(all[0]?.tick ?? Number.MAX_SAFE_INTEGER, PAGE * 2);
        ui.chronicle.value = [...page.entries, ...ui.chronicle.value];
        ui.chronicleMore.value = page.more;
      } catch (error) {
        ui.notice.value = `Could not load earlier entries: ${String(error)}`;
      } finally {
        setBusy(false);
      }
    }
    setShown(shown + PAGE);
  };
  return (
    <aside class="panel chronicle">
      <nav class="feed-tabs">
        <button class={tab === 'chronicle' ? 'on' : ''} onClick={() => setTab('chronicle')}>Chronicle</button>
        <button class={tab === 'voices' ? 'on' : ''} onClick={() => setTab('voices')}>Voices</button>
      </nav>
      {tab === 'voices' ? <Voices /> : (
      <ol>
        {entries.map((entry) => {
          const time = gameTime(entry.tick);
          const id = entry.ids[0];
          return (
            <li class={`chron ${entry.kind}`} key={`${entry.tick}:${entry.text}`}>
              <span class="chron-time">D{time.day} {formatClock(time.minuteOfDay)}</span>
              <span class="chron-icon">{KIND_ICON[entry.kind]}</span>
              {id ? (
                <button class="chron-text" onClick={() => follow(id)}>{entry.text}</button>
              ) : (
                <span class="chron-text">{entry.text}</span>
              )}
            </li>
          );
        })}
        {(all.length > shown || ui.chronicleMore.value) && (
          <li class="chron"><button class="link muted" disabled={busy} onClick={() => void earlier()}>{busy ? 'Loading…' : 'Earlier…'}</button></li>
        )}
      </ol>
      )}
    </aside>
  );
}
