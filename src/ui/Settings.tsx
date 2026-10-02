import type { AudioSettings } from '../audio/engine';
import { api } from '../net/api';
import { actions, ui } from './store';

/** This viewer's own settings: sound (kept in this browser) and signing out. */
export function SettingsPanel() {
  const audio = ui.audio.value;
  const setAudio = (patch: Partial<AudioSettings>): void => actions.applyAudio({ ...audio, ...patch });
  const me = ui.me.value;

  const signOut = async (): Promise<void> => {
    try {
      await api.logout();
    } finally {
      location.assign('/login');
    }
  };

  return (
    <div class="modal" onClick={() => (ui.panel.value = 'none')}>
      <section class="panel dialog small" onClick={(event) => event.stopPropagation()}>
        <button class="close" title="Close" onClick={() => (ui.panel.value = 'none')}>×</button>
        <h2>Settings</h2>

        <h3>Sound</h3>
        <p class="muted">Kept in this browser only.</p>
        <label class="check">
          <input type="checkbox" checked={audio.muted} onChange={(event) => setAudio({ muted: event.currentTarget.checked })} />
          Mute
        </label>
        <div class="grid">
          <label>Master<input type="range" min="0" max="1" step="0.05" value={audio.master} onInput={(event) => setAudio({ master: Number(event.currentTarget.value) })} /></label>
          <label>Ambience<input type="range" min="0" max="1" step="0.05" value={audio.ambient} onInput={(event) => setAudio({ ambient: Number(event.currentTarget.value) })} /></label>
          <label>Effects<input type="range" min="0" max="1" step="0.05" value={audio.effects} onInput={(event) => setAudio({ effects: Number(event.currentTarget.value) })} /></label>
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

        {me && (
          <>
            <h3>Account</h3>
            <div class="row">
              <span class="test-result">Signed in as {me.displayName ? `${me.displayName} (${me.email})` : me.email}{me.role === 'admin' ? ', admin' : ''}.</span>
              <button onClick={() => void signOut()}>Sign out</button>
            </div>
          </>
        )}

        <footer>
          <button class="primary" onClick={() => (ui.panel.value = 'none')}>Done</button>
        </footer>
      </section>
    </div>
  );
}
