import { afterEach, describe, expect, it } from 'vitest';
import type { Ceremony } from '../generated/api';
import { ceremonyBanner } from './ceremony';
import { setCeremony, ui } from './store';

const wedding: Ceremony = {
  id: 'y1', kind: 'wedding', phase: 'announced', placeId: 'green', placeName: 'the village green',
  ids: ['c1', 'c2'], names: ['Sylvi', 'Kael'], day: 5, time: '14:00',
};
const funeral: Ceremony = {
  id: 'y2', kind: 'funeral', phase: 'announced', placeId: 'graveyard', placeName: 'the graveyard',
  ids: ['c3', 'c4'], names: ['Garrick', 'Dorn'], day: 6, time: '09:00',
};

const trial: Ceremony = {
  id: 'y3', kind: 'trial', phase: 'announced', placeId: 'green', placeName: 'the village green',
  ids: ['c5', 'c6'], names: ['Bran', 'Edda'], day: 7, time: '16:00',
};

const harvest: Ceremony = {
  id: 'y4', kind: 'festival', phase: 'announced', placeId: 'green', placeName: 'the village green',
  ids: [], names: [], day: 27, time: '18:00', feast: 'harvest',
};
const election: Ceremony = {
  id: 'y5', kind: 'election', phase: 'announced', placeId: 'green', placeName: 'the village green',
  ids: ['c1', 'c2'], names: ['Sylvi', 'Kael'], day: 29, time: '12:00',
};

afterEach(() => {
  ui.ceremony.value = null;
});

describe('the ceremony banner', () => {
  it('names the feast and the candidates', () => {
    expect(ceremonyBanner(harvest)).toBe('🌾 The village keeps the harvest festival on the village green at 18:00');
    expect(ceremonyBanner({ ...harvest, feast: 'midwinter', phase: 'rite' })).toBe('❄️ The village is keeping midwinter on the village green');
    expect(ceremonyBanner({ ...harvest, phase: 'feast' })).toBe('🍻 The harvest feast is on at the tavern');
    expect(ceremonyBanner(election)).toBe('🎖️ The village chooses its elder on the village green at 12:00: Sylvi or Kael');
  });

  it('says what is arranged, what is under way and what is on at the tavern', () => {
    expect(ceremonyBanner(wedding)).toBe('💍 Sylvi and Kael marry on the village green at 14:00');
    expect(ceremonyBanner({ ...wedding, phase: 'gathering' })).toBe('💍 Sylvi and Kael are being married on the village green');
    expect(ceremonyBanner({ ...wedding, phase: 'feast' })).toBe('🍻 The wedding feast for Sylvi and Kael is on at the tavern');
    expect(ceremonyBanner(funeral)).toBe('🕯️ The village buries Garrick and Dorn at 09:00');
    expect(ceremonyBanner({ ...funeral, phase: 'rite' })).toBe('🕯️ The village is burying Garrick and Dorn');
    expect(ceremonyBanner({ ...funeral, phase: 'feast', names: ['Garrick'] })).toBe('🍻 The wake for Garrick is at the tavern');
    expect(ceremonyBanner(trial)).toBe('⚖️ Bran is to be tried on the village green at 16:00');
    expect(ceremonyBanner({ ...trial, phase: 'rite' })).toBe('⚖️ Bran is on trial on the village green');
  });
});

describe('the ceremony in the HUD', () => {
  it('is kept as it moves on and forgotten when it is over', () => {
    setCeremony(wedding);
    expect(ui.ceremony.value?.phase).toBe('announced');
    setCeremony({ ...wedding, phase: 'rite' });
    expect(ui.ceremony.value?.phase).toBe('rite');
    setCeremony({ ...wedding, phase: 'over' });
    expect(ui.ceremony.value).toBeNull();
  });
});
