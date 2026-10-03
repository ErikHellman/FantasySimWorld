import { afterEach, describe, expect, it, vi } from 'vitest';
import { watchLayout } from './layout';
import { ui } from './store';

/** A stand-in for matchMedia whose answer the test can change, telling listeners as a browser would. */
function stubMedia(matches: boolean): { set(matches: boolean): void; classes: Set<string> } {
  const listeners: Array<(event: { matches: boolean }) => void> = [];
  const query = { matches, media: '', addEventListener: (_: string, listener: (event: { matches: boolean }) => void) => listeners.push(listener) };
  const classes = new Set<string>();
  vi.stubGlobal('matchMedia', (media: string) => Object.assign(query, { media }));
  vi.stubGlobal('document', { documentElement: { classList: { toggle: (name: string, on: boolean) => (on ? classes.add(name) : classes.delete(name)) } } });
  return {
    set: (next) => {
      query.matches = next;
      for (const listener of listeners) listener({ matches: next });
    },
    classes,
  };
}

afterEach(() => {
  ui.mobile.value = false;
  ui.sheet.value = 'none';
  vi.unstubAllGlobals();
});

describe('layout', () => {
  it('follows the media query and marks the page for the stylesheet', () => {
    const media = stubMedia(true);
    watchLayout();
    expect(ui.mobile.value).toBe(true);
    expect(media.classes.has('mobile')).toBe(true);
    media.set(false);
    expect(ui.mobile.value).toBe(false);
    expect(media.classes.has('mobile')).toBe(false);
  });

  it('closes the phone sheet when the screen grows into the desktop layout', () => {
    const media = stubMedia(true);
    watchLayout();
    ui.sheet.value = 'villagers';
    media.set(false);
    expect(ui.sheet.value).toBe('none');
  });
});
