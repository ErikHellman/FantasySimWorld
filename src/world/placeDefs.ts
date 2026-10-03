/**
 * Hand-authored places. Coordinates are local to the place: the origin is the
 * hex centre (plus `offset`), +z points out of the front (towards the door /
 * the road), +x is to the right when looking at the front from outside... i.e.
 * for a place facing south (0°) local +x is east. Angles are degrees.
 */
export type ActivityId =
  | 'eat' | 'drink' | 'sleep' | 'rest' | 'smith' | 'farm' | 'chop_wood' | 'fish' | 'mine'
  | 'trade' | 'shop' | 'pray' | 'train' | 'forage' | 'watch' | 'relax' | 'brew' | 'mourn' | 'stroll' | 'read_board' | 'rebuild';

export type PlaceKind =
  | 'home' | 'tavern' | 'green' | 'market' | 'smithy' | 'chapel' | 'training' | 'farm' | 'lumber'
  | 'glade' | 'mine' | 'docks' | 'graveyard' | 'ruins' | 'watchtower' | 'road';

export interface PropDef {
  /** "bundle/model" */
  model: string;
  at: readonly [number, number];
  rot?: number;
  scale?: number;
  y?: number;
  /** blocking radius for navigation; omitted = derived from the model size, 0 = walk-through */
  block?: number;
}

export interface SlotDef {
  activities: readonly ActivityId[];
  at: readonly [number, number];
  /** direction the character faces, local degrees (0 = out of the front) */
  face: number;
  /** animation override (an AnimId) */
  anim?: string;
  /** for training: a spot to shoot from rather than strike from */
  ranged?: boolean;
}

/** A large billboard on two posts. Its face looks along board-local +z, towards the people reading it. */
export interface BillboardDef {
  id: string;
  at: readonly [number, number];
  /** local degrees */
  rot: number;
}

/** Size of a billboard's face, in metres. */
export const BILLBOARD_WIDTH = 3.4;
export const BILLBOARD_HEIGHT = 1.9;
/** Height of the bottom of the face above the ground. */
export const BILLBOARD_LIFT = 1.3;
/** Depth of the footprint people walk around. */
export const BILLBOARD_DEPTH = 0.5;
/** The two reading spots, board-local: (±x, z). */
export const BILLBOARD_READ_AT = [0.8, 2.0] as const;

export interface PlaceDef {
  id: string;
  name: string;
  kind: PlaceKind;
  /** one line for LLM prompts */
  description: string;
  hex: readonly [number, number];
  offset?: readonly [number, number];
  /** world direction of local +z, degrees: 0 south, 90 east, 180 north, 270 west */
  facing: number;
  building?: { model: string; at?: readonly [number, number]; scale?: number; rot?: number };
  /** where characters go in and out; default just in front of the building */
  door?: readonly [number, number];
  props?: readonly PropDef[];
  slots?: readonly SlotDef[];
  /** people who can live here */
  beds?: number;
  /** scatter (trees, rocks, grass) is kept this far from the origin */
  clear?: number;
  /** local points where the dead are buried / monsters rise */
  gravePlots?: ReadonlyArray<readonly [number, number]>;
  spawnPoints?: ReadonlyArray<readonly [number, number]>;
  billboards?: readonly BillboardDef[];
}

const seatsAround = (cx: number, cz: number, r: number, activities: readonly ActivityId[]): SlotDef[] => [
  { activities, at: [cx - r, cz], face: 90, anim: 'sit_chair' },
  { activities, at: [cx + r, cz], face: 270, anim: 'sit_chair' },
  { activities, at: [cx, cz - r], face: 0, anim: 'sit_chair' },
  { activities, at: [cx, cz + r], face: 180, anim: 'sit_chair' },
];

const stoolsAround = (cx: number, cz: number, r: number): PropDef[] => [
  { model: 'dungeon/stool', at: [cx - r, cz], block: 0 },
  { model: 'dungeon/stool', at: [cx + r, cz], block: 0 },
  { model: 'dungeon/stool', at: [cx, cz - r], block: 0 },
  { model: 'dungeon/stool', at: [cx, cz + r], block: 0 },
];

const FLOWERS = ['custom/flowers_red', 'custom/flowers_yellow', 'custom/flowers_violet', 'custom/flowers_mixed'];

interface HomeSpec {
  n: number;
  name: string;
  hex: readonly [number, number];
  facing: number;
  model: string;
}

function home(spec: HomeSpec): PlaceDef {
  const big = spec.model.includes('home_B');
  const front = big ? 3.9 : 2.7;
  return {
    id: `home_${spec.n}`,
    name: spec.name,
    kind: 'home',
    description: 'A cottage where villagers sleep.',
    hex: spec.hex,
    facing: spec.facing,
    building: { model: `hex/${spec.model}`, at: [0, -1.5] },
    door: [0, front - 1.5 + 1.4],
    beds: 2,
    clear: 7.5,
    props: [
      { model: 'halloween/bench', at: [-2.6, front - 0.7], rot: 0, block: 0 },
      { model: 'dungeon/barrel_small', at: [2.9, front - 1.0] },
      { model: 'halloween/post_lantern', at: [3.6, front + 1.6], rot: 90, block: 0.3 },
      { model: FLOWERS[spec.n % FLOWERS.length] as string, at: [1.5, front - 1.0], scale: 1.5, block: 0 },
      { model: FLOWERS[(spec.n + 2) % FLOWERS.length] as string, at: [-1.3, front - 1.1], scale: 1.3, rot: 70, block: 0 },
    ],
    slots: [
      { activities: ['rest'], at: [-3.0, front - 0.7], face: 0, anim: 'sit_chair' },
      { activities: ['rest'], at: [-2.2, front - 0.7], face: 0, anim: 'sit_chair' },
    ],
  };
}

export const PLACE_DEFS: readonly PlaceDef[] = [
  {
    id: 'tavern',
    name: 'The Gilded Tankard',
    kind: 'tavern',
    description: 'The village tavern: food, ale and gossip at the tables outside.',
    hex: [5, 6],
    facing: 0,
    building: { model: 'hex/building_tavern_red', at: [0, -3] },
    door: [0, 2.8],
    clear: 10,
    props: [
      { model: 'dungeon/table_medium_decorated_A', at: [-2.8, 5.4] },
      { model: 'dungeon/table_medium_decorated_B', at: [2.8, 5.4] },
      ...stoolsAround(-2.8, 5.4, 1.55),
      ...stoolsAround(2.8, 5.4, 1.55),
      { model: 'dungeon/keg_decorated', at: [-5.6, 1.6], rot: 20 },
      { model: 'dungeon/barrel_large', at: [5.6, 1.4] },
      { model: 'dungeon/barrel_small_stack', at: [6.2, 3.2], rot: 30 },
      { model: 'halloween/post_lantern', at: [-6.2, 6.6], rot: 90, block: 0.3 },
      { model: 'halloween/post_lantern', at: [6.2, 6.6], rot: 270, block: 0.3 },
      { model: 'dungeon/banner_red', at: [2.2, 1.0], y: 1.6, block: 0 },
    ],
    slots: [...seatsAround(-2.8, 5.4, 1.55, ['eat', 'drink']), ...seatsAround(2.8, 5.4, 1.55, ['eat', 'drink'])],
    billboards: [{ id: 'tavern_board', at: [-4.6, 9.0], rot: 10 }],
  },
  {
    id: 'green',
    name: 'the village green',
    kind: 'green',
    description: 'The open green at the crossroads, with the well and benches. Everyone passes through.',
    hex: [7, 6],
    facing: 0,
    offset: [-1, 1.5],
    clear: 9,
    props: [
      { model: 'hex/building_well_blue', at: [0, 0], scale: 4.2, block: 1.9 },
      { model: 'halloween/bench', at: [-4.2, 1.5], rot: 90, block: 0 },
      { model: 'halloween/bench', at: [4.2, 1.5], rot: 270, block: 0 },
      { model: 'halloween/bench', at: [0, -4.4], rot: 0, block: 0 },
      { model: 'halloween/sign_both', at: [-5.5, 6.2], rot: 20, block: 0.3 },
      { model: 'halloween/post_lantern', at: [5.2, 5.6], rot: 270, block: 0.3 },
      { model: 'custom/notice_board', at: [6.2, -2.4], rot: -35, block: 0.9 },
      { model: 'custom/flowers_mixed', at: [2.6, -2.2], scale: 1.6, block: 0 },
      { model: 'custom/flowers_yellow', at: [-2.4, -2.6], scale: 1.5, block: 0 },
      { model: 'custom/flowers_red', at: [-3.0, 4.6], scale: 1.5, block: 0 },
    ],
    slots: [
      { activities: ['rest'], at: [-4.2, 1.0], face: 90, anim: 'sit_chair' },
      { activities: ['rest'], at: [-4.2, 2.0], face: 90, anim: 'sit_chair' },
      { activities: ['rest'], at: [4.2, 1.0], face: 270, anim: 'sit_chair' },
      { activities: ['rest'], at: [4.2, 2.0], face: 270, anim: 'sit_chair' },
      { activities: ['rest'], at: [-0.5, -4.4], face: 0, anim: 'sit_chair' },
      { activities: ['rest'], at: [0.5, -4.4], face: 0, anim: 'sit_chair' },
    ],
    billboards: [{ id: 'green_board', at: [-2.0, 8.0], rot: 0 }],
  },
  {
    id: 'market',
    name: 'the market',
    kind: 'market',
    description: 'Market stalls with produce, cloth and trinkets; a place to trade and to buy food or small gifts.',
    hex: [8, 6],
    facing: 0,
    building: { model: 'hex/building_market_yellow', at: [0, -2.6] },
    door: [0, 4],
    clear: 10,
    props: [
      { model: 'props/crate_carrots', at: [-4.6, 3.4], rot: 8 },
      { model: 'props/crate_tomatoes', at: [-2.4, 3.6], rot: -4 },
      { model: 'props/crate_cheese', at: [2.6, 3.5], rot: 5 },
      { model: 'props/crate_buns', at: [4.8, 3.3], rot: -8 },
      { model: 'resource/Textiles_Stack_Large_Colored', at: [6.6, 1.2], rot: 15 },
      { model: 'resource/Food_Basket_A_Berries', at: [0.2, 3.2], block: 0 },
      { model: 'halloween/wagon', at: [-7.6, 1.0], rot: 100 },
    ],
    slots: [
      { activities: ['trade'], at: [-3.4, 1.7], face: 0, anim: 'work_a' },
      { activities: ['trade'], at: [3.6, 1.7], face: 0, anim: 'work_a' },
      { activities: ['shop'], at: [-3.4, 5.4], face: 180, anim: 'interact' },
      { activities: ['shop'], at: [0.2, 5.2], face: 180, anim: 'interact' },
      { activities: ['shop'], at: [3.6, 5.4], face: 180, anim: 'interact' },
    ],
    billboards: [{ id: 'market_board', at: [8.4, 6.4], rot: -40 }],
  },
  {
    id: 'smithy',
    name: 'the smithy',
    kind: 'smithy',
    description: 'The forge, with the anvil and grindstone out front. Weapons are mended and tools made here.',
    hex: [7, 8],
    facing: 180,
    building: { model: 'hex/building_blacksmith_blue', at: [0, -2.4] },
    door: [1.5, 3.4],
    clear: 9.5,
    props: [
      { model: 'tools/anvil', at: [-2.6, 4.6], rot: 90 },
      { model: 'tools/grindstone', at: [3.6, 4.4], rot: 0 },
      { model: 'resource/Iron_Bars', at: [-4.8, 3.2], rot: 30 },
      { model: 'dungeon/bucket', at: [-1.0, 3.6], block: 0 },
      { model: 'dungeon/sword_shield', at: [5.6, 2.2], y: 0.8, rot: 0, block: 0 },
      { model: 'resource/Stone_Chunks_Large', at: [-5.6, 5.2] },
    ],
    slots: [
      { activities: ['smith'], at: [-2.6, 5.9], face: 180, anim: 'hammer' },
      { activities: ['smith'], at: [3.6, 6.3], face: 180, anim: 'work_b' },
    ],
  },
  {
    id: 'chapel',
    name: 'the chapel',
    kind: 'chapel',
    description: 'A small stone chapel at the end of the north lane. Quiet; people come to pray and think.',
    hex: [6, 3],
    facing: 0,
    building: { model: 'hex/building_church_blue', at: [0, -2.4] },
    door: [0, 3.2],
    clear: 9,
    props: [
      { model: 'props/Cleric_Font', at: [3.6, 4.4] },
      { model: 'props/paladin_statue', at: [-4.0, 3.6], rot: 20 },
      { model: 'halloween/bench', at: [-2.0, 7.0], rot: 180, block: 0 },
      { model: 'halloween/bench', at: [2.0, 7.0], rot: 180, block: 0 },
      { model: 'halloween/lantern_standing', at: [-1.6, 2.4], block: 0 },
      { model: 'halloween/lantern_standing', at: [1.6, 2.4], block: 0 },
    ],
    slots: [
      { activities: ['pray'], at: [-1.2, 4.6], face: 180, anim: 'sit_floor' },
      { activities: ['pray'], at: [1.2, 4.6], face: 180, anim: 'sit_floor' },
      { activities: ['pray', 'rest'], at: [-2.0, 7.0], face: 180, anim: 'sit_chair' },
      { activities: ['pray', 'rest'], at: [2.0, 7.0], face: 180, anim: 'sit_chair' },
    ],
  },
  {
    id: 'training_yard',
    name: 'the training yard',
    kind: 'training',
    description: 'The barracks yard with practice dummies and archery targets.',
    hex: [3, 6],
    facing: 0,
    building: { model: 'hex/building_barracks_blue', at: [-1.5, -3] },
    door: [-1.5, 3.8],
    clear: 10,
    props: [
      { model: 'halloween/scarecrow', at: [-5.0, 5.6], rot: 0, block: 0.6 },
      { model: 'halloween/scarecrow', at: [-1.6, 6.4], rot: 0, block: 0.6 },
      { model: 'hex/target', at: [5.6, -1.0], scale: 7, rot: 0, block: 0.8 },
      { model: 'hex/target', at: [7.6, 0.4], scale: 7, rot: -20, block: 0.8 },
      { model: 'hex/weaponrack', at: [3.4, 2.6], scale: 7, rot: 0, block: 0.8 },
      { model: 'halloween/haybale', at: [1.6, 6.6], rot: 30 },
      { model: 'custom/fence_wood', at: [-3.3, 10.2] },
      { model: 'custom/fence_wood', at: [-7.8, 6.4], rot: 90 },
    ],
    slots: [
      { activities: ['train'], at: [-5.0, 7.3], face: 180 },
      { activities: ['train'], at: [-1.6, 8.1], face: 180 },
      { activities: ['train'], at: [5.6, 6.6], face: 180, ranged: true },
      { activities: ['train'], at: [7.9, 7.6], face: 180, ranged: true },
    ],
    billboards: [{ id: 'training_board', at: [3.0, 9.6], rot: 0 }],
  },
  {
    id: 'farm',
    name: 'the windmill farm',
    kind: 'farm',
    description: 'The windmill, vegetable plots and grain fields south-west of the village.',
    hex: [2, 9],
    facing: 90,
    building: { model: 'hex/building_windmill_blue', at: [-2, -2.5] },
    door: [-2, 1.8],
    clear: 10,
    props: [
      ...[-3.3, -1.1, 1.1, 3.3].flatMap((x): PropDef[] => [
        { model: 'props/dirt_plot', at: [x + 2.4, 4.4], scale: 2.2, block: 0 },
        { model: 'props/dirt_plot', at: [x + 2.4, 6.6], scale: 2.2, block: 0 },
      ]),
      { model: 'props/lettuce', at: [-0.9, 4.4], y: 0.5, block: 0 },
      { model: 'props/lettuce', at: [1.3, 6.6], y: 0.5, block: 0 },
      { model: 'props/carrot', at: [3.5, 4.4], y: 0.7, block: 0 },
      { model: 'props/carrot', at: [5.7, 6.6], y: 0.7, block: 0 },
      { model: 'props/lettuce', at: [5.7, 4.4], y: 0.5, block: 0 },
      { model: 'props/wheelbarrow', at: [-5.4, 3.6], rot: 60 },
      { model: 'halloween/haybale', at: [3.2, -0.6], rot: 10 },
      { model: 'halloween/haybale', at: [4.4, -2.4], rot: 80 },
      { model: 'halloween/scarecrow', at: [6.6, 1.6], rot: 30, block: 0.5 },
      { model: 'custom/fence_wood', at: [0.4, 8.5] },
      { model: 'custom/fence_wood_broken', at: [4.7, 8.5] },
      { model: 'custom/fence_wood', at: [7.6, 6.2], rot: 90 },
      { model: 'custom/fence_wood_post', at: [7.6, 3.9], block: 0.3 },
    ],
    slots: [
      { activities: ['farm'], at: [-0.9, 5.5], face: 180, anim: 'dig' },
      { activities: ['farm'], at: [2.4, 5.5], face: 0, anim: 'dig' },
      { activities: ['farm'], at: [4.6, 5.5], face: 180, anim: 'work_a' },
    ],
  },
  {
    id: 'lumber_camp',
    name: 'the lumber camp',
    kind: 'lumber',
    description: 'A sawmill at the edge of the western forest, stacked with logs.',
    hex: [2, 5],
    facing: 90,
    building: { model: 'hex/building_lumbermill_blue', at: [0, -2.5] },
    door: [0, 3.6],
    clear: 9.5,
    props: [
      { model: 'resource/Wood_Log_Stack', at: [-4.6, 4.4], rot: 20 },
      { model: 'props/log_stacks', at: [5.0, 3.2], rot: -30 },
      { model: 'hex/tree_single_A_cut', at: [-1.8, 5.6], scale: 7, block: 0.5 },
      { model: 'hex/tree_single_B_cut', at: [2.2, 6.4], scale: 7, block: 0.5 },
      { model: 'resource/Wood_Planks_Stack_Medium', at: [5.6, 6.2], rot: 70 },
    ],
    slots: [
      { activities: ['chop_wood'], at: [-1.8, 7.0], face: 180, anim: 'chop' },
      { activities: ['chop_wood'], at: [2.2, 7.8], face: 180, anim: 'chop' },
    ],
  },
  {
    id: 'glade',
    name: 'the Whispering Glade',
    kind: 'glade',
    description: 'A clearing in the western forest with an old shrine, a campfire and herbs to gather.',
    hex: [3, 3],
    facing: 0,
    clear: 8,
    props: [
      { model: 'halloween/shrine', at: [0, -4.2], block: 0.7 },
      { model: 'props/Campfire_Base', at: [0, 2.2], block: 0.9 },
      { model: 'props/Campfire_Logs', at: [0, 2.2], y: 0.15, block: 0 },
      { model: 'props/Tent', at: [-5.0, -0.4], rot: 70 },
      { model: 'resource/Wood_Log_A', at: [-2.4, 2.6], rot: 15, y: 0.3, block: 0 },
      { model: 'resource/Wood_Log_B', at: [2.4, 2.8], rot: -20, y: 0.3, block: 0 },
      { model: 'props/Potionstation', at: [4.8, -1.6], rot: -40 },
      { model: 'props/Cauldron', at: [5.4, 1.0], block: 0.7 },
      { model: 'custom/mushrooms_red', at: [-3.2, 5.2], scale: 1.6, block: 0 },
      { model: 'custom/mushrooms_brown', at: [3.6, 5.8], scale: 1.5, block: 0 },
      { model: 'custom/mushrooms_red', at: [-1.2, -6.0], scale: 1.8, rot: 120, block: 0 },
      { model: 'custom/flowers_violet', at: [-4.4, 3.6], scale: 1.5, block: 0 },
      { model: 'props/Basket_Mushrooms', at: [3.6, -3.2], block: 0 },
    ],
    slots: [
      { activities: ['pray'], at: [0, -2.6], face: 180, anim: 'sit_floor' },
      { activities: ['relax'], at: [-2.2, 2.4], face: 90, anim: 'sit_floor' },
      { activities: ['relax'], at: [2.2, 2.4], face: 270, anim: 'sit_floor' },
      { activities: ['relax'], at: [0, 4.4], face: 180, anim: 'sit_floor' },
      { activities: ['brew'], at: [3.9, -0.5], face: 140, anim: 'work_c' },
      { activities: ['forage'], at: [-3.2, 6.2], face: 180, anim: 'pick_up' },
      { activities: ['forage'], at: [4.4, 6.4], face: 200, anim: 'pick_up' },
      { activities: ['forage'], at: [-1.2, -7.0], face: 0, anim: 'pick_up' },
    ],
  },
  {
    id: 'mine',
    name: 'the old mine',
    kind: 'mine',
    description: 'A mine shaft in the northern hills where ore and stone are dug.',
    hex: [8, 2],
    facing: 0,
    building: { model: 'hex/building_mine_blue', at: [0, -1.5] },
    door: [0, 5.6],
    clear: 10,
    props: [
      { model: 'dungeon/rocks_gold', at: [-5.6, 5.4], rot: 30 },
      { model: 'dungeon/rocks', at: [5.8, 5.0], rot: -20 },
      { model: 'resource/Iron_Nuggets', at: [3.2, 6.0], block: 0 },
    ],
    slots: [
      { activities: ['mine'], at: [-4.6, 7.4], face: 150, anim: 'pickaxe' },
      { activities: ['mine'], at: [4.8, 7.2], face: 200, anim: 'pickaxe' },
    ],
  },
  {
    id: 'docks',
    name: 'the lake shore',
    kind: 'docks',
    description: 'The sandy shore of the lake, with a boat pulled up on the beach. Good fishing.',
    hex: [8, 12],
    offset: [-3.5, 1.5],
    facing: 90,
    door: [0, -1],
    clear: 7,
    props: [
      { model: 'hex/boat', at: [3.6, 1.0], scale: 7, rot: 25, block: 1.4 },
      { model: 'hex/boatrack', at: [3.2, -3.2], scale: 5, rot: -20, block: 1.2 },
      { model: 'resource/Food_Barrel_Fish', at: [-1.0, -1.6] },
      { model: 'tools/fishing_tacklebox', at: [-0.8, 0.5], block: 0 },
      { model: 'tools/bucket_metal', at: [-3.2, 0.2], block: 0 },
    ],
    slots: [
      { activities: ['fish'], at: [0.5, 1.9], face: 0, anim: 'fish' },
      { activities: ['fish'], at: [-2.0, 1.9], face: 0, anim: 'fish' },
      { activities: ['fish'], at: [-4.5, 1.9], face: 0, anim: 'fish' },
    ],
  },
  {
    id: 'watchtower',
    name: 'the watchtower',
    kind: 'watchtower',
    description: 'The watch post by the bridge road, looking east towards the graveyard.',
    hex: [9, 8],
    facing: 180,
    building: { model: 'hex/building_watchtower_blue', at: [0, -2] },
    door: [0, 2.8],
    clear: 8,
    props: [
      { model: 'custom/banner_board', at: [-4.4, 1.0], block: 0.9 },
      { model: 'dungeon/banner_shield_blue', at: [-4.4, 1.0], y: 0, block: 0 },
      { model: 'dungeon/torch_lit', at: [2.4, 2.2], y: 1.6, block: 0 },
      { model: 'dungeon/crates_stacked', at: [4.6, 0.6], rot: 20 },
    ],
    slots: [
      { activities: ['watch'], at: [-2.4, 6.4], face: 300, anim: 'guard' },
      { activities: ['watch'], at: [3.4, 6.8], face: 290, anim: 'idle' },
    ],
  },
  {
    id: 'graveyard',
    name: 'the graveyard',
    kind: 'graveyard',
    description: 'The old graveyard east of the river. The dead are buried here, and at night some of them do not rest.',
    hex: [15, 4],
    facing: -30,
    clear: 10,
    props: [
      { model: 'halloween/crypt', at: [0, -5.6], rot: 0 },
      { model: 'halloween/fence', at: [-4.0, 6.6], block: 0 },
      { model: 'halloween/fence_broken', at: [4.0, 6.6], block: 0 },
      { model: 'halloween/arch_gate', at: [0, 6.6], block: 0 },
      { model: 'halloween/grave_A', at: [-4.8, -5.0], rot: 0, block: 0.9 },
      { model: 'halloween/grave_B', at: [4.8, -5.0], rot: 0, block: 0.9 },
      { model: 'halloween/grave_A_destroyed', at: [-4.8, -2.5], rot: 0, block: 0.9 },
      { model: 'halloween/gravemarker_A', at: [4.8, -2.5], rot: 0, block: 0.4 },
      { model: 'halloween/tree_dead_large', at: [-6.6, 2.6], rot: 40, block: 0.5 },
      { model: 'halloween/tree_dead_medium', at: [6.4, 4.6], rot: 0, block: 0.4 },
      { model: 'halloween/lantern_standing', at: [-1.9, 5.4], block: 0 },
      { model: 'halloween/lantern_standing', at: [1.9, 5.4], block: 0 },
      { model: 'halloween/skull', at: [5.8, 0.4], rot: 60, block: 0 },
      { model: 'halloween/bone_A', at: [-6.0, -0.6], rot: 20, block: 0 },
      { model: 'halloween/ribcage', at: [6.4, 2.2], rot: 110, y: 0.4, block: 0 },
    ],
    slots: [
      { activities: ['mourn', 'pray'], at: [-2.4, 0.4], face: 270, anim: 'sit_floor' },
      { activities: ['mourn', 'pray'], at: [2.4, 0.4], face: 90, anim: 'idle_alt' },
    ],
    gravePlots: [[-4.8, 0], [4.8, 0], [-4.8, 2.5], [4.8, 2.5], [-2.9, 3.0], [2.9, 3.0], [-7.0, -3.6], [7.0, -3.6], [-7.0, -1.0], [7.0, -1.0]],
    spawnPoints: [[-3.4, -1.4], [3.4, -1.4], [-6.2, 4.4], [6.0, -5.6], [-2.2, 2.0], [2.2, 2.0], [0, -0.6]],
  },
  {
    id: 'ruins',
    name: 'the old ruins',
    kind: 'ruins',
    description: 'Crumbling walls deep in the eastern woods. Few go there by choice.',
    hex: [15, 2],
    facing: 200,
    clear: 9,
    props: [
      { model: 'dungeon/wall_broken', at: [-4.0, -4.0], rot: 0 },
      { model: 'dungeon/wall_cracked', at: [0, -4.0], rot: 0 },
      { model: 'dungeon/wall_corner', at: [3.2, -3.2], rot: 0 },
      { model: 'dungeon/wall_half', at: [4.0, 0.2], rot: 90 },
      { model: 'dungeon/rubble_large', at: [-5.0, 2.6], rot: 70 },
      { model: 'dungeon/rubble_half', at: [5.2, 4.6], rot: 200 },
      { model: 'dungeon/pillar', at: [-1.6, 3.6] },
      { model: 'dungeon/floor_tile_large_rocks', at: [0, 0], block: 0 },
      { model: 'dungeon/floor_tile_small_broken_A', at: [-3.0, 0], block: 0 },
      { model: 'dungeon/floor_tile_small_weeds_B', at: [0, 3.0], block: 0 },
      { model: 'dungeon/chest_gold', at: [1.6, -2.6], rot: 200 },
      { model: 'halloween/skull', at: [-2.2, -1.6], rot: 30, block: 0 },
    ],
    spawnPoints: [[0, 0.6], [-2.6, 1.6], [2.4, 2.2], [0, 5.8]],
  },
  {
    id: 'west_road',
    name: 'the west road',
    kind: 'road',
    description: 'Where the road from the outside world enters the valley. Newcomers arrive here.',
    hex: [0, 7],
    facing: 90,
    clear: 4,
    props: [
      { model: 'halloween/sign_right', at: [3.2, 4.0], rot: 180, block: 0.3 },
      { model: 'halloween/post_lantern', at: [-3.0, 5.0], rot: 180, block: 0.3 },
    ],
    billboards: [{ id: 'west_road_board', at: [4.4, 1.0], rot: -90 }],
  },
  home({ n: 1, name: 'the red cottage', hex: [4, 6], facing: 0, model: 'building_home_A_red' }),
  home({ n: 2, name: 'the blue house', hex: [9, 6], facing: 0, model: 'building_home_B_blue' }),
  home({ n: 3, name: 'the green cottage', hex: [8, 8], facing: 180, model: 'building_home_A_green' }),
  home({ n: 4, name: 'the yellow house', hex: [5, 8], facing: 180, model: 'building_home_B_yellow' }),
  home({ n: 5, name: 'the lane cottage', hex: [5, 5], facing: 90, model: 'building_home_A_blue' }),
  home({ n: 6, name: 'the tall house', hex: [7, 5], facing: 270, model: 'building_home_B_red' }),
  home({ n: 7, name: 'the south cottage', hex: [5, 9], facing: 90, model: 'building_home_A_yellow' }),
  home({ n: 8, name: 'the lakeside house', hex: [7, 9], facing: 270, model: 'building_home_B_green' }),
];
