import { afterEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_AUDIO } from '../audio/engine';
import { loadAudioSettings, saveAudioSettings } from './store';

function stubStorage(initial: Record<string, string> = {}): Map<string, string> {
  const store = new Map(Object.entries(initial));
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
  });
  return store;
}

afterEach(() => vi.unstubAllGlobals());

describe('audio settings', () => {
  it('fills in the music and on/off fields for settings saved before they existed', () => {
    stubStorage({ 'fsw.audio': JSON.stringify({ muted: false, master: 0.5, ambient: 0.3, effects: 0.9 }) });
    expect(loadAudioSettings()).toEqual({ ...DEFAULT_AUDIO, master: 0.5, ambient: 0.3, effects: 0.9 });
  });

  it('keeps the on/off choices in this browser', () => {
    stubStorage();
    saveAudioSettings({ ...DEFAULT_AUDIO, musicOn: false, ambientOn: false });
    expect(loadAudioSettings()).toMatchObject({ musicOn: false, ambientOn: false, effectsOn: true });
  });
});
