import { useEffect } from 'preact/hooks';
import { formatClock, gameTime } from '../core/time';
import { BrainLog } from './BrainLog';
import { Inspector } from './Inspector';
import { SettingsPanel } from './Settings';
import { actions, ui } from './store';

const SPEEDS = [
  { value: 0, label: '❚❚', title: 'Pause (space)' },
  { value: 1, label: '1×', title: 'Normal speed (1)' },
  { value: 2, label: '2×', title: 'Double speed (2)' },
  { value: 4, label: '4×', title: 'Four times speed (3)' },
];

const KIND_ICON: Record<string, string> = {
  world: '☀', social: '💬', romance: '❤', conflict: '⚡', combat: '⚔', death: '✝', arrival: '🚶',
};

const MOOD_ICON: Record<string, string> = { ok: '', fight: '⚔', sleep: '💤', talk: '💬', hurt: '🩹' };

function TopBar() {
  const clock = ui.clock.value;
  const llm = ui.llm.value;
  return (
    <header class="topbar">
      <div class="brand">Fantasy Sim World</div>
      <div class="clock" title={clock.part}>
        <span class="clock-day">Day {clock.day}</span>
        <span class="clock-time">{clock.time}</span>
        <span class="clock-part">{clock.part}</span>
      </div>
      <div class="speeds" role="group" aria-label="Simulation speed">
        {SPEEDS.map((speed) => (
          <button class={ui.speed.value === speed.value ? 'on' : ''} title={speed.title} onClick={() => actions.setSpeed(speed.value)}>
            {speed.label}
          </button>
        ))}
      </div>
      <div class="pills">
        <span class="pill" title="Villagers alive">👥 {ui.population.value}</span>
        {ui.monsters.value > 0 && <span class="pill danger" title="Monsters abroad">💀 {ui.monsters.value}</span>}
        {ui.graves.value.length > 0 && (
          <button class="pill" title="The graveyard" onClick={() => (ui.panel.value = ui.panel.value === 'graves' ? 'none' : 'graves')}>
            ✝ {ui.graves.value.length}
          </button>
        )}
        <button
          class={`pill llm ${llm.state}`}
          title={llm.message || 'Language model status. Click for the decision log.'}
          onClick={() => (ui.panel.value = ui.panel.value === 'brain' ? 'none' : 'brain')}
        >
          🧠 {llm.state === 'off' ? 'local minds' : llm.state === 'error' ? 'LLM error' : llm.inFlight + llm.queued > 0 ? `thinking ${llm.inFlight}+${llm.queued}` : 'LLM ready'}
        </button>
        <button class="pill" title="Settings" onClick={() => (ui.panel.value = ui.panel.value === 'settings' ? 'none' : 'settings')}>
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

function Chronicle() {
  const entries = ui.chronicle.value.slice(-40).reverse();
  return (
    <aside class="panel chronicle">
      <h2>Chronicle</h2>
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
      </ol>
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
          Drag to pan, scroll to zoom, right-drag or Q / E to turn. Click a villager to follow them and read their mind.
        </p>
        {loading ? (
          <div class="loading">{loading}</div>
        ) : (
          <button class="primary big" onClick={() => actions.start()}>Enter the valley</button>
        )}
        {!loading && ui.settings.value.llm.enabled === false && (
          <p class="muted small-text">
            The villagers are running on their built-in routines. Connect a language model in Settings to give them minds of their own.
          </p>
        )}
      </div>
    </div>
  );
}

export function App() {
  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      const target = event.target as HTMLElement | null;
      if (target && ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)) return;
      if (event.code === 'Space') {
        event.preventDefault();
        actions.setSpeed(ui.speed.value === 0 ? 1 : 0);
      } else if (event.code === 'Digit1') actions.setSpeed(1);
      else if (event.code === 'Digit2') actions.setSpeed(2);
      else if (event.code === 'Digit3') actions.setSpeed(4);
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
      {ui.panel.value === 'brain' && <BrainLog />}
      {ui.panel.value === 'graves' && <Graves />}
      {ui.notice.value && (
        <div class="notice" onClick={() => (ui.notice.value = null)}>{ui.notice.value}</div>
      )}
      {ui.alarm.value && <div class="alarm">⚔ The alarm has been raised</div>}
    </>
  );
}
