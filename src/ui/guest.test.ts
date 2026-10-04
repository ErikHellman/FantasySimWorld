import { afterEach, describe, expect, it } from 'vitest';
import type { CreditsState, Me } from '../generated/api';
import { creditsOn, guestTimeLeft, isGuest, ui } from './store';

const me = (role: Me['role']): Me => ({ email: '', displayName: 'Guest', role, guestSecondsLeft: role === 'guest' ? 900 : null });
const credits: CreditsState = { balance: 10, cap: 400, perMinute: 2, beatSeconds: 30, offers: [] };

afterEach(() => {
  ui.me.value = null;
  ui.credits.value = null;
});

describe('guests', () => {
  it('are told apart by their role', () => {
    expect(isGuest()).toBe(false);
    ui.me.value = me('viewer');
    expect(isGuest()).toBe(false);
    ui.me.value = me('guest');
    expect(isGuest()).toBe(true);
  });

  it('never see credits', () => {
    ui.credits.value = credits;
    ui.me.value = me('viewer');
    expect(creditsOn()).toBe(true);
    ui.me.value = me('guest');
    expect(creditsOn()).toBe(false);
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
