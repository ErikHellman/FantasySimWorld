/** Simulation timing. One game minute is one real second at 1x speed. */
export const TICKS_PER_SECOND = 20;
export const TICK_SECONDS = 1 / TICKS_PER_SECOND;
export const TICKS_PER_GAME_MINUTE = 20;
export const MINUTES_PER_DAY = 24 * 60;
export const TICKS_PER_DAY = MINUTES_PER_DAY * TICKS_PER_GAME_MINUTE;
export const GAME_MINUTES_PER_TICK = 1 / TICKS_PER_GAME_MINUTE;

/** The world starts at 06:00 on day 1. */
export const START_MINUTE = 6 * 60;

export interface GameTime {
  day: number;
  /** minutes since midnight, fractional */
  minuteOfDay: number;
  hour: number;
  /** total game minutes since the start of day 1 */
  totalMinutes: number;
}

export function gameTime(tick: number): GameTime {
  const totalMinutes = START_MINUTE + tick * GAME_MINUTES_PER_TICK;
  const day = Math.floor(totalMinutes / MINUTES_PER_DAY) + 1;
  const minuteOfDay = totalMinutes % MINUTES_PER_DAY;
  return { day, minuteOfDay, hour: minuteOfDay / 60, totalMinutes };
}

export function minutesToTicks(minutes: number): number {
  return Math.round(minutes * TICKS_PER_GAME_MINUTE);
}

export function secondsToTicks(seconds: number): number {
  return Math.max(1, Math.round(seconds * TICKS_PER_SECOND));
}

export function formatClock(minuteOfDay: number): string {
  const h = Math.floor(minuteOfDay / 60) % 24;
  const m = Math.floor(minuteOfDay % 60);
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`;
}

export function isNight(hour: number): boolean {
  return hour >= 21 || hour < 5.5;
}

export function partOfDay(hour: number): string {
  if (hour < 5.5) return 'night';
  if (hour < 8) return 'early morning';
  if (hour < 12) return 'morning';
  if (hour < 14) return 'midday';
  if (hour < 18) return 'afternoon';
  if (hour < 21) return 'evening';
  return 'night';
}
