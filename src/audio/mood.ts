import type { Mood as RosterMood, WireEvent } from '../generated/api';

/** What the background music is about. */
export type MusicMood = 'day' | 'evening' | 'night' | 'tension' | 'battle' | 'grief';
/** A short phrase played over the music when something ends. */
export type Sting = 'victory' | 'defeat';

export interface Listener {
  x: number;
  z: number;
  /** camera distance from the ground point it looks at */
  distance: number;
}

/** What the tracker needs to know about the world each time it is asked for a mood. */
export interface MusicScene {
  hour: number;
  /** a villager has raised the alarm */
  alarm: boolean;
  /** monsters near where the camera looks */
  monstersNear: number;
  /** the roster mood of the selected villager, if one is selected */
  selectedMood: RosterMood | null;
}

/** Seconds. */
export const MOOD_TIMING = {
  /** battle music lasts this long after the last blow the viewer saw */
  battleAfter: 12,
  /** and plays at least this long once it has started */
  battleAtLeast: 20,
  /** grief music after a villager the viewer was watching dies */
  griefFor: 75,
  /** tension music plays at least this long once it has started */
  tensionAtLeast: 20,
};

/** The same reach as effects: events further from the camera's target than this are not heard. */
export function inView(x: number, z: number, listener: Listener): boolean {
  return Math.hypot(x - listener.x, z - listener.z) <= 30 + listener.distance;
}

export function timeOfDayMood(hour: number): 'day' | 'evening' | 'night' {
  if (hour >= 6.5 && hour < 18) return 'day';
  if (hour >= 18 && hour < 21.5) return 'evening';
  return 'night';
}

/**
 * Follows what the viewer is watching (the camera's surroundings and the
 * selected villager) and picks the music for it. In order of precedence:
 * a fight in view, grief after a death in view, danger nearby (the alarm, or
 * monsters around), then the time of day. Battle and tension are held for a
 * while so the music does not flicker between them.
 *
 * `now` is in seconds on any steady clock.
 */
export class MoodTracker {
  private current: MusicMood = 'day';
  private lastBlow = -Infinity;
  private battleSince = -Infinity;
  private griefUntil = -Infinity;
  private tensionUntil = -Infinity;
  /** during the current battle */
  private monsterFell = false;
  private villagerFell = false;
  private pending: Sting | null = null;

  get mood(): MusicMood {
    return this.current;
  }

  /** Notes a tick's events. */
  noteEvents(events: readonly WireEvent[], listener: Listener, selectedId: string | null, now: number): void {
    const watched = (x: number, z: number, ...ids: string[]): boolean => inView(x, z, listener) || (selectedId !== null && ids.includes(selectedId));
    for (const event of events) {
      if (event.type === 'attack') {
        if (watched(event.from.x, event.from.z, event.attackerId, event.targetId) || inView(event.to.x, event.to.z, listener)) this.lastBlow = now;
      } else if (event.type === 'hit') {
        if (watched(event.pos.x, event.pos.z, event.attackerId, event.targetId)) this.lastBlow = now;
      } else if (event.type === 'death' && watched(event.pos.x, event.pos.z, event.id)) {
        if (event.monster) {
          this.monsterFell = true;
        } else {
          this.villagerFell = true;
          this.griefUntil = now + MOOD_TIMING.griefFor;
          // Mid-fight, the sting waits for the fight to end.
          if (this.current !== 'battle') this.pending = 'defeat';
        }
      }
    }
  }

  /** The mood the music should have now. */
  update(scene: MusicScene, now: number): MusicMood {
    if (scene.selectedMood === 'fight') this.lastBlow = now;
    const fighting = now - this.lastBlow < MOOD_TIMING.battleAfter;
    let next: MusicMood;
    if (fighting || (this.current === 'battle' && now - this.battleSince < MOOD_TIMING.battleAtLeast)) next = 'battle';
    else if (now < this.griefUntil) next = 'grief';
    else if (scene.alarm || scene.monstersNear > 0 || (this.current === 'tension' && now < this.tensionUntil)) next = 'tension';
    else next = timeOfDayMood(scene.hour);

    if (next === 'battle' && this.current !== 'battle') {
      this.battleSince = now;
      this.monsterFell = false;
      this.villagerFell = false;
    } else if (this.current === 'battle' && next !== 'battle') {
      if (this.villagerFell) {
        this.pending = 'defeat';
        this.griefUntil = now + MOOD_TIMING.griefFor;
        next = 'grief';
      } else if (this.monsterFell) {
        this.pending = 'victory';
      }
    }
    if (next === 'tension' && this.current !== 'tension') this.tensionUntil = now + MOOD_TIMING.tensionAtLeast;
    this.current = next;
    return next;
  }

  /** A sting that should play now, once. */
  takeSting(): Sting | null {
    const sting = this.pending;
    this.pending = null;
    return sting;
  }
}
