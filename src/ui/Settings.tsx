import { SOUND_CREDITS } from '../audio/credits';
import type { AudioSettings } from '../audio/engine';
import { api } from '../net/api';
import { actions, ui } from './store';

type Toggle = 'ambientOn' | 'effectsOn' | 'musicOn';
type Volume = 'ambient' | 'effects' | 'music';

const CATEGORIES: readonly { on: Toggle; volume: Volume; label: string; hint: string }[] = [
  { on: 'ambientOn', volume: 'ambient', label: 'Ambience', hint: 'Birds, wind, water, the village and the tavern, by where the camera looks' },
  { on: 'effectsOn', volume: 'effects', label: 'Effects', hint: 'Blows, spells, cries and the alarm bell' },
  { on: 'musicOn', volume: 'music', label: 'Music', hint: 'Background music that follows what you are watching' },
];

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
        <label>Master<input type="range" min="0" max="1" step="0.05" value={audio.master} onInput={(event) => setAudio({ master: Number(event.currentTarget.value) })} /></label>
        <div class="sound-rows">
          {CATEGORIES.map(({ on, volume, label, hint }) => (
            <div class="sound-row">
              <label class="check" title={hint}>
                <input type="checkbox" checked={audio[on]} onChange={(event) => setAudio({ [on]: event.currentTarget.checked })} />
                {label}
              </label>
              <input
                type="range" min="0" max="1" step="0.05" aria-label={`${label} volume`} disabled={!audio[on]} value={audio[volume]}
                onInput={(event) => setAudio({ [volume]: Number(event.currentTarget.value) })}
              />
            </div>
          ))}
        </div>
        <details>
          <summary>
            Sound files: {ui.audioSlots.value.filter((slot) => slot.loaded).length} of {ui.audioSlots.value.length} found
          </summary>
          <p class="muted">The files are in <code>public/audio/</code>, named after the slot (music in <code>public/audio/music/</code>), and are rebuilt with <code>pnpm audio</code>. Missing files are silent.</p>
          <ul class="slots">
            {ui.audioSlots.value.map((slot) => (
              <li class={slot.loaded ? 'ok' : ''}>{slot.loaded ? '●' : '○'} <code>{slot.id}</code> <span class="muted">{slot.label}</span></li>
            ))}
          </ul>
        </details>
        <details>
          <summary>Sound credits</summary>
          <ul class="slots">
            {SOUND_CREDITS.map((credit) => (
              <li>
                <a href={credit.url} target="_blank" rel="noreferrer">{credit.title}</a> <span class="muted">by {credit.author}, {credit.license}</span>
              </li>
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
