import { useEffect, useState } from 'preact/hooks';
import { formatClock, gameTime } from '../core/time';
import { api } from '../net/api';
import { AdminPanel } from './Admin';
import { BrainLog } from './BrainLog';
import { Inspector } from './Inspector';
import { SettingsPanel } from './Settings';
import { actions, isAdmin, ui } from './store';

const SPEEDS = [
  { value: 0, label: '❚❚', title: 'Pause (space)' },
  { value: 1, label: '1×', title: 'Normal speed (1)' },
  { value: 2, label: '2×', title: 'Double speed (2)' },
  { value: 4, label: '4×', title: 'Four times speed (3)' },
];

// \uFE0F asks for the colour emoji form; without it some of these fall back to plain text glyphs.
const KIND_ICON: Record<string, string> = {
  world: '☀\uFE0F', social: '💬', romance: '❤\uFE0F', conflict: '⚡', combat: '⚔\uFE0F', death: '🪦', arrival: '🚶',
};

const MOOD_ICON: Record<string, string> = { ok: '', fight: '⚔\uFE0F', sleep: '💤', talk: '💬', hurt: '🩹' };

const togglePanel = (panel: typeof ui.panel.value): void => {
  ui.panel.value = ui.panel.value === panel ? 'none' : panel;
};

function TopBar() {
  const clock = ui.clock.value;
  const llm = ui.llm.value;
  const admin = isAdmin();
  const connection = ui.connection.value;
  return (
    <header class="topbar">
      <div class="brand">Fantasy Sim World</div>
      <div class="clock" title={clock.part}>
        <span class="clock-day">Day {clock.day}</span>
        <span class="clock-time">{clock.time}</span>
        <span class="clock-part">{clock.part}</span>
      </div>
      {admin ? (
        <div class="speeds" role="group" aria-label="Simulation speed">
          {SPEEDS.map((speed) => (
            <button class={ui.speed.value === speed.value ? 'on' : ''} title={speed.title} onClick={() => actions.setSpeed(speed.value)}>
              {speed.label}
            </button>
          ))}
        </div>
      ) : (
        ui.speed.value !== 1 && <div class="pill" title="The world's speed, set by an admin">{ui.speed.value === 0 ? '❚❚ paused' : `${ui.speed.value}×`}</div>
      )}
      <div class="pills">
        {connection !== 'live' && <span class="pill danger" title="The connection to the server">{connection === 'down' ? 'offline' : 'reconnecting…'}</span>}
        <span class="pill" title="People watching">👁 {ui.viewers.value}</span>
        <span class="pill" title="Villagers alive">👥 {ui.population.value}</span>
        {ui.monsters.value > 0 && <span class="pill danger" title="Monsters abroad">💀 {ui.monsters.value}</span>}
        {ui.graves.value.length > 0 && (
          <button class="pill" title="The graveyard" onClick={() => togglePanel('graves')}>
            🪦 {ui.graves.value.length}
          </button>
        )}
        <button
          class={`pill llm ${llm.state}`}
          title={llm.message || (admin ? 'Language model status. Click for the decision log.' : 'Language model status')}
          disabled={!admin}
          onClick={() => togglePanel('brain')}
        >
          🧠 {llm.state === 'off' ? 'local minds' : llm.state === 'error' ? 'LLM error' : llm.inFlight + llm.queued > 0 ? `thinking ${llm.inFlight}+${llm.queued}` : 'LLM ready'}
        </button>
        {admin && (
          <button class="pill" title="Run the world: model, access, saves" onClick={() => togglePanel('admin')}>
            🛠 Admin
          </button>
        )}
        <button class="pill" title="Settings" onClick={() => togglePanel('settings')}>
          ⚙ Settings
        </button>
      </div>
    </header>
  );
}

function Roster() {
  const selected = ui.selectedId.value;
  return (
    <aside class="panel roster">
      <h2>Villagers</h2>
      <ul>
        {ui.roster.value.map((entry) => (
          <li>
            <button class={`roster-row ${selected === entry.id ? 'on' : ''}`} onClick={() => actions.select(selected === entry.id ? null : entry.id)}>
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
              <button class="link" onClick={() => actions.select(line.speakerId)}>{line.speaker}</button>
              {line.to && <span class="muted"> to {line.to}</span>}: {line.text}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

const PAGE = 40;

function Chronicle() {
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
                <button class="chron-text" onClick={() => actions.select(id)}>{entry.text}</button>
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

function Graves() {
  return (
    <div class="modal" onClick={() => (ui.panel.value = 'none')}>
      <section class="panel dialog small" onClick={(event) => event.stopPropagation()}>
        <h2>The graveyard</h2>
        <ul class="graves">
          {ui.graves.value.map((grave) => (
            <li>
              <strong>{grave.name}</strong> <span class="muted">day {grave.day}</span>
              <div class="muted">{grave.cause}</div>
            </li>
          ))}
        </ul>
        <button class="primary" onClick={() => (ui.panel.value = 'none')}>Close</button>
      </section>
    </div>
  );
}

function StartOverlay() {
  const loading = ui.loading.value;
  return (
    <div class="start">
      <div class="start-card">
        <h1>Fantasy Sim World</h1>
        <p>
          A small village in a valley. Its people work, eat, gossip, fall out and fall in love, and at night the dead in the
          old graveyard do not stay buried. You are only here to watch.
        </p>
        <p class="muted">
          Drag to pan, scroll to zoom, right-drag or Q / E to turn, Shift + scroll or R / F to tilt. Click a villager to follow them and read their mind.
        </p>
        {loading ? (
          <div class="loading">{loading}</div>
        ) : (
          <button class="primary big" onClick={() => actions.start()}>Enter the valley</button>
        )}
        {!loading && ui.llm.value.state === 'off' && (
          <p class="muted small-text">
            The villagers are running on their built-in routines{isAdmin() ? '. Set LLM_BASE_URL and LLM_MODEL on the server to give them minds of their own.' : '.'}
          </p>
        )}
        {!loading && ui.me.value && <p class="muted small-text">Signed in as {ui.me.value.displayName || ui.me.value.email}.</p>}
      </div>
    </div>
  );
}

export function App() {
  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      const target = event.target as HTMLElement | null;
      if (target && ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)) return;
      const admin = isAdmin();
      if (event.code === 'Space' && admin) {
        event.preventDefault();
        actions.setSpeed(ui.speed.value === 0 ? 1 : 0);
      } else if (event.code === 'Digit1' && admin) actions.setSpeed(1);
      else if (event.code === 'Digit2' && admin) actions.setSpeed(2);
      else if (event.code === 'Digit3' && admin) actions.setSpeed(4);
      else if (event.code === 'Escape') {
        if (ui.panel.value !== 'none') ui.panel.value = 'none';
        else actions.select(null);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  if (!ui.started.value) return <StartOverlay />;
  return (
    <>
      <TopBar />
      <Roster />
      <Chronicle />
      <Inspector />
      {ui.panel.value === 'settings' && <SettingsPanel />}
      {ui.panel.value === 'admin' && isAdmin() && <AdminPanel />}
      {ui.panel.value === 'brain' && isAdmin() && <BrainLog />}
      {ui.panel.value === 'graves' && <Graves />}
      {ui.notice.value && (
        <div class="notice" onClick={() => (ui.notice.value = null)}>{ui.notice.value}</div>
      )}
      {ui.alarm.value && <div class="alarm">⚔{'\uFE0F'} The alarm has been raised</div>}
      {ui.throttled.value && !ui.alarm.value && <div class="alarm calm">🧠 Slowed to 1× while the villagers think</div>}
    </>
  );
}
