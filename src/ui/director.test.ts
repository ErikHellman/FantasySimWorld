import { describe, expect, it } from 'vitest';
import type { Hotspot, HotspotKind } from '../generated/api';
import { CardDealer, chooseShot, hotspotKey } from './director';

const spot = (kind: HotspotKind, subject: string | null, score: number): Hotspot => ({
  kind, subject, at: { x: 10, z: -4 }, text: `${kind} ${subject ?? ''}`, score,
});
const anyone = (): boolean => true;

describe('the Auto camera', () => {
  it('takes the best hotspot it can follow', () => {
    const spots = [spot('fire', 'c1', 90), spot('music', 'c2', 40)];
    expect(chooseShot(spots, null, 0, anyone)?.subject).toBe('c1');
    expect(chooseShot(spots, null, 0, (id) => id !== 'c1')?.subject).toBe('c2');
    expect(chooseShot([], null, 0, anyone)).toBeNull();
  });

  it('stays on a shot for a while, then cuts to something much better', () => {
    const shot = { key: 'c2', since: 0 };
    const spots = [spot('dragon', 'm1', 100), spot('music', 'c2', 40)];
    expect(chooseShot(spots, shot, 10, anyone)?.subject).toBe('c2');
    expect(chooseShot(spots, shot, 25, anyone)?.subject).toBe('m1');
  });

  it('cuts at once when the shot is over', () => {
    expect(chooseShot([spot('quarrel', 'c3', 50)], { key: 'c2', since: 0 }, 1, anyone)?.subject).toBe('c3');
  });

  it('moves on after a long dwell when something nearly as good is going on', () => {
    const shot = { key: 'c2', since: 0 };
    const spots = [spot('music', 'c2', 45), spot('quarrel', 'c3', 40)];
    expect(chooseShot(spots, shot, 30, anyone)?.subject).toBe('c2');
    expect(chooseShot(spots, shot, 61, anyone)?.subject).toBe('c3');
    expect(chooseShot([spots[0]!, spot('romance', 'c4', 20)], shot, 61, anyone)?.subject).toBe('c2');
  });

  it('tells a hotspot without a subject by its kind and place', () => {
    expect(hotspotKey(spot('fire', null, 90))).toBe('fire@10,-4');
  });
});

describe('the Happening now cards', () => {
  it('announce big things that start, not what was going on when the page opened', () => {
    const dealer = new CardDealer();
    expect(dealer.deal([spot('fire', null, 90)], 0)).toEqual([]);
    expect(dealer.deal([spot('fire', null, 90)], 1)).toEqual([]);
    const raid = spot('raid', 'm4', 82);
    expect(dealer.deal([spot('fire', null, 90), raid, spot('music', 'c2', 40)], 2)).toEqual([raid]);
  });

  it('announce a kind once in a while, so a fight that changes hands is one card', () => {
    const dealer = new CardDealer();
    dealer.deal([], 0);
    expect(dealer.deal([spot('battle', 'c1', 60)], 1)).toHaveLength(1);
    expect(dealer.deal([spot('battle', 'c5', 60)], 5)).toEqual([]);
    expect(dealer.deal([spot('battle', 'c6', 60)], 200)).toHaveLength(1);
  });
});
