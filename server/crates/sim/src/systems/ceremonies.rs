//! Weddings, funerals and trials. A couple who have lasted are married on the
//! green before the whole village and move in together; the dead are buried the
//! next morning with everyone at the graveside; someone the village suspects of
//! an unseen killing is tried on the green (`systems::cases`). All call the
//! village together the way a fire does, and nothing but a fight or a flight
//! takes anyone away.
//!
//! Nothing here draws from the RNG, or changes anything at all, until a couple
//! is betrothed or someone has died, so a world with neither runs exactly as
//! it did before.

use fsw_core::jsmath::{cos, sin, utf16_truncate};
use fsw_core::math2d::{dist, facing_to, rotate};
use fsw_core::time::{TICKS_PER_DAY, minutes_to_ticks};
use fsw_core::{Vec2, vec2};
use fsw_world::places::{ActivityId, Place, get_place};

use crate::data::anims::AnimId;
use crate::data::ceremonies::{
    CHEERS, EULOGIES, FUNERAL_CLOSING, FUNERAL_OPENING, GRIEF, VOWS, WEDDING_CLOSING, WEDDING_OPENING, line,
};
use crate::data::classes::ClassId;
use crate::data::trials::{ACCUSATIONS, ACQUITTED, AGAINST, FOR, GUILTY, PLEAS, TRIAL_OPENING};
use crate::events::{Emote, SimEvent};
use crate::state::{
    ActPhase, Ceremony, CeremonyPhase, Character, ChronicleKind, Id, Intent, IntentAction, IntentSource, MemoryKind,
    MonsterPhase, Rite, SimState, Speech, StimulusKind,
};
use crate::systems::billboards::clean_text;
use crate::systems::cases;
use crate::systems::combat::max_hp;
use crate::systems::death::detach;
use crate::systems::director::tick_at;
use crate::systems::feuds;
use crate::systems::fire::{UNFIT, burning, fighting_fire, home_usable};
use crate::systems::intents::{finish, fresh_act, leave_building, travel, travel_to};
use crate::systems::relationships::{RelationDelta, adjust, marry};
use crate::util::{
    Ctx, bump_epoch, first_name, is_fighting, join_names, living, living_ids, name_of, rel_or_default, set_anim,
};

/// Days a couple must have been together before they are betrothed.
pub const BETROTHAL_DAYS: i64 = 2;
/// How well they must like each other, both ways.
pub const BETROTHAL_AFFINITY: f64 = 40.0;
/// And how little they may hold against each other.
pub const BETROTHAL_GRIEVANCE: f64 = 20.0;
/// A marriage ends only at these, where a courtship ends at 12 and 50.
pub const SPOUSE_SPLIT_AFFINITY: f64 = -10.0;
pub const SPOUSE_SPLIT_GRIEVANCE: f64 = 80.0;
/// When the rites are held.
pub const WEDDING_HOUR: f64 = 14.0;
pub const FUNERAL_HOUR: f64 = 9.0;
/// A death before this hour is buried the same morning.
const SAME_MORNING_BEFORE: f64 = 7.0;
/// How long before the hour the village is called together.
const GATHER_MINUTES: f64 = 40.0;
/// The rite begins this long after the hour even if the principals are still walking.
const GRACE_MINUTES: f64 = 15.0;
/// How long the feast or the wake lasts.
const FEAST_MINUTES: f64 = 60.0;
/// A ceremony put off this many times is called off.
const MAX_SLIPS: i64 = 3;
/// One of the dead this close to the place puts a ceremony off.
const DANGER_RADIUS: f64 = 18.0;
/// A monster this close to the ruins is lying low for the day, not abroad.
const LAIR_RADIUS: f64 = 8.0;
/// The most a speech may run to, in UTF-16 units.
pub const SPEECH_CAP: usize = 300;
/// A pause between one speaker and the next.
const PAUSE_TICKS: i64 = 20;
const WEDDING_PLACE: &str = "green";
const TRIAL_PLACE: &str = "green";
/// When a trial is held, today if it is early enough, else tomorrow.
pub const TRIAL_HOUR: f64 = 16.0;
const TRIAL_SAME_DAY_BEFORE: f64 = 13.0;
const FUNERAL_PLACE: &str = "graveyard";
const FEAST_PLACE: &str = "tavern";
/// Where on the green the couple stand, in the green's own frame: south of the well, clear of the benches.
/// `src/render/ceremony.ts` dresses the same spot.
const WEDDING_SPOT: [f64; 2] = [0.5, 5.2];
/// How many guests stand in the first arc before a second forms behind it.
const ARC: usize = 12;

/// Whether this character has been called to a ceremony.
pub fn attending(c: &Character) -> bool {
    matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::Attend { .. }))
}

/// The ceremony under way or announced, if any.
pub fn pending(state: &SimState) -> Option<&Ceremony> {
    state.ceremony.as_ref()
}

/// The ids a speech is wanted from: the couple's vows, or the officiant's eulogy.
pub fn speakers(ceremony: &Ceremony) -> Vec<Id> {
    match &ceremony.rite {
        Rite::Wedding { a, b } => vec![a.clone(), b.clone()],
        Rite::Funeral { .. } => vec![ceremony.officiant.clone()],
        // A trial's words come from what the village knows, not from the model.
        Rite::Trial { .. } => Vec::new(),
    }
}

/// At the midnight roll-over: bury any dead still waiting, and betroth a couple who have earned it.
pub fn daily(ctx: &mut Ctx) {
    if ctx.state.ceremony.is_none() && !ctx.state.director.awaiting_funeral.is_empty() {
        schedule_funeral(ctx);
    }
    if ctx.state.ceremony.is_none() {
        betrothals(ctx);
    }
}

/// The first couple who have been together long enough and like each other well enough.
fn betrothals(ctx: &mut Ctx) {
    let tick = ctx.tick();
    for id in living_ids(ctx.state) {
        let c = &ctx.state.characters[&id];
        let Some(partner) = c.partner_id.clone() else { continue };
        if id >= partner || c.married() || !ctx.alive(&partner) || ctx.cr(&partner).married() {
            continue;
        }
        if c.partner_since_tick != 0 && tick - c.partner_since_tick < BETROTHAL_DAYS * TICKS_PER_DAY {
            continue;
        }
        let mine = rel_or_default(c, &partner);
        let theirs = rel_or_default(ctx.cr(&partner), &id);
        let fond =
            |r: &crate::state::Relationship| r.affinity >= BETROTHAL_AFFINITY && r.grievance < BETROTHAL_GRIEVANCE;
        if mine.kin || !fond(&mine) || !fond(&theirs) {
            continue;
        }
        schedule_wedding(ctx, &id, &partner);
        return;
    }
}

fn schedule_wedding(ctx: &mut Ctx, a: &str, b: &str) {
    let day = ctx.time.day;
    let start_tick = tick_at(day, WEDDING_HOUR);
    let officiant = officiant_for(ctx.state, &[a, b]);
    let id = ctx.state.new_id("y");
    ctx.state.ceremony = Some(Ceremony {
        id: id.clone(),
        rite: Rite::Wedding { a: a.to_owned(), b: b.to_owned() },
        place_id: WEDDING_PLACE.into(),
        start_tick,
        phase: CeremonyPhase::Announced,
        step: 0,
        until_tick: 0,
        officiant: officiant.clone(),
        speeches: Vec::new(),
        attendees: Vec::new(),
        slips: 0,
    });
    let (a_name, b_name) = (first_name(ctx.cr(a)).to_owned(), first_name(ctx.cr(b)).to_owned());
    let officiant_name = first_name(ctx.cr(&officiant)).to_owned();
    for (me, other) in [(a, &b_name), (b, &a_name)] {
        ctx.remember(
            me,
            format!("I am to marry {other} this afternoon on the green."),
            8.0,
            MemoryKind::Event,
            vec![if me == a { b.into() } else { a.into() }],
        );
    }
    for other in living_ids(ctx.state) {
        if other == a || other == b {
            continue;
        }
        ctx.remember(
            &other,
            format!("{a_name} and {b_name} are to be married this afternoon on the green."),
            4.0,
            MemoryKind::Event,
            vec![a.into(), b.into()],
        );
    }
    ctx.chronicle(
        ChronicleKind::Romance,
        format!(
            "{a_name} and {b_name} are to be married on the village green this afternoon at two o'clock. \
             {officiant_name} will speak the rite."
        ),
        vec![a.into(), b.into()],
    );
    ctx.emit(SimEvent::CeremonyScheduled { ceremony_id: id });
    ctx.emit(SimEvent::SavePoint { reason: "betrothal".into() });
}

/// Everyone awaiting burial is given a funeral: this morning if it is early, else tomorrow.
fn schedule_funeral(ctx: &mut Ctx) {
    let dead = std::mem::take(&mut ctx.state.director.awaiting_funeral);
    if dead.is_empty() {
        return;
    }
    let day = ctx.time.day;
    let this_morning = ctx.time.hour < SAME_MORNING_BEFORE;
    let start_tick = if this_morning { tick_at(day, FUNERAL_HOUR) } else { tick_at(day + 1, FUNERAL_HOUR) };
    let officiant = officiant_for(ctx.state, &[]);
    let id = ctx.state.new_id("y");
    ctx.state.ceremony = Some(Ceremony {
        id: id.clone(),
        rite: Rite::Funeral { dead: dead.clone() },
        place_id: FUNERAL_PLACE.into(),
        start_tick,
        phase: CeremonyPhase::Announced,
        step: 0,
        until_tick: 0,
        officiant: officiant.clone(),
        speeches: Vec::new(),
        attendees: Vec::new(),
        slips: 0,
    });
    let names = names_of(ctx.state, &dead);
    let officiant_name = first_name(ctx.cr(&officiant)).to_owned();
    let when = if this_morning { "this morning" } else { "tomorrow morning" };
    for other in living_ids(ctx.state) {
        ctx.remember(&other, format!("{names} will be buried {when}."), 5.0, MemoryKind::Event, dead.clone());
    }
    ctx.chronicle(
        ChronicleKind::Death,
        format!(
            "The village will bury {names} in the graveyard {when} at nine. {officiant_name} will speak over the grave."
        ),
        dead,
    );
    ctx.emit(SimEvent::CeremonyScheduled { ceremony_id: id });
}

/// Someone is accused of the killing the case `case_id` is about: the village will try them on the green at
/// four, today if it is early, else tomorrow. `accuser` lays out the case, or the one who speaks the rite does.
pub fn schedule_trial(ctx: &mut Ctx, case_id: &str, accused: &str, accuser: Option<&str>) {
    let day = ctx.time.day;
    let today = ctx.time.hour < TRIAL_SAME_DAY_BEFORE;
    let start_tick = if today { tick_at(day, TRIAL_HOUR) } else { tick_at(day + 1, TRIAL_HOUR) };
    let except: Vec<&str> = std::iter::once(accused).chain(accuser).collect();
    let officiant = officiant_for(ctx.state, &except);
    let accuser = accuser.map_or_else(|| officiant.clone(), str::to_owned);
    let id = ctx.state.new_id("y");
    ctx.state.ceremony = Some(Ceremony {
        id: id.clone(),
        rite: Rite::Trial { accused: accused.to_owned(), accuser: accuser.clone(), case_id: case_id.to_owned() },
        place_id: TRIAL_PLACE.into(),
        start_tick,
        phase: CeremonyPhase::Announced,
        step: 0,
        until_tick: 0,
        officiant: officiant.clone(),
        speeches: Vec::new(),
        attendees: Vec::new(),
        slips: 0,
    });
    let (accused_name, accuser_name) = (name_of(ctx.state, accused), name_of(ctx.state, &accuser));
    let victim_name = name_of(ctx.state, case_id);
    let when = if today { "this afternoon" } else { "tomorrow afternoon" };
    ctx.remember(
        accused,
        format!("{accuser_name} accuses me of killing {victim_name}. The village will try me on the green {when}."),
        9.0,
        MemoryKind::Event,
        vec![accuser.clone(), case_id.to_owned()],
    );
    ctx.stimulate(
        accused,
        StimulusKind::Hostility,
        format!("You are accused of killing {victim_name}, and will be tried on the green {when}."),
        2,
        Some(&accuser),
    );
    for other in living_ids(ctx.state) {
        if other == accused {
            continue;
        }
        ctx.remember(
            &other,
            format!("{accused_name} is accused of killing {victim_name}. The trial is on the green {when} at four."),
            6.0,
            MemoryKind::Event,
            vec![accused.to_owned(), case_id.to_owned()],
        );
    }
    ctx.chronicle(
        ChronicleKind::Conflict,
        format!(
            "{accuser_name} accuses {accused_name} of killing {victim_name}. The village will hear it on the green {when} at four."
        ),
        vec![accuser.clone(), accused.to_owned(), case_id.to_owned()],
    );
    ctx.emit(SimEvent::CeremonyScheduled { ceremony_id: id });
    ctx.emit(SimEvent::SavePoint { reason: "accusation".into() });
}

/// A villager has died and has a grave: a wedding they were part of is off, and the funeral is arranged.
pub fn on_death(ctx: &mut Ctx, victim_id: &str) {
    if let Some(ceremony) = &ctx.state.ceremony
        && let Rite::Wedding { a, b } = &ceremony.rite
        && (a == victim_id || b == victim_id)
    {
        let (a, b) = (a.clone(), b.clone());
        let text = format!(
            "The wedding of {} and {} will not be held; {} is dead.",
            name_of(ctx.state, &a),
            name_of(ctx.state, &b),
            name_of(ctx.state, victim_id)
        );
        call_off(ctx, text);
    }
    ctx.state.director.awaiting_funeral.push(victim_id.to_owned());
    arrange_funeral(ctx);
}

/// A villager lost to the dark finally has a grave.
pub fn on_buried(ctx: &mut Ctx, id: &str) {
    ctx.state.director.awaiting_funeral.push(id.to_owned());
    arrange_funeral(ctx);
}

/// The village is in mourning: an announced wedding gives way, a funeral already announced takes the newly
/// dead in, and a ceremony under way is left to finish (the feast's end buries whoever waits).
fn arrange_funeral(ctx: &mut Ctx) {
    let Some(ceremony) = ctx.state.ceremony.clone() else {
        schedule_funeral(ctx);
        return;
    };
    if ceremony.phase != CeremonyPhase::Announced {
        return;
    }
    match ceremony.rite {
        Rite::Wedding { a, b } => {
            let text = format!(
                "The wedding of {} and {} is put off; the village is in mourning.",
                name_of(ctx.state, &a),
                name_of(ctx.state, &b)
            );
            call_off(ctx, text);
            schedule_funeral(ctx);
        }
        Rite::Funeral { mut dead } => {
            let newly = std::mem::take(&mut ctx.state.director.awaiting_funeral);
            let names = names_of(ctx.state, &newly);
            dead.extend(newly.iter().cloned());
            if let Some(ceremony) = &mut ctx.state.ceremony {
                ceremony.rite = Rite::Funeral { dead };
            }
            ctx.chronicle(ChronicleKind::Death, format!("{names} will be buried at the same hour."), newly);
        }
        // The trial goes ahead; the dead are buried when it is over.
        Rite::Trial { .. } => {}
    }
}

/// One of a couple was lost to the dark, or the two have parted: their wedding is off.
pub fn cancel_wedding_of(ctx: &mut Ctx, id: &str) {
    let Some(ceremony) = &ctx.state.ceremony else { return };
    let Rite::Wedding { a, b } = &ceremony.rite else { return };
    if a != id && b != id {
        return;
    }
    let text = format!("The wedding of {} and {} will not be held.", name_of(ctx.state, a), name_of(ctx.state, b));
    call_off(ctx, text);
}

fn call_off(ctx: &mut Ctx, text: String) {
    let Some(ceremony) = ctx.state.ceremony.take() else { return };
    let kind = match ceremony.rite {
        Rite::Wedding { .. } => ChronicleKind::Romance,
        Rite::Funeral { .. } => ChronicleKind::Death,
        Rite::Trial { .. } => ChronicleKind::Conflict,
    };
    ctx.chronicle(kind, text, principals(&ceremony));
}

/// The model's speech for a scheduled ceremony, kept if the ceremony still stands and the speaker still
/// speaks at it. Cleaned as a billboard is, and cut to the cap.
pub fn hold_speech(ctx: &mut Ctx, ceremony_id: &str, speaker_id: &str, text: String) {
    let Some(text) = clean_text(&text) else { return };
    let text = utf16_truncate(&text, SPEECH_CAP).to_owned();
    let Some(ceremony) = &mut ctx.state.ceremony else { return };
    if ceremony.id != ceremony_id
        || !matches!(ceremony.phase, CeremonyPhase::Announced | CeremonyPhase::Gathering)
        || !speakers(ceremony).iter().any(|s| s == speaker_id)
    {
        return;
    }
    ceremony.speeches.retain(|s| s.speaker_id != speaker_id);
    ceremony.speeches.push(Speech { speaker_id: speaker_id.to_owned(), text });
}

/// A cleric, a paladin or a druid, in that order of preference; otherwise the eldest. Never one of `except`,
/// and never a child, whatever they will grow into.
fn officiant_for(state: &SimState, except: &[&str]) -> Id {
    let candidates: Vec<&Character> = living(state)
        .filter(|c| !c.is_child() && c.exiled_since.is_none() && !except.contains(&c.id.as_str()))
        .collect();
    for class in [ClassId::Cleric, ClassId::Paladin, ClassId::Druid] {
        if let Some(c) = candidates.iter().find(|c| c.class_id == class) {
            return c.id.clone();
        }
    }
    let mut eldest: Option<&Character> = None;
    for c in &candidates {
        if eldest.is_none_or(|e| c.profile.age > e.profile.age) {
            eldest = Some(c);
        }
    }
    eldest.or(living(state).next()).map(|c| c.id.clone()).unwrap_or_default()
}

fn principals(ceremony: &Ceremony) -> Vec<Id> {
    match &ceremony.rite {
        Rite::Wedding { a, b } => vec![a.clone(), b.clone()],
        Rite::Funeral { dead } => dead.clone(),
        Rite::Trial { accused, accuser, .. } => vec![accused.clone(), accuser.clone()],
    }
}

fn names_of(state: &SimState, ids: &[Id]) -> String {
    let names: Vec<String> = ids.iter().map(|id| name_of(state, id)).collect();
    join_names(&names)
}

/// The place's own frame: `local` turned by its facing and laid from `centre`.
fn from_centre(place: &Place, centre: Vec2, local: [f64; 2]) -> Vec2 {
    let r = rotate(vec2(local[0], local[1]), place.facing);
    vec2(centre.x + r.x, centre.z + r.z)
}

/// Where the rite is held: the spot south of the well, or among the graves of the dead.
pub fn centre_of(state: &SimState, ceremony: &Ceremony) -> Vec2 {
    let place = get_place(&ceremony.place_id);
    match &ceremony.rite {
        Rite::Wedding { .. } | Rite::Trial { .. } => from_centre(place, place.origin, WEDDING_SPOT),
        Rite::Funeral { dead } => {
            let plots: Vec<Vec2> = dead
                .iter()
                .filter_map(|id| state.graves.iter().find(|g| &g.character_id == id))
                .filter_map(|g| place.grave_plots.get(g.plot as usize))
                .map(|plot| plot.pos)
                .collect();
            if plots.is_empty() {
                return place.origin;
            }
            let n = plots.len() as f64;
            let (x, z) = plots.iter().fold((0.0, 0.0), |(x, z), p| (x + p.x, z + p.z));
            vec2(x / n, z / n)
        }
    }
}

/// Each person's own spot: the couple face each other, the officiant stands before them, and the guests
/// form an arc on the far side, in the order they were called.
fn spot_for(state: &SimState, ceremony: &Ceremony, id: &str) -> Vec2 {
    let place = get_place(&ceremony.place_id);
    let centre = centre_of(state, ceremony);
    let local = if let Rite::Wedding { a, b } = &ceremony.rite
        && (a == id || b == id)
    {
        if a == id { [-1.1, 0.0] } else { [1.1, 0.0] }
    } else if let Rite::Trial { accused, accuser, .. } = &ceremony.rite
        && (accused == id || accuser == id)
        && ceremony.officiant != id
    {
        // The accused faces the one who accuses them, as a couple face each other.
        if accuser == id { [-1.6, 0.0] } else { [1.6, 0.0] }
    } else if ceremony.officiant == id {
        [0.0, -2.2]
    } else {
        let i = ceremony.attendees.iter().filter(|o| !is_principal(ceremony, o)).position(|o| o == id).unwrap_or(0);
        let (ring, k) = (i / ARC, i % ARC);
        let angle = std::f64::consts::TAU * (0.25 + 0.42 * ((k as f64 + 0.5) / ARC as f64 - 0.5));
        let radius = 4.6 + 1.6 * (k % 2) as f64 + 3.2 * ring as f64;
        [cos(angle) * radius, sin(angle) * radius]
    };
    let spot = from_centre(place, centre, local);
    state.nav.nearest_walkable(spot, 5).unwrap_or(place.door)
}

fn is_principal(ceremony: &Ceremony, id: &str) -> bool {
    match &ceremony.rite {
        Rite::Wedding { a, b } => a == id || b == id,
        Rite::Funeral { .. } => ceremony.officiant == id,
        Rite::Trial { accused, accuser, .. } => accused == id || accuser == id || ceremony.officiant == id,
    }
}

/// Why a ceremony cannot begin now, if it cannot.
fn cannot_begin(state: &SimState, ceremony: &Ceremony) -> Option<&'static str> {
    if burning(state).is_some() {
        return Some("a house is burning");
    }
    if state.monsters.values().any(|m| m.phase != MonsterPhase::Dying && m.kind.def().flies) {
        return Some("a dragon is over the village");
    }
    let centre = centre_of(state, ceremony);
    let lair = get_place("ruins").door;
    if state.war_band.is_some() {
        return Some("raiders are in the valley");
    }
    let abroad = state.monsters.values().any(|m| {
        m.phase != MonsterPhase::Dying
            && dist(m.pos, centre) < DANGER_RADIUS
            && !(m.phase == MonsterPhase::Roam && dist(m.pos, lair) < LAIR_RADIUS)
    });
    if abroad {
        return Some("the dead are abroad");
    }
    if let Rite::Wedding { a, b } = &ceremony.rite {
        for id in [a, b] {
            let c = &state.characters[id];
            if c.hp < max_hp(c) * 0.5 || is_fighting(c) || c.combat.yielded {
                return Some("one of the couple is in no state for it");
            }
        }
    }
    None
}

/// Whether the couple are still a couple, and the officiant still breathes.
fn still_stands(ctx: &mut Ctx) -> bool {
    let Some(ceremony) = ctx.state.ceremony.clone() else { return false };
    if let Rite::Wedding { a, b } = &ceremony.rite {
        let together = ctx.alive(a) && ctx.alive(b) && ctx.cr(a).partner_id.as_deref() == Some(b.as_str());
        if !together {
            let text = format!(
                "The wedding of {} and {} will not be held; they have parted.",
                name_of(ctx.state, a),
                name_of(ctx.state, b)
            );
            call_off(ctx, text);
            return false;
        }
    }
    if let Rite::Trial { accused, case_id, .. } = &ceremony.rite {
        let open = ctx.state.cases.iter().any(|case| case.victim == *case_id && !case.closed);
        let here = ctx.state.characters.get(accused).is_some_and(|c| c.exiled_since.is_none());
        if !open || !here {
            let text = format!("The trial of {} will not be held.", name_of(ctx.state, accused));
            call_off(ctx, text);
            return false;
        }
    }
    if !ctx.alive(&ceremony.officiant) {
        let principals = principals(&ceremony);
        let except: Vec<&str> = principals.iter().map(String::as_str).collect();
        let officiant = officiant_for(ctx.state, &except);
        if let Some(c) = &mut ctx.state.ceremony {
            c.officiant = officiant;
        }
    }
    // An accuser who is gone leaves the one who speaks the rite to lay out the case.
    if let Some(c) = &mut ctx.state.ceremony
        && let Rite::Trial { accuser, .. } = &mut c.rite
        && !ctx.state.characters.contains_key(accuser.as_str())
    {
        *accuser = c.officiant.clone();
    }
    true
}

/// Put off to the same hour tomorrow, or called off after too many days.
fn postpone(ctx: &mut Ctx, reason: &str) {
    let Some(ceremony) = &mut ctx.state.ceremony else { return };
    ceremony.start_tick += TICKS_PER_DAY;
    ceremony.slips += 1;
    ceremony.phase = CeremonyPhase::Announced;
    ceremony.attendees.clear();
    let ceremony = ceremony.clone();
    let (what, kind) = describe_rite(ctx.state, &ceremony);
    if ceremony.slips >= MAX_SLIPS {
        call_off(ctx, format!("{what} has been called off."));
        if let Rite::Funeral { dead } = ceremony.rite {
            // The dead are not forgotten: they wait for a quieter day.
            ctx.state.director.awaiting_funeral.extend(dead);
        }
        return;
    }
    ctx.chronicle(kind, format!("{what} is put off until tomorrow: {reason}."), principals(&ceremony));
}

/// "The wedding of A and B" / "The burial of X", and the chronicle kind it belongs to.
fn describe_rite(state: &SimState, ceremony: &Ceremony) -> (String, ChronicleKind) {
    match &ceremony.rite {
        Rite::Wedding { a, b } => {
            (format!("The wedding of {} and {}", name_of(state, a), name_of(state, b)), ChronicleKind::Romance)
        }
        Rite::Funeral { dead } => (format!("The burial of {}", names_of(state, dead)), ChronicleKind::Death),
        Rite::Trial { accused, .. } => (format!("The trial of {}", name_of(state, accused)), ChronicleKind::Conflict),
    }
}

/// Calls everyone who can stand to the place, asleep or not, a few each tick so they do not move as one.
fn muster(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let Some(ceremony) = ctx.state.ceremony.clone() else { return };
    let place = get_place(&ceremony.place_id);
    let (what, _) = describe_rite(ctx.state, &ceremony);
    let nav = ctx.state.nav.clone();
    for (i, id) in living_ids(ctx.state).iter().enumerate() {
        let Some(c) = ctx.state.characters.get(id) else { continue };
        if (tick + i as i64 * 3) % 10 != 0
            || c.combat.yielded
            || is_fighting(c)
            || fighting_fire(c)
            || attending(c)
            || c.hp < max_hp(c) * UNFIT
        {
            continue;
        }
        detach(ctx, id);
        let c = ctx.c(id);
        let was_asleep = c.asleep;
        if c.asleep || c.inside.is_some() {
            leave_building(&nav, c);
        }
        c.tool = None;
        c.queue.clear();
        c.intent = Some(Intent::new(IntentAction::Attend { place_id: place.id.clone() }, IntentSource::Reflex));
        c.act = fresh_act(tick);
        bump_epoch(c);
        if let Some(ceremony) = &mut ctx.state.ceremony {
            ceremony.attendees.push(id.clone());
        }
        let text = format!("The village is gathering at {}. {what} is about to begin.", place.name);
        ctx.stimulate(id, StimulusKind::Witness, text, 1, None);
        if was_asleep {
            ctx.emit(SimEvent::Emote { id: id.clone(), emote: Emote::Alert });
        }
    }
}

/// One character's part: walk to their spot and stand with the village until it is over.
pub fn run_attend(ctx: &mut Ctx, id: &str, place_id: &str) {
    let tick = ctx.tick();
    let Some(ceremony) = ctx
        .state
        .ceremony
        .clone()
        .filter(|c| c.place_id == place_id && matches!(c.phase, CeremonyPhase::Gathering | CeremonyPhase::Rite))
    else {
        let c = ctx.c(id);
        if c.anim.is_seated() || !c.anim.loops() {
            set_anim(c, AnimId::Idle, false);
        }
        finish(ctx, id, "The ceremony is over.");
        return;
    };
    let c = ctx.c(id);
    match c.act.phase {
        ActPhase::Start => {
            let spot = spot_for(ctx.state, &ceremony, id);
            let nav = ctx.state.nav.clone();
            if !travel_to(tick, &nav, ctx.c(id), spot) {
                finish(ctx, id, "You could not get there.");
            }
        }
        ActPhase::Travel => {
            if !travel(tick, c, None) {
                return;
            }
            // The couple look at each other; everyone else looks to the middle.
            let look_at = match &ceremony.rite {
                Rite::Wedding { a, b } if a == id => spot_for(ctx.state, &ceremony, b),
                Rite::Wedding { a, b } if b == id => spot_for(ctx.state, &ceremony, a),
                Rite::Trial { accused, accuser, .. } if accused == id && accuser != id => {
                    spot_for(ctx.state, &ceremony, accuser)
                }
                Rite::Trial { accused, accuser, .. } if accuser == id && accused != id => {
                    spot_for(ctx.state, &ceremony, accused)
                }
                _ => centre_of(ctx.state, &ceremony),
            };
            let c = ctx.c(id);
            c.facing = facing_to(c.pos, look_at);
            let anim = if fsw_core::rng::hash_string(id).is_multiple_of(2) { AnimId::Idle } else { AnimId::IdleAlt };
            set_anim(c, anim, false);
            c.act.phase = ActPhase::Perform;
            c.act.since_tick = tick;
        }
        _ => {}
    }
}

/// Whether this attendee has reached their spot.
fn in_place(state: &SimState, id: &str) -> bool {
    state.characters.get(id).is_some_and(|c| attending(c) && c.act.phase == ActPhase::Perform)
}

/// Each tick: calls the village together, speaks the rite, and sends everyone to the feast.
pub fn update_ceremonies(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let Some(ceremony) = ctx.state.ceremony.clone() else { return };
    match ceremony.phase {
        CeremonyPhase::Announced => {
            if tick < ceremony.start_tick - minutes_to_ticks(GATHER_MINUTES) || !still_stands(ctx) {
                return;
            }
            match cannot_begin(ctx.state, &ceremony) {
                Some(reason) => postpone(ctx, reason),
                None => {
                    let c = ctx.state.ceremony.as_mut().expect("the ceremony stands");
                    c.phase = CeremonyPhase::Gathering;
                    c.until_tick = ceremony.start_tick;
                }
            }
        }
        CeremonyPhase::Gathering => {
            if !still_stands(ctx) {
                return;
            }
            if let Some(reason) = cannot_begin(ctx.state, &ceremony) {
                postpone(ctx, reason);
                return;
            }
            muster(ctx);
            let ready = speakers(&ceremony)
                .iter()
                .chain(std::iter::once(&ceremony.officiant))
                .all(|id| in_place(ctx.state, id));
            if tick >= ceremony.start_tick && (ready || tick >= ceremony.start_tick + minutes_to_ticks(GRACE_MINUTES)) {
                let c = ctx.state.ceremony.as_mut().expect("the ceremony stands");
                c.phase = CeremonyPhase::Rite;
                c.step = 0;
                c.until_tick = tick;
            }
        }
        CeremonyPhase::Rite => {
            if !still_stands(ctx) {
                return;
            }
            if burning(ctx.state).is_some() {
                postpone(ctx, "a house is burning");
                return;
            }
            muster(ctx);
            if tick >= ceremony.until_tick {
                rite_step(ctx);
            }
        }
        CeremonyPhase::Feast => {
            if tick >= ceremony.until_tick {
                ctx.state.ceremony = None;
                if !ctx.state.director.awaiting_funeral.is_empty() {
                    schedule_funeral(ctx);
                }
            }
        }
    }
}

/// What the ceremony held for `speaker`, or a scripted line.
fn speech_for(ctx: &mut Ctx, ceremony: &Ceremony, speaker: &str, lines: &[&str], names: &[(&str, &str)]) -> String {
    match ceremony.speeches.iter().find(|s| s.speaker_id == speaker) {
        Some(speech) => speech.text.clone(),
        None => line(ctx.pick(lines), names),
    }
}

/// `speaker` says `text` aloud; the next step follows when the words are done and a breath taken.
fn speak(ctx: &mut Ctx, speaker: &str, text: String, to: Option<&str>) {
    let ticks = ctx.say(speaker, text, to, true);
    let tick = ctx.tick();
    if let Some(c) = &mut ctx.state.ceremony {
        c.step += 1;
        c.until_tick = tick + ticks + PAUSE_TICKS;
    }
}

fn rite_step(ctx: &mut Ctx) {
    let Some(ceremony) = ctx.state.ceremony.clone() else { return };
    let officiant = ceremony.officiant.clone();
    match &ceremony.rite {
        Rite::Wedding { a, b } => {
            let (a, b) = (a.clone(), b.clone());
            let (a_name, b_name) = (first_name(ctx.cr(&a)).to_owned(), first_name(ctx.cr(&b)).to_owned());
            let names = [("a", a_name.as_str()), ("b", b_name.as_str())];
            match ceremony.step {
                0 => {
                    let text = line(ctx.pick(&WEDDING_OPENING), &names);
                    speak(ctx, &officiant, text, None);
                }
                1 | 2 => {
                    let (me, other, other_name) =
                        if ceremony.step == 1 { (&a, &b, &b_name) } else { (&b, &a, &a_name) };
                    let text = speech_for(ctx, &ceremony, me, &VOWS, &[("name", other_name.as_str())]);
                    speak(ctx, me, text, Some(other.as_str()));
                    ctx.emit(SimEvent::Emote { id: me.clone(), emote: Emote::Heart });
                }
                3 => {
                    // The pronouncement: hearts, cheers and good wishes while the officiant speaks.
                    let text = line(ctx.pick(&WEDDING_CLOSING), &names);
                    speak(ctx, &officiant, text, None);
                    marry(ctx, &a, &b);
                    for id in [&a, &b] {
                        ctx.emit(SimEvent::Emote { id: id.clone(), emote: Emote::Heart });
                    }
                    let mut guests: Vec<Id> =
                        present(ctx.state, &ceremony).into_iter().filter(|id| *id != a && *id != b).collect();
                    // Someone in the crowd still sweet on one of them does not cheer.
                    if let Some(at) =
                        guests.iter().position(|id| feuds::heartbroken_at(ctx.state, id, &a, &b).is_some())
                    {
                        let guest = guests.remove(at);
                        feuds::watch_them_wed(ctx, &guest, &a, &b);
                    }
                    for id in &guests {
                        set_anim(ctx.c(id), AnimId::Cheer, true);
                    }
                    // Two guests call out, each a different good wish.
                    let mut voices = guests;
                    let mut cheers: Vec<&str> = CHEERS.to_vec();
                    for _ in 0..2 {
                        if voices.is_empty() {
                            break;
                        }
                        let speaker = voices.remove(ctx.pick_index(voices.len()));
                        let cheer = cheers.remove(ctx.pick_index(cheers.len()));
                        ctx.say(&speaker, line(cheer, &names), None, true);
                    }
                }
                _ => conclude_wedding(ctx, &ceremony, &a, &b),
            }
        }
        Rite::Funeral { dead } => {
            let dead = dead.clone();
            let names = names_of(ctx.state, &dead);
            let slots = [("names", names.as_str())];
            match ceremony.step {
                0 => {
                    let text = line(ctx.pick(&FUNERAL_OPENING), &slots);
                    speak(ctx, &officiant, text, None);
                }
                1 => {
                    let text = speech_for(ctx, &ceremony, &officiant, &EULOGIES, &slots);
                    speak(ctx, &officiant, text, None);
                }
                step => {
                    let mourners = mourners(ctx.state, &ceremony, &dead);
                    let spoken = step - 2;
                    if let Some((mourner, lost, close)) = mourners.get(spoken as usize) {
                        let lost_name = name_of(ctx.state, lost);
                        let text = line(ctx.pick(&GRIEF), &[("name", lost_name.as_str())]);
                        speak(ctx, mourner, text, None);
                        ctx.emit(SimEvent::Emote { id: mourner.clone(), emote: Emote::Grief });
                        if *close {
                            set_anim(ctx.c(mourner), AnimId::SitFloor, false);
                        }
                    } else if spoken == mourners.len() as i64 {
                        let text = line(ctx.pick(&FUNERAL_CLOSING), &slots);
                        speak(ctx, &officiant, text, None);
                    } else {
                        conclude_funeral(ctx, &ceremony, &dead);
                    }
                }
            }
        }
        Rite::Trial { accused, accuser, case_id } => {
            let (accused, accuser, case_id) = (accused.clone(), accuser.clone(), case_id.clone());
            trial_step(ctx, &ceremony, &accused, &accuser, &case_id);
        }
    }
}

/// The trial, step by step: the charge, the case against them, their plea, a voice from each side of the crowd,
/// and the verdict, which the attendees decide between them (`cases::votes`).
fn trial_step(ctx: &mut Ctx, ceremony: &Ceremony, accused: &str, accuser: &str, case_id: &str) {
    let officiant = ceremony.officiant.clone();
    let (accused_name, victim_name) = (name_of(ctx.state, accused), name_of(ctx.state, case_id));
    let clue = cases::strongest_clue(ctx, case_id, accused);
    let fact = clue
        .as_ref()
        .map_or_else(|| format!("{accused_name} had no cause to be glad of {victim_name}"), |(_, fact)| fact.clone());
    let names = [("accused", accused_name.as_str()), ("victim", victim_name.as_str()), ("fact", fact.as_str())];
    let crowd: Vec<Id> = present(ctx.state, ceremony)
        .into_iter()
        .filter(|id| id != accused && id != accuser && *id != officiant)
        .collect();
    let voters: Vec<Id> = present(ctx.state, ceremony);
    let (guilty, innocent) = cases::votes(ctx, case_id, accused, clue.as_ref().map(|(i, _)| *i), &voters);
    match ceremony.step {
        0 => {
            let text = line(ctx.pick(&TRIAL_OPENING), &names);
            speak(ctx, &officiant, text, None);
        }
        1 => {
            let text = line(ctx.pick(&ACCUSATIONS), &names);
            speak(ctx, accuser, text, Some(accused));
            ctx.emit(SimEvent::Emote { id: accuser.to_owned(), emote: Emote::Anger });
        }
        2 => {
            let text = line(ctx.pick(&PLEAS), &names);
            speak(ctx, accused, text, None);
        }
        3 | 4 => {
            // One voice from each side, the accusers first, if anyone in the crowd takes that side.
            let (side, lines): (Vec<Id>, &[&str]) =
                if ceremony.step == 3 { (guilty.clone(), &AGAINST) } else { (innocent.clone(), &FOR) };
            let voices: Vec<Id> = side.into_iter().filter(|id| crowd.contains(id)).collect();
            if voices.is_empty() {
                if let Some(c) = &mut ctx.state.ceremony {
                    c.step += 1;
                }
                return;
            }
            let voice = voices[ctx.pick_index(voices.len())].clone();
            let text = line(ctx.pick(lines), &names);
            speak(ctx, &voice, text, None);
            let emote = if ceremony.step == 3 { Emote::Anger } else { Emote::Alert };
            ctx.emit(SimEvent::Emote { id: voice, emote });
        }
        5 => {
            let convicted = guilty.len() > innocent.len();
            let text = line(ctx.pick(if convicted { &GUILTY[..] } else { &ACQUITTED[..] }), &names);
            speak(ctx, &officiant, text, None);
            let emote = if convicted { Emote::BrokenHeart } else { Emote::Alert };
            ctx.emit(SimEvent::Emote { id: accused.to_owned(), emote });
            cases::conclude_trial(ctx, case_id, accused, accuser, convicted, &voters);
        }
        _ => {
            // No feast after a trial: the village goes back to its day.
            let tick = ctx.tick();
            if let Some(c) = &mut ctx.state.ceremony {
                c.phase = CeremonyPhase::Feast;
                c.until_tick = tick;
            }
        }
    }
}

/// Those at the graveside who grieve most, with whom they grieve for and whether they were spouse or kin:
/// the widowed first, then kin, then the closest friends, three at most.
fn mourners(state: &SimState, ceremony: &Ceremony, dead: &[Id]) -> Vec<(Id, Id, bool)> {
    let mut ranked: Vec<(i64, f64, Id, Id, bool)> = Vec::new();
    for id in &ceremony.attendees {
        if !in_place(state, id) || *id == ceremony.officiant {
            continue;
        }
        let c = &state.characters[id];
        let mut best: Option<(i64, f64, Id, bool)> = None;
        for lost in dead {
            let widowed = state.dead.get(lost).is_some_and(|r| r.spouse_id.as_deref() == Some(id.as_str()));
            let r = rel_or_default(c, lost);
            let rank = if widowed {
                0
            } else if r.kin {
                1
            } else if r.affinity >= BETROTHAL_AFFINITY {
                2
            } else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|(b_rank, b_aff, _, _)| rank < *b_rank || (rank == *b_rank && r.affinity > *b_aff))
            {
                best = Some((rank, r.affinity, lost.clone(), rank < 2));
            }
        }
        if let Some((rank, affinity, lost, close)) = best {
            ranked.push((rank, affinity, id.clone(), lost, close));
        }
    }
    ranked.sort_by(|x, y| x.0.cmp(&y.0).then(y.1.total_cmp(&x.1)));
    ranked.into_iter().take(3).map(|(_, _, id, lost, close)| (id, lost, close)).collect()
}

/// Everyone who stood at the ceremony and is still there.
fn present(state: &SimState, ceremony: &Ceremony) -> Vec<Id> {
    ceremony.attendees.iter().filter(|id| in_place(state, id)).cloned().collect()
}

/// Pronounced wed: spouses, one home, memories all round, and off to the tavern.
fn conclude_wedding(ctx: &mut Ctx, ceremony: &Ceremony, a: &str, b: &str) {
    let guests = present(ctx.state, ceremony);
    let (a_name, b_name) = (first_name(ctx.cr(a)).to_owned(), first_name(ctx.cr(b)).to_owned());
    let officiant_name = first_name(ctx.cr(&ceremony.officiant)).to_owned();
    let home = move_in_together(ctx, a, b);
    for (me, other, other_name) in [(a, b, &b_name), (b, a, &a_name)] {
        ctx.remember(
            me,
            format!(
                "I married {other_name} today on the green, before the whole village. \
                 {officiant_name} spoke the rite."
            ),
            10.0,
            MemoryKind::Event,
            vec![other.into()],
        );
    }
    for id in &guests {
        if id == a || id == b {
            continue;
        }
        ctx.remember(
            id,
            format!("Saw {a_name} and {b_name} married on the green. The whole village was there."),
            6.0,
            MemoryKind::Event,
            vec![a.into(), b.into()],
        );
        ctx.stimulate(
            id,
            StimulusKind::Romance,
            format!("You have just seen {a_name} and {b_name} married; the feast is at the tavern."),
            1,
            Some(a),
        );
    }
    // Standing together warms everyone a little to everyone.
    let warm = RelationDelta { affinity: 2.0, familiarity: 2.0, ..Default::default() };
    for from in &guests {
        for to in &guests {
            if from != to {
                adjust(ctx, from, to, warm);
            }
        }
    }
    // Anyone who was sweet on either of them takes it hard.
    for other in living_ids(ctx.state) {
        if other == a || other == b {
            continue;
        }
        for (loved, rival, loved_name, rival_name) in [(a, b, &a_name, &b_name), (b, a, &b_name, &a_name)] {
            if rel_or_default(ctx.cr(&other), loved).attraction >= 40.0 {
                adjust(ctx, &other, rival, RelationDelta { grievance: 10.0, affinity: -6.0, ..Default::default() });
                ctx.remember(
                    &other,
                    format!("{loved_name} married {rival_name} today. I stood there and smiled."),
                    7.0,
                    MemoryKind::Event,
                    vec![loved.into(), rival.into()],
                );
                ctx.emit(SimEvent::Emote { id: other.clone(), emote: Emote::BrokenHeart });
            }
        }
    }
    feast(ctx, &guests, 35.0, 30.0);
    let home_name = get_place(&home).name;
    ctx.chronicle(
        ChronicleKind::Romance,
        format!(
            "{a_name} and {b_name} were married on the village green before the whole village. \
             They make their home at {home_name}."
        ),
        vec![a.into(), b.into(), crate::systems::world::building_id(&home)],
    );
    ctx.emit(SimEvent::SavePoint { reason: "wedding".into() });
    begin_feast(ctx);
}

/// The couple share whichever of their homes stands and has fewer other people in it. Returns the home.
fn move_in_together(ctx: &mut Ctx, a: &str, b: &str) -> String {
    let (home_a, home_b) = (ctx.cr(a).home_id.clone(), ctx.cr(b).home_id.clone());
    if home_a == home_b {
        return home_a;
    }
    // Children share their parents' beds, so only the grown-ups count against a house.
    let others = |state: &SimState, home: &str| {
        living(state).filter(|c| !c.is_child() && c.home_id == home && c.id != a && c.id != b).count()
    };
    let usable_a = home_usable(ctx.state, ctx.cr(a));
    let usable_b = home_usable(ctx.state, ctx.cr(b));
    let pick_a = match (usable_a, usable_b) {
        (true, false) => true,
        (false, true) => false,
        _ => {
            let (oa, ob) = (others(ctx.state, &home_a), others(ctx.state, &home_b));
            oa < ob || (oa == ob && home_a <= home_b)
        }
    };
    let (home, mover, other) = if pick_a { (home_a, b, a) } else { (home_b, a, b) };
    ctx.c(mover).home_id = home.clone();
    let other_name = first_name(ctx.cr(other)).to_owned();
    ctx.remember(
        mover,
        format!("Moved into {} with {other_name}.", get_place(&home).name),
        6.0,
        MemoryKind::Event,
        vec![other.into()],
    );
    home
}

/// Laid to rest: everyone remembers, the bereaved most of all, and the village goes to the wake.
fn conclude_funeral(ctx: &mut Ctx, ceremony: &Ceremony, dead: &[Id]) {
    let attendees = present(ctx.state, ceremony);
    let officiant_name = first_name(ctx.cr(&ceremony.officiant)).to_owned();
    let names = names_of(ctx.state, dead);
    let mut bereaved: Vec<Id> = Vec::new();
    for other in living_ids(ctx.state) {
        for lost in dead {
            let name = name_of(ctx.state, lost);
            let widowed = ctx.state.dead.get(lost).is_some_and(|r| r.spouse_id.as_deref() == Some(other.as_str()));
            let r = rel_or_default(ctx.cr(&other), lost);
            let (text, weight) = if widowed {
                let text = format!(
                    "We buried {name} today; {officiant_name} spoke over the grave. I will carry this all my days."
                );
                (text, 10.0)
            } else if r.kin {
                let text = format!(
                    "We buried {name} today; {officiant_name} spoke over the grave. My own blood lies in the earth."
                );
                (text, 10.0)
            } else if r.affinity >= 25.0 {
                (format!("We buried {name} today. {officiant_name} spoke well of them."), 8.0)
            } else {
                (format!("The village buried {name} today."), 5.0)
            };
            ctx.remember(&other, text, weight, MemoryKind::Event, vec![lost.clone()]);
            if weight >= 8.0 {
                ctx.stimulate(
                    &other,
                    StimulusKind::Death,
                    format!("You have just buried {name}; the wake is at the tavern."),
                    1,
                    Some(lost),
                );
                if !bereaved.contains(&other) {
                    bereaved.push(other.clone());
                }
            }
        }
    }
    // Those who grieve trust the people who stood with them a little more.
    let comfort = RelationDelta { trust: 3.0, affinity: 2.0, ..Default::default() };
    for from in &bereaved {
        if !attendees.contains(from) {
            continue;
        }
        for to in &attendees {
            if from != to {
                adjust(ctx, from, to, comfort);
            }
        }
    }
    feast(ctx, &attendees, 0.0, 15.0);
    ctx.chronicle(
        ChronicleKind::Death,
        format!("The village buried {names} in the graveyard. {officiant_name} spoke over the grave."),
        dead.to_vec(),
    );
    ctx.emit(SimEvent::SavePoint { reason: "funeral".into() });
    begin_feast(ctx);
}

/// Everyone present is fed and heartened, and sent to the tavern for the rest of the hour.
fn feast(ctx: &mut Ctx, ids: &[Id], hunger: f64, social: f64) {
    let tick = ctx.tick();
    for id in ids {
        let c = ctx.c(id);
        c.needs.hunger = (c.needs.hunger + hunger).min(100.0);
        c.needs.social = (c.needs.social + social).min(100.0);
        if attending(c) {
            c.intent = Some(Intent::new(
                IntentAction::Activity {
                    activity: ActivityId::Drink,
                    place_id: Some(FEAST_PLACE.into()),
                    minutes: Some(FEAST_MINUTES - 10.0),
                },
                IntentSource::Routine,
            ));
            c.queue.clear();
            c.act = fresh_act(tick);
            if c.anim.is_seated() {
                set_anim(c, AnimId::Idle, false);
            }
        }
    }
}

fn begin_feast(ctx: &mut Ctx) {
    let tick = ctx.tick();
    if let Some(c) = &mut ctx.state.ceremony {
        c.phase = CeremonyPhase::Feast;
        c.until_tick = tick + minutes_to_ticks(FEAST_MINUTES);
    }
}
