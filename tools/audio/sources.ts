// Where every sound in public/audio/ comes from, and how it is cut. Read by
// build-audio.ts, which downloads the sources and writes the game's files.
// Only CC0, public domain and CC BY are used; CC BY needs the credit that
// build-audio.ts writes to public/audio/CREDITS.md and src/audio/credits.ts.

export type License = 'CC0' | 'Public domain' | 'CC BY 4.0';

export interface Source {
  title: string;
  author: string;
  license: License;
  /** the page that states the license */
  page: string;
  /** the file to download */
  url: string;
  /** for archives: the file inside it */
  member?: string;
}

const OGA = 'https://opengameart.org/sites/default/files/';
const COMMONS = 'https://upload.wikimedia.org/wikipedia/commons/';
const NATURE_PACK = `${OGA}nature_ambient_pack_vol_1_-_mp3.7z`;
const NATURE_PAGE = 'https://opengameart.org/content/jc-sounds-nature-ambient-pack-vol-1';
const RPG_SFX = `${OGA}80-CC0-RPG-SFX_0.zip`;
const CREATURE_SFX = `${OGA}80-CC0-creature-SFX_0.zip`;
const RPG_SOUND_PACK = `${OGA}rpg_sound_pack.zip`;
const METAL_WOOD_SFX = `${OGA}100-CC0-wood-metal-SFX.zip`;

function nature(title: string, member: string): Source {
  return {
    title: `Nature Ambient Pack Vol 1: ${title}`,
    author: 'JC Sounds',
    license: 'CC BY 4.0',
    page: NATURE_PAGE,
    url: NATURE_PACK,
    member: `Nature Ambient Pack Vol 1 - mp3/${member}`,
  };
}

function randomMind(title: string, page: string, file: string): Source {
  return { title, author: 'RandomMind', license: 'CC0', page: `https://opengameart.org/content/${page}`, url: OGA + file };
}

export const SOURCES = {
  forestDay: nature('Forest Day', 'Forest Enviroments/AMB_Nature Pack Vol 1_Forest Enviroments_Forest Day.mp3'),
  pineForest: nature('Pine Forest', 'Forest Enviroments/AMB_Nature Pack Vol 1_Forest Enviroments_Pine Forest .mp3'),
  forestNight: nature('Forest Night', 'Forest Enviroments/AMB_Nature Pack Vol 1_Forest Enviroments_Forest Night.mp3'),
  winterWind: nature('Winter Wind', 'Weather/AMB_Nature Pack Vol 1_Weather_Winter Wind.mp3'),
  mountainStream: nature('Mountain Stream', 'Water Enviroments/AMB_Nature Pack Vol 1_Water Enviroments_Mountain Stream.mp3'),
  peacefulLake: nature('Peaceful Lake', 'Water Enviroments/AMB_Nature Pack Vol 1_Water Enviroments_Peaceful Lake .mp3'),
  campfire: nature('Campfire Crackling', 'Fire & Elemental/AMB_Nature Pack Vol 1_ Fire & Elemental_Campfire Crackling.mp3'),
  torch: nature('Torch Flame', 'Fire & Elemental/AMB_Nature Pack Vol 1_ Fire & Elemental_Torch Flame.mp3'),

  restaurant: {
    title: 'Restaurant ambience',
    author: 'stephan (PDSounds)',
    license: 'Public domain',
    page: 'https://commons.wikimedia.org/wiki/File:Restaurant_ambience.ogg',
    url: `${COMMONS}b/b5/Restaurant_ambience.ogg`,
  },
  crowd: {
    title: 'Festival concert people crowd',
    author: 'stephan (PDSounds)',
    license: 'Public domain',
    page: 'https://commons.wikimedia.org/wiki/File:Festival_concert_people_crowd.ogg',
    url: `${COMMONS}1/15/Festival_concert_people_crowd.ogg`,
  },
  chickens: {
    title: 'Chickens demanding food',
    author: 'alys (PDSounds)',
    license: 'Public domain',
    page: 'https://commons.wikimedia.org/wiki/File:Chickens_demanding_food.ogg',
    url: `${COMMONS}d/d6/Chickens_demanding_food.ogg`,
  },
  churchBells: {
    title: 'Church bells, Leverkusen 2007',
    author: 'natalie (PDSounds)',
    license: 'Public domain',
    page: 'https://commons.wikimedia.org/wiki/File:Church_bells_-_Leverkusen,_2007.oga',
    url: `${COMMONS}3/36/Church_bells_-_Leverkusen%2C_2007.oga`,
  },
  warHorn: {
    title: 'Their Coming (generic horn sound)',
    author: 'StumpyStrust',
    license: 'CC0',
    page: 'https://opengameart.org/content/their-coming-generic-horn-sound',
    url: `${OGA}theircoming3_0.ogg`,
  },
  anvil: {
    title: "Blacksmith's Hammer",
    author: 'VishwaJai',
    license: 'CC0',
    page: 'https://opengameart.org/content/blacksmiths-hammer',
    url: `${OGA}blacksmithhammer.wav`,
  },
  hammer1: { title: '100 CC0 metal and wood SFX', author: 'rubberduck', license: 'CC0', page: 'https://opengameart.org/content/100-cc0-metal-and-wood-sfx', url: METAL_WOOD_SFX, member: 'hammer_01.ogg' },
  hammer2: { title: '100 CC0 metal and wood SFX', author: 'rubberduck', license: 'CC0', page: 'https://opengameart.org/content/100-cc0-metal-and-wood-sfx', url: METAL_WOOD_SFX, member: 'hammer_03.ogg' },

  blade: { title: '80 CC0 RPG SFX', author: 'rubberduck', license: 'CC0', page: 'https://opengameart.org/content/80-cc0-rpg-sfx', url: RPG_SFX, member: 'blade_03.ogg' },
  creatureDie: { title: '80 CC0 RPG SFX', author: 'rubberduck', license: 'CC0', page: 'https://opengameart.org/content/80-cc0-rpg-sfx', url: RPG_SFX, member: 'creature_die_01.ogg' },
  clang: { title: '80 CC0 RPG SFX', author: 'rubberduck', license: 'CC0', page: 'https://opengameart.org/content/80-cc0-rpg-sfx', url: RPG_SFX, member: 'metal_02.ogg' },
  roar: { title: '80 CC0 RPG SFX', author: 'rubberduck', license: 'CC0', page: 'https://opengameart.org/content/80-cc0-rpg-sfx', url: RPG_SFX, member: 'creature_roar_03.ogg' },
  hurt: { title: '80 CC0 creature SFX', author: 'rubberduck', license: 'CC0', page: 'https://opengameart.org/content/80-cc0-creature-sfx', url: CREATURE_SFX, member: 'hurt_01.ogg' },
  groan: { title: '80 CC0 creature SFX', author: 'rubberduck', license: 'CC0', page: 'https://opengameart.org/content/80-cc0-creature-sfx', url: CREATURE_SFX, member: 'grunt_01.ogg' },
  swing: { title: 'RPG Sound Pack', author: 'artisticdude', license: 'CC0', page: 'https://opengameart.org/content/rpg-sound-pack', url: RPG_SOUND_PACK, member: 'RPG Sound Pack/battle/swing.wav' },
  whoosh: { title: 'RPG Sound Pack', author: 'artisticdude', license: 'CC0', page: 'https://opengameart.org/content/rpg-sound-pack', url: RPG_SOUND_PACK, member: 'RPG Sound Pack/battle/swing3.wav' },
  magic: { title: 'RPG Sound Pack', author: 'artisticdude', license: 'CC0', page: 'https://opengameart.org/content/rpg-sound-pack', url: RPG_SOUND_PACK, member: 'RPG Sound Pack/battle/magic1.wav' },

  marketDay: randomMind('Medieval: Market Day', 'medieval-market-day', 'Market_Day.mp3'),
  minstrelDance: randomMind('Medieval: Minstrel Dance', 'medieval-minstrel-dance', 'Minstrel_Dance_0.mp3'),
  harvestSeason: randomMind('Medieval: Harvest Season', 'medieval-harvest-season', 'harvestseason_2.mp3'),
  oldTowerInn: randomMind('Medieval: The Old Tower Inn', 'medieval-the-old-tower-inn', 'The_Old_Tower_Inn.mp3'),
  bardsTale: randomMind("Medieval: The Bard's Tale", 'medieval-the-bards-tale', 'The_Bards_Tale.mp3'),
  risingMoon: randomMind('Fantasy: Rising Moon', 'fantasy-rising-moon', 'Rising_Moon_0.mp3'),
  battle: randomMind('Medieval: Battle', 'medieval-battle', 'battle_8.mp3'),
  lament: randomMind("Fantasy: Lament for a Warrior's Soul", 'fantasy-lament-for-a-warriors-soul', 'Lament_for_a_Warriors_Soul_REUPLOAD.mp3'),
  victory: randomMind('Medieval: Victory Theme', 'medieval-victory-theme', 'victory_0.mp3'),
  defeat: randomMind('Medieval: Defeat Theme', 'medieval-defeat-theme', 'defeat_0.mp3'),
  peacefulForest: { title: 'Peaceful Forest', author: 'Samza', license: 'CC0', page: 'https://opengameart.org/content/peaceful-forest', url: `${OGA}Peaceful%20forest.mp3` },
  darkForest: { title: 'Dark Forest Theme', author: 'cynicmusic', license: 'CC0', page: 'https://opengameart.org/content/dark-forest-theme', url: `${OGA}GameMusic_ForestTheme_24_0.mp3` },
  evilApproach: { title: 'Evil Approach', author: 'nene', license: 'CC0', page: 'https://opengameart.org/content/evil-approach', url: `${OGA}Evil%20Approach.wav` },
  pursuit: { title: 'Determined Pursuit', author: 'Emma_MA', license: 'CC0', page: 'https://opengameart.org/content/determined-pursuit-epic-orchestra-loop', url: `${OGA}determined_pursuit_loop.wav` },
} satisfies Record<string, Source>;

export type SourceId = keyof typeof SOURCES;

/** One recording placed in a loop: a stretch of the source, how loud, and (for short sounds) where. */
export interface Layer {
  source: SourceId;
  /** seconds into the source */
  from?: number;
  gain?: number;
  /** ffmpeg filters applied while decoding, e.g. a low-pass to push a crowd into the distance */
  filter?: string;
  /** for one-shots in a loop: seconds into the loop where each copy starts */
  at?: readonly number[];
}

export interface AmbientLoop {
  slot: string;
  seconds: number;
  layers: readonly Layer[];
}

export interface Effect {
  slot: string;
  source: SourceId;
  from?: number;
  /** cut to at most this long, fading out over the last quarter */
  seconds?: number;
  filter?: string;
}

/** Ambient slots (see src/audio/slots.ts). Each is cut into a seamless loop. */
export const AMBIENT: readonly AmbientLoop[] = [
  { slot: 'birds_day', seconds: 40, layers: [{ source: 'forestDay', from: 10 }] },
  { slot: 'wind', seconds: 40, layers: [{ source: 'winterWind', from: 20 }] },
  { slot: 'forest', seconds: 40, layers: [{ source: 'pineForest', from: 25 }] },
  { slot: 'river', seconds: 30, layers: [{ source: 'mountainStream', from: 15 }] },
  { slot: 'shore', seconds: 30, layers: [{ source: 'peacefulLake', from: 15, filter: 'acompressor=threshold=-28dB:ratio=4:attack=5:release=120' }] },
  { slot: 'crickets_night', seconds: 40, layers: [{ source: 'forestNight', from: 10 }] },
  {
    slot: 'village_day',
    seconds: 40,
    layers: [
      { source: 'crowd', from: 8, gain: 0.8, filter: 'lowpass=f=2400,highpass=f=120' },
      { source: 'chickens', gain: 0.35, at: [6] },
    ],
  },
  { slot: 'tavern', seconds: 40, layers: [{ source: 'restaurant', from: 12, filter: 'lowpass=f=5000' }] },
  {
    slot: 'smithy',
    seconds: 16,
    layers: [
      { source: 'torch', from: 5, gain: 0.25 },
      // A smith's rhythm: a run of blows, a pause to turn the work, another run.
      { source: 'anvil', gain: 0.9, at: [0.4, 1.15, 1.9, 2.65, 3.4, 7.2, 7.85, 8.5, 9.15, 12.1, 12.9, 13.7] },
      { source: 'hammer1', gain: 0.45, at: [4.3, 10.1] },
      { source: 'hammer2', gain: 0.4, at: [14.6] },
    ],
  },
  // Crackle is all peaks; compress it so it can be levelled with the other loops.
  { slot: 'campfire', seconds: 30, layers: [{ source: 'campfire', from: 10, filter: 'acompressor=threshold=-30dB:ratio=8:attack=0.1:release=40' }] },
];

/** Effect slots: short one-shots. */
export const EFFECTS: readonly Effect[] = [
  { slot: 'sword_hit', source: 'blade' },
  { slot: 'block_clang', source: 'clang' },
  { slot: 'dodge_whoosh', source: 'whoosh' },
  { slot: 'arrow_shot', source: 'swing' },
  { slot: 'magic_cast', source: 'magic' },
  { slot: 'hurt', source: 'hurt' },
  { slot: 'death', source: 'groan', filter: 'asetrate=44100*0.9,aresample=44100' },
  { slot: 'monster_spawn', source: 'roar' },
  { slot: 'monster_death', source: 'creatureDie' },
  { slot: 'alarm', source: 'churchBells', from: 4, seconds: 6 },
  { slot: 'war_horn', source: 'warHorn' },
];

/** Music: the track ids are listed in src/audio/musicTracks.ts. */
export const MUSIC: Readonly<Record<string, { source: SourceId; seconds?: number }>> = {
  market_day: { source: 'marketDay' },
  minstrel_dance: { source: 'minstrelDance' },
  harvest_season: { source: 'harvestSeason', seconds: 150 },
  old_tower_inn: { source: 'oldTowerInn' },
  bards_tale: { source: 'bardsTale', seconds: 150 },
  rising_moon: { source: 'risingMoon', seconds: 150 },
  peaceful_forest: { source: 'peacefulForest' },
  dark_forest: { source: 'darkForest' },
  evil_approach: { source: 'evilApproach' },
  battle: { source: 'battle' },
  determined_pursuit: { source: 'pursuit' },
  lament: { source: 'lament', seconds: 150 },
  sting_victory: { source: 'victory', seconds: 9 },
  sting_defeat: { source: 'defeat', seconds: 12 },
};
