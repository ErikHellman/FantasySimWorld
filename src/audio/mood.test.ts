import { describe, expect, it } from 'vitest';
import type { WireEvent } from '../generated/api';
import { MOOD_TIMING, MoodTracker, timeOfDayMood, type MusicScene } from './mood';

const camera = { x: 0, z: 0, distance: 40 };
const calm: MusicScene = { hour: 12, alarm: false, monstersNear: 0, selectedMood: null };

const hit = (x: number, attackerId = 'm1', targetId = 'v1'): WireEvent => ({ type: 'hit', attackerId, targetId, damage: 3, pos: { x, z: 0 }, fatal: false });
const death = (x: number, id: string, monster: boolean): WireEvent => ({ type: 'death', id, name: id, pos: { x, z: 0 }, monster });

describe('timeOfDayMood', () => {
  it('splits the day into day, evening and night', () => {
    expect(timeOfDayMood(6)).toBe('night');
    expect(timeOfDayMood(7)).toBe('day');
    expect(timeOfDayMood(17.9)).toBe('day');
    expect(timeOfDayMood(19)).toBe('evening');
    expect(timeOfDayMood(22)).toBe('night');
    expect(timeOfDayMood(2)).toBe('night');
  });
});

describe('MoodTracker', () => {
  it('follows the time of day when nothing is happening', () => {
    const tracker = new MoodTracker();
    expect(tracker.update(calm, 0)).toBe('day');
    expect(tracker.update({ ...calm, hour: 19 }, 1)).toBe('evening');
    expect(tracker.update({ ...calm, hour: 23 }, 2)).toBe('night');
  });

  it('turns tense on the alarm or monsters nearby, and holds it for a while', () => {
    const tracker = new MoodTracker();
    expect(tracker.update({ ...calm, alarm: true }, 0)).toBe('tension');
    expect(tracker.update(calm, MOOD_TIMING.tensionAtLeast - 1)).toBe('tension');
    expect(tracker.update(calm, MOOD_TIMING.tensionAtLeast + 1)).toBe('day');
    expect(tracker.update({ ...calm, monstersNear: 2 }, 100)).toBe('tension');
  });

  it('goes to battle on a blow in view, holds it, then lets go', () => {
    const tracker = new MoodTracker();
    tracker.noteEvents([hit(10)], camera, null, 0);
    expect(tracker.update(calm, 0)).toBe('battle');
    expect(tracker.update(calm, MOOD_TIMING.battleAtLeast - 1)).toBe('battle');
    expect(tracker.update(calm, MOOD_TIMING.battleAtLeast + 1)).toBe('day');
  });

  it('keeps the battle going while blows keep landing', () => {
    const tracker = new MoodTracker();
    for (let t = 0; t <= 60; t += 5) {
      tracker.noteEvents([hit(5)], camera, null, t);
      expect(tracker.update(calm, t)).toBe('battle');
    }
    expect(tracker.update(calm, 60 + MOOD_TIMING.battleAfter + 1)).toBe('day');
  });

  it('ignores fights far from the camera, unless the selected villager is in them', () => {
    const tracker = new MoodTracker();
    tracker.noteEvents([hit(500)], camera, null, 0);
    expect(tracker.update(calm, 0)).toBe('day');
    tracker.noteEvents([hit(500, 'm1', 'hero')], camera, 'hero', 1);
    expect(tracker.update(calm, 1)).toBe('battle');
  });

  it('treats the selected villager fighting as a battle', () => {
    const tracker = new MoodTracker();
    expect(tracker.update({ ...calm, selectedMood: 'fight' }, 0)).toBe('battle');
  });

  it('grieves after a villager dies in view, with the defeat sting once', () => {
    const tracker = new MoodTracker();
    tracker.update(calm, 0);
    tracker.noteEvents([death(5, 'v1', false)], camera, null, 1);
    expect(tracker.update(calm, 1)).toBe('grief');
    expect(tracker.takeSting()).toBe('defeat');
    expect(tracker.takeSting()).toBeNull();
    expect(tracker.update({ ...calm, alarm: true }, 2)).toBe('grief');
    expect(tracker.update(calm, 1 + MOOD_TIMING.griefFor + 1)).toBe('day');
  });

  it('grieves for the selected villager wherever they die', () => {
    const tracker = new MoodTracker();
    tracker.noteEvents([death(900, 'hero', false)], camera, 'hero', 0);
    expect(tracker.update(calm, 0)).toBe('grief');
  });

  it('plays the victory sting when a fight ends with monsters slain and no villager lost', () => {
    const tracker = new MoodTracker();
    tracker.noteEvents([hit(5)], camera, null, 0);
    tracker.update(calm, 0);
    tracker.noteEvents([death(5, 'm1', true)], camera, null, 3);
    expect(tracker.update(calm, 3)).toBe('battle');
    expect(tracker.takeSting()).toBeNull();
    expect(tracker.update(calm, 30)).toBe('day');
    expect(tracker.takeSting()).toBe('victory');
  });

  it('saves the defeat sting for the end of a fight, then grieves', () => {
    const tracker = new MoodTracker();
    tracker.noteEvents([hit(5)], camera, null, 0);
    tracker.update(calm, 0);
    tracker.noteEvents([death(5, 'm1', true), death(6, 'v1', false)], camera, null, 2);
    tracker.update(calm, 2);
    expect(tracker.takeSting()).toBeNull();
    expect(tracker.update(calm, 30)).toBe('grief');
    expect(tracker.takeSting()).toBe('defeat');
  });

  it('does not react to deaths out of view', () => {
    const tracker = new MoodTracker();
    tracker.noteEvents([death(900, 'v9', false)], camera, null, 0);
    expect(tracker.update(calm, 0)).toBe('day');
    expect(tracker.takeSting()).toBeNull();
  });
});
