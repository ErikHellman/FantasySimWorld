//! Weather: clear skies, rain, wind and hail, and how the villagers take them.
//!
//! The sky keeps its own course. Each spell (a stretch of one kind of weather) is rolled from a hash of
//! the world's seed and the tick it is rolled at (`roll`), with nothing drawn from the RNG, so the
//! weather is the same for a seed whatever the villagers get up to. A spell comes on and dies away over
//! a few game minutes; a change of kind waits for the old one to die away first.
//!
//! How each villager takes it follows from who they are (`taste`), the way a talent for music does:
//! words in their trade, traits, goals or story, or else their name. Those who love the weather in
//! the sky tire and grow lonely a little slower, and those who hate it a little faster; the model is
//! told how they feel, and when the weather turns they show it. Those who hate the rain go indoors
//! when it comes down hard, and hail sends everyone in: it hurts, though it never kills.
//!
//! Rain and hail also keep fires down, and wind fans them (`systems::fire`).

use std::f64::consts::TAU;

use fsw_core::rng::hash_string;
use fsw_core::time::{TICKS_PER_GAME_MINUTE, is_night, minutes_to_ticks};

use crate::data::activities::activity;
use crate::data::anims::AnimId;
use crate::data::classes::RUN_SPEED;
use crate::data::weather::{
    HAIL, HATE_RAIN, HATE_WIND, LOVE_RAIN, LOVE_SUN, LOVE_WIND, RAIN_HATERS, RAIN_LOVERS, WIND_HATERS, WIND_LOVERS,
};
use crate::events::{Emote, SimEvent};
use crate::state::{
    ActPhase, ActivityId, Character, ChronicleKind, Intent, IntentAction, IntentSource, SimState, Spell, StimulusKind,
    WeatherKind,
};
use crate::systems::death::detach;
use crate::systems::fire::{fighting_fire, shelter_of};
use crate::systems::intents::{finish, fresh_act, leave_building, travel, travel_to};
use crate::systems::routine::block_at;
use crate::util::{Ctx, bump_epoch, has_recent_stimulus, is_fighting, living_ids, mentions, set_anim};

/// A new world has fair weather while its founders settle in.
const CALM_START_TICKS: i64 = 6 * 60 * TICKS_PER_GAME_MINUTE;
/// What follows each kind of weather, as odds out of a hundred, both in the order of `WeatherKind::ALL`.
const FOLLOWS: [[u32; 4]; 4] = [[55, 22, 20, 3], [45, 30, 15, 10], [45, 30, 20, 5], [30, 60, 10, 0]];
/// How long each kind lasts, in game hours.
const HOURS: [[f64; 2]; 4] = [[3.0, 10.0], [1.0, 5.0], [2.0, 6.0], [10.0 / 60.0, 0.5]];
/// How hard it rains, hails or blows at its height.
const STRENGTH: [[f64; 2]; 4] = [[0.0, 0.0], [0.4, 1.0], [0.5, 1.0], [0.5, 1.0]];
/// The wind that comes with each kind.
const WIND: [[f64; 2]; 4] = [[0.0, 0.2], [0.2, 0.5], [0.5, 1.0], [0.4, 0.8]];
/// How fast the weather comes on and dies away, per game minute: hail all at once, the rest gradually.
const EASE_PER_MINUTE: f64 = 0.05;
const HAIL_EASE_PER_MINUTE: f64 = 0.2;
const WIND_EASE_PER_MINUTE: f64 = 0.03;
/// Weather at least this strong is felt to the full; anything slighter, in proportion.
const FELT: f64 = 0.5;
/// A fair sky pleases those who like one only half as much as rain pleases those who love it.
const SUN_FELT: f64 = 0.5;
/// How much a liking for the weather slows tiredness and loneliness, and a dislike speeds them.
pub const MOOD_PACE: f64 = 0.15;
/// Rain at least this hard sends those who hate it indoors; they come out once it eases below `SHELTER_RAIN_LEFT`.
pub const SHELTER_RAIN: f64 = 0.5;
const SHELTER_RAIN_LEFT: f64 = 0.3;
/// Hail any slighter than this is not worth running from.
const HAIL_FELT: f64 = 0.15;
/// The chance each game minute, in the worst of it, that someone out in the hail is struck, and what it costs.
const HAIL_CHANCE: f64 = 0.25;
const HAIL_DAMAGE: [i64; 2] = [1, 3];
/// Hail hurts, but never takes anyone below this.
pub const HAIL_FLOOR: f64 = 5.0;
/// Game minutes over which the village notices a change of weather, one by one.
const NOTICE_MINUTES: f64 = 6.0;
/// One in this many of those who feel strongly about it says so aloud.
const REMARK_ONE_IN: u32 = 3;

fn index(kind: WeatherKind) -> usize {
    WeatherKind::ALL.iter().position(|k| *k == kind).unwrap_or(0)
}

/// A number in 0..1 for this world, this tick and this purpose.
fn unit(seed: u32, tick: i64, what: &str) -> f64 {
    f64::from(hash_string(&format!("weather {seed} {tick} {what}"))) / 4_294_967_296.0
}

fn between(seed: u32, tick: i64, what: &str, [low, high]: [f64; 2]) -> f64 {
    low + (high - low) * unit(seed, tick, what)
}

/// The next spell of weather, rolled at `tick`: a fixed function of the seed, the tick and the weather it follows.
pub fn roll(state: &SimState, tick: i64) -> Spell {
    let after = state.weather.next.as_ref().map_or(state.weather.kind, |next| next.kind);
    let odds = FOLLOWS[index(after)];
    let mut pick = hash_string(&format!("weather {} {tick} kind", state.seed)) % odds.iter().sum::<u32>();
    let mut kind = WeatherKind::Clear;
    for (i, chance) in odds.iter().enumerate() {
        if pick < *chance {
            kind = WeatherKind::ALL[i];
            break;
        }
        pick -= chance;
    }
    // Nobody is married or buried in hail.
    if kind == WeatherKind::Hail && state.ceremony.is_some() {
        kind = WeatherKind::Rain;
    }
    let i = index(kind);
    let seed = state.seed;
    let minutes = (between(seed, tick, "hours", HOURS[i]) * 60.0).floor() as i64;
    let wind = between(seed, tick, "wind", WIND[i]);
    let strength = if kind == WeatherKind::Wind { wind } else { between(seed, tick, "strength", STRENGTH[i]) };
    Spell {
        kind,
        strength,
        wind,
        wind_dir: unit(seed, tick, "direction") * TAU,
        until_tick: tick + minutes.max(1) * TICKS_PER_GAME_MINUTE,
    }
}

/// How a villager takes each kind of weather: 1 loves it, 0 does not mind, -1 hates it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Taste {
    pub rain: i8,
    pub wind: i8,
    pub sun: i8,
}

impl Taste {
    pub fn of(self, kind: WeatherKind) -> i8 {
        match kind {
            WeatherKind::Clear => self.sun,
            WeatherKind::Rain => self.rain,
            WeatherKind::Wind => self.wind,
            // Nobody likes being pelted.
            WeatherKind::Hail => -1,
        }
    }
}

/// How this character takes the weather. Someone whose trade, traits, goals or story give it away (a
/// farmer, a poet, a sailor, someone vain or frail) feels as they would; of the rest, their name decides,
/// about a third loving the rain and a third hating it, a quarter loving the wind and a quarter hating
/// it. Everyone but the rain's lovers likes a fair day.
pub fn taste(c: &Character) -> Taste {
    let p = &c.profile;
    let text = format!("{} {} {} {}", p.occupation, p.traits.join(" "), p.goals.join(" "), p.backstory);
    let says = |words: &[&str]| words.iter().any(|word| mentions(&text, word));
    let h = hash_string(&format!("weather {}", p.name.to_lowercase()));
    let rain = if says(&RAIN_LOVERS) {
        1
    } else if says(&RAIN_HATERS) {
        -1
    } else {
        [-1, 0, 1][(h % 3) as usize]
    };
    let wind = if says(&WIND_LOVERS) {
        1
    } else if says(&WIND_HATERS) {
        -1
    } else {
        [-1, 0, 0, 1][((h / 3) % 4) as usize]
    };
    Taste { rain, wind, sun: if rain > 0 { 0 } else { 1 } }
}

/// How the weather right now sits with this character, -1 (miserable) .. 1 (delighted); slight weather
/// is felt slightly, and a fair night not at all.
pub fn mood(state: &SimState, c: &Character) -> f64 {
    let w = &state.weather;
    let felt = match w.kind {
        WeatherKind::Clear => {
            if is_night(fsw_core::time::game_time(state.tick).hour) {
                0.0
            } else {
                SUN_FELT
            }
        }
        _ => (w.intensity / FELT).min(1.0),
    };
    f64::from(taste(c).of(w.kind)) * felt
}

/// A few words for the inspector on how the weather sits with this character, when it is worth saying.
pub fn feeling_words(state: &SimState, c: &Character) -> Option<&'static str> {
    let w = &state.weather;
    let mood = mood(state, c);
    if mood.abs() < 0.5 {
        return None;
    }
    match (w.kind, mood > 0.0) {
        (WeatherKind::Rain, true) => Some("glad of the rain"),
        (WeatherKind::Rain, false) => Some("glum in the rain"),
        (WeatherKind::Wind, true) => Some("enjoying the wind"),
        (WeatherKind::Wind, false) => Some("braced against the wind"),
        (WeatherKind::Hail, _) if c.inside.is_none() && !sheltering(c) => Some("pelted by hail"),
        _ => None,
    }
}

/// Hail coming down hard enough to run from.
pub fn hailing(state: &SimState) -> bool {
    state.weather.kind == WeatherKind::Hail && state.weather.intensity >= HAIL_FELT
}

/// How hard rain or hail is falling, 0..1: what keeps fires down.
pub fn wetness(state: &SimState) -> f64 {
    if state.weather.kind.wet() { state.weather.intensity } else { 0.0 }
}

/// Whether this character is going indoors, or staying in, out of the weather.
pub fn sheltering(c: &Character) -> bool {
    matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::Shelter))
}

/// Sheltering from hail: nothing but a fight or flight brings them out.
pub fn sheltering_from_hail(state: &SimState, c: &Character) -> bool {
    sheltering(c) && state.weather.kind == WeatherKind::Hail
}

/// Rain hard enough, and this character minds it enough, to be better off indoors.
pub fn rain_drives_in(state: &SimState, c: &Character, threshold: f64) -> bool {
    let w = &state.weather;
    w.kind == WeatherKind::Rain && w.intensity >= threshold && taste(c).rain < 0
}

/// Whether a rain-hater would go in rather than take up this routine intent: work and leisure out in the
/// open, but not eating, sleeping or keeping watch, which they do whatever the weather.
fn outdoor_routine(intent: &Intent) -> bool {
    if intent.source != IntentSource::Routine {
        return false;
    }
    match &intent.action {
        IntentAction::Activity { activity: id, .. } => {
            !matches!(id, ActivityId::Eat | ActivityId::Sleep | ActivityId::Watch) && !activity(*id).indoors
        }
        IntentAction::Nap { .. } | IntentAction::WorldWork { .. } => true,
        _ => false,
    }
}

/// Sends a character indoors out of the weather.
fn send_in(ctx: &mut Ctx, id: &str, source: IntentSource) {
    let tick = ctx.tick();
    let nav = ctx.state.nav.clone();
    detach(ctx, id);
    let c = ctx.c(id);
    if c.asleep && c.inside.is_none() {
        // Sleeping rough: up off the grass.
        leave_building(&nav, c);
    }
    c.tool = None;
    c.queue.clear();
    c.intent = Some(Intent::new(IntentAction::Shelter, source));
    c.act = fresh_act(tick);
    bump_epoch(c);
}

/// Indoors out of the weather: home, or the tavern for those without one, until it passes.
pub fn run_shelter(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    let hour = ctx.time.hour;
    let nav = ctx.state.nav.clone();
    let hail = hailing(ctx.state);
    let c = ctx.cr(id);
    let home = shelter_of(ctx.state, c);
    match c.act.phase {
        ActPhase::Start => {
            let c = ctx.c(id);
            if c.inside.is_some() {
                c.act.phase = ActPhase::Perform;
                c.act.since_tick = tick;
            } else if !travel_to(tick, &nav, c, home.door) {
                finish(ctx, id, "There was nowhere to get out of the weather.");
            }
        }
        ActPhase::Travel => {
            let c = ctx.c(id);
            if !travel(tick, c, hail.then_some(RUN_SPEED)) {
                return;
            }
            c.inside = Some(home.id.to_owned());
            c.act.phase = ActPhase::Perform;
            c.act.since_tick = tick;
        }
        _ => {
            let stay = if ctx.state.weather.kind == WeatherKind::Hail {
                true
            } else {
                // Rain keeps them in until it eases, unless hunger or tiredness calls, or it is time for bed.
                let bedtime = usize::try_from(block_at(&c.plan, hour))
                    .ok()
                    .and_then(|i| c.plan.get(i))
                    .is_some_and(|block| block.activity == ActivityId::Sleep);
                rain_drives_in(ctx.state, c, SHELTER_RAIN_LEFT)
                    && c.needs.hunger >= 24.0
                    && c.needs.energy >= 9.0
                    && !bedtime
            };
            if !stay {
                let c = ctx.c(id);
                leave_building(&nav, c);
                set_anim(c, AnimId::Idle, false);
                finish(ctx, id, "You stayed in until the weather eased.");
            }
        }
    }
}

/// Starts a new kind of weather.
fn set_in(ctx: &mut Ctx, spell: Spell) {
    let tick = ctx.tick();
    let w = &mut ctx.state.weather;
    let was = w.kind;
    w.kind = spell.kind;
    w.intensity = 0.0;
    w.target = spell.strength;
    w.wind_target = spell.wind;
    w.wind_dir = spell.wind_dir;
    w.until_tick = spell.until_tick;
    w.since_tick = tick;
    w.next = None;
    if spell.kind == was {
        return;
    }
    let text = match spell.kind {
        WeatherKind::Clear => "The skies clear over the valley.",
        WeatherKind::Rain => "Rain sweeps in over the valley.",
        WeatherKind::Wind => "A strong wind gets up over the valley.",
        WeatherKind::Hail => "Hail comes down on the valley, and everyone runs for cover.",
    };
    ctx.chronicle(ChronicleKind::World, text, Vec::new());
}

/// Rolls the next spell when the current one is over, and moves the sky toward it.
fn advance_sky(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let w = &mut ctx.state.weather;
    if w.until_tick == 0 {
        w.until_tick = tick.max(CALM_START_TICKS);
    }
    if tick >= w.until_tick && w.next.is_none() {
        let spell = roll(ctx.state, tick);
        let w = &mut ctx.state.weather;
        w.wind_target = spell.wind;
        if spell.kind == w.kind {
            w.target = spell.strength;
            w.until_tick = spell.until_tick;
        } else if w.intensity <= 0.0 {
            set_in(ctx, spell);
        } else {
            // The old weather dies away before the new one comes on.
            w.target = 0.0;
            w.next = Some(spell);
        }
    }
    let w = &mut ctx.state.weather;
    if w.intensity <= 0.0
        && let Some(next) = w.next.take()
    {
        set_in(ctx, next);
    }
    let w = &mut ctx.state.weather;
    let rate = if w.kind == WeatherKind::Hail { HAIL_EASE_PER_MINUTE } else { EASE_PER_MINUTE };
    w.intensity = ease(w.intensity, w.target, rate);
    w.wind = ease(w.wind, w.wind_target, WIND_EASE_PER_MINUTE);
}

fn ease(value: f64, target: f64, step: f64) -> f64 {
    if value < target { (value + step).min(target) } else { (value - step).max(target) }
}

/// One by one over a few minutes after the weather turns, the village notices: each who feels strongly
/// about it is told, shows it, and some say so.
fn notice(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let (kind, since) = (ctx.state.weather.kind, ctx.state.weather.since_tick);
    let window = minutes_to_ticks(NOTICE_MINUTES);
    if since == 0 || tick <= since || tick > since + window {
        return;
    }
    if kind == WeatherKind::Clear && is_night(ctx.time.hour) {
        return;
    }
    for id in living_ids(ctx.state) {
        let h = hash_string(&format!("{id} {since}"));
        if tick != since + 1 + i64::from(h) % window {
            continue;
        }
        let c = ctx.cr(&id);
        let feeling = taste(c).of(kind);
        if c.asleep || feeling == 0 {
            continue;
        }
        let outdoors = c.inside.is_none();
        let text = match (kind, feeling > 0) {
            (WeatherKind::Clear, _) => "The clouds have gone and the sun is out. You like a fine day.",
            (WeatherKind::Rain, true) => "It has started to rain. You love the rain.",
            (WeatherKind::Rain, false) => "It has started to rain. You hate being out in the rain.",
            (WeatherKind::Wind, true) => "A strong wind has got up. You love a good wind.",
            (WeatherKind::Wind, false) => "A strong wind has got up. You hate the wind.",
            (WeatherKind::Hail, _) => "Hail is coming down hard. It hurts; get indoors!",
        };
        let urgency = if kind == WeatherKind::Hail { 2 } else { 0 };
        ctx.stimulate(&id, StimulusKind::Weather, text, urgency, None);
        if !outdoors {
            continue;
        }
        let emote = match (kind, feeling > 0) {
            (WeatherKind::Hail, _) => Emote::Alert,
            (_, true) => Emote::Laugh,
            (_, false) => Emote::Anger,
        };
        ctx.emit(SimEvent::Emote { id: id.clone(), emote });
        if (h / 1000).is_multiple_of(REMARK_ONE_IN) {
            let lines: &[&str] = match (kind, feeling > 0) {
                (WeatherKind::Clear, _) => &LOVE_SUN,
                (WeatherKind::Rain, true) => &LOVE_RAIN,
                (WeatherKind::Rain, false) => &HATE_RAIN,
                (WeatherKind::Wind, true) => &LOVE_WIND,
                (WeatherKind::Wind, false) => &HATE_WIND,
                (WeatherKind::Hail, _) => &HAIL,
            };
            let line = lines[((h / 7) % lines.len() as u32) as usize];
            ctx.say(&id, line, None, kind == WeatherKind::Hail);
        }
    }
}

/// While hail comes down: everyone out in it runs for cover, a few at a time, and anyone still out is struck.
fn hail(ctx: &mut Ctx) {
    if !hailing(ctx.state) {
        return;
    }
    let tick = ctx.tick();
    for (i, id) in living_ids(ctx.state).iter().enumerate() {
        let c = ctx.cr(id);
        if (tick + i as i64 * 3) % 10 != 0
            || c.inside.is_some()
            || sheltering(c)
            || is_fighting(c)
            || fighting_fire(c)
            || c.combat.yielded
        {
            continue;
        }
        send_in(ctx, id, IntentSource::Reflex);
    }
    if tick % TICKS_PER_GAME_MINUTE != 0 {
        return;
    }
    let strength = ctx.state.weather.intensity;
    for id in living_ids(ctx.state) {
        let c = ctx.cr(&id);
        if c.inside.is_some() || c.hp <= HAIL_FLOOR || !ctx.chance(HAIL_CHANCE * strength) {
            continue;
        }
        let amount = ctx.int_range(HAIL_DAMAGE[0], HAIL_DAMAGE[1]) as f64;
        let c = ctx.c(&id);
        c.hp = (c.hp - amount).max(HAIL_FLOOR);
        set_anim(c, AnimId::Hit, true);
        if !has_recent_stimulus(c, StimulusKind::Attacked, tick - minutes_to_ticks(5.0), None) {
            ctx.stimulate(&id, StimulusKind::Attacked, "Hailstones are pelting you.", 2, None);
        }
    }
}

/// While rain comes down hard: those who hate it leave off their work and leisure out in the open and go
/// in, a few at a time. Those whose minds chose what they are doing are only told about it.
fn rain(ctx: &mut Ctx) {
    let w = &ctx.state.weather;
    if w.kind != WeatherKind::Rain || w.intensity < SHELTER_RAIN {
        return;
    }
    let tick = ctx.tick();
    for (i, id) in living_ids(ctx.state).iter().enumerate() {
        if (tick + i as i64 * 7) % 40 != 0 {
            continue;
        }
        let c = ctx.cr(id);
        if c.inside.is_some()
            || c.asleep
            || c.convo_id.is_some()
            || !c.intent.as_ref().is_some_and(outdoor_routine)
            || !rain_drives_in(ctx.state, c, SHELTER_RAIN)
        {
            continue;
        }
        send_in(ctx, id, IntentSource::Routine);
    }
}

/// Each tick: the sky moves on once a game minute, and the village answers to it.
pub fn update_weather(ctx: &mut Ctx) {
    if ctx.tick() % TICKS_PER_GAME_MINUTE == 0 {
        advance_sky(ctx);
    }
    notice(ctx);
    hail(ctx);
    rain(ctx);
}
