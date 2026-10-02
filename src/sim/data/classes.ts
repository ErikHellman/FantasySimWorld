import type { ActivityId } from '../../world/places';
import type { AnimId } from '../../render/anims';

export type ClassId =
  | 'knight' | 'barbarian' | 'mage' | 'ranger' | 'rogue' | 'scout' | 'druid' | 'engineer'
  | 'paladin' | 'cleric' | 'witch' | 'tiefling' | 'lorekeeper' | 'farmer';

export type WeaponStyle = 'melee' | 'bow' | 'crossbow' | 'magic';

export interface Weapon {
  style: WeaponStyle;
  /** damage per hit, uniform in this range */
  damage: readonly [number, number];
  /** reach in world units */
  range: number;
  /** seconds between attacks */
  interval: number;
  /** attack animations, picked at random */
  anims: readonly AnimId[];
  /** stance while fighting but not swinging */
  stance: AnimId;
}

export interface ClassDef {
  id: ClassId;
  label: string;
  model: string;
  maxHp: number;
  weapon: Weapon;
  mainHand: string | null;
  offHand: string | null;
  jobs: readonly ActivityId[];
  hobbies: readonly ActivityId[];
}

const melee = (damage: readonly [number, number], interval: number, anims: readonly AnimId[], stance: AnimId = 'idle'): Weapon =>
  ({ style: 'melee', damage, range: 2.1, interval, anims, stance });

export const CLASSES: Record<ClassId, ClassDef> = {
  knight: {
    id: 'knight', label: 'knight', model: 'Knight', maxHp: 120,
    weapon: melee([14, 20], 1.3, ['attack_1h_chop', 'attack_1h_slice', 'attack_1h_stab'], 'block'),
    mainHand: 'sword_1handed', offHand: 'shield_badge',
    jobs: ['watch', 'train'], hobbies: ['drink', 'pray', 'stroll'],
  },
  barbarian: {
    id: 'barbarian', label: 'barbarian', model: 'Barbarian', maxHp: 130,
    weapon: melee([20, 28], 1.8, ['attack_2h_chop', 'attack_2h_slice'], 'guard'),
    mainHand: 'axe_2handed', offHand: null,
    jobs: ['chop_wood', 'mine'], hobbies: ['drink', 'train', 'relax'],
  },
  mage: {
    id: 'mage', label: 'mage', model: 'Mage', maxHp: 70,
    weapon: { style: 'magic', damage: [16, 24], range: 14, interval: 2.0, anims: ['magic_shoot'], stance: 'idle' },
    mainHand: 'staff', offHand: null,
    jobs: ['brew', 'trade'], hobbies: ['pray', 'stroll', 'rest'],
  },
  ranger: {
    id: 'ranger', label: 'ranger', model: 'Ranger', maxHp: 85,
    weapon: { style: 'bow', damage: [12, 18], range: 16, interval: 1.7, anims: ['bow_shoot'], stance: 'bow_idle' },
    mainHand: null, offHand: 'bow_withString',
    jobs: ['forage', 'fish', 'watch'], hobbies: ['train', 'relax', 'stroll'],
  },
  rogue: {
    id: 'rogue', label: 'rogue', model: 'Rogue', maxHp: 85,
    weapon: melee([9, 13], 0.95, ['attack_dual_chop', 'attack_dual_slice']),
    mainHand: 'dagger', offHand: 'dagger',
    jobs: ['trade', 'fish'], hobbies: ['drink', 'stroll', 'shop'],
  },
  scout: {
    id: 'scout', label: 'scout', model: 'Rogue_Hooded', maxHp: 80,
    weapon: { style: 'crossbow', damage: [13, 19], range: 13, interval: 2.0, anims: ['crossbow_shoot'], stance: 'idle' },
    mainHand: 'crossbow_1handed', offHand: null,
    jobs: ['watch', 'forage'], hobbies: ['stroll', 'relax', 'train'],
  },
  druid: {
    id: 'druid', label: 'druid', model: 'Druid', maxHp: 90,
    weapon: { style: 'magic', damage: [12, 18], range: 12, interval: 1.9, anims: ['magic_cast'], stance: 'idle' },
    mainHand: 'druid_staff', offHand: null,
    jobs: ['forage', 'brew', 'farm'], hobbies: ['pray', 'relax', 'stroll'],
  },
  engineer: {
    id: 'engineer', label: 'engineer', model: 'Engineer', maxHp: 100,
    weapon: melee([13, 19], 1.35, ['attack_1h_chop', 'attack_1h_slice']),
    mainHand: 'engineer_Wrench', offHand: null,
    jobs: ['smith', 'mine'], hobbies: ['drink', 'shop', 'rest'],
  },
  paladin: {
    id: 'paladin', label: 'paladin', model: 'Paladin', maxHp: 125,
    weapon: melee([15, 21], 1.4, ['attack_1h_chop', 'attack_1h_slice'], 'block'),
    mainHand: 'paladin_hammer', offHand: 'paladin_shield',
    jobs: ['watch', 'train'], hobbies: ['pray', 'stroll', 'drink'],
  },
  cleric: {
    id: 'cleric', label: 'cleric', model: 'Cleric', maxHp: 100,
    weapon: melee([12, 18], 1.4, ['attack_1h_chop', 'attack_1h_slice'], 'block'),
    mainHand: 'Cleric_Mace', offHand: 'Cleric_Shield',
    jobs: ['brew', 'trade'], hobbies: ['pray', 'rest', 'stroll'],
  },
  witch: {
    id: 'witch', label: 'witch', model: 'Witch', maxHp: 70,
    weapon: { style: 'magic', damage: [15, 22], range: 13, interval: 2.0, anims: ['magic_shoot'], stance: 'idle' },
    mainHand: 'Broom', offHand: null,
    jobs: ['brew', 'forage'], hobbies: ['relax', 'shop', 'stroll'],
  },
  tiefling: {
    id: 'tiefling', label: 'swordsman', model: 'Tiefling', maxHp: 95,
    weapon: melee([15, 21], 1.15, ['attack_1h_slice', 'attack_1h_stab', 'attack_1h_chop']),
    mainHand: 'Tiefling_Sword', offHand: null,
    jobs: ['watch', 'trade'], hobbies: ['drink', 'train', 'stroll'],
  },
  lorekeeper: {
    id: 'lorekeeper', label: 'lorekeeper', model: 'Lorekeeper', maxHp: 70,
    weapon: { style: 'magic', damage: [14, 20], range: 13, interval: 2.1, anims: ['magic_cast'], stance: 'idle' },
    mainHand: 'Lorekeeper_Staff', offHand: null,
    jobs: ['trade', 'brew'], hobbies: ['pray', 'rest', 'stroll'],
  },
  farmer: {
    id: 'farmer', label: 'farmer', model: 'Farmer_A', maxHp: 85,
    weapon: melee([11, 16], 1.6, ['attack_2h_slice', 'attack_2h_chop'], 'guard'),
    mainHand: 'farmer_pitchfork', offHand: null,
    jobs: ['farm', 'chop_wood'], hobbies: ['drink', 'rest', 'fish'],
  },
};

/** Bare fists, used in brawls whatever the class. */
export const FISTS: Weapon = {
  style: 'melee', damage: [4, 7], range: 1.7, interval: 1.1, anims: ['attack_punch', 'attack_kick'], stance: 'unarmed_idle',
};

export const WALK_SPEED = 3.1;
export const RUN_SPEED = 6.2;
