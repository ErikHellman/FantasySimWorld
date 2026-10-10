import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { HeraldIssue } from '../src/generated/api';

const issue = (day: number): HeraldIssue => ({ day, headline: `News of day ${day - 1}`, stories: ['Something happened.'], byModel: true });

/** A fresh store (its once-a-page look for missed news included) over this browser's storage. */
async function freshStore(stored: Record<string, string>) {
  vi.resetModules();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => stored[key] ?? null,
    setItem: (key: string, value: string) => {
      stored[key] = value;
    },
  });
  return import('../src/ui/store');
}

describe('the Valley Herald', () => {
  beforeEach(() => vi.unstubAllGlobals());

  it('opens for a viewer back after missing an issue', async () => {
    const stored = { 'fsw.herald': '3' };
    const { setHerald, ui } = await freshStore(stored);
    setHerald(issue(5), true);
    expect(ui.panel.value).toBe('herald');
    expect(ui.heraldAway.value).toBe(true);
    expect(stored['fsw.herald']).toBe('5');
  });

  it('stays shut on a first visit and only remembers where the viewer starts', async () => {
    const stored: Record<string, string> = {};
    const { setHerald, ui } = await freshStore(stored);
    setHerald(issue(4), true);
    expect(ui.panel.value).toBe('none');
    expect(stored['fsw.herald']).toBe('4');
    // An issue that comes out while they watch is marked as new, not opened.
    setHerald(issue(5));
    expect(ui.panel.value).toBe('none');
    expect(ui.heraldUnread.value).toBe(true);
  });

  it('looks for missed news once a page, not on every reconnect', async () => {
    const stored = { 'fsw.herald': '5' };
    const { setHerald, ui } = await freshStore(stored);
    setHerald(issue(5), true);
    setHerald(issue(6));
    setHerald(issue(6), true);
    expect(ui.panel.value).toBe('none');
    expect(ui.heraldUnread.value).toBe(true);
  });

  it('counts afresh in a new world', async () => {
    const stored = { 'fsw.herald': '30' };
    const { setHerald, ui } = await freshStore(stored);
    setHerald(issue(2), true);
    expect(ui.panel.value).toBe('none');
    expect(stored['fsw.herald']).toBe('2');
  });
});
