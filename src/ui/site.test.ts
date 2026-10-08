import { afterEach, describe, expect, it } from 'vitest';
import type { SiteDetail } from '../generated/api';
import { selectedName } from './Mobile';
import { condition } from './SitePanel';
import { isSiteId, ui } from './store';

const site = (over: Partial<SiteDetail>): SiteDetail => ({
  id: 'b:smithy', kind: 'building', name: 'the smithy', placeId: 'smithy', placeName: 'the smithy', description: 'The forge.',
  stage: 'whole', sinceTick: 0, progress: 0, fire: 0, beds: null, residents: [], present: [], project: null, stockpile: null, history: [],
  ...over,
});

afterEach(() => {
  ui.selectedId.value = null;
  ui.site.value = null;
  ui.detail.value = null;
  ui.roster.value = [];
});

describe('site ids', () => {
  it('tells buildings and built things from villagers', () => {
    expect(isSiteId('b:smithy')).toBe(true);
    expect(isSiteId('n:12')).toBe(true);
    expect(isSiteId('c7')).toBe(false);
  });
});

describe('a site page', () => {
  it('describes the condition from the stage and how far things have got', () => {
    expect(condition(site({}))).toBe('Standing whole');
    expect(condition(site({ kind: 'built', stage: 'whole' }))).toBe('Standing');
    expect(condition(site({ stage: 'burning', fire: 40 }))).toBe('On fire, 40%');
    expect(condition(site({ stage: 'ruin' }))).toBe('In ruins');
    expect(condition(site({ stage: 'ruin', progress: 35 }))).toBe('In ruins, rebuilt 35%');
    expect(condition(site({ stage: 'raising', progress: 65 }))).toBe('Being raised, 65%');
    expect(condition(site({ stage: 'dismantled' }))).toBe('Taken down to its footings');
    expect(condition(site({ stage: 'mined_out' }))).toBe('Mined out');
  });
});

describe('the phone chip', () => {
  it('names the followed villager or the site being looked at', () => {
    expect(selectedName()).toBeNull();
    ui.selectedId.value = 'c1';
    ui.roster.value = [{ id: 'c1', name: 'Aldric Vane', role: 'guard', status: '', hp: 1, mood: 'ok', partner: false, child: false }];
    expect(selectedName()).toEqual({ name: 'Aldric', site: false });
    ui.selectedId.value = 'b:smithy';
    expect(selectedName()).toEqual({ name: '…', site: true });
    ui.site.value = site({});
    expect(selectedName()).toEqual({ name: 'The smithy', site: true });
  });
});
