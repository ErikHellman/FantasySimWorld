//! The murder mystery: a killing nobody saw becomes a case. Whoever saw the murderer trailing after the victim
//! that day, whoever finds the body and whoever knew of a grudge holds a clue; clues spread in conversation;
//! when enough of the village points at one person, they are tried on the green (`systems::ceremonies`), and
//! the guilty, or whoever the village takes for guilty, is driven out by the west road. A case nobody is
//! convicted for goes cold after a week. Viewers are told who did it as soon as the case opens (the chronicle
//! names them, and the murderer's page and the Auto camera show the stalking); the villagers never are.
//!
//! Nothing here draws from the RNG until someone has been murdered: the sightings that may become clues are
//! settled by a hash, so a world without a murder runs exactly as it did before.

use fsw_core::math2d::dist;
use fsw_core::rng::hash_string;
use fsw_core::time::{TICKS_PER_DAY, TICKS_PER_GAME_MINUTE, minutes_to_ticks};
use fsw_core::{Vec2, vec2};
use fsw_world::places::get_place;

use crate::data::ceremonies::line;
use crate::data::trials::TRACES;
use crate::events::{Emote, SimEvent};
use crate::state::{
    Case, Character, ChronicleKind, Clue, Id, Intent, IntentAction, IntentSource, MemoryKind, Sighting, StimulusKind,
};
use crate::systems::ceremonies::{self, schedule_trial};
use crate::systems::death::{detach, schedule_arrival_after_loss};
use crate::systems::intents::fresh_act;
use crate::systems::relationships::{RelationDelta, adjust};
use crate::util::{Ctx, bump_epoch, first_name, is_fighting, is_present, living, living_ids, name_of, place_name_at};

/// How close a witness must be to notice someone trailing after someone else.
const SIGHT_RANGE: f64 = 15.0;
/// The chance, in a hundred, that a witness in range notices, each game minute.
const SIGHT_CHANCE: u32 = 8;
/// How close one must come to the body to find it.
const FIND_RANGE: f64 = 7.0;
/// If nobody has come upon the body by then, whoever is nearest is said to have found it.
const FIND_MINUTES: f64 = 50.0;
/// What each kind of clue counts for, per person who knows it.
const SIGHTING_WEIGHT: f64 = 4.0;
const TRACE_WEIGHT: f64 = 3.0;
const MOTIVE_WEIGHT: f64 = 2.0;
/// The share of scene traces that point at the killer; the rest point at someone with a grudge, or anyone.
const TRUE_TRACE_CHANCE: f64 = 0.55;
/// A grievance this deep is a motive.
const MOTIVE_GRIEVANCE: f64 = 30.0;
/// The village does not accuse anyone sooner than this after the killing.
const ACCUSE_AFTER_HOURS: f64 = 8.0;
/// How much the village must have against someone to put them on trial.
pub const ACCUSE_SCORE: f64 = 10.0;
/// A case is tried at most this many times.
const MAX_TRIALS: i64 = 2;
/// After this many days without a conviction, the trail is cold.
const COLD_DAYS: i64 = 7;
/// A voter believes the accused guilty from this much.
const GUILTY_BELIEF: f64 = 4.0;
/// An exile who has not reached the road by then is gone anyway.
const EXILE_HOURS: f64 = 3.0;

/// Where the exiled leave the valley: the spot on the west road newcomers arrive by.
fn exit_point(ctx: &Ctx) -> Vec2 {
    let road = get_place("west_road");
    ctx.state.nav.nearest_walkable(vec2(road.origin.x - 2.0, road.origin.z), 6).unwrap_or(road.door)
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().collect::<String>() + chars.as_str())
}

/// The open case about `victim`, if there is one.
fn case_mut<'a>(ctx: &'a mut Ctx, victim: &str) -> Option<&'a mut Case> {
    ctx.state.cases.iter_mut().find(|case| case.victim == victim)
}

/// How much the village has against `suspect` in `case`: each clue's weight for everyone who knows it.
pub fn suspicion(case: &Case, suspect: &str) -> f64 {
    case.clues.iter().filter(|clue| clue.suspect == suspect).map(|clue| clue.weight * clue.known_by.len() as f64).sum()
}

/// A killing went unseen: the case opens with what the village already has, the sightings of the killer
/// trailing after the victim today and the grudges against them that a friend of theirs knew of.
pub fn open_case(ctx: &mut Ctx, victim: &Character, killer: &str) {
    let tick = ctx.tick();
    let (victim_id, victim_name) = (victim.id.clone(), first_name(victim).to_owned());
    let killer_name = name_of(ctx.state, killer);
    let mut clues: Vec<Clue> = Vec::new();

    let since = tick - TICKS_PER_DAY;
    let seen: Vec<Sighting> = ctx
        .state
        .sightings
        .iter()
        .filter(|s| s.stalker == killer && s.target == victim_id && s.tick >= since && ctx.alive(&s.witness))
        .cloned()
        .collect();
    for sighting in seen {
        let fact = format!("{killer_name} was seen trailing after {victim_name} near {} that day", sighting.place);
        ctx.remember(
            &sighting.witness,
            format!(
                "{victim_name} is dead, and I saw {killer_name} trailing after them near {} that day.",
                sighting.place
            ),
            9.0,
            MemoryKind::Event,
            vec![victim_id.clone(), killer.to_owned()],
        );
        ctx.stimulate(
            &sighting.witness,
            StimulusKind::Witness,
            format!("You saw {killer_name} following {victim_name} on the day they were killed."),
            2,
            Some(killer),
        );
        clues.push(Clue {
            suspect: killer.to_owned(),
            fact,
            weight: SIGHTING_WEIGHT,
            known_by: vec![sighting.witness],
        });
    }
    ctx.state.sightings.retain(|s| s.target != victim_id);

    // Who had it in for the victim, as their closest friend knew it.
    let friend = living(ctx.state)
        .filter(|c| c.id != killer)
        .filter_map(|c| victim.relationships.get(&c.id).map(|r| (c.id.clone(), r.affinity)))
        .filter(|(_, affinity)| *affinity >= 20.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id);
    if let Some(friend) = friend {
        let mut grudges: Vec<(Id, f64)> = living(ctx.state)
            .filter(|c| c.id != friend && !c.is_child())
            .filter_map(|c| {
                let theirs = c.relationships.get(&victim_id).map_or(0.0, |r| r.grievance);
                let victims = victim.relationships.get(&c.id).map_or(0.0, |r| r.grievance);
                let grudge = theirs.max(victims);
                (grudge >= MOTIVE_GRIEVANCE).then(|| (c.id.clone(), grudge))
            })
            .collect();
        grudges.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (suspect, _) in grudges.into_iter().take(2) {
            let fact = format!("{} had a grudge against {victim_name}", name_of(ctx.state, &suspect));
            ctx.remember(
                &friend,
                format!("{victim_name} is dead. {fact}."),
                6.0,
                MemoryKind::Event,
                vec![victim_id.clone(), suspect.clone()],
            );
            clues.push(Clue { suspect, fact, weight: MOTIVE_WEIGHT, known_by: vec![friend.clone()] });
        }
    }

    ctx.state.cases.push(Case {
        victim: victim_id,
        victim_name: victim.profile.name.clone(),
        killer: killer.to_owned(),
        tick,
        pos: victim.pos,
        clues,
        found_by: None,
        trials: 0,
        cleared: Vec::new(),
        closed: false,
    });
    // Viewers watch the village try to work out what they already know. The chronicle is never shown to a mind.
    ctx.chronicle(
        ChronicleKind::Death,
        format!("Only you know it: {killer_name} killed {victim_name}, and nobody saw."),
        vec![killer.to_owned(), victim.id.clone()],
    );
}

/// Each tick: who sees a stalker at work, who finds a body, who is accused, which cases go cold, and the
/// exiles on their way out.
pub fn update_cases(ctx: &mut Ctx) {
    let tick = ctx.tick();
    if tick % TICKS_PER_GAME_MINUTE == 0 {
        note_stalkers(ctx);
    }
    if !ctx.state.cases.iter().any(|case| !case.closed) && !living(ctx.state).any(|c| c.exiled_since.is_some()) {
        return;
    }
    find_bodies(ctx);
    if tick % minutes_to_ticks(60.0) == 0 {
        accuse(ctx);
        go_cold(ctx);
    }
    walk_out(ctx);
}

/// Anyone nearby may notice someone trailing after someone else; it matters only if the one followed is killed.
fn note_stalkers(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let stalking: Vec<(Id, Id, Vec2)> = living(ctx.state)
        .filter_map(|c| match c.intent.as_ref().map(|i| &i.action) {
            Some(IntentAction::Stalk { target_id }) if is_present(c) => Some((c.id.clone(), target_id.clone(), c.pos)),
            _ => None,
        })
        .collect();
    if stalking.is_empty() {
        if !ctx.state.sightings.is_empty() {
            ctx.state.sightings.retain(|s| tick - s.tick < TICKS_PER_DAY);
        }
        return;
    }
    for (stalker, target, pos) in stalking {
        let witnesses: Vec<Id> = living(ctx.state)
            .filter(|w| w.id != stalker && w.id != target && is_present(w) && dist(w.pos, pos) < SIGHT_RANGE)
            .map(|w| w.id.clone())
            .collect();
        for witness in witnesses {
            let already =
                ctx.state.sightings.iter().any(|s| s.witness == witness && s.stalker == stalker && s.target == target);
            if already || hash_string(&format!("{witness}:{stalker}:{tick}")) % 100 >= SIGHT_CHANCE {
                continue;
            }
            let place = place_name_at(pos);
            ctx.remember(
                &witness,
                format!(
                    "Saw {} trailing after {} near {place}. Odd.",
                    name_of(ctx.state, &stalker),
                    name_of(ctx.state, &target)
                ),
                3.0,
                MemoryKind::Event,
                vec![stalker.clone(), target.clone()],
            );
            ctx.state.sightings.push(Sighting {
                witness,
                stalker: stalker.clone(),
                target: target.clone(),
                tick,
                place,
            });
        }
    }
}

/// The first to come upon a body finds it, and something beside it.
fn find_bodies(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let unfound: Vec<(Id, Vec2, i64)> = ctx
        .state
        .cases
        .iter()
        .filter(|case| !case.closed && case.found_by.is_none())
        .map(|case| (case.victim.clone(), case.pos, case.tick))
        .collect();
    for (victim, pos, since) in unfound {
        let late = tick - since >= minutes_to_ticks(FIND_MINUTES);
        let finder = living(ctx.state)
            .filter(|c| c.exiled_since.is_none() && (late || (is_present(c) && dist(c.pos, pos) < FIND_RANGE)))
            .min_by(|a, b| dist(a.pos, pos).total_cmp(&dist(b.pos, pos)))
            .map(|c| c.id.clone());
        if let Some(finder) = finder {
            found(ctx, &victim, &finder);
        }
    }
}

fn found(ctx: &mut Ctx, victim: &str, finder: &str) {
    let Some(case) = case_mut(ctx, victim) else { return };
    case.found_by = Some(finder.to_owned());
    let case = case.clone();
    let victim_name = first_name_of(&case.victim_name);
    let where_ = place_name_at(case.pos);
    // The trace points at the killer more often than not; otherwise at someone with a grudge, or anyone.
    let suspect = if ctx.chance(TRUE_TRACE_CHANCE) {
        Some(case.killer.clone())
    } else {
        let grudges: Vec<Id> = case
            .clues
            .iter()
            .filter(|clue| clue.suspect != case.killer && clue.suspect != finder && ctx.alive(&clue.suspect))
            .map(|clue| clue.suspect.clone())
            .collect();
        let anyone: Vec<Id> = living(ctx.state)
            .filter(|c| c.id != finder && c.id != case.killer && !c.is_child() && c.exiled_since.is_none())
            .map(|c| c.id.clone())
            .collect();
        let pool = if grudges.is_empty() { anyone } else { grudges };
        (!pool.is_empty()).then(|| pool[ctx.pick_index(pool.len())].clone())
    };
    let mut memory = format!("I found {victim_name}'s body at {where_}.");
    if let Some(suspect) = suspect.filter(|s| ctx.alive(s)) {
        let trace = ctx.pick(&TRACES);
        let fact = capitalised(&line(trace, &[("suspect", name_of(ctx.state, &suspect).as_str())]));
        memory = format!("{memory} {fact}.");
        if let Some(case) = case_mut(ctx, victim) {
            case.clues.push(Clue { suspect, fact, weight: TRACE_WEIGHT, known_by: vec![finder.to_owned()] });
        }
    }
    ctx.remember(finder, memory, 9.0, MemoryKind::Event, vec![victim.to_owned()]);
    ctx.stimulate(finder, StimulusKind::Death, format!("You found {victim_name}'s body at {where_}."), 2, Some(victim));
    ctx.emit(SimEvent::Emote { id: finder.to_owned(), emote: Emote::Alert });
    let finder_name = name_of(ctx.state, finder);
    ctx.chronicle(
        ChronicleKind::Death,
        format!("{finder_name} found {victim_name}'s body at {where_}."),
        vec![finder.to_owned(), victim.to_owned()],
    );
}

fn first_name_of(name: &str) -> String {
    name.split(' ').next().unwrap_or_default().to_owned()
}

/// Two villagers who talk share what they know about any open case, the heaviest thing first, one each way.
pub fn share(ctx: &mut Ctx, a: &str, b: &str) {
    if !ctx.state.cases.iter().any(|case| !case.closed) {
        return;
    }
    for (teller, listener) in [(a, b), (b, a)] {
        let mut best: Option<(usize, usize, f64)> = None;
        for (i, case) in ctx.state.cases.iter().enumerate().filter(|(_, case)| !case.closed) {
            for (j, clue) in case.clues.iter().enumerate() {
                let tellable = clue.known_by.iter().any(|k| k == teller)
                    && !clue.known_by.iter().any(|k| k == listener)
                    && clue.suspect != listener
                    && clue.weight > 0.0;
                if tellable && best.is_none_or(|(_, _, w)| clue.weight > w) {
                    best = Some((i, j, clue.weight));
                }
            }
        }
        let Some((i, j, _)) = best else { continue };
        let clue = &mut ctx.state.cases[i].clues[j];
        clue.known_by.push(listener.to_owned());
        let (fact, suspect) = (clue.fact.clone(), clue.suspect.clone());
        let teller_name = name_of(ctx.state, teller);
        ctx.remember(
            listener,
            format!("{teller_name} told me: {fact}."),
            6.0,
            MemoryKind::Event,
            vec![teller.to_owned(), suspect.clone()],
        );
        adjust(ctx, listener, &suspect, RelationDelta { trust: -6.0, affinity: -3.0, ..Default::default() });
    }
}

/// Once an hour: when the village has enough against someone, they are accused and a trial is called.
fn accuse(ctx: &mut Ctx) {
    let tick = ctx.tick();
    if ctx.state.ceremony.is_some() {
        return;
    }
    let ready = ctx.state.cases.iter().find(|case| {
        !case.closed && case.trials < MAX_TRIALS && tick - case.tick >= minutes_to_ticks(ACCUSE_AFTER_HOURS * 60.0)
    });
    let Some(case) = ready.cloned() else { return };
    let suspects: Vec<Id> = living(ctx.state)
        .filter(|c| !c.is_child() && c.exiled_since.is_none() && !case.cleared.contains(&c.id))
        .map(|c| c.id.clone())
        .collect();
    let Some((accused, score)) = suspects
        .iter()
        .map(|id| (id.clone(), suspicion(&case, id)))
        .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
    else {
        return;
    };
    if score < ACCUSE_SCORE {
        return;
    }
    // The one who knows the weightiest thing against them speaks it, if they are grown and here.
    let mut against: Vec<&Clue> = case.clues.iter().filter(|clue| clue.suspect == accused).collect();
    against.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    let accuser = against
        .iter()
        .flat_map(|clue| clue.known_by.iter())
        .find(|id| {
            **id != accused && ctx.state.characters.get(*id).is_some_and(|c| !c.is_child() && c.exiled_since.is_none())
        })
        .cloned();
    schedule_trial(ctx, &case.victim, &accused, accuser.as_deref());
}

/// After a week with nobody convicted, the trail is cold.
fn go_cold(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let cold: Vec<Id> = ctx
        .state
        .cases
        .iter()
        .filter(|case| !case.closed && tick - case.tick >= COLD_DAYS * TICKS_PER_DAY)
        .map(|case| case.victim.clone())
        .collect();
    for victim in cold {
        let Some(case) = case_mut(ctx, &victim) else { continue };
        case.closed = true;
        let case = case.clone();
        ctx.chronicle(
            ChronicleKind::Death,
            format!(
                "Nobody was ever named for the killing of {}. The trail has gone cold.",
                first_name_of(&case.victim_name)
            ),
            vec![victim.clone()],
        );
        reveal(ctx, &case, None);
    }
}

/// What the village believes of `accused` in `case`, voter by voter: the clues against them that each knows,
/// the one laid out at the trial (`aired`), grudges and fondness, and the killer's wish to see someone else hang.
/// Returns those who find them guilty and those who do not.
pub fn votes(ctx: &Ctx, case_id: &str, accused: &str, aired: Option<usize>, voters: &[Id]) -> (Vec<Id>, Vec<Id>) {
    let Some(case) = ctx.state.cases.iter().find(|case| case.victim == case_id) else {
        return (Vec::new(), voters.to_vec());
    };
    let (mut guilty, mut innocent) = (Vec::new(), Vec::new());
    for voter in voters.iter().filter(|v| v.as_str() != accused) {
        let Some(c) = ctx.state.characters.get(voter) else { continue };
        let mut belief: f64 = case
            .clues
            .iter()
            .enumerate()
            .filter(|(i, clue)| clue.suspect == accused && (clue.known_by.contains(voter) || Some(*i) == aired))
            .map(|(_, clue)| clue.weight)
            .sum();
        let r = c.relationships.get(accused);
        belief += r.map_or(0.0, |r| r.grievance / 20.0 - r.affinity / 20.0);
        if r.is_some_and(|r| r.kin) || c.spouse_id.as_deref() == Some(accused) {
            belief -= 6.0;
        }
        if *voter == case.killer {
            belief += 3.0;
        }
        if belief >= GUILTY_BELIEF { guilty.push(voter.clone()) } else { innocent.push(voter.clone()) }
    }
    (guilty, innocent)
}

/// The weightiest clue against `accused`, by index, for the accuser to lay out.
pub fn strongest_clue(ctx: &Ctx, case_id: &str, accused: &str) -> Option<(usize, String)> {
    let case = ctx.state.cases.iter().find(|case| case.victim == case_id)?;
    case.clues
        .iter()
        .enumerate()
        .filter(|(_, clue)| clue.suspect == accused)
        .max_by(|a, b| a.1.weight.total_cmp(&b.1.weight))
        .map(|(i, clue)| (i, clue.fact.clone()))
}

/// The verdict is in: the guilty are driven out, the acquitted go home, and the village remembers.
pub fn conclude_trial(ctx: &mut Ctx, case_id: &str, accused: &str, accuser: &str, guilty: bool, present: &[Id]) {
    let tick = ctx.tick();
    let accused_name = name_of(ctx.state, accused);
    let Some(case) = case_mut(ctx, case_id) else { return };
    case.trials += 1;
    let victim_name = first_name_of(&case.victim_name);
    let ids = vec![accused.to_owned(), case_id.to_owned()];
    if guilty {
        case.closed = true;
        let case = case.clone();
        let killer = case.killer == accused;
        let mine = if killer {
            format!("The village found me out for killing {victim_name}. I am driven from the valley.")
        } else {
            format!(
                "The village found me guilty of killing {victim_name}. I never did it. I am driven from the valley."
            )
        };
        ctx.remember(accused, mine, 10.0, MemoryKind::Event, vec![case_id.to_owned()]);
        for id in present.iter().filter(|id| id.as_str() != accused) {
            ctx.remember(
                id,
                format!("The village found {accused_name} guilty of killing {victim_name} and drove them out."),
                7.0,
                MemoryKind::Event,
                ids.clone(),
            );
            adjust(ctx, id, accused, RelationDelta { affinity: -25.0, trust: -40.0, ..Default::default() });
        }
        let c = ctx.c(accused);
        c.exiled_since = Some(tick);
        ctx.chronicle(
            ChronicleKind::Conflict,
            format!("The village found {accused_name} guilty of killing {victim_name} and drove them from the valley."),
            ids,
        );
        reveal(ctx, &case, Some(accused));
    } else {
        case.cleared.push(accused.to_owned());
        // What pointed at them is set aside; the village has heard it and not believed it.
        for clue in case.clues.iter_mut().filter(|clue| clue.suspect == accused) {
            clue.weight = 0.0;
        }
        ctx.remember(
            accused,
            format!("I was tried on the green for killing {victim_name}, and the village let me go."),
            9.0,
            MemoryKind::Event,
            vec![case_id.to_owned()],
        );
        if accuser != accused && ctx.alive(accuser) {
            adjust(ctx, accused, accuser, RelationDelta { grievance: 25.0, affinity: -15.0, ..Default::default() });
        }
        for id in present.iter().filter(|id| id.as_str() != accused) {
            ctx.remember(
                id,
                format!("{accused_name} was tried for killing {victim_name} and let go. The killer is still among us."),
                6.0,
                MemoryKind::Event,
                ids.clone(),
            );
        }
        ctx.chronicle(
            ChronicleKind::Conflict,
            format!("The village could not agree that {accused_name} killed {victim_name}, and let them go."),
            ids,
        );
    }
    ctx.emit(SimEvent::SavePoint { reason: "trial".into() });
}

/// When a case ends without the killer found out, the viewers are reminded how it really stands.
fn reveal(ctx: &mut Ctx, case: &Case, convicted: Option<&str>) {
    let victim_name = first_name_of(&case.victim_name);
    let killer_name = name_of(ctx.state, &case.killer);
    let still_here = ctx.alive(&case.killer);
    let text = match convicted {
        Some(convicted) if convicted == case.killer => return,
        Some(convicted) => {
            let innocent = name_of(ctx.state, convicted);
            if still_here {
                format!("{innocent} was innocent. {killer_name} killed {victim_name}, and is still among them.")
            } else {
                format!("{innocent} was innocent. It was {killer_name} who killed {victim_name}.")
            }
        }
        None => format!("The valley never learnt that it was {killer_name} who killed {victim_name}."),
    };
    ctx.chronicle(ChronicleKind::Death, text, vec![case.killer.clone(), case.victim.clone()]);
}

/// The exiled walk to the west road and are gone.
fn walk_out(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let exit = exit_point(ctx);
    let exiles: Vec<(Id, i64)> =
        living(ctx.state).filter_map(|c| c.exiled_since.map(|since| (c.id.clone(), since))).collect();
    for (id, since) in exiles {
        let c = ctx.cr(&id);
        if dist(c.pos, exit) < 3.0 || tick - since >= minutes_to_ticks(EXILE_HOURS * 60.0) {
            depart(ctx, &id);
            continue;
        }
        if is_fighting(c) || ceremonies::attending(c) {
            continue;
        }
        let leaving = matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::GoTo { place_id }) if place_id == "west_road");
        if !leaving {
            detach(ctx, &id);
            let c = ctx.c(&id);
            c.queue.clear();
            c.intent = Some(Intent::new(IntentAction::GoTo { place_id: "west_road".into() }, IntentSource::Reflex));
            c.act = fresh_act(tick);
            bump_epoch(c);
        }
    }
}

/// Leaves the valley for good: out of the living, out of everyone's plans, a bed free for a newcomer.
pub fn depart(ctx: &mut Ctx, id: &str) {
    detach(ctx, id);
    ceremonies::cancel_wedding_of(ctx, id);
    let Some(gone) = ctx.state.characters.shift_remove(id) else { return };
    ctx.state.order.retain(|o| o != id);
    ctx.state.exiles.insert(id.to_owned(), gone.profile.name.clone());
    let name = first_name(&gone).to_owned();
    for other in living_ids(ctx.state) {
        let c = ctx.c(&other);
        if c.combat.target_id.as_deref() == Some(id) {
            c.combat.target_id = None;
        }
        if c.combat.mercy_target_id.as_deref() == Some(id) {
            c.combat.mercy_target_id = None;
            c.combat.mercy_until_tick = 0;
        }
        let partner = c.partner_id.as_deref() == Some(id) || c.spouse_id.as_deref() == Some(id);
        if partner {
            c.partner_id = None;
            c.partner_since_tick = 0;
            c.spouse_id = None;
            ctx.remember(
                &other,
                format!("{name} is gone from the valley, driven out. I am alone."),
                9.0,
                MemoryKind::Event,
                vec![id.to_owned()],
            );
        }
    }
    ctx.chronicle(
        ChronicleKind::World,
        format!("{} has left the valley by the west road.", gone.profile.name),
        vec![id.to_owned()],
    );
    ctx.emit(SimEvent::SavePoint { reason: "exile".into() });
    schedule_arrival_after_loss(ctx);
}
