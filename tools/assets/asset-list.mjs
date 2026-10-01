// Which KayKit source files end up in public/assets. Paths are relative to the
// collection root (see build-assets.mjs for how the root is resolved).

const HEX = 'KayKit Medieval Hexagon Pack 1.0.1/Assets/gltf';
const ADV = 'KayKit Adventurers 2.0';
const SKEL = 'KayKit Skeletons 1.1';
const ANIM = 'KayKit Character Animations 1.1/Animations/gltf/Rig_Medium';
const S4 = 'KayKit Mystery Monthly Series 4 (1.1)';
const S5 = 'KayKit Mystery Monthly Series 5 (1.1)';
const S6 = 'KayKit Mystery Monthly Series 6 (1.1)';

const ALL_COLOURS = ['blue', 'green', 'red', 'yellow'];
const BLUE_ONLY_BUILDINGS = [
  'archeryrange', 'barracks', 'church', 'docks', 'lumbermill', 'mine', 'shrine', 'stables',
  'townhall', 'watchtower', 'watermill', 'well', 'windmill', 'workshop', 'tower_A',
];
const EVERY_COLOUR_BUILDINGS = ['home_A', 'home_B', 'tavern', 'market', 'blacksmith', 'tent'];

/**
 * Environment bundles: every matching model becomes a named node in one GLB.
 * `include`/`exclude` are tested against the file name without extension.
 * `rename` maps a file name to the model id used at runtime.
 */
export const bundles = [
  {
    id: 'hex',
    sources: [
      { dir: `${HEX}/tiles/base` },
      { dir: `${HEX}/tiles/coast` },
      { dir: `${HEX}/tiles/rivers` },
      { dir: `${HEX}/tiles/roads` },
      { dir: `${HEX}/buildings/neutral`, include: /^building_(bridge_A|bridge_B|grain|dirt|destroyed)$/ },
      ...ALL_COLOURS.map((colour) => ({
        dir: `${HEX}/buildings/${colour}`,
        include: new RegExp(
          `^building_(${[
            ...EVERY_COLOUR_BUILDINGS,
            ...(colour === 'blue' ? BLUE_ONLY_BUILDINGS : []),
          ].join('|')})_${colour}$`,
        ),
      })),
      { dir: `${HEX}/decoration/nature` },
      { dir: `${HEX}/decoration/props`, include: /^(boat|boatrack|anchor|flag_.*|target|weaponrack)$/ },
    ],
  },
  {
    id: 'forest',
    sources: [
      {
        dir: 'KayKit Forest Nature Pack 1.0/Assets/gltf/Color1',
        include: /^(Tree|Bush|Rock|Grass)_/,
        exclude: /_Singlesided/,
        rename: (name) => name.replace(/_Color1$/, ''),
      },
    ],
  },
  {
    id: 'halloween',
    sources: [
      {
        dir: 'KayKit Halloween Bits 1.0/Assets/gltf',
        exclude: /^(candy|lollipop|tractor|maze|candle|wooden_gate_halloween)|_decorated$|jackolantern/,
      },
    ],
  },
  {
    id: 'dungeon',
    sources: [
      {
        dir: 'KayKit Dungeon Pack 1.1/Assets/gltf',
        include:
          /^(banner_(blue|red|green|yellow)|banner_shield_(blue|red)|barrel_.*|bench|chair|stool|stool_round|keg|keg_decorated|crate_.*|crates_stacked|box_.*|bucket|chest|chest_gold|table_(long|medium|small|round_.*)(_decorated_[A-C]|_tablecloth)?|torch_lit|torch|pillar|column|post|rocks|rocks_small|rocks_gold|rubble_half|rubble_large|wall_broken|wall_cracked|wall_half|wall_arched|wall_corner|wall|floor_dirt_.*|floor_tile_small_broken_[AB]|floor_tile_small_weeds_[AB]|floor_tile_large_rocks|bed_floor|plate_food_[AB]|bottle_A_brown|bottle_B_green|sword_shield|sword_shield_broken|barrier|barrier_half|barrier_corner|barrier_column)$/,
      },
    ],
  },
  {
    id: 'resource',
    sources: [
      {
        dir: 'KayKit Resource Bits 1.0/Assets/gltf',
        include:
          /^(Wood_.*|Stone_.*|Food_.*|Iron_(Bar|Bars|Bars_Stack_Small|Nuggets)|Gold_Nuggets|Containers_Crate_.*|Containers_Pile_.*|Textiles_.*|Pallet_Wood.*|Money_Coins_Stack_Small)$/,
      },
    ],
  },
  {
    id: 'tools',
    sources: [
      {
        dir: 'KayKit RPG Tools Bits 1.0/Assets/gltf',
        include:
          /^(anvil|grindstone|bucket_metal|lantern|torch|axe|pickaxe|shovel|hammer|saw|fishing_rod|fishing_tacklebox|map|journal_open|rope_bundle_A|tongs)$/,
      },
    ],
  },
  {
    id: 'props',
    sources: [
      { dir: `${S5}/8 - February 2025 - Caveman/assets/gltf`, include: /^Campfire_/ },
      { dir: `${S5}/11 - May 2025 - Hiker/assets/gltf`, include: /^Tent$/ },
      { dir: `${S5}/5 - November 2024 - Witch/assets/gltf`, include: /^(Cauldron|Mushroom|Basket_Mushrooms|Potionstation|Table_Small)$/ },
      { dir: `${S6}/12 - June 2026 - Farmers/assets/gltf`, include: /^(dirt_plot|carrot|lettuce|pitchfork|wheelbarrow)$/ },
      { dir: `${S4}/4 - October 2023 - Werewolf/assets/gltf`, include: /^log_/ },
      { dir: `${S6}/3 - September 2025 - Cleric/assets/gltf`, include: /^Cleric_Font$/ },
      { dir: `${S4}/10 - April 2024 - Paladin/assets/gltf`, include: /^paladin_statue$/ },
      { dir: 'KayKit Mixed Bag 1/Assets/gltf', include: /^flax_flower_[AB]$/ },
      {
        dir: 'KayKit Restaurant Bits 1.0/Assets/gltf',
        include: /^(crate_(carrots|potatoes|onions|tomatoes|lettuce|cheese|buns)|stew_pot|food_stew)$/,
      },
    ],
  },
  {
    id: 'weapons',
    sources: [
      {
        dir: `${ADV}/Assets/gltf`,
        include:
          /^(sword_1handed|sword_2handed|shield_badge|shield_round|shield_square|shield_spikes|axe_1handed|axe_2handed|shield_round_barbarian|mug_full|dagger|crossbow_1handed|arrow_crossbow|bow_withString|arrow_bow|staff|wand|spellbook_open|druid_staff|engineer_Wrench|potion_medium_red)$/,
      },
      {
        dir: `${SKEL}/assets/gltf`,
        include: /^Skeleton_(Blade|Axe|Dagger|Crossbow|Staff|Scythe|Mace|Shield_Small_A|Shield_Large_A|Arrow)$/,
      },
      { dir: `${S4}/10 - April 2024 - Paladin/assets/gltf`, include: /^paladin_(hammer|shield)$/ },
      { dir: `${S6}/3 - September 2025 - Cleric/assets/gltf`, include: /^Cleric_(Mace|Shield)$/ },
      { dir: `${S5}/12 - June 2025 - Tiefling/assets/gltf`, include: /^Tiefling_Sword$/ },
      { dir: `${S6}/1 - July 2025 - Lorekeeper/assets/gltf`, include: /^Lorekeeper_Staff$/ },
      { dir: `${S5}/5 - November 2024 - Witch/assets/gltf`, include: /^Broom$/ },
      { dir: `${S6}/12 - June 2026 - Farmers/assets/gltf`, include: /^pitchfork$/, rename: () => 'farmer_pitchfork' },
    ],
  },
];

// Props made for this project with Blender (tools/blender/make_props.py); `local` paths are relative to the repo.
bundles.push({ id: 'custom', sources: [{ dir: 'tools/blender/out', local: true }] });

/** Rigged characters, one GLB each (mesh + skeleton, no clips). */
export const characters = [
  ...['Knight', 'Barbarian', 'Mage', 'Ranger', 'Rogue', 'Rogue_Hooded', 'Druid', 'Engineer'].map((name) => ({
    id: name,
    file: `${ADV}/Characters/gltf/${name}.glb`,
  })),
  ...['Skeleton_Minion', 'Skeleton_Warrior', 'Skeleton_Rogue', 'Skeleton_Mage', 'Necromancer'].map((name) => ({
    id: name,
    file: `${SKEL}/characters/gltf/${name}.glb`,
  })),
  { id: 'Paladin', file: `${S4}/10 - April 2024 - Paladin/characters/gltf/Paladin.glb` },
  { id: 'Cleric', file: `${S6}/3 - September 2025 - Cleric/characters/Cleric.glb` },
  { id: 'Witch', file: `${S5}/5 - November 2024 - Witch/characters/Witch.glb` },
  { id: 'Tiefling', file: `${S5}/12 - June 2025 - Tiefling/characters/Tiefling.glb` },
  { id: 'Lorekeeper', file: `${S6}/1 - July 2025 - Lorekeeper/characters/Lorekeeper.glb` },
  { id: 'Farmer_A', file: `${S6}/12 - June 2026 - Farmers/characters/Farmer_A.glb` },
  { id: 'Farmer_B', file: `${S6}/12 - June 2026 - Farmers/characters/Farmer_B.glb` },
];

/** Extra textures that are not referenced by any model (recolours). */
export const extraTextures = [
  ...['knight', 'barbarian', 'mage', 'ranger', 'rogue', 'druid', 'engineer'].flatMap((cls) =>
    ['A', 'B', 'C'].map((v) => ({
      id: `${cls}_texture_alt_${v}`,
      file: `${ADV}/Textures/${cls}_texture_alt_${v}.png`,
    })),
  ),
  { id: 'skeleton_texture_B', file: `${SKEL}/textures/skeleton_texture_B.png` },
];

/** Animation source files and the clips taken from each. */
export const animations = {
  rig: 'Rig_Medium',
  files: [
    {
      file: `${ANIM}/Rig_Medium_General.glb`,
      clips: ['Idle_A', 'Idle_B', 'Death_A', 'Death_A_Pose', 'Death_B', 'Death_B_Pose', 'Hit_A', 'Hit_B', 'Interact', 'PickUp', 'Use_Item', 'Throw'],
    },
    {
      file: `${ANIM}/Rig_Medium_MovementBasic.glb`,
      clips: ['Walking_A', 'Walking_B', 'Walking_C', 'Running_A', 'Running_B'],
    },
    {
      file: `${ANIM}/Rig_Medium_CombatMelee.glb`,
      clips: [
        'Melee_1H_Attack_Chop', 'Melee_1H_Attack_Slice_Diagonal', 'Melee_1H_Attack_Slice_Horizontal', 'Melee_1H_Attack_Stab',
        'Melee_2H_Attack_Chop', 'Melee_2H_Attack_Slice', 'Melee_2H_Attack_Spin', 'Melee_2H_Idle',
        'Melee_Block', 'Melee_Blocking', 'Melee_Block_Hit',
        'Melee_Dualwield_Attack_Chop', 'Melee_Dualwield_Attack_Slice',
        'Melee_Unarmed_Attack_Kick', 'Melee_Unarmed_Attack_Punch_A', 'Melee_Unarmed_Idle',
      ],
    },
    {
      file: `${ANIM}/Rig_Medium_CombatRanged.glb`,
      clips: [
        'Ranged_Bow_Aiming_Idle', 'Ranged_Bow_Draw', 'Ranged_Bow_Idle', 'Ranged_Bow_Release',
        'Ranged_Magic_Raise', 'Ranged_Magic_Shoot', 'Ranged_Magic_Spellcasting', 'Ranged_Magic_Spellcasting_Long', 'Ranged_Magic_Summon',
        'Ranged_1H_Aiming', 'Ranged_1H_Shoot', 'Ranged_1H_Reload',
      ],
    },
    {
      file: `${ANIM}/Rig_Medium_Simulation.glb`,
      clips: [
        'Cheering', 'Waving', 'Lie_Down', 'Lie_Idle', 'Lie_StandUp',
        'Sit_Chair_Down', 'Sit_Chair_Idle', 'Sit_Chair_StandUp',
        'Sit_Floor_Down', 'Sit_Floor_Idle', 'Sit_Floor_StandUp', 'Push_Ups', 'Sit_Ups',
      ],
    },
    {
      file: `${ANIM}/Rig_Medium_Special.glb`,
      clips: [
        'Skeletons_Awaken_Floor', 'Skeletons_Death', 'Skeletons_Death_Pose', 'Skeletons_Idle',
        'Skeletons_Spawn_Ground', 'Skeletons_Taunt', 'Skeletons_Walking', 'Skeletons_Inactive_Floor_Pose',
      ],
    },
    {
      file: `${ANIM}/Rig_Medium_Tools.glb`,
      clips: [
        'Chopping', 'Digging', 'Hammering', 'Pickaxing', 'Sawing',
        'Fishing_Cast', 'Fishing_Idle', 'Fishing_Reeling', 'Fishing_Catch',
        'Working_A', 'Working_B', 'Working_C', 'Holding_A',
      ],
    },
  ],
};
