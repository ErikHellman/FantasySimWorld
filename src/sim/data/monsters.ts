import type { AnimId } from '../../render/anims';
import type { WeaponStyle } from './classes';

export type MonsterKind = 'skeleton_minion' | 'skeleton_warrior' | 'skeleton_rogue' | 'skeleton_mage' | 'necromancer';

export interface MonsterDef {
  kind: MonsterKind;
  label: string;
  model: string;
  texture: string | null;
  mainHand: string | null;
  offHand: string | null;
  maxHp: number;
  damage: readonly [number, number];
  range: number;
  interval: number;
  speed: number;
  style: WeaponStyle;
  attackAnims: readonly AnimId[];
  /** how far it notices villagers */
  sight: number;
  /** relative chance in an ordinary wave */
  weight: number;
}

export const MONSTERS: Record<MonsterKind, MonsterDef> = {
  skeleton_minion: {
    kind: 'skeleton_minion', label: 'skeleton', model: 'Skeleton_Minion', texture: null,
    mainHand: 'Skeleton_Dagger', offHand: null, maxHp: 38, damage: [5, 9], range: 1.9, interval: 1.5, speed: 2.7,
    style: 'melee', attackAnims: ['attack_1h_chop', 'attack_1h_stab'], sight: 17, weight: 5,
  },
  skeleton_warrior: {
    kind: 'skeleton_warrior', label: 'skeleton warrior', model: 'Skeleton_Warrior', texture: null,
    mainHand: 'Skeleton_Blade', offHand: 'Skeleton_Shield_Large_A', maxHp: 70, damage: [9, 14], range: 2.1, interval: 1.6, speed: 2.5,
    style: 'melee', attackAnims: ['attack_1h_chop', 'attack_1h_slice'], sight: 17, weight: 3,
  },
  skeleton_rogue: {
    kind: 'skeleton_rogue', label: 'skeleton stalker', model: 'Skeleton_Rogue', texture: null,
    mainHand: 'Skeleton_Dagger', offHand: 'Skeleton_Dagger', maxHp: 48, damage: [7, 11], range: 1.9, interval: 1.0, speed: 3.4,
    style: 'melee', attackAnims: ['attack_dual_chop', 'attack_dual_slice'], sight: 20, weight: 2,
  },
  skeleton_mage: {
    kind: 'skeleton_mage', label: 'skeleton mage', model: 'Skeleton_Mage', texture: null,
    mainHand: 'Skeleton_Staff', offHand: null, maxHp: 42, damage: [9, 14], range: 12, interval: 2.4, speed: 2.4,
    style: 'magic', attackAnims: ['magic_shoot'], sight: 19, weight: 2,
  },
  necromancer: {
    kind: 'necromancer', label: 'necromancer', model: 'Necromancer', texture: null,
    mainHand: 'Skeleton_Scythe', offHand: null, maxHp: 170, damage: [13, 19], range: 12, interval: 2.2, speed: 2.3,
    style: 'magic', attackAnims: ['magic_shoot', 'magic_cast'], sight: 22, weight: 0,
  },
};
