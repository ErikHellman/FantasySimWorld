//! What sets some villagers apart from the rest: the murderer's hunting days, the
//! drunk's trouble, the gossip's stories, the secret necromancer's long nights,
//! and the nap on the grass that can catch anyone. Runs after the routine, so it
//! can step in whether a plan came from the routine or from the model.

use fsw_core::math2d::dist;
use fsw_core::rng;
use fsw_core::time::{TICKS_PER_DAY, TICKS_PER_GAME_MINUTE, minutes_to_ticks};
use fsw_core::vec2;
use fsw_world::places::get_place;

use crate::data::activities::{ActivityKind, activity};
use crate::data::smalltalk::FALSE_STORIES;
use crate::events::{Emote, SimEvent};
use crate::state::{
    ActivityId, ChronicleKind, DeadRecord, FightMode, Id, Intent, IntentAction, IntentSource, MemoryKind,
    MemoryKind::Event, MonsterKind, Quirk, Rumour,
};
use crate::systems::combat::{max_hp, start_fight, witnesses};
use crate::systems::death::{detach, schedule_arrival_after_loss};
use crate::systems::intents::fresh_act;
use crate::systems::monsters::spawn_monster;
use crate::systems::relationships::{RelationDelta, adjust};
use crate::util::{Ctx, bump_epoch, first_name, is_fighting, is_present, living, living_ids, place_name_at, rel};

/// A plan block with this note is a nap on the grass rather than the activity it names.
pub const NAP_NOTE: &str = "a nap on the grass";
/// A plan block with this note is a long night over forbidden books.
pub const STUDY_NOTE: &str = "reading late by candlelight";

/// Hours of the day a nap out of doors is possible: nobody dozes off in the open after dark.
pub const NAP_HOURS: std::ops::Range<f64> = 7.0..18.5;
/// How long the one who has turned to the dark must have lived here before it can claim them.
const FALL_FROM_DAY: i64 = 7;
/// The chance, each night over the forbidden books, that the dark claims them for good.
const FALL_CHANCE: f64 = 0.02;

const DRUNK_LINES: [&str; 5] = [
    "Another round! Who'sh buying?",
    "I love thish village. I love all of you. Mosht of you.",
    "Which of you moved the road?",
    "Shing with me! No? Cowards, the lot of you.",
    "I'm not drunk. The ground is jusht... uneven.",
];
const DRUNK_TAUNTS: [&str; 4] = [
    "What're you looking at, {b}?",
    "You! Yes, you, {b}! Put 'em up!",
    "Shay that again, {b}. Go on, I dare you.",
    "Been wanting to do thish for ages, {b}.",
];

/// Whether this character is free for a quirk to take over what they are doing.
fn idle_enough(ctx: &Ctx, id: &str) -> bool {
    let c = ctx.cr(id);
    let tick = ctx.tick();
    if c.convo_id.is_some() || c.combat.yielded || is_fighting(c) || tick < c.watch_until_tick && c.inside.is_none() {
        return false;
    }
    match c.intent.as_ref() {
        None => true,
        Some(intent) if intent.source == IntentSource::Llm => false,
        Some(Intent { action: IntentAction::Activity { activity: id, .. }, .. }) => {
            *id != ActivityId::Sleep && *id != ActivityId::Watch && *id != ActivityId::Eat
        }
        Some(Intent { action: IntentAction::GoTo { .. } | IntentAction::Wait { .. }, .. }) => true,
        Some(_) => false,
    }
}

fn replace_intent(ctx: &mut Ctx, id: &str, intent: Intent) {
    detach(ctx, id);
    let tick = ctx.tick();
    let c = ctx.c(id);
    c.tool = None;
    c.queue.clear();
    c.intent = Some(intent);
    c.act = fresh_act(tick);
    bump_epoch(c);
}

pub fn update_quirks(ctx: &mut Ctx) {
    let tick = ctx.tick();
    for (i, id) in living_ids(ctx.state).into_iter().enumerate() {
        if !ctx.alive(&id) {
            continue;
        }
        sober_up(ctx, &id);
        // Each character is considered once a game minute, at a different tick.
        if (tick + i as i64) % TICKS_PER_GAME_MINUTE != 0 {
            continue;
        }
        let c = ctx.cr(&id);
        if c.has(Quirk::Murderous) {
            hunt(ctx, &id);
        }
        if ctx.alive(&id) && ctx.cr(&id).drunk(tick) {
            drunken_trouble(ctx, &id);
        }
        if ctx.alive(&id) && ctx.cr(&id).has(Quirk::Occultist) {
            dark_studies(ctx, &id);
        }
        if ctx.alive(&id) {
            doze_off(ctx, &id);
        }
    }
}

// --- Naps -------------------------------------------------------------------

/// The tired, and the lazy, lie down on the grass for a while.
fn doze_off(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    let hour = ctx.time.hour;
    let c = ctx.cr(id);
    if !NAP_HOURS.contains(&hour) || c.has(Quirk::Diligent) || !is_present(c) || c.drunk(tick) {
        return;
    }
    let lazy = c.has(Quirk::Lazy);
    let tired = if lazy { 55.0 } else { 30.0 };
    if c.needs.energy >= tired || (tick / TICKS_PER_GAME_MINUTE) % 10 != 0 || !idle_enough(ctx, id) {
        return;
    }
    // Only leisure, wandering or nothing at all gives way to a nap; work does for the lazy.
    let shirkable = match c.intent.as_ref().map(|i| &i.action) {
        Some(IntentAction::Activity { activity: a, .. }) => {
            let kind = activity(*a).kind;
            kind == ActivityKind::Leisure || (lazy && kind == ActivityKind::Work)
        }
        Some(IntentAction::Nap { .. }) => false,
        _ => true,
    };
    if shirkable && ctx.chance(if lazy { 0.3 } else { 0.2 }) {
        let minutes = ctx.range(40.0, 90.0);
        replace_intent(ctx, id, Intent::new(IntentAction::Nap { minutes }, IntentSource::Routine));
    }
}

// --- The murderer -------------------------------------------------------------

/// A murderer only hunts in a village this full, so the killings never empty it.
const HUNT_MIN_POPULATION: usize = 10;

/// Whether a killing is allowed at all today: the guard rails of any duel to the death, and a little more.
fn killing_allowed(ctx: &Ctx, id: &str) -> bool {
    let tick = ctx.tick();
    let c = ctx.cr(id);
    tick - c.arrived_tick >= TICKS_PER_DAY * 2
        && tick - ctx.state.director.last_lethal_tick >= TICKS_PER_DAY * 3
        && ctx.state.order.len() >= HUNT_MIN_POPULATION
        && c.hp >= max_hp(c) * 0.7
}

/// At dawn the murderer decides whether today is a hunting day; on one, they pick
/// someone alone and follow them.
fn hunt(ctx: &mut Ctx, id: &str) {
    let (day, hour) = (ctx.time.day, ctx.time.hour);
    if ctx.cr(id).quirk_state.hunt_rolled_day != day && hour >= 6.0 {
        ctx.c(id).quirk_state.hunt_rolled_day = day;
        if killing_allowed(ctx, id) && ctx.chance(0.25) {
            ctx.c(id).quirk_state.hunt_day = day;
        }
    }
    let c = ctx.cr(id);
    if c.quirk_state.hunt_day != day || !(8.0..19.0).contains(&hour) || !is_present(c) {
        return;
    }
    if matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::Stalk { .. }))
        || !idle_enough(ctx, id)
        || !killing_allowed(ctx, id)
        || !ctx.chance(0.05)
    {
        return;
    }
    let Some(target) = pick_prey(ctx, id) else { return };
    replace_intent(ctx, id, Intent::new(IntentAction::Stalk { target_id: target }, IntentSource::Routine));
}

/// Someone out of doors and on their own; the sleeping most of all.
fn pick_prey(ctx: &mut Ctx, id: &str) -> Option<Id> {
    let candidates: Vec<(Id, f64)> = living(ctx.state)
        .filter(|o| {
            o.id != id
                && o.inside.is_none()
                && o.convo_id.is_none()
                && !is_fighting(o)
                && !o.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Watch))
        })
        .map(|o| (o.id.clone(), if o.asleep { 4.0 } else { 1.0 }))
        .collect();
    let alone: Vec<(Id, f64)> =
        candidates.into_iter().filter(|(o, _)| witnesses(ctx, ctx.cr(o).pos, &[id, o.as_str()]).is_empty()).collect();
    let index = rng::weighted_pick_index(&mut ctx.state.rng, &alone, |(_, w)| *w)?;
    Some(alone[index].0.clone())
}

/// After a strike (or a hunt abandoned), the murderer lies low until another day.
pub fn end_hunt(ctx: &mut Ctx, id: &str) {
    ctx.c(id).quirk_state.hunt_day = 0;
}

// --- The drunk ----------------------------------------------------------------

/// Called when a drink at the tavern is over: a drunkard rarely stops at one.
pub fn after_drink(ctx: &mut Ctx, id: &str) {
    if !ctx.cr(id).has(Quirk::Drunkard) || !ctx.chance(0.4) {
        return;
    }
    let tick = ctx.tick();
    let hours = ctx.range(2.0, 4.0);
    let until = tick + minutes_to_ticks(hours * 60.0);
    let c = ctx.c(id);
    let already = c.drunk(tick);
    c.quirk_state.drunk_until_tick = c.quirk_state.drunk_until_tick.max(until);
    if already {
        return;
    }
    let line = ctx.pick(&DRUNK_LINES);
    ctx.say(id, line, None, true);
    let name = first_name(ctx.cr(id)).to_owned();
    let pos = ctx.cr(id).pos;
    ctx.chronicle(
        ChronicleKind::Social,
        format!("{name} staggered out of {} roaring drunk.", place_name_at(pos)),
        vec![id.into()],
    );
}

fn sober_up(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    let c = ctx.cr(id);
    if c.quirk_state.drunk_until_tick == 0 || tick < c.quirk_state.drunk_until_tick {
        return;
    }
    ctx.c(id).quirk_state.drunk_until_tick = 0;
    ctx.remember(id, "Sobered up with a sore head and only patchy memories of the night.", 3.0, Event, Vec::new());
}

/// Out in public and drunk: songs, slurred wisdom, and now and then a fight with whoever is nearest.
fn drunken_trouble(ctx: &mut Ctx, id: &str) {
    let c = ctx.cr(id);
    if !is_present(c) || c.convo_id.is_some() || is_fighting(c) || c.combat.yielded {
        return;
    }
    let (pos, steady) = (c.pos, c.hp >= max_hp(c) * 0.5);
    if steady && ctx.chance(0.012) {
        let near = living(ctx.state)
            .filter(|o| o.id != id && is_present(o) && !is_fighting(o) && !o.combat.yielded)
            .map(|o| (o.id.clone(), dist(o.pos, pos)))
            .filter(|(_, d)| *d <= 8.0)
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((other, _)) = near {
            let name = first_name(ctx.cr(&other)).to_owned();
            let taunt = ctx.pick(&DRUNK_TAUNTS).replace("{b}", &name);
            start_fight(ctx, id, &other, FightMode::Brawl, IntentSource::Routine, Some(taunt));
            return;
        }
    }
    if ctx.chance(0.01) {
        let line = ctx.pick(&DRUNK_LINES);
        ctx.say(id, line, None, true);
    }
}

// --- The gossip ---------------------------------------------------------------

/// A story for a gossip to tell `listener` about someone else; mostly invented.
pub fn pick_rumour(ctx: &mut Ctx, teller: &str, listener: &str) -> Option<Rumour> {
    if !ctx.cr(teller).has(Quirk::Rumourmonger) {
        return None;
    }
    let others: Vec<(Id, f64)> = living(ctx.state)
        .filter(|o| o.id != teller && o.id != listener)
        .map(|o| {
            // The ones the gossip has no love for make the best stories.
            let r = ctx.cr(teller).relationships.get(&o.id);
            let dislike = r.map_or(0.0, |r| (-r.affinity).max(0.0) / 15.0 + r.grievance / 10.0);
            (o.id.clone(), 1.0 + dislike)
        })
        .collect();
    let index = rng::weighted_pick_index(&mut ctx.state.rng, &others, |(_, w)| *w)?;
    let about = others[index].0.clone();
    let (subject, listener_name) = (first_name(ctx.cr(&about)).to_owned(), first_name(ctx.cr(listener)).to_owned());
    let since = ctx.tick() - TICKS_PER_DAY * 3;
    let true_story = ctx
        .cr(teller)
        .memories
        .iter()
        .rev()
        .find(|m| m.kind == MemoryKind::Event && m.tick >= since && m.subjects.iter().any(|s| s == &about))
        .map(|m| m.text.clone());
    let rumour = match true_story {
        Some(text) if ctx.chance(0.25) => Rumour { about, text: format!("Saw it myself: {text}"), truthful: true },
        _ => {
            let story = ctx.pick(&FALSE_STORIES);
            let text = story.replace("{t}", &subject).replace("{b}", &listener_name);
            Rumour { about, text, truthful: false }
        }
    };
    Some(rumour)
}

/// The listener takes the story in, as far as they trust the one who told it.
pub fn pass_on_rumour(ctx: &mut Ctx, teller: &str, listener: &str, rumour: &Rumour) {
    if !ctx.alive(&rumour.about) {
        return;
    }
    let trust = rel(ctx.c(listener), teller).trust;
    let belief = ((trust + 40.0) / 80.0).clamp(0.2, 1.0);
    let delta = RelationDelta { affinity: -6.0 * belief, trust: -5.0 * belief, ..Default::default() };
    adjust(ctx, listener, &rumour.about, delta);
    let (teller_name, listener_name, subject) = (
        first_name(ctx.cr(teller)).to_owned(),
        first_name(ctx.cr(listener)).to_owned(),
        first_name(ctx.cr(&rumour.about)).to_owned(),
    );
    ctx.remember(
        listener,
        format!("{teller_name} told me that {}", lowercase_first(&rumour.text)),
        4.0,
        Event,
        vec![teller.into(), rumour.about.clone()],
    );
    let what = if rumour.truthful { "a story" } else { "a made-up story" };
    ctx.chronicle(
        ChronicleKind::Social,
        format!("{teller_name} told {listener_name} {what} about {subject}."),
        vec![teller.into(), listener.into(), rumour.about.clone()],
    );
}

fn lowercase_first(text: &str) -> String {
    // Names stay capitalised; only a leading "Saw" or similar is lowered.
    match text.strip_prefix("Saw it myself: ") {
        Some(rest) => format!("they saw it themselves: {rest}"),
        None => text.to_owned(),
    }
}

// --- The secret necromancer -----------------------------------------------------

fn night(hour: f64) -> bool {
    !(5.0..19.5).contains(&hour)
}

/// A night over forbidden books began today, or yesterday if it is past midnight.
fn studying_tonight(ctx: &Ctx, id: &str) -> bool {
    let (day, hour) = (ctx.time.day, ctx.time.hour);
    let study = ctx.cr(id).quirk_state.study_day;
    study != 0 && (study == day || (hour < 12.0 && study == day - 1))
}

/// Hours of a study night spent awake at home over the books.
fn reading(hour: f64) -> bool {
    !(1.5..19.0).contains(&hour)
}

/// Awake after dark, the dabbler in necromancy now and then wakes the dead close by;
/// and one night the dark may claim them altogether.
fn dark_studies(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    let (day, hour) = (ctx.time.day, ctx.time.hour);
    let studying = studying_tonight(ctx, id);
    let c = ctx.cr(id);
    // On a study night the bed waits: indoors, out of sight, up with the books.
    let in_bed = c.inside.as_deref() == Some(c.home_id.as_str())
        && c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Sleep));
    if studying && in_bed {
        ctx.c(id).asleep = !reading(hour);
    }
    let c = ctx.cr(id);
    if !night(hour) || c.asleep || is_fighting(c) || tick < c.quirk_state.next_roll_tick {
        return;
    }
    ctx.c(id).quirk_state.next_roll_tick = tick + minutes_to_ticks(60.0);
    // Once a study night, in the small hours, the dark may claim them.
    if studying && day >= FALL_FROM_DAY && hour < 1.0 && ctx.chance(FALL_CHANCE) {
        fall_to_necromancy(ctx, id);
        return;
    }
    if ctx.chance(if studying { 0.08 } else { 0.02 }) {
        raise_the_dead(ctx, id);
    }
}

/// Whether this character is up late at home over forbidden books right now.
pub fn reading_late(state: &crate::state::SimState, c: &crate::state::Character) -> bool {
    let hour = fsw_core::time::game_time(state.tick).hour;
    let day = fsw_core::time::game_time(state.tick).day;
    let study = c.quirk_state.study_day;
    c.has(Quirk::Occultist)
        && study != 0
        && (study == day || (hour < 12.0 && study == day - 1))
        && reading(hour)
        && c.inside.as_deref() == Some(c.home_id.as_str())
        && !c.asleep
}

pub fn raise_the_dead(ctx: &mut Ctx, id: &str) {
    let home = get_place(&ctx.cr(id).home_id);
    let count = ctx.int_range(1, 2);
    let mut raised = 0;
    for _ in 0..count {
        if ctx.state.monsters.len() >= crate::systems::monsters::MAX_MONSTERS {
            break;
        }
        let angle = ctx.range(0.0, std::f64::consts::TAU);
        let reach = ctx.range(9.0, 16.0);
        let x = home.door.x + reach * fsw_core::jsmath::cos(angle);
        let z = home.door.z + reach * fsw_core::jsmath::sin(angle);
        let Some(pos) = ctx.state.nav.nearest_walkable(vec2(x, z), 6) else { continue };
        spawn_monster(ctx, MonsterKind::SkeletonMinion, pos, home.facing);
        raised += 1;
    }
    if raised == 0 {
        return;
    }
    ctx.chronicle(
        ChronicleKind::World,
        format!("The dead clawed their way up near {}, far from any grave.", home.name),
        Vec::new(),
    );
    ctx.remember(
        id,
        "The dead rose near my house while I read. It was my doing; I am sure of it.",
        7.0,
        Event,
        Vec::new(),
    );
    let name = first_name(ctx.cr(id)).to_owned();
    let kin: Vec<Id> =
        ctx.cr(id).relationships.iter().filter(|(o, r)| r.kin && ctx.alive(o)).map(|(o, _)| o.clone()).collect();
    for other in kin {
        ctx.remember(
            &other,
            format!("The dead rose right by our door tonight, and {name} was still up with those books of hers."),
            6.0,
            Event,
            vec![id.into()],
        );
    }
}

/// The dark claims them: they leave the living and walk with the dead, for good.
pub fn fall_to_necromancy(ctx: &mut Ctx, id: &str) {
    detach(ctx, id);
    let tick = ctx.tick();
    let day = ctx.time.day;
    let fallen = ctx.state.characters.shift_remove(id).expect("the fallen one is alive");
    ctx.state.order.retain(|o| o != id);
    let name = first_name(&fallen).to_owned();
    let where_ = place_name_at(fallen.pos);
    ctx.state.dead.insert(
        id.to_owned(),
        DeadRecord {
            name: fallen.profile.name.clone(),
            day,
            cause: "lost to necromancy".into(),
            killer_id: None,
            hidden_killer_id: None,
            spouse_id: fallen.spouse_id.clone(),
        },
    );
    for other in living_ids(ctx.state) {
        let c = ctx.c(&other);
        if c.combat.target_id.as_deref() == Some(id) {
            c.combat.target_id = None;
        }
        let was_partner = c.partner_id.as_deref() == Some(id);
        if was_partner {
            c.partner_id = None;
            c.partner_since_tick = 0;
        }
        if c.spouse_id.as_deref() == Some(id) {
            c.spouse_id = None;
        }
        let close = was_partner || c.relationships.get(id).is_some_and(|r| r.kin || r.affinity >= 25.0);
        if close {
            ctx.remember(
                &other,
                format!(
                    "{name} has given {} to the dark. Whoever walks out there now is not the one I knew.",
                    pronoun_self(&fallen)
                ),
                10.0,
                Event,
                vec![id.into()],
            );
            ctx.emit(SimEvent::Emote { id: other.clone(), emote: Emote::Grief });
        } else {
            ctx.remember(
                &other,
                format!("{name} has turned necromancer and walks with the dead."),
                8.0,
                Event,
                vec![id.into()],
            );
        }
        ctx.stimulate(
            &other,
            crate::state::StimulusKind::Death,
            format!("{name} has become a necromancer, and the dead answer to them now."),
            2,
            Some(id),
        );
    }
    let monster = spawn_monster(ctx, MonsterKind::Necromancer, fallen.pos, fallen.facing);
    let m = ctx.m(&monster);
    m.name = Some(format!("{name} the Necromancer"));
    m.lasting = true;
    m.was_id = Some(id.to_owned());
    ctx.chronicle(
        ChronicleKind::Death,
        format!(
            "{} gave {} to necromancy at {where_}. The dead walk at {} side now.",
            fallen.profile.name,
            pronoun_self(&fallen),
            crate::util::their(&fallen)
        ),
        vec![id.into()],
    );
    ctx.emit(SimEvent::Death { id: id.into(), name: fallen.profile.name.clone(), pos: fallen.pos, monster: false });
    ctx.emit(SimEvent::SavePoint { reason: "death".into() });
    ctx.state.director.death_ticks.push(tick);
    // No grave yet, so no funeral; but a wedding they were to have is off.
    crate::systems::ceremonies::cancel_wedding_of(ctx, id);
    schedule_arrival_after_loss(ctx);
}

fn pronoun_self(c: &crate::state::Character) -> &'static str {
    match c.profile.pronouns {
        crate::state::Pronouns::SheHer => "herself",
        crate::state::Pronouns::HeHim => "himself",
        crate::state::Pronouns::TheyThem => "themself",
    }
}

/// The victim of a strike from behind who lived through it knows exactly who it was.
pub fn ambushed(ctx: &mut Ctx, victim: &str, attacker: &str) {
    let an = first_name(ctx.cr(attacker)).to_owned();
    let where_ = place_name_at(ctx.cr(victim).pos);
    adjust(
        ctx,
        victim,
        attacker,
        RelationDelta { grievance: 70.0, affinity: -60.0, trust: -70.0, ..Default::default() },
    );
    ctx.remember(
        victim,
        format!("{an} crept up behind me at {where_} and tried to kill me."),
        10.0,
        Event,
        vec![attacker.into()],
    );
}

/// The one who was struck from behind got away, and will not keep quiet about it.
pub fn escaped(ctx: &mut Ctx, victim: &str, attacker: &str) {
    end_hunt(ctx, attacker);
    let (an, vn) = (first_name(ctx.cr(attacker)).to_owned(), first_name(ctx.cr(victim)).to_owned());
    ctx.chronicle(
        ChronicleKind::Conflict,
        format!("{vn} got away after {an} struck at them from behind."),
        vec![attacker.into(), victim.into()],
    );
}
