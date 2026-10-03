//! What a character is doing right now, in a few words ("eating at The Gilded Tankard").

use fsw_world::places::{ActivityId, find_place, get_slot};

use crate::data::activities::activity;
use crate::data::anims::AnimId;
use crate::state::{ActPhase, Character, FightMode, IntentAction, SimState};
use crate::systems::fire::home_usable;
use crate::systems::quirks::reading_late;
use crate::util::name_of;

pub fn describe_activity(state: &SimState, c: &Character) -> String {
    let doing = doing(state, c);
    if c.drunk(state.tick) { format!("{doing} (drunk)") } else { doing }
}

fn doing(state: &SimState, c: &Character) -> String {
    if c.combat.yielded {
        return "beaten, at someone's mercy".into();
    }
    if let Some(convo_id) = &c.convo_id {
        let other = state.conversations.get(convo_id).map(|convo| if convo.a == c.id { &convo.b } else { &convo.a });
        return match other {
            Some(other) => format!("talking with {}", name_of(state, other)),
            None => "talking".into(),
        };
    }
    let Some(intent) = &c.intent else {
        return if c.asleep { "sleeping".into() } else { "standing about".into() };
    };
    match &intent.action {
        IntentAction::GoTo { place_id } => {
            format!("walking to {}", find_place(place_id).map_or("somewhere", |place| place.name))
        }
        IntentAction::Activity { activity: id, place_id, .. } => {
            let def = activity(*id);
            if *id == ActivityId::Sleep {
                if reading_late(state, c) {
                    return "up late at home, reading by candlelight".into();
                }
                if !home_usable(state, c) && c.inside.is_none() {
                    let rough =
                        if c.asleep { "sleeping rough by the campfire" } else { "looking for a place to sleep" };
                    return rough.into();
                }
                return if c.asleep { "asleep at home".into() } else { "heading home to sleep".into() };
            }
            if *id == ActivityId::Stroll {
                return "strolling around the village".into();
            }
            if *id == ActivityId::Rebuild {
                let name = place_id.as_deref().and_then(find_place).map_or("the ruin", |place| place.name);
                return if c.act.phase == ActPhase::Perform {
                    format!("rebuilding {name}")
                } else {
                    format!("on the way to rebuild {name}")
                };
            }
            let place = c.slot_id.as_deref().and_then(|slot| find_place(&get_slot(slot).place_id));
            if c.act.phase != ActPhase::Perform {
                return match place {
                    Some(place) => format!("on the way to {} to {}", place.name, short_label(def.label)),
                    None => format!("going to {}", short_label(def.label)),
                };
            }
            match place {
                Some(place) => format!("{} at {}", def.verb, place.name),
                None => def.verb.into(),
            }
        }
        IntentAction::Talk { target_id, .. } => format!("going to talk to {}", name_of(state, target_id)),
        IntentAction::Give { target_id, .. } => format!("taking something to {}", name_of(state, target_id)),
        IntentAction::Fight { target_id, mode } => match mode {
            FightMode::Monster => match state.monsters.get(target_id) {
                Some(monster) => format!("fighting a {}", monster.kind.def().label),
                None => "fighting".into(),
            },
            FightMode::Brawl => format!("brawling with {}", name_of(state, target_id)),
            FightMode::Lethal => format!("attacking {}", name_of(state, target_id)),
        },
        IntentAction::Flee => if c.inside.is_some() { "hiding indoors" } else { "running for safety" }.into(),
        IntentAction::Wait { .. } => {
            if c.anim == AnimId::SitFloor { "sitting on the ground, recovering" } else { "pausing a moment" }.into()
        }
        IntentAction::Nap { .. } => if c.asleep { "napping on the grass" } else { "looking for a spot to nap" }.into(),
        // A stalker looks like anyone else taking the air.
        IntentAction::Stalk { .. } => "strolling around the village".into(),
        IntentAction::FightFire { place_id } => {
            let name = find_place(place_id).map_or("the house", |place| place.name);
            if c.tool.is_some() {
                format!("carrying water to the fire at {name}")
            } else {
                format!("fetching water for the fire at {name}")
            }
        }
    }
}

/// "eat a meal at the tavern" → "eat a meal" (the place is named separately).
fn short_label(label: &str) -> &str {
    for marker in [" at the ", " in the "] {
        if let Some(index) = label.find(marker) {
            return &label[..index];
        }
    }
    label
}

#[cfg(test)]
mod tests {
    use super::short_label;

    #[test]
    fn short_labels_drop_the_place() {
        assert_eq!(short_label("eat a meal at the tavern"), "eat a meal");
        assert_eq!(short_label("sit by the campfire in the glade"), "sit by the campfire");
        assert_eq!(short_label("pray or meditate in quiet"), "pray or meditate in quiet");
    }
}
