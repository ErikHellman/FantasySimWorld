import { afterEach, describe, expect, it } from 'vitest';
import type { Me } from '../generated/api';
import { guestTimeLeft, isGuest, ui } from './store';

const me = (role: Me['role']): Me => ({ email: '', displayName: 'Guest', role, guestSecondsLeft: role === 'guest' ? 900 : null });

afterEach(() => {
  ui.me.value = null;
});

describe('guests', () => {
  it('are told apart by their role', () => {
    expect(isGuest()).toBe(false);
    ui.me.value = me('viewer');
    expect(isGuest()).toBe(false);
    ui.me.value = me('guest');
    expect(isGuest()).toBe(true);
  });

  it('see how long they have left in whole minutes, rounded up', () => {
    expect(guestTimeLeft(15 * 60_000)).toBe('15 min left');
    expect(guestTimeLeft(14 * 60_000 + 1)).toBe('15 min left');
    expect(guestTimeLeft(61_000)).toBe('2 min left');
    expect(guestTimeLeft(60_000)).toBe('1 min left');
    expect(guestTimeLeft(59_000)).toBe('under a minute left');
    expect(guestTimeLeft(-5)).toBe('under a minute left');
  });
});
