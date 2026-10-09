import { TIMING_RIG } from '../core/assetTypes';
import { assetManifest } from '../generated/assetManifest';

/**
 * Abstract animation states the sim puts on characters and monsters. The
 * renderer maps them to KayKit clips; the sim uses the durations for timing.
 */
export interface AnimDef {
  clip: string;
  loop: boolean;
  /** playback speed multiplier */
  speed?: number;
}

const def = (clip: string, loop: boolean, speed?: number): AnimDef => ({ clip, loop, speed });

export const ANIMS = {
  idle: def('Idle_A', true),
  idle_alt: def('Idle_B', true),
  /** astride a horse: the idle clip for the body, with the legs posed over it (`ActorView.ride`) */
  ride: def('Idle_A', true),
  walk: def('Walking_A', true),
  walk_b: def('Walking_B', true),
  walk_c: def('Walking_C', true),
  walk_drunk: def('Walking_C', true, 0.7),
  run: def('Running_A', true),
  run_b: def('Running_B', true),
  hit: def('Hit_A', false),
  hit_b: def('Hit_B', false),
  death: def('Death_A', false),
  death_b: def('Death_B', false),
  death_pose: def('Death_A_Pose', false),
  interact: def('Interact', false),
  pick_up: def('PickUp', false),
  use_item: def('Use_Item', false),
  wave: def('Waving', false),
  cheer: def('Cheering', false),
  sit_chair: def('Sit_Chair_Idle', true),
  sit_floor: def('Sit_Floor_Idle', true),
  lie: def('Lie_Idle', true),
  lie_down: def('Lie_Down', false),
  push_ups: def('Push_Ups', true),
  chop: def('Chopping', true),
  dig: def('Digging', true),
  hammer: def('Hammering', true),
  pickaxe: def('Pickaxing', true),
  saw: def('Sawing', true),
  fish: def('Fishing_Idle', true),
  fish_cast: def('Fishing_Cast', false),
  work_a: def('Working_A', true),
  work_b: def('Working_B', true),
  work_c: def('Working_C', true),
  hold: def('Holding_A', true),
  cast_long: def('Ranged_Magic_Spellcasting_Long', true),
  guard: def('Melee_2H_Idle', true),
  unarmed_idle: def('Melee_Unarmed_Idle', true),
  block: def('Melee_Blocking', true),
  block_hit: def('Melee_Block_Hit', false),
  dodge_back: def('Dodge_Backward', false),
  dodge_left: def('Dodge_Left', false),
  dodge_right: def('Dodge_Right', false),
  attack_1h_chop: def('Melee_1H_Attack_Chop', false),
  attack_1h_slice: def('Melee_1H_Attack_Slice_Diagonal', false),
  attack_1h_stab: def('Melee_1H_Attack_Stab', false, 1.3),
  attack_2h_chop: def('Melee_2H_Attack_Chop', false, 1.25),
  attack_2h_slice: def('Melee_2H_Attack_Slice', false),
  attack_dual_chop: def('Melee_Dualwield_Attack_Chop', false),
  attack_dual_slice: def('Melee_Dualwield_Attack_Slice', false),
  attack_punch: def('Melee_Unarmed_Attack_Punch_A', false),
  attack_kick: def('Melee_Unarmed_Attack_Kick', false),
  bow_idle: def('Ranged_Bow_Idle', true),
  bow_shoot: def('Ranged_Bow_Release', false),
  bow_draw: def('Ranged_Bow_Draw', false),
  magic_shoot: def('Ranged_Magic_Shoot', false),
  magic_cast: def('Ranged_Magic_Spellcasting', false),
  magic_summon: def('Ranged_Magic_Summon', false),
  crossbow_shoot: def('Ranged_1H_Shoot', false),
  skeleton_spawn: def('Skeletons_Spawn_Ground', false),
  skeleton_idle: def('Skeletons_Idle', true),
  skeleton_walk: def('Skeletons_Walking', true),
  skeleton_taunt: def('Skeletons_Taunt', false),
  skeleton_death: def('Skeletons_Death', false),
  dragon_fly: def('Dragon_Fly', true),
  dragon_hover: def('Dragon_Hover', true),
  dragon_breath: def('Dragon_Breath', false),
  dragon_roar: def('Dragon_Roar', false),
  dragon_hit: def('Dragon_Hit', false),
  dragon_death: def('Dragon_Death', false),
} as const satisfies Record<string, AnimDef>;

export type AnimId = keyof typeof ANIMS;

/** Whether a rig carries a clip: every rig a villager wears has the same ones, but the dragon's has its own. */
export function hasClip(rig: string, clip: string): boolean {
  return assetManifest.rigs[rig]?.clips[clip] !== undefined;
}

/** Seconds a clip lasts on a rig, before any playback speed. */
export function clipSeconds(rig: string, clip: string): number {
  const duration = assetManifest.rigs[rig]?.clips[clip];
  if (duration === undefined) throw new Error(`Clip ${clip} missing from rig ${rig} in the asset manifest`);
  return duration;
}

/** Seconds one play of the animation takes at its playback speed (the sim's timing). */
export function animSeconds(id: AnimId): number {
  const anim: AnimDef = ANIMS[id];
  // The dragon's clips are timed by its own rig, as the sim does.
  const rig = hasClip(TIMING_RIG, anim.clip) ? TIMING_RIG : 'dragon';
  return clipSeconds(rig, anim.clip) / (anim.speed ?? 1);
}
