//! Music: some villagers sing or play, mostly of an evening outside the tavern, and whoever is about
//! comes to listen. A good tune heartens its listeners for a few hours; a poor one empties the yard.
//!
//! Who has a talent, and for what, follows from who they are (`talent`), with nothing drawn from the
//! RNG, so a village lives exactly as it did before music until someone strikes up a tune. The
//! performer takes a spot of their own in front of the tavern or on the green (no slot), the record of
//! the performance lives in `SimState.performances`, and the audience stands in an arc before them.

use fsw_core::jsmath::{cos, sin};
use fsw_core::math2d::{dist, facing_to, rotate};
use fsw_core::rng::hash_string;
use fsw_core::time::{TICKS_PER_GAME_MINUTE, minutes_to_ticks, seconds_to_ticks};
use fsw_core::{Vec2, vec2};
use fsw_world::places::{ActivityId, Place, get_place};

use crate::data::activities::{ActivityKind, activity};
use crate::data::anims::AnimId;
use crate::events::{Emote, SimEvent};
use crate::state::{
    ActPhase, Character, ChronicleKind, Id, Instrument, Intent, IntentAction, IntentSource, MemoryKind, Performance,
    SimState, StimulusKind,
};
use crate::systems::intents::{activity_minute, finish, fresh_act, travel, travel_to};
use crate::systems::relationships::{RelationDelta, adjust};
use crate::util::{Ctx, first_name, is_present, join_names, living_ids, mentions, set_anim};

/// Close enough to hear it where one stands: those within count as listeners.
pub const EARSHOT: f64 = 14.0;
/// Close enough to hear that something is going on and walk over.
pub const DRAWN_FROM: f64 = 26.0;
/// Quality at which a performance pleases, and heartens whoever listened.
pub const GOOD: f64 = 0.45;
/// Quality at which it is something to talk about.
pub const SUPERB: f64 = 0.75;
/// Below this the audience winces.
pub const POOR: f64 = 0.25;
/// Game minutes a performance must last before it counts for anything.
pub const MIN_MINUTES: f64 = 8.0;
/// Hours a good tune heartens its listeners, at `GOOD`; a better one lasts longer.
pub const HEARTENED_HOURS: f64 = 3.0;
/// How much each performance given adds to the next, and the most practice can add.
const PRACTICE: f64 = 0.015;
const PRACTICE_CAP: f64 = 0.3;
/// Of a hundred grown villagers whose description says nothing about music, how many have a talent anyway.
const GIFTED_PERCENT: u32 = 30;
/// Standing listeners a ring of the audience holds.
const ARC: usize = 6;

/// Where one may perform, and the spot in front of the place (local coordinates, +z out of the front).
const VENUES: [(&str, [f64; 2]); 2] = [("tavern", [0.0, 8.4]), ("green", [0.0, 3.4])];

const TUNES: [&str; 10] = [
    "a ballad of the old road",
    "the miller's reel",
    "a song about a lost sheep",
    "a lament for the drowned",
    "the drinking song everyone knows",
    "a jig for a wedding",
    "a hymn to the harvest moon",
    "a love song with too many verses",
    "the ballad of the watchtower",
    "a tune of their own making",
];
const SONG_OPENERS: [&str; 3] =
    ["Gather round, I have a song for you.", "Here is one my mother used to sing.", "Who wants to hear a song?"];
const TUNE_OPENERS: [&str; 3] =
    ["Gather round, I have a tune for you.", "Here is one for the end of the day.", "Who wants to hear a tune?"];
const CHEERS: [&str; 4] =
    ["Another! Another!", "That is the one!", "Well done, {a}!", "I have not heard that in years."];
const HECKLES: [&str; 3] = ["Somebody stop that noise!", "My ears, {a}, my ears!", "Give it a rest, {a}!"];

/// Words in a description that give a musician away, and what they point to.
const TELLTALE: [(&str, Instrument); 23] = [
    ("singer", Instrument::Voice),
    ("sings", Instrument::Voice),
    ("singing", Instrument::Voice),
    ("song", Instrument::Voice),
    ("songs", Instrument::Voice),
    ("ballad", Instrument::Voice),
    ("ballads", Instrument::Voice),
    ("lute", Instrument::Lute),
    ("fiddle", Instrument::Fiddle),
    ("fiddler", Instrument::Fiddle),
    ("flute", Instrument::Pipe),
    ("pipes", Instrument::Pipe),
    ("piper", Instrument::Pipe),
    ("whistle", Instrument::Pipe),
    ("drum", Instrument::Drum),
    ("drums", Instrument::Drum),
    ("drummer", Instrument::Drum),
    ("bard", Instrument::Lute),
    ("minstrel", Instrument::Lute),
    ("musician", Instrument::Lute),
    ("music", Instrument::Lute),
    ("tune", Instrument::Lute),
    ("tunes", Instrument::Lute),
];
/// What those with an undeclared talent lean to: most of them sing.
const LEANINGS: [Instrument; 7] = [
    Instrument::Voice,
    Instrument::Voice,
    Instrument::Lute,
    Instrument::Lute,
    Instrument::Fiddle,
    Instrument::Pipe,
    Instrument::Drum,
];

/// A villager's gift for music: what they sing or play, and how well before any practice (0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Talent {
    pub instrument: Instrument,
    pub skill: f64,
}

/// The talent this character was born with, if any. Someone whose trade, traits or story mention music
/// has one for the instrument named; of the rest, a fixed share do, settled by their name. Children have
/// none yet.
pub fn talent(c: &Character) -> Option<Talent> {
    if c.is_child() {
        return None;
    }
    let p = &c.profile;
    let text = format!("{} {} {} {}", p.occupation, p.traits.join(" "), p.goals.join(" "), p.backstory);
    let h = hash_string(&p.name.to_lowercase());
    if let Some((_, instrument)) = TELLTALE.iter().find(|(word, _)| mentions(&text, word)) {
        return Some(Talent { instrument: *instrument, skill: 0.6 + f64::from(h % 100) / 100.0 * 0.35 });
    }
    if h % 100 >= GIFTED_PERCENT {
        return None;
    }
    let instrument = LEANINGS[((h / 100) % LEANINGS.len() as u32) as usize];
    Some(Talent { instrument, skill: 0.15 + f64::from((h / 1000) % 100) / 100.0 * 0.6 })
}

/// What this character would sing or play, and how well, practice counted: their talent, or the thin
/// voice anyone has when a mind tells them to perform.
pub fn skill_of(c: &Character) -> (Instrument, f64) {
    let (instrument, base) = talent(c).map_or((Instrument::Voice, 0.1), |t| (t.instrument, t.skill));
    (instrument, (base + (c.music.performances as f64 * PRACTICE).min(PRACTICE_CAP)).min(1.0))
}

/// "superb", "good", "passable" or "poor".
pub fn quality_word(quality: f64) -> &'static str {
    if quality >= SUPERB {
        "superb"
    } else if quality >= GOOD {
        "good"
    } else if quality >= POOR {
        "passable"
    } else {
        "poor"
    }
}

/// "well", "passably" or "badly": how a musician plays, for prompts.
pub fn skill_word(skill: f64) -> &'static str {
    if skill >= 0.6 {
        "well"
    } else if skill >= 0.35 {
        "passably"
    } else {
        "badly"
    }
}

/// The performance `id` is giving, if any.
pub fn performance<'s>(state: &'s SimState, id: &str) -> Option<&'s Performance> {
    state.performances.iter().find(|p| p.performer_id == id)
}

fn performance_mut<'s>(state: &'s mut SimState, id: &str) -> Option<&'s mut Performance> {
    state.performances.iter_mut().find(|p| p.performer_id == id)
}

/// Singing or playing right now, before an audience or not.
pub fn performing(c: &Character) -> bool {
    c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Perform)) && c.act.phase == ActPhase::Perform
}

/// Standing in `performer`'s audience, or on the way to it.
pub fn listening_to(c: &Character, performer: &str) -> bool {
    matches!(&c.intent, Some(Intent { action: IntentAction::Listen { performer_id }, .. }) if performer_id == performer)
}

/// "outside The Gilded Tankard" or "on the village green".
pub fn venue_phrase(place_id: &str) -> String {
    let place = get_place(place_id);
    if place.has_building { format!("outside {}", place.name) } else { format!("on {}", place.name) }
}

fn from_place(place: &Place, local: [f64; 2]) -> Vec2 {
    let r = rotate(vec2(local[0], local[1]), place.facing);
    vec2(place.origin.x + r.x, place.origin.z + r.z)
}

/// Where to stand: the venue asked for, else the tavern as a rule and the green now and then.
fn stage(ctx: &mut Ctx, place_id: Option<&str>) -> (&'static Place, Vec2) {
    let id = match place_id {
        Some(p) if VENUES.iter().any(|(v, _)| *v == p) => p,
        _ => {
            if ctx.chance(0.75) {
                "tavern"
            } else {
                "green"
            }
        }
    };
    let (_, local) = VENUES.iter().find(|(v, _)| *v == id).expect("a venue");
    let place = get_place(id);
    let spot = from_place(place, *local);
    (place, ctx.state.nav.nearest_walkable(spot, 5).unwrap_or(place.door))
}

/// Walks to the spot, strikes up, plays until the time is up, and takes the applause or the jeers.
pub fn run_perform(ctx: &mut Ctx, id: &str, place_id: Option<String>, minutes: f64) {
    let tick = ctx.tick();
    let nav = ctx.state.nav.clone();
    match ctx.cr(id).act.phase {
        ActPhase::Start => {
            let (place, spot) = stage(ctx, place_id.as_deref());
            // The venue goes on the intent, so the status line can name it before the first note.
            if let Some(Intent { action: IntentAction::Activity { place_id, .. }, .. }) = &mut ctx.c(id).intent {
                *place_id = Some(place.id.clone());
            }
            if !travel_to(tick, &nav, ctx.c(id), spot) {
                finish(ctx, id, format!("You could not get to {}.", place.name));
            }
        }
        ActPhase::Travel => {
            if !travel(tick, ctx.c(id), None) {
                return;
            }
            let place_id = place_id.unwrap_or_else(|| "tavern".into());
            strike_up(ctx, id, &place_id, minutes);
        }
        _ => {
            if performance(ctx.state, id).is_none() {
                // Drawn away mid-tune (a chat, the alarm): the music does not pick up again.
                set_anim(ctx.c(id), AnimId::Idle, false);
                finish(ctx, id, "You were interrupted in the middle of your music.");
                return;
            }
            let c = ctx.c(id);
            let elapsed = tick - c.act.since_tick;
            if elapsed > 0 && elapsed % TICKS_PER_GAME_MINUTE == 0 {
                activity_minute(c, ActivityId::Perform, elapsed / TICKS_PER_GAME_MINUTE);
            }
            // The note over their head is renewed while they play.
            if elapsed % seconds_to_ticks(6.0) == 0 {
                ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Music });
            }
            if tick >= ctx.cr(id).act.until_tick {
                conclude(ctx, id, false);
            }
        }
    }
}

/// The first note: settles how well it will go, tells the village, and opens the record of it.
fn strike_up(ctx: &mut Ctx, id: &str, place_id: &str, minutes: f64) {
    let tick = ctx.tick();
    let day = ctx.time.day;
    let (instrument, skill) = skill_of(ctx.cr(id));
    let c = ctx.cr(id);
    let (drunk, tired) = (c.drunk(tick), c.needs.energy < 25.0);
    let luck = ctx.range(-0.15, 0.15);
    let quality = (skill + luck - if drunk { 0.2 } else { 0.0 } - if tired { 0.1 } else { 0.0 }).clamp(0.0, 1.0);
    let tune = ctx.pick(&TUNES).to_owned();
    let opener = if instrument == Instrument::Voice { ctx.pick(&SONG_OPENERS) } else { ctx.pick(&TUNE_OPENERS) };
    let place = get_place(place_id);
    let c = ctx.c(id);
    c.facing = place.facing;
    c.act.phase = ActPhase::Perform;
    c.act.since_tick = tick;
    c.act.until_tick = tick + minutes_to_ticks(minutes);
    c.music.last_day = day;
    set_anim(c, if instrument == Instrument::Voice { AnimId::IdleAlt } else { AnimId::Hold }, true);
    let (pos, name) = (c.pos, first_name(c).to_owned());
    ctx.state.performances.push(Performance {
        performer_id: id.to_owned(),
        instrument,
        quality,
        tune: tune.clone(),
        place_id: place_id.to_owned(),
        pos,
        since_tick: tick,
        until_tick: tick + minutes_to_ticks(minutes),
        listeners: Vec::new(),
        next_reaction_tick: tick + minutes_to_ticks(3.0),
    });
    ctx.say(id, opener, None, true);
    ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Music });
    let where_ = venue_phrase(place_id);
    let text = if instrument == Instrument::Voice {
        format!("{name} began to sing {tune} {where_}.")
    } else {
        format!("{name} struck up {tune} on the {} {where_}.", instrument.as_str())
    };
    ctx.chronicle(ChronicleKind::Social, text, vec![id.into()]);
}

/// Has nothing better to do than listen: idle, waiting, or at leisure or a meal. Work, duty, a fight,
/// a fire, a child to fetch and a ceremony all come before a song.
fn attentive(c: &Character) -> bool {
    if !is_present(c) || c.convo_id.is_some() || c.combat.yielded {
        return false;
    }
    match c.intent.as_ref().map(|i| &i.action) {
        None | Some(IntentAction::Wait { .. }) | Some(IntentAction::Listen { .. }) => true,
        Some(IntentAction::Activity { activity: id, .. }) => {
            !matches!(id, ActivityId::Sleep | ActivityId::Perform)
                && matches!(activity(*id).kind, ActivityKind::Leisure | ActivityKind::Need)
        }
        _ => false,
    }
}

/// Could walk over and join the standing audience: attentive, on their feet, not already there, and on
/// nothing the model asked of them.
fn free_to_come(c: &Character) -> bool {
    if !attentive(c) || c.anim.is_seated() || c.slot_id.is_some() {
        return false;
    }
    match &c.intent {
        None => true,
        Some(intent) => !matches!(intent.action, IntentAction::Listen { .. }) && intent.source == IntentSource::Routine,
    }
}

/// Drops what they were doing and goes to stand in the audience.
fn come_to_listen(ctx: &mut Ctx, id: &str, performer: &str) {
    let tick = ctx.tick();
    let listen = Intent::new(IntentAction::Listen { performer_id: performer.to_owned() }, IntentSource::Routine);
    if ctx.cr(id).intent.is_some() {
        finish(ctx, id, "You stopped to listen to the music.");
        let c = ctx.c(id);
        c.queue.clear();
        c.queue.push(listen);
    } else {
        let c = ctx.c(id);
        c.intent = Some(listen);
        c.act = fresh_act(tick);
    }
}

/// Once a game minute: counts everyone within earshot, and draws the idle over to stand and listen.
fn gather(ctx: &mut Ctx, performer: &str) {
    let Some(p) = performance(ctx.state, performer).cloned() else { return };
    for other in living_ids(ctx.state) {
        if other == performer {
            continue;
        }
        let o = ctx.cr(&other);
        if !attentive(o) {
            continue;
        }
        let d = dist(o.pos, p.pos);
        if d <= EARSHOT
            && !p.listeners.contains(&other)
            && let Some(record) = performance_mut(ctx.state, performer)
        {
            record.listeners.push(other.clone());
        }
        let o = ctx.cr(&other);
        if d > DRAWN_FROM || !free_to_come(o) {
            continue;
        }
        // Children run to it; grown-ups come at their own pace, and few cross the road for a racket.
        let mut draw = if o.is_child() { 0.8 } else { 0.45 + o.profile.sociability * 0.3 };
        if p.quality < POOR && d > EARSHOT {
            draw *= 0.3;
        }
        if ctx.chance(draw) {
            come_to_listen(ctx, &other, performer);
        }
    }
}

/// Now and then someone in the audience shows what they think: a cheer, a laugh, a heckle, or their back.
fn react(ctx: &mut Ctx, performer: &str) {
    let tick = ctx.tick();
    let Some(p) = performance(ctx.state, performer).cloned() else { return };
    let wait = ctx.range(4.0, 8.0);
    if let Some(record) = performance_mut(ctx.state, performer) {
        record.next_reaction_tick = tick + minutes_to_ticks(wait);
    }
    let present: Vec<Id> = p
        .listeners
        .iter()
        .filter(|l| ctx.state.characters.get(*l).is_some_and(|c| attentive(c) && dist(c.pos, p.pos) <= EARSHOT))
        .cloned()
        .collect();
    if present.is_empty() {
        return;
    }
    let who = present[ctx.pick_index(present.len())].clone();
    let name = first_name(ctx.cr(performer)).to_owned();
    let standing = listening_to(ctx.cr(&who), performer) && ctx.cr(&who).act.phase == ActPhase::Perform;
    if p.quality >= GOOD {
        let emote = if p.quality >= SUPERB && ctx.chance(0.5) { Emote::Heart } else { Emote::Laugh };
        ctx.emit(SimEvent::Emote { id: who.clone(), emote });
        if standing {
            let c = ctx.c(&who);
            set_anim(c, AnimId::Cheer, true);
            c.act.repath_tick = tick + seconds_to_ticks(AnimId::Cheer.seconds());
        }
        if ctx.chance(0.35) {
            let line = ctx.pick(&CHEERS).replace("{a}", &name);
            ctx.say(&who, line, Some(performer), false);
        }
    } else if p.quality < POOR {
        let temper = ctx.cr(&who).profile.temper;
        if temper > 0.5 && ctx.chance(0.5) {
            let line = ctx.pick(&HECKLES).replace("{a}", &name);
            ctx.say(&who, line, Some(performer), true);
            ctx.emit(SimEvent::Emote { id: who.clone(), emote: Emote::Anger });
            adjust(ctx, &who, performer, RelationDelta { affinity: -2.0, ..Default::default() });
        } else if standing && ctx.chance(0.5) {
            set_anim(ctx.c(&who), AnimId::Idle, false);
            finish(ctx, &who, "You could not bear the noise and walked off.");
        }
    } else if ctx.chance(0.3) {
        ctx.emit(SimEvent::Emote { id: who, emote: Emote::Laugh });
    }
}

/// Each listener's own spot: an arc before the performer, a ring further back when the first is full.
fn audience_spot(state: &SimState, p: &Performance, id: &str) -> Vec2 {
    let k = p
        .listeners
        .iter()
        .filter(|l| state.characters.get(*l).is_some_and(|c| listening_to(c, &p.performer_id)))
        .position(|l| l == id)
        .unwrap_or(0);
    let (ring, i) = (k / ARC, k % ARC);
    let angle = std::f64::consts::TAU * 0.34 * ((i as f64 + 0.5) / ARC as f64 - 0.5);
    let radius = 3.4 + 1.3 * (i % 2) as f64 + 2.4 * ring as f64;
    let facing = state.characters.get(&p.performer_id).map_or(0.0, |c| c.facing);
    let r = rotate(vec2(sin(angle) * radius, cos(angle) * radius), facing);
    let spot = vec2(p.pos.x + r.x, p.pos.z + r.z);
    state.nav.nearest_walkable(spot, 5).unwrap_or(p.pos)
}

/// Walks over, finds a spot and stands facing the music until it stops.
pub fn run_listen(ctx: &mut Ctx, id: &str, performer: &str) {
    let tick = ctx.tick();
    let Some(p) = performance(ctx.state, performer).cloned() else {
        let c = ctx.c(id);
        if c.anim == AnimId::Cheer || c.anim == AnimId::IdleAlt {
            set_anim(c, AnimId::Idle, false);
        }
        finish(ctx, id, "The music stopped.");
        return;
    };
    match ctx.cr(id).act.phase {
        ActPhase::Start => {
            let spot = audience_spot(ctx.state, &p, id);
            let nav = ctx.state.nav.clone();
            if !travel_to(tick, &nav, ctx.c(id), spot) {
                finish(ctx, id, "You could not get near enough to hear.");
            }
        }
        ActPhase::Travel => {
            let c = ctx.c(id);
            if !travel(tick, c, None) {
                return;
            }
            c.facing = facing_to(c.pos, p.pos);
            let anim = if hash_string(id).is_multiple_of(2) { AnimId::Idle } else { AnimId::IdleAlt };
            set_anim(c, anim, false);
            c.act.phase = ActPhase::Perform;
            c.act.since_tick = tick;
            c.act.repath_tick = 0;
        }
        _ => {
            let c = ctx.c(id);
            if c.anim == AnimId::Cheer && tick >= c.act.repath_tick {
                set_anim(c, AnimId::IdleAlt, false);
            }
            // Company, of a kind.
            let elapsed = tick - c.act.since_tick;
            if elapsed > 0 && elapsed % TICKS_PER_GAME_MINUTE == 0 {
                c.needs.social = (c.needs.social + 0.15).min(100.0);
            }
        }
    }
}

/// "Mira, Theron and Kael" or "Mira, Theron, Kael and 3 others".
fn some_names(state: &SimState, ids: &[Id]) -> String {
    let mut names: Vec<String> =
        ids.iter().filter_map(|id| state.characters.get(id)).map(|c| first_name(c).to_owned()).collect();
    if names.len() > 4 {
        let more = names.len() - 3;
        names.truncate(3);
        names.push(format!("{more} others"));
    }
    join_names(&names)
}

/// The last note, or the music breaking off (`cut`): the audience is released and, if the music lasted,
/// heartened or put out by it; everyone remembers; the chronicle records how it went.
pub fn conclude(ctx: &mut Ctx, performer: &str, cut: bool) {
    let tick = ctx.tick();
    let Some(index) = ctx.state.performances.iter().position(|p| p.performer_id == performer) else {
        if !cut && ctx.alive(performer) {
            finish(ctx, performer, "You finished your music.");
        }
        return;
    };
    let p = ctx.state.performances.remove(index);
    let played = (tick - p.since_tick) as f64 / TICKS_PER_GAME_MINUTE as f64;
    let counted = played >= MIN_MINUTES;
    let (good, superb, poor) = (p.quality >= GOOD, p.quality >= SUPERB, p.quality < POOR);
    let name = first_name(ctx.cr(performer)).to_owned();
    let where_ = venue_phrase(&p.place_id);
    let (did, to_do) = (p.instrument.did(), p.instrument.to_do());
    let heard: Vec<Id> = p.listeners.iter().filter(|l| ctx.alive(l)).cloned().collect();

    for l in &heard {
        let lc = ctx.c(l);
        if listening_to(lc, performer) {
            if good && !cut {
                set_anim(lc, AnimId::Cheer, true);
            } else if lc.anim == AnimId::Cheer || lc.anim == AnimId::IdleAlt {
                set_anim(lc, AnimId::Idle, false);
            }
            let outcome = if cut {
                format!("{name} broke off in the middle of the music.")
            } else if good {
                format!("You heard {name} {to_do} {where_}, and loved it.")
            } else if poor {
                format!("You heard {name} {to_do} {where_}, and wished you had not.")
            } else {
                format!("You heard {name} {to_do} {where_}.")
            };
            finish(ctx, l, outcome);
        }
        if !counted {
            continue;
        }
        let tune = &p.tune;
        if good {
            let hours = HEARTENED_HOURS + (p.quality - GOOD) * 8.0;
            let lc = ctx.c(l);
            lc.heartened_until_tick = lc.heartened_until_tick.max(tick + minutes_to_ticks(hours * 60.0));
            lc.needs.social = (lc.needs.social + if superb { 20.0 } else { 12.0 }).min(100.0);
            // A good tune mends moods a little all round.
            for r in lc.relationships.values_mut() {
                if r.grievance > 0.0 {
                    r.grievance = (r.grievance - 3.0).max(0.0);
                }
            }
            let affinity = if superb { 6.0 } else { 4.0 };
            adjust(ctx, l, performer, RelationDelta { affinity, familiarity: 2.0, ..Default::default() });
            ctx.remember(
                l,
                format!("Listened to {name} {to_do} {tune} {where_}. It lifted my spirits."),
                if superb { 4.0 } else { 3.0 },
                MemoryKind::Event,
                vec![performer.to_owned()],
            );
            ctx.stimulate(
                l,
                StimulusKind::Music,
                format!("You have just heard {name} {to_do} {tune} {where_}, and it has left you in good spirits."),
                0,
                Some(performer),
            );
        } else if poor {
            adjust(ctx, l, performer, RelationDelta { affinity: -2.0, familiarity: 1.0, ..Default::default() });
            ctx.remember(
                l,
                format!("Sat through {name}'s {tune} {where_}. Painful."),
                2.0,
                MemoryKind::Event,
                vec![performer.to_owned()],
            );
        } else {
            adjust(ctx, l, performer, RelationDelta { affinity: 1.0, familiarity: 1.0, ..Default::default() });
            ctx.remember(
                l,
                format!("Heard {name} {to_do} {tune} {where_}."),
                2.0,
                MemoryKind::Event,
                vec![performer.to_owned()],
            );
        }
    }

    let tune = &p.tune;
    let n = heard.len();
    let people = if n == 1 { "one person".to_owned() } else { format!("{n} people") };
    if counted {
        let c = ctx.c(performer);
        c.music.performances += 1;
        if n > 0 {
            c.needs.social = (c.needs.social + 10.0).min(100.0);
        }
    }
    let (outcome, memory, chronicle) = if cut {
        (
            String::new(),
            format!("Was interrupted in the middle of {tune} {where_}."),
            (n > 0).then(|| format!("{name} broke off {tune} {where_}.")),
        )
    } else if n == 0 {
        (
            format!("You {did} {tune} {where_}, but nobody stopped to listen."),
            format!("{} {tune} {where_} to an empty yard.", capitalise(did)),
            Some(format!("{name} {did} {tune} {where_} to an empty yard.")),
        )
    } else if good {
        (
            format!("You {did} {tune} {where_} for {people}, and they loved it."),
            format!("{} {tune} {where_} for {people}. They loved it.", capitalise(did)),
            Some(format!("{name} {did} {tune} {where_}, and {} cheered.", some_names(ctx.state, &heard))),
        )
    } else if poor {
        (
            format!("You {did} {tune} {where_} for {people}, and they did not care for it."),
            format!("{} {tune} {where_}. It did not go well.", capitalise(did)),
            Some(format!(
                "{name} {did} {tune} {where_}; {} wished they had not heard it.",
                some_names(ctx.state, &heard)
            )),
        )
    } else {
        (
            format!("You {did} {tune} {where_} for {people}, and they listened politely."),
            format!("{} {tune} {where_} for {people}.", capitalise(did)),
            Some(format!("{name} {did} {tune} {where_} for {}.", some_names(ctx.state, &heard))),
        )
    };
    ctx.remember(performer, memory, if good && n > 0 { 4.0 } else { 3.0 }, MemoryKind::Event, heard.clone());
    if counted && !cut {
        let text = match (good, n) {
            (_, 0) => format!("Your music {where_} drew no listeners."),
            (true, _) => format!("Your {tune} {where_} went down well: {people} cheered you."),
            (false, _) => format!("Your {tune} {where_} was {}; {people} heard it.", quality_word(p.quality)),
        };
        ctx.stimulate(performer, StimulusKind::Music, text, if superb && n > 0 { 1 } else { 0 }, None);
    }
    if superb && n > 0 && !cut {
        ctx.emit(SimEvent::Emote { id: performer.to_owned(), emote: Emote::Heart });
    }
    if let Some(text) = chronicle
        && counted
    {
        let mut ids = vec![performer.to_owned()];
        ids.extend(heard);
        ctx.chronicle(ChronicleKind::Social, text, ids);
    }
    if !cut {
        set_anim(ctx.c(performer), AnimId::Idle, false);
        finish(ctx, performer, outcome);
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Each tick: ends the music of anyone drawn away from it, and once a game minute gathers and stirs the
/// audience of everyone still playing.
pub fn update_music(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let stopped: Vec<Id> = ctx
        .state
        .performances
        .iter()
        .filter(|p| !ctx.state.characters.get(&p.performer_id).is_some_and(|c| performing(c) && c.convo_id.is_none()))
        .map(|p| p.performer_id.clone())
        .collect();
    for id in stopped {
        if ctx.alive(&id) {
            conclude(ctx, &id, true);
        } else {
            ctx.state.performances.retain(|p| p.performer_id != id);
        }
    }
    let performers: Vec<Id> = ctx.state.performances.iter().map(|p| p.performer_id.clone()).collect();
    for id in performers {
        if tick % TICKS_PER_GAME_MINUTE == 0 {
            gather(ctx, &id);
        }
        if performance(ctx.state, &id).is_some_and(|p| tick >= p.next_reaction_tick) {
            react(ctx, &id);
        }
    }
}
