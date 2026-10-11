//! What is worth watching right now: the dragon, a boss, a fire, a raid, a wedding or a trial, a fight, a murderer
//! on the prowl, a quarrel, a tune with an audience. The digest ranks them once a second and sends the best few in
//! `Status`; the client's Auto camera follows the top one while its viewer leaves the camera alone, and the
//! biggest earn a "Happening now" card. Read-only: nothing here touches the sim.

use fsw_core::Vec2 as SimVec2;
use fsw_sim::state::{
    ActPhase, CeremonyPhase, Character, FightMode, GhostPhase, HousePhase, IntentAction, MonsterPhase, Quirk, Rite,
    SimState, TalkTone,
};
use fsw_sim::systems::fire::{site_name, site_pos};
use fsw_sim::systems::seasons;
use fsw_sim::util::{first_name, is_present, living, name_of, place_name_at};
use fsw_world::places::{ActivityId, get_place};

use crate::api::wire::{Hotspot, HotspotKind, Vec2};

/// How many hotspots a status carries.
pub const HOTSPOT_LIMIT: usize = 5;
/// An audience this big is a crowd.
const CROWD: u32 = 6;

/// Whole metres: near enough to look at, and the status is not resent for every step someone takes.
fn at(pos: SimVec2) -> Vec2 {
    Vec2 { x: pos.x.round(), z: pos.z.round() }
}

fn spot(kind: HotspotKind, subject: Option<&str>, pos: SimVec2, text: String, score: u32) -> Hotspot {
    Hotspot { kind, subject: subject.map(str::to_owned), at: at(pos), text, score: score.min(100) as u8 }
}

/// Upper-cases the first letter, for a line that starts with a place's name ("the tavern").
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().collect::<String>() + chars.as_str())
}

/// The villager a fight is about, and what they are fighting, if they are fighting at all.
fn fighting(c: &Character) -> Option<(&str, FightMode)> {
    match c.intent.as_ref().map(|i| &i.action) {
        Some(IntentAction::Fight { target_id, mode }) => Some((target_id.as_str(), *mode)),
        _ => None,
    }
}

/// Everything worth watching right now, best first, at most [`HOTSPOT_LIMIT`], one per subject.
pub fn hotspots_of(state: &SimState) -> Vec<Hotspot> {
    let mut found: Vec<Hotspot> = Vec::new();

    // The dragon and the bosses: the whole village's business.
    for m in state.monsters.values().filter(|m| m.phase != MonsterPhase::Dying) {
        let def = m.kind.def();
        if def.flies {
            found.push(spot(HotspotKind::Dragon, Some(&m.id), m.pos, "The dragon is over the valley".into(), 100));
        } else if def.boss {
            let name = m.name.clone().unwrap_or_else(|| format!("A {}", def.label));
            found.push(spot(HotspotKind::Boss, Some(&m.id), m.pos, format!("{name} is abroad"), 95));
        }
    }

    // Fires, fiercest first; followed through whoever carries water to it.
    for (site, house) in state.houses.iter().filter(|(_, h)| h.phase == HousePhase::Burning) {
        let helper = house.helpers.iter().find(|id| state.characters.get(*id).is_some_and(is_present));
        let pos = helper.and_then(|id| state.characters.get(id)).map_or_else(|| site_pos(state, site), |c| c.pos);
        let text = format!("{} is on fire", capitalised(&site_name(site)));
        found.push(spot(HotspotKind::Fire, helper.map(String::as_str), pos, text, 80 + (house.fire / 10.0) as u32));
    }

    // A war band, followed through its warchief or whoever leads the way.
    if let Some(band) = &state.war_band {
        let leader = band
            .warchief
            .iter()
            .chain(band.members.iter())
            .find_map(|id| state.monsters.get(id).filter(|m| m.phase != MonsterPhase::Dying));
        if let Some(m) = leader
            && !m.kind.def().boss
        {
            let text = format!("Orc raiders at {}", place_name_at(m.pos));
            found.push(spot(HotspotKind::Raid, Some(&m.id), m.pos, text, 82));
        }
    }

    // A wedding or a funeral, from the gathering to the feast.
    if let Some(ceremony) = &state.ceremony
        && ceremony.phase != CeremonyPhase::Announced
    {
        let place = get_place(&ceremony.place_id);
        let (subject, text) = match &ceremony.rite {
            Rite::Wedding { a, b } => {
                (a.clone(), format!("{} and {} are getting married", name_of(state, a), name_of(state, b)))
            }
            Rite::Funeral { dead } => {
                let names: Vec<String> = dead.iter().map(|id| name_of(state, id)).collect();
                (ceremony.officiant.clone(), format!("The village buries {}", names.join(" and ")))
            }
            Rite::Trial { accused, case_id, .. } => {
                let victim = state.cases.iter().find(|case| &case.victim == case_id).map_or_else(
                    || "a neighbour".to_owned(),
                    |case| case.victim_name.split(' ').next().unwrap_or_default().to_owned(),
                );
                (accused.clone(), format!("{} is on trial for killing {victim}", name_of(state, accused)))
            }
            Rite::Festival { feast } => {
                let name = seasons::festival_name(*feast);
                (ceremony.officiant.clone(), format!("The village keeps {name}"))
            }
            Rite::Election { candidates } => {
                let names: Vec<String> = candidates.iter().map(|id| name_of(state, id)).collect();
                let first = candidates.first().cloned().unwrap_or_default();
                (first, format!("The village chooses its elder: {}", names.join(" or ")))
            }
        };
        let score = match ceremony.phase {
            CeremonyPhase::Rite => 85,
            CeremonyPhase::Gathering => 70,
            _ => 50,
        };
        let present = state.characters.get(&subject).filter(|c| is_present(c));
        let pos = present.map_or(place.origin, |c| c.pos);
        found.push(spot(HotspotKind::Ceremony, present.map(|c| c.id.as_str()), pos, text, score));
    }

    // Fights: against the dead or raiders, the more on our side the bigger; between villagers, a brawl.
    let mut battle: Option<(&Character, u32)> = None;
    for c in living(state) {
        let Some((target, mode)) = fighting(c) else { continue };
        match mode {
            FightMode::Monster => {
                let count = battle.map_or(0, |(_, n)| n) + 1;
                battle = Some((battle.map_or(c, |(first, _)| first), count));
            }
            FightMode::Brawl | FightMode::Lethal => {
                // One line per pair: the one whose id sorts first tells it.
                if state.characters.get(target).is_some_and(|t| fighting(t).is_some_and(|(back, _)| back == c.id))
                    && target < c.id.as_str()
                {
                    continue;
                }
                let murder = mode == FightMode::Lethal && c.has(Quirk::Murderous);
                let (text, score) = if murder {
                    (format!("{} is striking down {}", first_name(c), name_of(state, target)), 92)
                } else if mode == FightMode::Lethal {
                    (format!("{} and {} are fighting to the death", first_name(c), name_of(state, target)), 88)
                } else {
                    (format!("{} and {} are brawling", first_name(c), name_of(state, target)), 72)
                };
                found.push(spot(HotspotKind::Brawl, Some(&c.id), c.pos, text, score));
            }
        }
    }
    // A murderer trailing after their next victim: the village does not see it, the viewers do.
    for c in living(state) {
        if let Some(IntentAction::Stalk { target_id }) = c.intent.as_ref().map(|i| &i.action) {
            let text = format!("{} is stalking {}", first_name(c), name_of(state, target_id));
            found.push(spot(HotspotKind::Stalking, Some(&c.id), c.pos, text, 64));
        }
    }
    if let Some((c, count)) = battle {
        let text = format!("Fighting at {}", place_name_at(c.pos));
        found.push(spot(HotspotKind::Battle, Some(&c.id), c.pos, text, 58 + 4 * count.min(6)));
    } else if let Some(m) =
        state.monsters.values().find(|m| m.phase != MonsterPhase::Dying && !m.kind.def().boss && !m.kind.def().flies)
    {
        found.push(spot(
            HotspotKind::Dead,
            Some(&m.id),
            m.pos,
            format!("The dead walk at {}", place_name_at(m.pos)),
            45,
        ));
    }

    for ghost in state.ghosts.values().filter(|g| g.phase == GhostPhase::Abroad) {
        let text = format!("The ghost of {} walks", ghost.name.split(' ').next().unwrap_or_default());
        found.push(spot(HotspotKind::Ghost, Some(&ghost.id), ghost.pos, text, 48));
    }

    // Conversations worth overhearing.
    for convo in state.conversations.values().filter(|c| !c.closed) {
        let (Some(a), Some(b)) = (state.characters.get(&convo.a), state.characters.get(&convo.b)) else { continue };
        let (kind, text, score) = match convo.tone {
            TalkTone::Hostile => {
                (HotspotKind::Quarrel, format!("{} and {} are quarrelling", first_name(a), first_name(b)), 52)
            }
            TalkTone::Romantic => {
                (HotspotKind::Romance, format!("{} is sweet on {}", first_name(a), first_name(b)), 40)
            }
            TalkTone::Apology => {
                (HotspotKind::Daily, format!("{} is making it up with {}", first_name(a), first_name(b)), 34)
            }
            TalkTone::Friendly => {
                (HotspotKind::Daily, format!("{} and {} are chatting", first_name(a), first_name(b)), 18)
            }
        };
        found.push(spot(kind, Some(&a.id), a.pos, text, score));
    }

    for show in &state.performances {
        let Some(performer) = state.characters.get(&show.performer_id) else { continue };
        if show.listeners.is_empty() {
            continue;
        }
        let what = match show.instrument.as_str() {
            "voice" => "sings".to_owned(),
            instrument => format!("plays the {instrument}"),
        };
        // The score grows with the audience; the line only says whether it is a crowd, so it changes seldom.
        let listeners = show.listeners.len() as u32;
        let audience = if listeners >= CROWD { " to a crowd" } else { "" };
        let text = format!("{} {what}{audience}", first_name(performer));
        found.push(spot(HotspotKind::Music, Some(&performer.id), show.pos, text, 30 + 4 * listeners.min(6)));
    }

    // Everyday life, so the Auto camera has something to show on a quiet morning: one of each at most.
    let performing = |c: &&Character, id: ActivityId| {
        c.act.phase == ActPhase::Perform && c.intent.as_ref().is_some_and(|i| i.is_activity(id)) && is_present(c)
    };
    if let Some(c) = living(state).find(|c| performing(c, ActivityId::ReadBoard)) {
        found.push(spot(
            HotspotKind::Daily,
            Some(&c.id),
            c.pos,
            format!("{} is reading a billboard", first_name(c)),
            28,
        ));
    }
    if let Some(c) = living(state).find(|c| c.mount.is_some()) {
        let horse = c.mount.as_ref().and_then(|id| state.animals.get(id)).and_then(|a| a.name.clone());
        let text = horse.map_or_else(
            || format!("{} is riding out", first_name(c)),
            |horse| format!("{} rides out on {horse}", first_name(c)),
        );
        found.push(spot(HotspotKind::Daily, Some(&c.id), c.pos, text, 26));
    }
    if let Some(c) = living(state).find(|c| c.is_child() && performing(c, ActivityId::Play)) {
        found.push(spot(HotspotKind::Daily, Some(&c.id), c.pos, format!("{} is playing", first_name(c)), 22));
    }
    if let Some(c) = living(state).find(|c| performing(c, ActivityId::Smith) || performing(c, ActivityId::Train)) {
        let what = if performing(&c, ActivityId::Smith) { "at the anvil" } else { "training" };
        found.push(spot(HotspotKind::Daily, Some(&c.id), c.pos, format!("{} is {what}", first_name(c)), 12));
    }

    // Best first; a tie keeps the order found, so the list does not flicker.
    found.sort_by_key(|h| std::cmp::Reverse(h.score));
    let mut seen: Vec<String> = Vec::new();
    found.retain(|h| match &h.subject {
        Some(id) if seen.contains(id) => false,
        Some(id) => {
            seen.push(id.clone());
            true
        }
        None => true,
    });
    found.truncate(HOTSPOT_LIMIT);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsw_core::time::TICKS_PER_DAY;
    use fsw_sim::Sim;
    use fsw_sim::state::House;
    use fsw_testing::create_test_state;

    #[test]
    fn a_burning_home_comes_before_a_tune() {
        let mut state = create_test_state(1);
        let house = House { phase: HousePhase::Burning, fire: 40.0, built: 0.0, since_tick: 0, helpers: Vec::new() };
        state.houses.insert("home_4".into(), house);
        let spots = hotspots_of(&state);
        let fire = &spots[0];
        assert_eq!(fire.kind, HotspotKind::Fire);
        assert_eq!(fire.subject, None);
        assert!(fire.text.ends_with("is on fire"), "{}", fire.text);
        assert_eq!(fire.score, 84);
    }

    #[test]
    fn viewers_see_the_murderer_stalking() {
        let mut state = create_test_state(1);
        let mut events = Vec::new();
        let hesper = fsw_testing::admit(&mut fsw_sim::util::Ctx::new(&mut state, &mut events), "hesper");
        let kael = state.characters.values().find(|c| first_name(c) == "Kael").unwrap().id.clone();
        state.characters.get_mut(&hesper).unwrap().intent = Some(fsw_sim::state::Intent::new(
            IntentAction::Stalk { target_id: kael },
            fsw_sim::state::IntentSource::Routine,
        ));
        let spots = hotspots_of(&state);
        let stalking = spots.iter().find(|h| h.kind == HotspotKind::Stalking).expect("a stalking hotspot");
        assert_eq!(stalking.subject.as_deref(), Some(hesper.as_str()));
        assert!(stalking.text.ends_with("is stalking Kael"), "{}", stalking.text);
        let detail = crate::digest::character_detail_of(&state, &hesper).unwrap();
        assert!(detail.secret.is_some_and(|s| s.starts_with("A murderer")));
        assert!(detail.status.starts_with("stalking Kael"), "{}", detail.status);
    }

    #[test]
    fn days_of_village_life_are_ranked_and_never_repeat_a_subject() {
        let mut sim = Sim::new(create_test_state(7));
        let mut kinds = std::collections::HashSet::new();
        for _ in 0..4 * TICKS_PER_DAY / 20 {
            for _ in 0..20 {
                sim.step();
            }
            let spots = hotspots_of(&sim.state);
            assert!(spots.len() <= HOTSPOT_LIMIT);
            assert!(spots.windows(2).all(|pair| pair[0].score >= pair[1].score));
            let mut subjects: Vec<&String> = spots.iter().filter_map(|h| h.subject.as_ref()).collect();
            let before = subjects.len();
            subjects.dedup();
            subjects.sort();
            subjects.dedup();
            assert_eq!(subjects.len(), before);
            kinds.extend(spots.iter().map(|h| h.kind));
        }
        // A few days always bring the dead and someone to stand against them.
        assert!(kinds.contains(&HotspotKind::Battle), "{kinds:?}");
    }
}
