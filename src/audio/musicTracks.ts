import type { MusicMood, Sting } from './mood';

/**
 * Background music, played from public/audio/music/<id>.mp3. Each mood has a
 * small playlist; the player rotates through it. Where the files come from is
 * in tools/audio/sources.ts (rebuilt with `pnpm audio`).
 */
export interface MusicTrack {
  id: string;
  title: string;
  moods: readonly MusicMood[];
  /** loudness relative to the other tracks */
  gain: number;
}

export const MUSIC_TRACKS: readonly MusicTrack[] = [
  { id: 'market_day', title: 'Market Day', moods: ['day'], gain: 0.8 },
  { id: 'minstrel_dance', title: 'Minstrel Dance', moods: ['day'], gain: 0.8 },
  { id: 'harvest_season', title: 'Harvest Season', moods: ['day'], gain: 0.8 },
  { id: 'old_tower_inn', title: 'The Old Tower Inn', moods: ['evening'], gain: 0.85 },
  { id: 'bards_tale', title: "The Bard's Tale", moods: ['evening'], gain: 0.85 },
  { id: 'rising_moon', title: 'Rising Moon', moods: ['night'], gain: 0.6 },
  { id: 'peaceful_forest', title: 'Peaceful Forest', moods: ['night'], gain: 0.6 },
  { id: 'dark_forest', title: 'Dark Forest Theme', moods: ['tension'], gain: 0.9 },
  { id: 'evil_approach', title: 'Evil Approach', moods: ['tension'], gain: 0.8 },
  { id: 'battle', title: 'Battle', moods: ['battle'], gain: 1 },
  { id: 'determined_pursuit', title: 'Determined Pursuit', moods: ['battle'], gain: 0.95 },
  { id: 'lament', title: "Lament for a Warrior's Soul", moods: ['grief'], gain: 0.9 },
];

export const STING_TRACKS: Readonly<Record<Sting, string>> = { victory: 'sting_victory', defeat: 'sting_defeat' };
