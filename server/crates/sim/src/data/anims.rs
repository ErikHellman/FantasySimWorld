//! Abstract animation states the sim puts on characters and monsters. The
//! renderer maps them to KayKit clips; the sim uses the durations for timing.

use fsw_world::assets::{asset_manifest, timing_clips};
use serde::{Deserialize, Serialize};

pub struct AnimDef {
    pub clip: &'static str,
    pub loops: bool,
    /// playback speed multiplier
    pub speed: f64,
}

macro_rules! anims {
    ($($variant:ident = $name:literal => ($clip:literal, $loops:literal $(, $speed:literal)?)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum AnimId {
            $(#[serde(rename = $name)] $variant,)*
        }

        impl AnimId {
            pub const ALL: &'static [AnimId] = &[$(AnimId::$variant,)*];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(AnimId::$variant => $name,)*
                }
            }

            pub fn def(self) -> AnimDef {
                match self {
                    $(AnimId::$variant => AnimDef { clip: $clip, loops: $loops, speed: anims!(@speed $($speed)?) },)*
                }
            }

            pub fn parse(name: &str) -> Option<AnimId> {
                match name {
                    $($name => Some(AnimId::$variant),)*
                    _ => None,
                }
            }
        }
    };
    (@speed) => { 1.0 };
    (@speed $speed:literal) => { $speed };
}

anims! {
    Idle = "idle" => ("Idle_A", true),
    IdleAlt = "idle_alt" => ("Idle_B", true),
    Walk = "walk" => ("Walking_A", true),
    WalkB = "walk_b" => ("Walking_B", true),
    WalkC = "walk_c" => ("Walking_C", true),
    WalkDrunk = "walk_drunk" => ("Walking_C", true, 0.7),
    Run = "run" => ("Running_A", true),
    RunB = "run_b" => ("Running_B", true),
    Hit = "hit" => ("Hit_A", false),
    HitB = "hit_b" => ("Hit_B", false),
    Death = "death" => ("Death_A", false),
    DeathB = "death_b" => ("Death_B", false),
    // lying where it fell, without falling again: for a corpse that was a monster first
    DeathPose = "death_pose" => ("Death_A_Pose", false),
    Interact = "interact" => ("Interact", false),
    PickUp = "pick_up" => ("PickUp", false),
    UseItem = "use_item" => ("Use_Item", false),
    Wave = "wave" => ("Waving", false),
    Cheer = "cheer" => ("Cheering", false),
    SitChair = "sit_chair" => ("Sit_Chair_Idle", true),
    SitFloor = "sit_floor" => ("Sit_Floor_Idle", true),
    Lie = "lie" => ("Lie_Idle", true),
    LieDown = "lie_down" => ("Lie_Down", false),
    PushUps = "push_ups" => ("Push_Ups", true),
    Chop = "chop" => ("Chopping", true),
    Dig = "dig" => ("Digging", true),
    Hammer = "hammer" => ("Hammering", true),
    Pickaxe = "pickaxe" => ("Pickaxing", true),
    Saw = "saw" => ("Sawing", true),
    Fish = "fish" => ("Fishing_Idle", true),
    FishCast = "fish_cast" => ("Fishing_Cast", false),
    WorkA = "work_a" => ("Working_A", true),
    WorkB = "work_b" => ("Working_B", true),
    WorkC = "work_c" => ("Working_C", true),
    Hold = "hold" => ("Holding_A", true),
    CastLong = "cast_long" => ("Ranged_Magic_Spellcasting_Long", true),
    Guard = "guard" => ("Melee_2H_Idle", true),
    UnarmedIdle = "unarmed_idle" => ("Melee_Unarmed_Idle", true),
    Block = "block" => ("Melee_Blocking", true),
    BlockHit = "block_hit" => ("Melee_Block_Hit", false),
    DodgeBack = "dodge_back" => ("Dodge_Backward", false),
    DodgeLeft = "dodge_left" => ("Dodge_Left", false),
    DodgeRight = "dodge_right" => ("Dodge_Right", false),
    Attack1hChop = "attack_1h_chop" => ("Melee_1H_Attack_Chop", false),
    Attack1hSlice = "attack_1h_slice" => ("Melee_1H_Attack_Slice_Diagonal", false),
    Attack1hStab = "attack_1h_stab" => ("Melee_1H_Attack_Stab", false, 1.3),
    Attack2hChop = "attack_2h_chop" => ("Melee_2H_Attack_Chop", false, 1.25),
    Attack2hSlice = "attack_2h_slice" => ("Melee_2H_Attack_Slice", false),
    AttackDualChop = "attack_dual_chop" => ("Melee_Dualwield_Attack_Chop", false),
    AttackDualSlice = "attack_dual_slice" => ("Melee_Dualwield_Attack_Slice", false),
    AttackPunch = "attack_punch" => ("Melee_Unarmed_Attack_Punch_A", false),
    AttackKick = "attack_kick" => ("Melee_Unarmed_Attack_Kick", false),
    BowIdle = "bow_idle" => ("Ranged_Bow_Idle", true),
    BowShoot = "bow_shoot" => ("Ranged_Bow_Release", false),
    BowDraw = "bow_draw" => ("Ranged_Bow_Draw", false),
    MagicShoot = "magic_shoot" => ("Ranged_Magic_Shoot", false),
    MagicCast = "magic_cast" => ("Ranged_Magic_Spellcasting", false),
    MagicSummon = "magic_summon" => ("Ranged_Magic_Summon", false),
    CrossbowShoot = "crossbow_shoot" => ("Ranged_1H_Shoot", false),
    SkeletonSpawn = "skeleton_spawn" => ("Skeletons_Spawn_Ground", false),
    SkeletonIdle = "skeleton_idle" => ("Skeletons_Idle", true),
    SkeletonWalk = "skeleton_walk" => ("Skeletons_Walking", true),
    SkeletonTaunt = "skeleton_taunt" => ("Skeletons_Taunt", false),
    SkeletonDeath = "skeleton_death" => ("Skeletons_Death", false),
    DragonFly = "dragon_fly" => ("Dragon_Fly", true),
    DragonHover = "dragon_hover" => ("Dragon_Hover", true),
    DragonBreath = "dragon_breath" => ("Dragon_Breath", false),
    DragonRoar = "dragon_roar" => ("Dragon_Roar", false),
    DragonHit = "dragon_hit" => ("Dragon_Hit", false),
    DragonDeath = "dragon_death" => ("Dragon_Death", false),
}

/// The rig only the dragon plays, and the prefix of its clips. The timing rig has none of them, so
/// they are timed by their own.
pub const DRAGON_RIG: &str = "dragon";
const DRAGON_CLIPS: &str = "Dragon_";

impl AnimId {
    pub fn loops(self) -> bool {
        self.def().loops
    }

    pub fn is_seated(self) -> bool {
        matches!(self, AnimId::SitChair | AnimId::SitFloor)
    }

    /// Seconds one play of the animation takes at its playback speed.
    pub fn seconds(self) -> f64 {
        let def = self.def();
        let clips = if def.clip.starts_with(DRAGON_CLIPS) {
            &asset_manifest().rigs.get(DRAGON_RIG).expect("the dragon's rig is in the asset manifest").clips
        } else {
            timing_clips()
        };
        let duration =
            clips.get(def.clip).unwrap_or_else(|| panic!("Clip {} missing from the asset manifest", def.clip));
        duration / def.speed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_clip_is_in_the_manifest() {
        for anim in AnimId::ALL {
            assert!(anim.seconds() >= 0.0, "{}", anim.as_str());
        }
    }

    #[test]
    fn every_rig_carries_the_clips_villagers_play() {
        use fsw_world::assets::asset_manifest;
        for (rig, entry) in &asset_manifest().rigs {
            for anim in AnimId::ALL {
                let clip = anim.def().clip;
                // The dragon plays its own clips and nothing else.
                if (rig == DRAGON_RIG) != clip.starts_with(DRAGON_CLIPS) {
                    continue;
                }
                assert!(entry.clips.contains_key(clip), "rig {rig} has no {clip}");
            }
        }
    }

    #[test]
    fn slots_only_use_known_animations() {
        for place in fsw_world::places::places() {
            for slot in &place.slots {
                if let Some(anim) = slot.anim {
                    assert!(AnimId::parse(anim).is_some(), "{}", slot.id);
                }
            }
        }
    }
}
