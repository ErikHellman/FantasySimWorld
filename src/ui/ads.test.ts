import { describe, expect, it } from 'vitest';
import { hudAdVisible, hudAdWanted, type HudAdState } from './ads';

const base: HudAdState = {
  ads: { client: 'ca-pub-1', startSlot: '2', hudSlot: '3' },
  admin: false,
  mobile: false,
  selected: false,
  panel: 'none',
  sheet: 'none',
};

describe('the ad beside the chat', () => {
  it('shows for a guest on a wide screen with nothing open', () => {
    expect(hudAdVisible(base)).toBe(true);
  });

  it('is never there for the admin, on a phone, or without a slot', () => {
    expect(hudAdWanted({ ...base, admin: true })).toBe(false);
    expect(hudAdWanted({ ...base, mobile: true })).toBe(false);
    expect(hudAdWanted({ ...base, ads: { client: 'ca-pub-1', startSlot: '2', hudSlot: null } })).toBe(false);
    expect(hudAdWanted({ ...base, ads: null })).toBe(false);
  });

  it('steps aside, but stays loaded, while the viewer reads something', () => {
    for (const reading of [{ selected: true }, { panel: 'help' as const }, { sheet: 'chat' as const }]) {
      const state = { ...base, ...reading };
      expect(hudAdWanted(state)).toBe(true);
      expect(hudAdVisible(state)).toBe(false);
    }
  });
});
