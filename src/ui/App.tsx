import { useEffect } from 'preact/hooks';
import { AdminPanel } from './Admin';
import { BrainLog } from './BrainLog';
import { ChatPanel } from './Chat';
import { HelpPanel } from './Help';
import { Inspector } from './Inspector';
import { isTouchScreen } from './layout';
import { MobileHud } from './Mobile';
import { Chronicle, LlmPill, Roster, Speeds } from './Panes';
import { SettingsPanel } from './Settings';
import { actions, closeSheet, isAdmin, togglePanel, ui } from './store';

function TopBar() {
  const clock = ui.clock.value;
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
        <Speeds />
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
        <LlmPill />
        {admin && (
          <button class="pill" title="Run the world: model, access, saves" onClick={() => togglePanel('admin')}>
            🛠 Admin
          </button>
        )}
        <button class="pill" title="Settings" onClick={() => togglePanel('settings')}>
          ⚙ Settings
        </button>
        <button class="pill" title="Controls and how the village works (?)" onClick={() => togglePanel('help')}>
          ❓ Help
        </button>
      </div>
    </header>
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
        {isTouchScreen() ? (
          <p class="muted">
            Drag with one finger to look around, pinch to zoom, twist two fingers to turn and drag two fingers up or down to tilt.
            Tap a villager to follow them. Once inside, ❓ Help under ☰ lists every control.
          </p>
        ) : (
          <p class="muted">
            Drag to pan, scroll to zoom, right-drag or Q / E to turn, Shift + scroll or R / F to tilt. Click a villager to follow them and read their mind.
            Once inside, ❓ Help (or the ? key) lists every control.
          </p>
        )}
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
      else if ((event.key === '?' || event.code === 'KeyH') && ui.started.value && !event.ctrlKey && !event.metaKey && !event.altKey) togglePanel('help');
      else if (event.code === 'Escape') {
        if (ui.panel.value !== 'none') ui.panel.value = 'none';
        else if (ui.sheet.value !== 'none') closeSheet();
        else actions.select(null);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  if (!ui.started.value) return <StartOverlay />;
  const burning = ui.houses.value.find((house) => house.phase === 'burning');
  return (
    <>
      {ui.mobile.value ? (
        <MobileHud />
      ) : (
        <>
          <TopBar />
          <Roster />
          <Chronicle />
          <Inspector />
          <ChatPanel />
        </>
      )}
      {ui.panel.value === 'settings' && <SettingsPanel />}
      {ui.panel.value === 'admin' && isAdmin() && <AdminPanel />}
      {ui.panel.value === 'brain' && isAdmin() && <BrainLog />}
      {ui.panel.value === 'graves' && <Graves />}
      {ui.panel.value === 'help' && <HelpPanel />}
      {ui.notice.value && (
        <div class="notice" onClick={() => (ui.notice.value = null)}>{ui.notice.value}</div>
      )}
      {burning && <div class="alarm">🔥 {burning.name.charAt(0).toUpperCase() + burning.name.slice(1)} is on fire</div>}
      {ui.alarm.value && !burning && <div class="alarm">⚔{'\uFE0F'} The alarm has been raised</div>}
      {ui.throttled.value && !ui.alarm.value && !burning && <div class="alarm calm">🧠 Slowed to 1× while the villagers think</div>}
    </>
  );
}
