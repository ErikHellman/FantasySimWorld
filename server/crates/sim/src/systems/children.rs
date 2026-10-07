//! Children: birthdays, coming of age, births, and the trouble a child gets
//! into. `daily_children` runs at the day roll-over; `update_children` runs
//! each tick after the quirks. Nothing here draws from the RNG unless a child is
//! alive or a couple qualifies for a child, so a village without either runs as
//! it did before children existed.

use fsw_core::math2d::clamp;
use fsw_world::places::get_place;

use crate::data::cast::{CastEntry, ChildEntry};
use crate::data::children::{
    BIRTH_CHANCE, BIRTH_COOLDOWN_TICKS, BIRTHDAY_DAYS, CHILD_FIRST_NAMES, CHILD_TRAITS, COMING_OF_AGE,
    LOVERS_BEFORE_CHILD_TICKS, MAX_CHILDREN, NEWBORN_AGE,
};
use crate::events::SimEvent;
use crate::state::{ChronicleKind, Id, Look, MemoryKind, PlanSource, Profile, StimulusKind, VoiceProfile};
use crate::systems::arrivals::{free_home, make_character};
use crate::systems::death::detach;
use crate::systems::intents::{fresh_act, leave_building};
use crate::systems::relationships::label_for;
use crate::util::{Ctx, adults, bump_epoch, children, first_name, living, living_ids, new_relationship};

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
        if day < child.next_birthday_day {
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
/// dawn. At most one a day, and the RNG is only asked when a couple qualifies.
fn maybe_birth(ctx: &mut Ctx) {
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
            let entry = child_of(ctx, &a, &b);
            welcome_child(ctx, &entry, vec![a, b]);
            return;
        }
    }
}

/// A newborn's entry, made up the way a wanderer is: a name nobody living or dead
/// has, a parent's surname and class, and a few traits.
fn child_of(ctx: &mut Ctx, a: &str, b: &str) -> CastEntry {
    let parent = if ctx.chance(0.5) { a } else { b };
    let class_id = ctx.cr(parent).class_id;
    let surname = ctx.cr(parent).profile.name.split(' ').nth(1).map(str::to_owned);
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
    let (a_name, b_name) = (first_name(ctx.cr(a)).to_owned(), first_name(ctx.cr(b)).to_owned());
    CastEntry {
        key: format!("child{}", ctx.state.next_id),
        class_id,
        model: None,
        texture: ctx.cr(parent).look.texture.clone(),
        hide: None,
        main_hand: None,
        off_hand: None,
        home_id: None,
        jobs: None,
        hobbies: None,
        profile: Profile {
            name,
            pronouns,
            age: NEWBORN_AGE,
            occupation: "child".into(),
            backstory: format!("Born in the village to {a_name} and {b_name}, and underfoot ever since."),
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
        bonds: Vec::new(),
        quirks: Vec::new(),
        child: Some(ChildEntry { mischief, parents: Vec::new() }),
    }
}

/// A child joins the village the way a newcomer does: known to everyone, kin to
/// the people who raise them, remembered and written into the chronicle.
pub fn welcome_child(ctx: &mut Ctx, entry: &CastEntry, parents: Vec<Id>) -> Id {
    let tick = ctx.tick();
    let day = ctx.time.day;
    let home = parents
        .first()
        .and_then(|p| ctx.state.characters.get(p))
        .map_or_else(|| free_home(ctx.state), |p| p.home_id.clone());
    let mut c = make_character(ctx.state, entry, &home);
    c.arrived_tick = tick;
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
        let text = if parent {
            format!("Our child, {name}, was born today.")
        } else {
            format!("{} have a child: {name}.", join(&parent_names))
        };
        ctx.remember(&other, text, if parent { 9.0 } else { 4.0 }, MemoryKind::Event, vec![id.clone()]);
        let stimulus = format!("A child has been born in the village: {name}, to {}.", join(&parent_names));
        ctx.stimulate(&other, StimulusKind::Family, stimulus, 1, Some(&id));
    }
    ctx.chronicle(ChronicleKind::Social, format!("{} have a child: {name}.", join(&parent_names)), vec![id.clone()]);
    ctx.emit(SimEvent::Arrival { id: id.clone() });
    ctx.emit(SimEvent::SavePoint { reason: "birth".into() });
    ctx.state.director.last_birth_tick = tick;
    id
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
    let c = ctx.c(id);
    let parents = c.child.take().map_or(Vec::new(), |child| child.parents);
    let def = c.class_id.def();
    if c.inside.is_some() {
        leave_building(c);
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
        ctx.c(id).home_id = new_home;
    }
    let c = ctx.c(id);
    c.plan_day = 0;
    c.plan_source = PlanSource::Routine;
    c.intent = None;
    c.queue.clear();
    c.act = fresh_act(tick);
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
