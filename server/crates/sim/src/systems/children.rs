//! Children: birthdays, coming of age, births, and the trouble a child gets
//! into. `daily_children` runs at the day roll-over; `update_children` runs
//! each tick after the quirks. Nothing here draws from the RNG unless a child is
//! alive or a couple qualifies for a child, so a village without either runs as
//! it did before children existed.

use fsw_core::math2d::{clamp, dist};
use fsw_core::rng;
use fsw_core::time::{TICKS_PER_GAME_MINUTE, minutes_to_ticks, seconds_to_ticks};
use fsw_world::places::{get_place, homes};

use crate::data::activities::{ActivityKind, activity};
use crate::data::anims::AnimId;
use crate::data::children::{
    BIRTH_CHANCE, BIRTH_COOLDOWN_TICKS, BIRTHDAY_DAYS, CHILD_FIRST_NAMES, CHILD_LOOKS, CHILD_TRAITS, COMING_OF_AGE,
    FOUNDING_CHILDREN, LOVERS_BEFORE_CHILD_TICKS, MAX_CHILDREN, NEWBORN_AGE,
};
use crate::data::classes::RUN_SPEED;
use crate::events::{Emote, Projectile, SimEvent};
use crate::state::{
    ActPhase, Character, ChronicleKind, Id, Intent, IntentAction, IntentSource, ItemId, Look, MemoryKind, Mischief,
    MischiefPlan, PlanSource, Profile, StimulusKind, VoiceProfile,
};
use crate::systems::arrivals::{free_home, make_character};
use crate::systems::death::detach;
use crate::systems::fire::{fighting_fire, ignite_home};
use crate::systems::founding::{Blueprint, ChildSeed};
use crate::systems::intents::{finish, fresh_act, give, leave_building, travel};
use crate::systems::movement::route_to;
use crate::systems::quirks::{idle_enough, replace_intent};
use crate::systems::relationships::{RelationDelta, adjust, label_for};
use crate::util::{
    Ctx, adults, bump_epoch, children, first_name, is_fighting, is_present, living, living_ids, new_relationship,
    place_name_at, set_anim,
};

/// How likely each kind of trouble is, once a child is up to something.
const MISCHIEF_WEIGHTS: [(Mischief, f64); 6] = [
    (Mischief::PinchFood, 30.0),
    (Mischief::ThrowFood, 25.0),
    (Mischief::HideTool, 20.0),
    (Mischief::TallTale, 15.0),
    (Mischief::WanderOff, 8.0),
    (Mischief::PlayWithFire, 2.0),
];
/// Only a child at least this naughty plays with fire.
const FIRE_MISCHIEF: f64 = 0.6;
/// A grown-up this close notices a child slipping away at dusk.
const NOTICE_RANGE: f64 = 18.0;
/// A child this far from their door after dusk has wandered off.
const WANDERED_RANGE: f64 = 25.0;
/// Where a child who wanders off is drawn to.
const WILDS: [&str; 3] = ["glade", "graveyard", "ruins"];
/// Pinching needs an arm's reach; a thrown loaf carries a little further.
const PINCH_REACH: f64 = 3.0;
const THROW_REACH: f64 = 6.0;
const SIGHT: f64 = 12.0;

/// Ages in words, for memories: "Today I am twelve."
fn age_word(age: i64) -> String {
    const WORDS: [&str; 17] = [
        "nought", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
        "thirteen", "fourteen", "fifteen", "sixteen",
    ];
    usize::try_from(age).ok().and_then(|i| WORDS.get(i)).map_or_else(|| age.to_string(), |w| (*w).to_owned())
}

/// Once a day: birthdays, the coming of age that the sixteenth brings, and now and
/// then a child for a couple long together.
pub fn daily_children(ctx: &mut Ctx) {
    let day = ctx.time.day;
    for id in living_ids(ctx.state) {
        let Some(child) = ctx.cr(&id).child.as_ref() else { continue };
        let birthday_due = day >= child.next_birthday_day;
        plan_mischief(ctx, &id);
        if !birthday_due {
            continue;
        }
        birthday(ctx, &id);
        if ctx.cr(&id).profile.age >= COMING_OF_AGE {
            come_of_age(ctx, &id);
        }
    }
    maybe_birth(ctx);
}

/// A couple who have been together long enough, when the village has room for
/// another child and the last birth is far enough behind, may have a child at
/// the turn of the day. At most one a day, and the RNG is only asked when a couple qualifies.
pub fn maybe_birth(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let last = ctx.state.director.last_birth_tick;
    if (last > 0 && tick - last < BIRTH_COOLDOWN_TICKS) || children(ctx.state).count() >= MAX_CHILDREN {
        return;
    }
    let couples: Vec<(Id, Id)> = adults(ctx.state)
        .filter_map(|c| {
            let partner = c.partner_id.as_deref()?;
            let p = ctx.state.characters.get(partner)?;
            // Each couple once, from whichever of them comes first in the order.
            let first =
                ctx.state.order.iter().position(|id| id == &c.id) < ctx.state.order.iter().position(|id| id == partner);
            // A start of 0 is a couple from before it was recorded: together since the beginning.
            (first && !p.is_child() && tick - c.partner_since_tick >= LOVERS_BEFORE_CHILD_TICKS)
                .then(|| (c.id.clone(), p.id.clone()))
        })
        .collect();
    for (a, b) in couples {
        if ctx.chance(BIRTH_CHANCE) {
            let blueprint = child_of(ctx, &[a.clone(), b.clone()], NEWBORN_AGE);
            welcome_child(ctx, &blueprint, vec![a, b], Welcome::Born);
            return;
        }
    }
}

/// A child made up from the lists, the way the sim invents a villager: a first name nobody living or
/// dead has, a parent's surname and class where there is a parent, and a few traits. With no parents,
/// a foundling of the glade with a class of their own.
fn child_of(ctx: &mut Ctx, parents: &[Id], age: i64) -> Blueprint {
    let parent = match parents {
        [] => None,
        [one] => Some(one.clone()),
        [a, b, ..] => Some(if ctx.chance(0.5) { a.clone() } else { b.clone() }),
    };
    let class_id = match &parent {
        Some(p) => ctx.cr(p).class_id,
        None => CHILD_LOOKS[ctx.pick_index(CHILD_LOOKS.len())].class,
    };
    let surname = parent.as_ref().and_then(|p| ctx.cr(p).profile.name.split(' ').nth(1).map(str::to_owned));
    let used: Vec<String> = living(ctx.state)
        .map(|c| first_name(c).to_owned())
        .chain(ctx.state.dead.values().map(|d| d.name.split(' ').next().unwrap_or_default().to_owned()))
        .collect();
    let mut first = String::new();
    let mut i = 0;
    while i < 20 && (first.is_empty() || used.contains(&first)) {
        first = ctx.pick(&CHILD_FIRST_NAMES).to_owned();
        i += 1;
    }
    let name = match surname {
        Some(surname) => format!("{first} {surname}"),
        None => first,
    };
    let picked = [ctx.pick(&CHILD_TRAITS), ctx.pick(&CHILD_TRAITS)];
    let mut traits: Vec<String> = Vec::new();
    for t in picked {
        if !traits.iter().any(|existing| existing == t) {
            traits.push(t.to_owned());
        }
    }
    let pronouns =
        ctx.pick(&[crate::state::Pronouns::SheHer, crate::state::Pronouns::HeHim, crate::state::Pronouns::TheyThem]);
    let temper = clamp(ctx.next_float(), 0.1, 0.8);
    let bravery = clamp(ctx.next_float(), 0.2, 0.9);
    let sociability = clamp(ctx.next_float(), 0.3, 0.9);
    let mischief = ctx.range(0.2, 0.8);
    let pitch = ctx.range(1.2, 1.4);
    let parent_names: Vec<String> = parents.iter().map(|p| first_name(ctx.cr(p)).to_owned()).collect();
    let backstory = match parent_names.as_slice() {
        [] => {
            "Found as a baby in a basket in the glade one morning, and raised by whoever had a bed and a bowl to spare."
                .to_owned()
        }
        [one] => format!("{one}'s child, born in the village and underfoot ever since."),
        both => format!("Born in the village to {}, and underfoot ever since.", crate::util::join_names(both)),
    };
    Blueprint {
        class_id,
        model: None,
        texture: parent.as_ref().and_then(|p| ctx.cr(p).look.texture.clone()),
        hide: Vec::new(),
        main_hand: None,
        off_hand: None,
        home_id: None,
        jobs: None,
        hobbies: None,
        profile: Profile {
            name,
            pronouns,
            age,
            occupation: "child".into(),
            backstory,
            traits,
            goals: vec!["play all day".into(), "see what is past the bridge".into(), "be let in on a secret".into()],
            speech_style: "Short and breathless, full of questions.".into(),
            temper,
            bravery,
            sociability,
            spite: 0.0,
            allure: 0.0,
            shyness: 0.0,
            looks: 0.0,
            voice: VoiceProfile { pitch, rate: 1.1, timbre: "small and piping".into() },
            secret: String::new(),
        },
        quirks: Vec::new(),
        child: Some(ChildSeed { mischief, parents: Vec::new() }),
    }
}

/// How a child comes to the village: born in the night to a couple, or already here when the founding ends.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Welcome {
    Born,
    Founding,
}

/// A child joins the village the way a newcomer does: known to everyone, kin to
/// the people who raise them, remembered and written into the chronicle.
pub fn welcome_child(ctx: &mut Ctx, entry: &Blueprint, parents: Vec<Id>, welcome: Welcome) -> Id {
    let tick = ctx.tick();
    let day = ctx.time.day;
    let home = parents
        .first()
        .and_then(|p| ctx.state.characters.get(p))
        .map_or_else(|| free_home(ctx.state), |p| p.home_id.clone());
    let mut c = make_character(ctx.state, entry, &home);
    if welcome == Welcome::Born {
        c.arrived_tick = tick;
        // Born at the turn of the day: indoors and asleep, not on the step for the dead to find.
        c.inside = Some(home.clone());
        c.asleep = true;
    }
    if let Some(child) = &mut c.child {
        child.parents = parents.clone();
        child.next_birthday_day = day + BIRTHDAY_DAYS;
    }
    let id = c.id.clone();
    let name = c.profile.name.clone();
    ctx.state.characters.insert(id.clone(), c);
    ctx.state.order.push(id.clone());

    let parent_names: Vec<String> = parents.iter().map(|p| first_name(ctx.cr(p)).to_owned()).collect();
    for other in living_ids(ctx.state) {
        if other == id {
            continue;
        }
        let parent = parents.contains(&other);
        let mut r = new_relationship();
        let mut back = new_relationship();
        if parent {
            (r.affinity, r.trust, r.familiarity, r.kin) = (80.0, 60.0, 90.0, true);
            (back.affinity, back.trust, back.familiarity, back.kin) = (80.0, 85.0, 90.0, true);
            r.summary = format!("My child, {}.", first_name(ctx.cr(&id)));
            back.summary = "Mine. The whole world, at home.".into();
        }
        r.label = label_for(&r, false);
        back.label = label_for(&back, false);
        ctx.c(&other).relationships.insert(id.clone(), r);
        ctx.c(&id).relationships.insert(other.clone(), back);
        if welcome == Welcome::Founding {
            continue;
        }
        let text = if parent {
            format!("Our child, {name}, was born today.")
        } else {
            format!("{} have a child: {name}.", join(&parent_names))
        };
        ctx.remember(&other, text, if parent { 9.0 } else { 4.0 }, MemoryKind::Event, vec![id.clone()]);
        let stimulus = format!("A child has been born in the village: {name}, to {}.", join(&parent_names));
        ctx.stimulate(&other, StimulusKind::Family, stimulus, 1, Some(&id));
    }
    ctx.emit(SimEvent::Arrival { id: id.clone() });
    if welcome == Welcome::Born {
        ctx.chronicle(
            ChronicleKind::Social,
            format!("{} have a child: {name}.", join(&parent_names)),
            vec![id.clone()],
        );
        ctx.emit(SimEvent::SavePoint { reason: "birth".into() });
        ctx.state.director.last_birth_tick = tick;
    }
    id
}

/// When the founding ends the village has its children: four or five, raised by its founders (couples
/// first, then single parents) and one foundling the village raises. A cast that brings its own
/// children already, as the test cast does, gets none, and the dice are not touched.
pub fn found_children(ctx: &mut Ctx) {
    let have = children(ctx.state).count();
    if have >= FOUNDING_CHILDREN[1] as usize {
        return;
    }
    let wanted = ctx.int_range(FOUNDING_CHILDREN[0], FOUNDING_CHILDREN[1]) as usize;
    if have >= wanted {
        return;
    }
    let founders: Vec<Id> = adults(ctx.state).map(|c| c.id.clone()).collect();
    let mut couples: Vec<(Id, Id)> = adults(ctx.state)
        .filter_map(|c| {
            let partner = c.partner_id.clone()?;
            let first = ctx.state.order.iter().position(|id| id == &c.id)
                < ctx.state.order.iter().position(|id| id == &partner);
            first.then(|| (c.id.clone(), partner))
        })
        .collect();
    let mut names = Vec::new();
    for n in have..wanted {
        let foundling = n + 1 == wanted;
        let parents: Vec<Id> = if foundling {
            Vec::new()
        } else if let Some((a, b)) = couples.pop() {
            vec![a, b]
        } else {
            vec![founders[ctx.pick_index(founders.len())].clone()]
        };
        let age = ctx.int_range(6, 12);
        let blueprint = child_of(ctx, &parents, age);
        let id = welcome_child(ctx, &blueprint, parents, Welcome::Founding);
        names.push(first_name(ctx.cr(&id)).to_owned());
    }
    let ids: Vec<Id> = children(ctx.state).map(|c| c.id.clone()).collect();
    ctx.chronicle(ChronicleKind::Social, format!("The village's children: {}.", join(&names)), ids);
}

fn join(names: &[String]) -> String {
    crate::util::join_names(names)
}

fn birthday(ctx: &mut Ctx, id: &str) {
    let c = ctx.c(id);
    c.profile.age += 1;
    let age = c.profile.age;
    let parents = c.child.as_mut().map_or(Vec::new(), |child| {
        child.next_birthday_day += BIRTHDAY_DAYS;
        child.parents.clone()
    });
    let name = first_name(ctx.cr(id)).to_owned();
    let word = age_word(age);
    ctx.remember(id, format!("Today I am {word}."), 3.0, MemoryKind::Event, Vec::new());
    for parent in parents {
        if ctx.alive(&parent) {
            ctx.remember(&parent, format!("{name} is {word} today."), 4.0, MemoryKind::Event, vec![id.to_owned()]);
        }
    }
}

/// The same person, grown: the adult of their class, with its work and its weapon,
/// a bed of their own, and a place among the adults the director counts.
fn come_of_age(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    let day = ctx.time.day;
    detach(ctx, id);
    let nav = ctx.state.nav.clone();
    let c = ctx.c(id);
    let parents = c.child.take().map_or(Vec::new(), |child| child.parents);
    let def = c.class_id.def();
    // At the turn of the day most children are asleep at home; they sleep on and take up the day in the
    // morning. Anyone awake is given a fresh start now.
    let sleeping = c.asleep && c.inside.is_some();
    if !sleeping && c.inside.is_some() {
        leave_building(&nav, c);
    }
    c.look = Look {
        model: def.model.into(),
        texture: c.look.texture.clone(),
        hide: Vec::new(),
        main_hand: def.main_hand.map(str::to_owned),
        off_hand: def.off_hand.map(str::to_owned),
    };
    c.jobs = def.jobs.to_vec();
    c.hobbies = def.hobbies.to_vec();
    c.profile.occupation = format!("young {}", def.label);
    c.profile.backstory.push_str(&format!(" Grew up in the village and came of age on day {day}."));
    let raised_by = if parents.is_empty() { "the village that raised me" } else { "the people who raised me" };
    c.profile.goals = vec![format!("find my feet as a grown {}", def.label), format!("repay {raised_by}")];
    c.hp = def.max_hp;
    c.inventory.potion = if c.profile.bravery >= 0.6 { 1 } else { 0 };
    // A bed of their own, if the house they grew up in has none to spare.
    let home = get_place(&c.home_id);
    let full = adults(ctx.state).filter(|o| o.home_id == home.id).count() > home.beds as usize;
    if full {
        let new_home = free_home(ctx.state);
        let c = ctx.c(id);
        c.home_id = new_home.clone();
        // A sleeper is carried to the new bed: indoors nobody is drawn, so no viewer sees the move, and the
        // routine's sleep carries on there instead of walking them across the village at midnight.
        if sleeping {
            c.inside = Some(new_home.clone());
            c.pos = get_place(&new_home).door;
        }
    }
    let c = ctx.c(id);
    c.plan_day = 0;
    c.plan_source = PlanSource::Routine;
    if !sleeping {
        c.intent = None;
        c.queue.clear();
        c.act = fresh_act(tick);
    }
    bump_epoch(c);

    let name = first_name(ctx.cr(id)).to_owned();
    let occupation = ctx.cr(id).profile.occupation.clone();
    let subject: Vec<Id> = vec![id.to_owned()];
    ctx.remember(
        id,
        format!("I am sixteen today. No longer a child: a {occupation}, with work of my own to do."),
        9.0,
        MemoryKind::Event,
        Vec::new(),
    );
    for other in living_ids(ctx.state) {
        if other == id {
            continue;
        }
        let parent = parents.contains(&other);
        let text = if parent {
            format!("{name} has come of age. Not a child any more, whatever I may think; a {occupation} now.")
        } else {
            format!("{name} has come of age and taken up work as a {occupation}.")
        };
        ctx.remember(&other, text, if parent { 8.0 } else { 5.0 }, MemoryKind::Event, subject.clone());
        let stimulus = format!("{name} has come of age today and is counted among the grown-ups now.");
        ctx.stimulate(&other, StimulusKind::Family, stimulus, 1, Some(id));
    }
    ctx.chronicle(
        ChronicleKind::Social,
        format!("{name} has come of age and taken up work as a {occupation}."),
        subject,
    );
    ctx.emit(SimEvent::SavePoint { reason: "coming of age".into() });
}

// --- Mischief -----------------------------------------------------------------

/// At the turn of the day a child rolls their `mischief`; on a hit one piece of trouble is planned for it.
fn plan_mischief(ctx: &mut Ctx, id: &str) {
    let mischief = ctx.cr(id).child.as_ref().map_or(0.0, |child| child.mischief);
    if let Some(child) = &mut ctx.c(id).child {
        child.plan = None;
    }
    if mischief <= 0.0 || !ctx.chance(mischief * 0.6) {
        return;
    }
    let kinds: Vec<(Mischief, f64)> = MISCHIEF_WEIGHTS
        .iter()
        .copied()
        .filter(|(kind, _)| *kind != Mischief::PlayWithFire || mischief >= FIRE_MISCHIEF)
        .collect();
    let Some(index) = rng::weighted_pick_index(&mut ctx.state.rng, &kinds, |(_, w)| *w) else { return };
    if let Some(child) = &mut ctx.c(id).child {
        child.plan = Some(MischiefPlan { kind: kinds[index].0, place_id: None, done_tick: 0 });
    }
}

/// Today's trouble, if it is still to come.
fn pending(c: &Character) -> Option<&MischiefPlan> {
    c.child.as_ref().and_then(|child| child.plan.as_ref()).filter(|plan| plan.done_tick == 0)
}

fn mark_done(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    if let Some(plan) = ctx.c(id).child.as_mut().and_then(|child| child.plan.as_mut()) {
        plan.done_tick = tick;
    }
}

/// Whoever raises the child hears of it, and may have something to say.
fn report_to_parents(ctx: &mut Ctx, id: &str, text: String) {
    let parents = ctx.cr(id).child.as_ref().map_or(Vec::new(), |child| child.parents.clone());
    for parent in parents {
        if !ctx.alive(&parent) {
            continue;
        }
        ctx.remember(&parent, text.clone(), 5.0, MemoryKind::Event, vec![id.to_owned()]);
        ctx.stimulate(&parent, StimulusKind::Family, text.clone(), 1, Some(id));
    }
}

/// Each tick, after the quirks: a child with trouble planned looks for the chance, and
/// grown-ups keep half an eye on children out after dusk.
pub fn update_children(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let ids: Vec<Id> = children(ctx.state).map(|c| c.id.clone()).collect();
    for (i, id) in ids.iter().enumerate() {
        if !ctx.alive(id) {
            continue;
        }
        if tick % 10 == 0 {
            watch_over(ctx, id);
        }
        // Each child is considered once a game minute, at a different tick.
        if (tick + i as i64) % TICKS_PER_GAME_MINUTE != 0 {
            continue;
        }
        let Some(plan) = pending(ctx.cr(id)) else { continue };
        match plan.kind {
            Mischief::PinchFood => pinch_food(ctx, id),
            Mischief::ThrowFood => throw_food(ctx, id),
            Mischief::HideTool => hide_tool(ctx, id),
            // A tall tale is told in conversation; see `quirks::pick_rumour`.
            Mischief::TallTale => {}
            Mischief::WanderOff => wander_off(ctx, id),
            Mischief::PlayWithFire => play_with_fire(ctx, id),
        }
    }
}

/// Grown-ups out in the open, awake and free, who could notice or go after a child.
fn free_adult(c: &Character) -> bool {
    !c.asleep
        && c.inside.is_none()
        && c.convo_id.is_none()
        && !is_fighting(c)
        && !fighting_fire(c)
        && !fetching(c)
        && !c.combat.yielded
}

fn nearest_adult(ctx: &Ctx, pos: fsw_core::Vec2, reach: f64, fit: impl Fn(&Character) -> bool) -> Option<Id> {
    adults(ctx.state)
        .filter(|o| is_present(o) && o.convo_id.is_none() && !is_fighting(o) && fit(o))
        .map(|o| (o.id.clone(), dist(o.pos, pos)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(id, _)| id)
}

fn pinch_food(ctx: &mut Ctx, id: &str) {
    let c = ctx.cr(id);
    if !is_present(c) || c.convo_id.is_some() {
        return;
    }
    let pos = c.pos;
    let name = first_name(c).to_owned();
    if let Some(victim) = nearest_adult(ctx, pos, PINCH_REACH, |o| o.inventory.food > 0) {
        ctx.c(&victim).inventory.food -= 1;
        give(ctx.c(id), ItemId::Food);
        adjust(ctx, &victim, id, RelationDelta { grievance: 8.0, affinity: -3.0, ..Default::default() });
        let victim_name = first_name(ctx.cr(&victim)).to_owned();
        ctx.stimulate(
            &victim,
            StimulusKind::Witness,
            format!("{name} just pinched food out of your pack and ran off laughing."),
            1,
            Some(id),
        );
        ctx.remember(&victim, format!("{name} pinched food from my pack."), 5.0, MemoryKind::Event, vec![id.into()]);
        ctx.remember(
            id,
            format!("Pinched a bite from {victim_name}'s pack. They did not look pleased."),
            3.0,
            MemoryKind::Event,
            vec![victim.clone()],
        );
        ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Laugh });
        ctx.chronicle(
            ChronicleKind::Social,
            format!("{name} pinched food from {victim_name}'s pack near {}.", place_name_at(pos)),
            vec![id.into(), victim.clone()],
        );
        mark_done(ctx, id);
        report_to_parents(ctx, id, format!("{name} pinched food from {victim_name}'s pack."));
    } else if dist(pos, get_place("tavern").door) <= THROW_REACH {
        give(ctx.c(id), ItemId::Food);
        ctx.remember(
            id,
            "Pinched a loaf from the tavern kitchen when nobody was looking.",
            3.0,
            MemoryKind::Event,
            vec![],
        );
        ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Laugh });
        ctx.chronicle(
            ChronicleKind::Social,
            format!("{name} pinched a loaf from the tavern kitchen."),
            vec![id.into()],
        );
        mark_done(ctx, id);
        report_to_parents(ctx, id, format!("{name} was caught pinching a loaf from the tavern kitchen."));
    }
}

fn throw_food(ctx: &mut Ctx, id: &str) {
    let c = ctx.cr(id);
    if !is_present(c) || c.convo_id.is_some() || c.inventory.food == 0 {
        return;
    }
    let (pos, home) = (c.pos, c.home_id.clone());
    let name = first_name(c).to_owned();
    let Some(victim) = nearest_adult(ctx, pos, THROW_REACH, |_| true) else { return };
    let victim_pos = ctx.cr(&victim).pos;
    let victim_name = first_name(ctx.cr(&victim)).to_owned();
    ctx.c(id).inventory.food -= 1;
    ctx.emit(SimEvent::Attack {
        attacker_id: id.into(),
        target_id: victim.clone(),
        ranged: Some(Projectile::Food),
        from: pos,
        to: victim_pos,
    });
    set_anim(ctx.c(id), AnimId::Interact, true);
    set_anim(ctx.c(&victim), AnimId::Hit, true);
    ctx.emit(SimEvent::Emote { id: victim.clone(), emote: Emote::Anger });
    ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Laugh });
    adjust(ctx, &victim, id, RelationDelta { grievance: 12.0, affinity: -4.0, ..Default::default() });
    // A loaf is not an attack: nobody hits back, and no reflex fires.
    ctx.stimulate(
        &victim,
        StimulusKind::Witness,
        format!("{name} just threw an apple at you, hit you square on, and ran off laughing."),
        1,
        Some(id),
    );
    ctx.remember(&victim, format!("{name} threw food at me and ran."), 5.0, MemoryKind::Event, vec![id.into()]);
    ctx.remember(
        id,
        format!("Got {victim_name} right on the ear with an apple and ran for it."),
        4.0,
        MemoryKind::Event,
        vec![victim.clone()],
    );
    ctx.chronicle(
        ChronicleKind::Social,
        format!("{name} threw food at {victim_name} near {} and ran for it.", place_name_at(pos)),
        vec![id.into(), victim.clone()],
    );
    replace_intent(ctx, id, Intent::new(IntentAction::GoTo { place_id: home }, IntentSource::Reflex));
    mark_done(ctx, id);
    report_to_parents(ctx, id, format!("{name} threw food at {victim_name} and ran off."));
}

fn hide_tool(ctx: &mut Ctx, id: &str) {
    let c = ctx.cr(id);
    if !is_present(c) || c.convo_id.is_some() {
        return;
    }
    let pos = c.pos;
    let name = first_name(c).to_owned();
    let at_work = |o: &Character| {
        o.act.phase == ActPhase::Perform
            && o.tool.is_some()
            && matches!(o.intent.as_ref().map(|i| &i.action), Some(IntentAction::Activity { activity: a, .. }) if activity(*a).kind == ActivityKind::Work)
    };
    let Some(victim) = nearest_adult(ctx, pos, SIGHT, at_work) else { return };
    let tool = ctx.cr(&victim).tool.clone().unwrap_or_default();
    let tool = tool.rsplit('/').next().unwrap_or("tool").replace('_', " ");
    let delay = minutes_to_ticks(ctx.range(20.0, 40.0));
    let v = ctx.c(&victim);
    v.tool = None;
    v.act.until_tick += delay;
    let victim_name = first_name(v).to_owned();
    adjust(ctx, &victim, id, RelationDelta { grievance: 10.0, affinity: -3.0, ..Default::default() });
    ctx.stimulate(
        &victim,
        StimulusKind::Witness,
        format!("Your {tool} has gone missing while your back was turned, and {name} was seen nearby."),
        1,
        Some(id),
    );
    ctx.remember(
        &victim,
        format!("{name} hid my {tool} and the work ran long."),
        5.0,
        MemoryKind::Event,
        vec![id.into()],
    );
    ctx.remember(
        id,
        format!("Hid {victim_name}'s {tool} and watched them look for it."),
        3.0,
        MemoryKind::Event,
        vec![victim.clone()],
    );
    ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Laugh });
    ctx.chronicle(
        ChronicleKind::Social,
        format!("{name} hid {victim_name}'s {tool}, and the work ran long."),
        vec![id.into(), victim.clone()],
    );
    mark_done(ctx, id);
    report_to_parents(ctx, id, format!("{name} hid {victim_name}'s {tool} for a prank."));
}

/// At dusk, instead of going home, a child slips away toward the wilds.
fn wander_off(ctx: &mut Ctx, id: &str) {
    let hour = ctx.time.hour;
    if !(18.3..19.3).contains(&hour) || !idle_enough(ctx, id) {
        return;
    }
    let place = ctx.pick(&WILDS);
    let name = first_name(ctx.cr(id)).to_owned();
    replace_intent(ctx, id, Intent::new(IntentAction::GoTo { place_id: place.into() }, IntentSource::Reflex));
    ctx.remember(
        id,
        format!("Slipped away toward {} as the light went. Nobody saw.", get_place(place).name),
        4.0,
        MemoryKind::Event,
        vec![],
    );
    ctx.chronicle(
        ChronicleKind::Social,
        format!("{name} slipped away toward {} at dusk.", get_place(place).name),
        vec![id.into()],
    );
    mark_done(ctx, id);
}

/// Whether this grown-up is on their way to bring a child home.
pub fn fetching(c: &Character) -> bool {
    matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::Fetch { .. }))
}

/// Whether this child has slipped away today and not yet been brought home.
fn wandered(c: &Character) -> bool {
    c.child
        .as_ref()
        .and_then(|child| child.plan.as_ref())
        .is_some_and(|plan| plan.kind == Mischief::WanderOff && plan.done_tick > 0)
}

/// A grown-up near a child who has wandered off after dusk, or one of the people who raise them, goes
/// after them. A child merely late home is left to walk.
fn watch_over(ctx: &mut Ctx, id: &str) {
    let hour = ctx.time.hour;
    let c = ctx.cr(id);
    if (5.0..18.3).contains(&hour) || c.inside.is_some() || c.asleep || !wandered(c) {
        return;
    }
    let (pos, home) = (c.pos, get_place(&c.home_id).door);
    if dist(pos, home) <= WANDERED_RANGE {
        return;
    }
    if living(ctx.state).any(|o| {
        o.intent.as_ref().is_some_and(|i| matches!(&i.action, IntentAction::Fetch { target_id } if target_id == id))
    }) {
        return;
    }
    let parents = c.child.as_ref().map_or(Vec::new(), |child| child.parents.clone());
    let finder = adults(ctx.state)
        .filter(|o| free_adult(o) && (parents.contains(&o.id) || dist(o.pos, pos) <= NOTICE_RANGE))
        .map(|o| (o.id.clone(), if parents.contains(&o.id) { 0.0 } else { dist(o.pos, pos) }))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(id, _)| id);
    let Some(adult) = finder else { return };
    let name = first_name(ctx.cr(id)).to_owned();
    let where_ = place_name_at(pos);
    ctx.stimulate(
        &adult,
        StimulusKind::Family,
        format!("{name} has wandered off toward {where_} as the dark comes on. You are going after them."),
        2,
        Some(id),
    );
    replace_intent(ctx, &adult, Intent::new(IntentAction::Fetch { target_id: id.into() }, IntentSource::Reflex));
}

/// Runs after a child and, on catching up, marches them home.
pub fn run_fetch(ctx: &mut Ctx, id: &str, target_id: &str) {
    let tick = ctx.tick();
    let Some(target) = ctx.state.characters.get(target_id) else {
        finish(ctx, id, "They were nowhere to be found.");
        return;
    };
    let (target_pos, safe) = (target.pos, target.inside.is_some());
    let nav = ctx.state.nav.clone();
    let c = ctx.c(id);
    if c.act.phase == ActPhase::Start {
        leave_building(&nav, c);
        c.act.phase = ActPhase::Travel;
        c.act.since_tick = tick;
        c.act.until_tick = tick + minutes_to_ticks(60.0);
        c.act.repath_tick = 0;
    }
    if safe || tick >= c.act.until_tick {
        set_anim(c, AnimId::Idle, false);
        finish(ctx, id, "You gave up looking; they must have gone home.");
        return;
    }
    if dist(c.pos, target_pos) <= 2.5 {
        c.act.path.clear();
        set_anim(c, AnimId::Idle, false);
        let (name, child_name) = (first_name(ctx.cr(id)).to_owned(), first_name(ctx.cr(target_id)).to_owned());
        let where_ = place_name_at(target_pos);
        ctx.say(id, format!("{child_name}! What are you doing out here? Home, now."), Some(target_id), false);
        ctx.remember(
            id,
            format!("Fetched {child_name} home from {where_} after dark. My heart was in my mouth."),
            5.0,
            MemoryKind::Event,
            vec![target_id.into()],
        );
        ctx.remember(
            target_id,
            format!("{name} came running and marched me home from {where_}. I was not even scared."),
            6.0,
            MemoryKind::Event,
            vec![id.into()],
        );
        adjust(ctx, id, target_id, RelationDelta { grievance: 5.0, ..Default::default() });
        adjust(ctx, target_id, id, RelationDelta { trust: 5.0, ..Default::default() });
        ctx.chronicle(
            ChronicleKind::Social,
            format!("{name} fetched {child_name} home from {where_} after dark."),
            vec![id.into(), target_id.into()],
        );
        report_to_parents(
            ctx,
            target_id,
            format!("{name} found {child_name} out at {where_} after dark and brought them home."),
        );
        let bed =
            IntentAction::Activity { activity: fsw_world::places::ActivityId::Sleep, place_id: None, minutes: None };
        replace_intent(ctx, target_id, Intent::new(bed, IntentSource::Reflex));
        // Brought home: the day's wandering is over, and nobody goes after them twice.
        if let Some(child) = &mut ctx.c(target_id).child {
            child.plan = None;
        }
        finish(ctx, id, format!("You brought {child_name} home."));
        return;
    }
    if c.act.path.is_empty() || tick >= c.act.repath_tick {
        c.act.path = route_to(&nav, c.pos, target_pos).unwrap_or_default();
        c.act.repath_tick = tick + seconds_to_ticks(1.5);
    }
    travel(tick, c, Some(RUN_SPEED));
}

/// By day, a child goes to a house that is not their own and plays with a flint until it catches.
fn play_with_fire(ctx: &mut Ctx, id: &str) {
    let hour = ctx.time.hour;
    if !(10.0..17.0).contains(&hour) || !ctx.state.houses.is_empty() {
        return;
    }
    let own = ctx.cr(id).home_id.clone();
    let target = match pending(ctx.cr(id)).and_then(|plan| plan.place_id.clone()) {
        Some(place) => place,
        None => {
            let standing: Vec<&str> =
                homes().iter().filter(|home| home.id != own).map(|home| home.id.as_str()).collect();
            if standing.is_empty() {
                return;
            }
            let place = ctx.pick(&standing).to_owned();
            if let Some(plan) = ctx.c(id).child.as_mut().and_then(|child| child.plan.as_mut()) {
                plan.place_id = Some(place.clone());
            }
            place
        }
    };
    let door = get_place(&target).door;
    let c = ctx.cr(id);
    if dist(c.pos, door) > 2.5 {
        let going =
            matches!(&c.intent, Some(Intent { action: IntentAction::GoTo { place_id }, .. }) if place_id == &target);
        if !going && idle_enough(ctx, id) {
            replace_intent(ctx, id, Intent::new(IntentAction::GoTo { place_id: target }, IntentSource::Reflex));
        }
        return;
    }
    let name = first_name(c).to_owned();
    let house = get_place(&target).name;
    // Arson is arson: the chronicle names the child, and those whose home it was and those who saw it hold
    // it against them (capped, as all anger at a child is).
    ignite_home(ctx, &target, Some(id));
    ctx.remember(
        id,
        format!("I only wanted to see the sparks. {house} caught, and everyone came running."),
        7.0,
        MemoryKind::Event,
        vec![],
    );
    replace_intent(ctx, id, Intent::new(IntentAction::GoTo { place_id: own }, IntentSource::Reflex));
    mark_done(ctx, id);
    report_to_parents(ctx, id, format!("{name} set fire to {house}, playing with a flint."));
}
