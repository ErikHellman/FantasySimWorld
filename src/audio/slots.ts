/**
 * Named sound slots, played from public/audio/<id>.ogg (or .mp3 / .wav). The
 * shipped files are built by `pnpm audio` from tools/audio/sources.ts; any of
 * them can be replaced by dropping in another file. A slot with no file is
 * simply silent.
 */
export interface AudioSlot {
  id: string;
  label: string;
  kind: 'ambient' | 'effect';
  /** loudness of the slot relative to the others */
  gain: number;
}

export const AUDIO_SLOTS: readonly AudioSlot[] = [
  { id: 'birds_day', label: 'Birdsong, daytime loop', kind: 'ambient', gain: 0.6 },
  { id: 'wind', label: 'Soft wind loop, heard when zoomed out', kind: 'ambient', gain: 0.5 },
  { id: 'forest', label: 'Forest loop (leaves, distant birds)', kind: 'ambient', gain: 0.6 },
  { id: 'river', label: 'Running water loop, near the river', kind: 'ambient', gain: 0.7 },
  { id: 'shore', label: 'Lapping water loop, near the lake', kind: 'ambient', gain: 0.6 },
  { id: 'crickets_night', label: 'Crickets and night insects loop', kind: 'ambient', gain: 0.55 },
  { id: 'village_day', label: 'Village murmur loop (voices, hens, a cart)', kind: 'ambient', gain: 0.5 },
  { id: 'tavern', label: 'Tavern chatter loop, when people are at the tables', kind: 'ambient', gain: 0.7 },
  { id: 'smithy', label: 'Hammer on anvil loop, when someone is at the forge', kind: 'ambient', gain: 0.7 },
  { id: 'campfire', label: 'Crackling fire loop, at the glade', kind: 'ambient', gain: 0.7 },
  { id: 'sword_hit', label: 'A weapon striking', kind: 'effect', gain: 0.8 },
  { id: 'arrow_shot', label: 'A bow or crossbow loosing', kind: 'effect', gain: 0.7 },
  { id: 'magic_cast', label: 'A spell being cast', kind: 'effect', gain: 0.7 },
  { id: 'hurt', label: 'A villager crying out in pain', kind: 'effect', gain: 0.8 },
  { id: 'death', label: 'A villager falling', kind: 'effect', gain: 0.9 },
  { id: 'monster_spawn', label: 'The dead clawing out of the ground', kind: 'effect', gain: 0.9 },
  { id: 'monster_death', label: 'A skeleton collapsing', kind: 'effect', gain: 0.8 },
  { id: 'alarm', label: 'The alarm being raised (a bell or horn)', kind: 'effect', gain: 1 },
];

export const AUDIO_EXTENSIONS = ['ogg', 'mp3', 'wav'] as const;
