//! What a character is doing right now, in a few words ("eating at The Gilded Tankard").

use fsw_world::places::{ActivityId, find_place, get_slot};

use crate::data::activities::activity;
use crate::data::anims::AnimId;
use crate::state::{ActPhase, Character, FightMode, IntentAction, ItemId, Rite, SimState, WorldJob};
use crate::systems::animals::call;
use crate::systems::economy::{Commission, loot_label};
use crate::systems::fire::home_usable;
use crate::systems::music::{performance, venue_phrase};
use crate::systems::quirks::reading_late;
use crate::systems::world::describe_target;
use crate::util::{join_names, name_of, place_name_at};

pub fn describe_activity(state: &SimState, c: &Character) -> String {
    let doing = doing(state, c);
    if c.drunk(state.tick) {
        format!("{doing} (drunk)")
    } else if c.heartened(state.tick) {
        format!("{doing} (in good spirits)")
    } else {
        doing
    }
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
            if *id == ActivityId::Play {
                return if c.act.phase == ActPhase::Perform || c.act.dest.is_none() {
                    format!("playing at {}", place_name_at(c.pos))
                } else {
                    "playing".into()
                };
            }
            if *id == ActivityId::Perform {
                let venue = place_id.as_deref().filter(|p| find_place(p).is_some()).unwrap_or("tavern");
                let where_ = venue_phrase(venue);
                return match performance(state, &c.id) {
                    Some(p) if c.act.phase == ActPhase::Perform => format!("{} {where_}", p.instrument.doing()),
                    _ => {
                        let (instrument, _) = crate::systems::music::skill_of(c);
                        format!(
                            "on the way to {} to {}",
                            find_place(venue).map_or("the tavern", |p| p.name),
                            instrument.to_do()
                        )
                    }
                };
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
        IntentAction::Commission { what } => match Commission::parse(what, &c.home_id) {
            Some(choice) if c.act.phase == ActPhase::Perform => {
                format!("paying for {}", choice.label().replace("your", "their"))
            }
            Some(choice) => format!("off to the market to order {}", choice.label().replace("your", "their")),
            None => "off to the market".into(),
        },
        IntentAction::Trade { target_id, item, buy } => {
            let what = short_item(*item);
            if *buy {
                format!("going to buy {what} from {}", name_of(state, target_id))
            } else {
                format!("going to sell {what} to {}", name_of(state, target_id))
            }
        }
        IntentAction::Pet { animal_id } => match state.animals.get(animal_id) {
            Some(animal) if c.act.phase == ActPhase::Perform => format!("making a fuss of {}", call(animal)),
            Some(animal) => format!("going over to {}", call(animal)),
            None => "looking for an animal".into(),
        },
        IntentAction::PickUp { loot_id } => match state.loot.get(loot_id) {
            Some(loot) if c.act.phase == ActPhase::Perform => format!("picking up {}", loot.kind.label()),
            Some(loot) => format!("going to pick up {}", loot_label(loot)),
            None => "looking for something on the ground".into(),
        },
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
        IntentAction::Fetch { target_id } => format!("fetching {} home", name_of(state, target_id)),
        IntentAction::Listen { performer_id } => match performance(state, performer_id) {
            Some(p) => format!("listening to {} {}", name_of(state, performer_id), p.instrument.to_do()),
            None => "listening to the music".into(),
        },
        IntentAction::Attend { .. } => {
            let there = c.act.phase == ActPhase::Perform;
            match state.ceremony.as_ref().map(|ceremony| &ceremony.rite) {
                Some(Rite::Wedding { a, b }) => {
                    let (a, b) = (name_of(state, a), name_of(state, b));
                    if there {
                        format!("at the wedding of {a} and {b}")
                    } else {
                        "on the way to the wedding on the green".into()
                    }
                }
                Some(Rite::Funeral { dead }) => {
                    let names: Vec<String> = dead.iter().map(|id| name_of(state, id)).collect();
                    let names = join_names(&names);
                    if there {
                        format!("at the graveside for {names}")
                    } else {
                        format!("on the way to the funeral of {names}")
                    }
                }
                None => "going to the ceremony".into(),
            }
        }
        IntentAction::WorldWork { job, target, .. } => {
            let what = describe_target(state, *job, target);
            if *job == WorldJob::Deliver {
                return format!("carrying {} to {what}", c.carried.describe());
            }
            let verb = crate::data::jobs::job(*job).verb;
            if c.act.phase == ActPhase::Perform { format!("{verb} {what}") } else { format!("on the way to {what}") }
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

/// "food", "a potion", "a gift", "a trinket", "a relic": an item in a line about trading.
fn short_item(item: ItemId) -> &'static str {
    match item {
        ItemId::Food => "food",
        ItemId::Potion => "a potion",
        ItemId::Gift => "a gift",
        ItemId::Trinket => "a trinket",
        ItemId::Relic => "a relic",
    }
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
