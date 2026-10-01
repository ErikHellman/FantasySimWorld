import 'fake-indexeddb/auto';
import { describe, expect, it } from 'vitest';
import { createState } from '../sim/create';
import { Sim } from '../sim/sim';
import { SAVE_VERSION } from '../sim/state';
import { clearSaves, loadSave, loadSettings, saveSettings, writeSave } from './db';
import { migrate } from './migrations';
import { DEFAULT_SETTINGS, normalizeSettings, parseHeaders } from './settings';

describe('saves', () => {
  it('round-trips the world through IndexedDB', async () => {
    const sim = new Sim(createState(3));
    for (let i = 0; i < 1500; i++) sim.step();
    await writeSave(sim.state);
    const loaded = await loadSave();
    expect(loaded?.version).toBe(SAVE_VERSION);
    expect(loaded?.state).toEqual(sim.state);
    await clearSaves();
    expect(await loadSave()).toBeNull();
  });

  it('rejects saves it cannot use', () => {
    expect(migrate(null)).toBeNull();
    expect(migrate({ version: SAVE_VERSION + 5, state: {} })).toBeNull();
    expect(migrate({ version: 0, state: createState(1) })).toBeNull();
    expect(migrate({ version: SAVE_VERSION, savedAt: 1, state: { tick: 'x' } })).toBeNull();
    expect(migrate({ version: SAVE_VERSION, savedAt: 1, state: createState(1) })).not.toBeNull();
  });
});

describe('settings', () => {
  it('fills in defaults and persists', async () => {
    expect(await loadSettings()).toEqual(DEFAULT_SETTINGS);
    const settings = normalizeSettings({ llm: { baseUrl: 'http://localhost:11434/v1', model: 'x' } });
    expect(settings.llm.concurrency).toBe(DEFAULT_SETTINGS.llm.concurrency);
    await saveSettings(settings);
    expect((await loadSettings()).llm.model).toBe('x');
  });

  it('parses extra headers', () => {
    expect(parseHeaders('HTTP-Referer: https://example.org\nX-Title: Fantasy Sim\n\nbroken')).toEqual({
      'HTTP-Referer': 'https://example.org',
      'X-Title': 'Fantasy Sim',
    });
  });
});
