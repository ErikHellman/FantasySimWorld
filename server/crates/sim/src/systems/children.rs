//! Children: birthdays, coming of age, births, and the trouble a child gets
//! into. `daily_children` runs at the day roll-over; `update_children` runs
//! each tick after the quirks. Nothing here draws from the RNG unless a child is
//! alive or a couple qualifies for a child, so a village without either runs as
//! it did before children existed.

use fsw_world::places::get_place;

use crate::data::children::{BIRTHDAY_DAYS, COMING_OF_AGE};
use crate::events::SimEvent;
use crate::state::{ChronicleKind, Id, Look, MemoryKind, PlanSource, StimulusKind};
use crate::systems::arrivals::free_home;
use crate::systems::death::detach;
use crate::systems::intents::{fresh_act, leave_building};
use crate::util::{Ctx, adults, bump_epoch, first_name, living_ids};

/// Ages in words, for memories: "Today I am twelve."
fn age_word(age: i64) -> String {
    const WORDS: [&str; 17] = [
        "nought", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
        "thirteen", "fourteen", "fifteen", "sixteen",
    ];
    usize::try_from(age).ok().and_then(|i| WORDS.get(i)).map_or_else(|| age.to_string(), |w| (*w).to_owned())
}

/// Once a day: birthdays, and the coming of age that the sixteenth brings.
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
