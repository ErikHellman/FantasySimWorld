//! Children: how they look, what they are called, and the numbers that govern a
//! childhood. A child is an adult mesh drawn small (`pnpm assets` builds the
//! `Child_*` files from the adult ones at `CHILD_SCALE`), so every child look
//! names the adult class it grows into.

use fsw_core::time::TICKS_PER_DAY;

use super::classes::ClassId;

/// The scale `tools/assets/asset-list.mjs` draws a `Child_*` model at.
pub const CHILD_SCALE: f64 = 0.6;

/// A child's look: the class they will grow into, the small model, and the parts
/// of it that are hidden because they read as grown-up kit.
pub struct ChildLook {
    pub class: ClassId,
    pub model: &'static str,
    pub hide: &'static [&'static str],
}

pub static CHILD_LOOKS: [ChildLook; 8] = [
    ChildLook { class: ClassId::Knight, model: "Child_Knight", hide: &["Knight_Helmet", "Knight_HelmetVisor"] },
    ChildLook { class: ClassId::Barbarian, model: "Child_Barbarian", hide: &["Barbarian_BearHat"] },
    ChildLook { class: ClassId::Mage, model: "Child_Mage", hide: &["Mage_Hat"] },
    ChildLook { class: ClassId::Ranger, model: "Child_Ranger", hide: &["Ranger_Quiver"] },
    ChildLook { class: ClassId::Rogue, model: "Child_Rogue", hide: &[] },
    ChildLook { class: ClassId::Druid, model: "Child_Druid", hide: &[] },
    ChildLook { class: ClassId::Engineer, model: "Child_Engineer", hide: &[] },
    ChildLook { class: ClassId::Farmer, model: "Child_Farmer", hide: &[] },
];

/// The child look for a class, or the farmer's when the class has none (a child
/// of two paladins is a small farmer until they grow into their armour).
pub fn child_look(class: ClassId) -> &'static ChildLook {
    CHILD_LOOKS.iter().find(|look| look.class == class).unwrap_or(&CHILD_LOOKS[7])
}

/// A child ages one year every this many sim days.
pub const BIRTHDAY_DAYS: i64 = 3;
/// The age at which a child becomes an adult.
pub const COMING_OF_AGE: i64 = 16;
/// The age a child is born at: old enough to run about the village.
pub const NEWBORN_AGE: i64 = 5;
/// No more children than this are alive at once.
pub const MAX_CHILDREN: usize = 6;
/// Lovers must have been together this long before a child comes.
pub const LOVERS_BEFORE_CHILD_TICKS: i64 = 8 * TICKS_PER_DAY;
/// Each dawn, a couple who qualify have this chance of a child.
pub const BIRTH_CHANCE: f64 = 1.0 / 6.0;
/// Days between births in the village.
pub const BIRTH_COOLDOWN_TICKS: i64 = 6 * TICKS_PER_DAY;
/// A child has this share of the adult class's hit points.
pub const CHILD_HP_FRACTION: f64 = 0.5;
/// Nobody's grievance toward a child, or a child's toward anyone, passes this
/// (below the rival label, far below a brawl).
pub const CHILD_GRIEVANCE_CAP: f64 = 30.0;

/// First names for children born in the village (none shared with the wanderers' list).
pub const CHILD_FIRST_NAMES: [&str; 14] = [
    "Pim", "Tilly", "Rook", "Nettle", "Bo", "Wick", "Fenna", "Oakes", "Lark", "Dot", "Hob", "Juniper", "Sprout", "Mab",
];

/// Traits a child may be given at birth.
pub const CHILD_TRAITS: [&str; 12] = [
    "curious",
    "fearless",
    "shy",
    "chatty",
    "stubborn",
    "dreamy",
    "quick to laugh",
    "always hungry",
    "a collector of odd stones",
    "afraid of the dark",
    "never still",
    "fond of animals",
];

#[cfg(test)]
mod tests {
    use super::*;
    use fsw_world::assets::asset_manifest;

    #[test]
    fn every_child_look_is_a_small_medium_rig_model_in_the_manifest() {
        let manifest = asset_manifest();
        for look in &CHILD_LOOKS {
            let model =
                manifest.characters.get(look.model).unwrap_or_else(|| panic!("{}: not in the manifest", look.model));
            assert_eq!(model.scale, CHILD_SCALE, "{}", look.model);
            assert_eq!(model.rig, "medium", "{}", look.model);
            let adult = manifest.characters.get(look.class.def().model).unwrap();
            assert_eq!(model.texture, adult.texture, "{}: a child shares the adult's texture", look.model);
        }
    }

    #[test]
    fn child_names_do_not_overlap_the_wanderers() {
        for name in CHILD_FIRST_NAMES {
            assert!(!crate::systems::arrivals::FIRST_NAMES.contains(&name), "{name}");
        }
    }
}
