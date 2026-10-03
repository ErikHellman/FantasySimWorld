import { afterEach, describe, expect, it } from 'vitest';
import type { CreditOffer, CreditsState } from '../generated/api';
import { creditsOn, offerBlocked, setBalance, togglePanel, ui } from './store';

const offer = (target: CreditOffer['target'], cost: number): CreditOffer => ({
  id: 'x', name: 'X', description: '', cost, target, group: 'gift',
});
const state = (balance: number, perMinute = 2): CreditsState => ({ balance, cap: 400, perMinute, beatSeconds: 30, offers: [] });

afterEach(() => {
  ui.credits.value = null;
  ui.panel.value = 'none';
});

describe('credits in the HUD', () => {
  it('show only once the server says they are on', () => {
    expect(creditsOn()).toBe(false);
    ui.credits.value = state(0, 0);
    expect(creditsOn()).toBe(false);
    ui.credits.value = state(0);
    expect(creditsOn()).toBe(true);
  });

  it('take a new balance from the server', () => {
    setBalance(5); // nothing known yet: nothing to update
    expect(ui.credits.value).toBeNull();
    ui.credits.value = state(10);
    setBalance(42);
    expect(ui.credits.value.balance).toBe(42);
    expect(ui.credits.value.cap).toBe(400);
  });

  it('say why an offer cannot be bought', () => {
    expect(offerBlocked(offer('none', 80), 100, null)).toBeNull();
    expect(offerBlocked(offer('none', 80), 30, null)).toBe('You need 50 more.');
    expect(offerBlocked(offer('character', 5), 100, null)).toBe('Select a villager first.');
    expect(offerBlocked(offer('character', 5), 100, 'c1')).toBeNull();
    expect(offerBlocked(offer('point', 20), 100, null)).toBeNull();
  });

  it('open the shop as a window', () => {
    togglePanel('shop');
    expect(ui.panel.value).toBe('shop');
    togglePanel('shop');
    expect(ui.panel.value).toBe('none');
  });
});
