//! The dragon. Once in a long while, by day, it comes over the hills, sets one
//! to three homes alight, burns whoever it catches in the open, and flies off
//! again. Only those with a bow, a crossbow or magic can reach it in the air;
//! everyone else runs for cover. It is hard to bring down, but it can be killed.
//!
//! When it is next due is worked out from a hash of the world's seed and the
//! day, as loot is, so nothing here draws from the RNG until a dragon actually
//! comes, and a world that has not seen one yet runs as it did before.

use std::f64::consts::{PI, TAU};

use fsw_core::jsmath::{cos, sin};
use fsw_core::math2d::{dist, facing_to};
use fsw_core::rng::hash_string;
use fsw_core::time::{TICK_SECONDS, minutes_to_ticks, seconds_to_ticks};
use fsw_core::{Vec2, vec2};
use fsw_world::places::{get_place, homes};

use crate::data::anims::AnimId;
use crate::data::monsters::MonsterKind;
use crate::events::{Projectile, SimEvent};
use crate::state::{ChronicleKind, FightMode, Id, MonsterOrigin, MonsterPhase, Raid, RaidPhase, SimState};
use crate::systems::combat::{Assailant, damage_character};
use crate::systems::director::tick_at;
use crate::systems::fire::{MAX_FIRES, burning, burning_sites, ignite_from_above};
use crate::systems::monsters::{MAX_MONSTERS, spawn_monster};
use crate::systems::movement::{face, step_toward};
use crate::systems::world::building_whole;
use crate::util::{Ctx, living, nearby_characters, set_anim};

/// No dragon comes before this day, nor to a village of fewer than this many.
pub const FIRST_DAY: i64 = 8;
const MIN_ALIVE: usize = 6;
/// A dragon an admin calls needs only this many in the village.
const CALLED_MIN_ALIVE: usize = 3;
/// The first visit falls within this many days of `FIRST_DAY`; later ones are this many days apart.
const FIRST_SPREAD_DAYS: u32 = 10;
const GAP_DAYS: [i64; 2] = [15, 30];
/// It comes by day, between these hours.
pub const HOURS: [f64; 2] = [10.5, 16.5];
/// The middle of the map, and how far out from it the dragon appears and leaves.
const CENTRE: Vec2 = vec2(129.5, 85.0);
const EDGE: f64 = 150.0;
/// Heights it keeps, in metres: cruising in and out, hanging over the village, and swooping on a home.
const CRUISE: f64 = 22.0;
const HOVER: f64 = 10.0;
const SWOOP: f64 = 7.0;
/// How far out from a home it starts its swoop.
const SWOOP_FROM: f64 = 30.0;
/// How fast it climbs and drops, in metres a second.
const CLIMB: f64 = 5.0;
/// How long it hangs over the village once its homes are alight, in seconds, and how much longer it stays to
/// fight back while someone below is shooting at it.
const HOVER_SECONDS: [f64; 2] = [6.0, 10.0];
const DUEL_SECONDS: f64 = 8.0;
/// How long it holds still over a home it has set alight, for the breath to play out.
const AFTER_PASS: f64 = 0.9;
/// The slow circle it hangs in, and how fast it goes round.
const HOVER_RADIUS: f64 = 6.0;
const HOVER_SPEED: f64 = 4.0;
/// How far from the spot below it it can breathe on someone as it hovers.
const BREATH_REACH: f64 = 12.0;
/// How long into the breath the fire lands, in seconds: when the jaw is open.
const BREATH_LANDS: f64 = 0.75;
/// Below this share of its health it gives up and flies off, labouring at this share of its speed: slow enough
/// for those on the ground to run after it and finish it.
const FLEE_HP: f64 = 0.3;
const WOUNDED_SPEED: f64 = 0.45;
/// It falls this fast when it dies, and lies where it fell this long.
const FALL: f64 = 9.0;
const CARCASS_SECONDS: f64 = 60.0;

fn roll(seed: u32, what: &str, day: i64) -> u32 {
    hash_string(&format!("dragon:{what}:{seed}:{day}"))
}

/// The tick of a visit on `day`, at an hour the seed fixes.
fn visit_on(seed: u32, day: i64) -> i64 {
    let share = f64::from(roll(seed, "hour", day) % 1000) / 1000.0;
    tick_at(day, HOURS[0] + share * (HOURS[1] - HOURS[0]))
}

/// The dragon abroad, if there is one (dying or not).
pub fn dragon(state: &SimState) -> Option<&crate::state::Monster> {
    state.monsters.values().find(|m| m.kind == MonsterKind::Dragon)
}

/// Whether a home still stands to be burned.
fn standing(state: &SimState, home: &str) -> bool {
    !state.houses.contains_key(home) && building_whole(state, home)
}

fn may_come(ctx: &Ctx, least_alive: usize) -> bool {
    let state = &*ctx.state;
    (HOURS[0]..HOURS[1]).contains(&ctx.time.hour)
        && living(state).count() >= least_alive
        && burning(state).is_none()
        && burning_sites(state).len() < MAX_FIRES
        && dragon(state).is_none()
        && state.monsters.len() < MAX_MONSTERS
        && homes().iter().any(|home| standing(state, &home.id))
}

/// An admin's call: the dragon comes as soon as it may. False while the village is being founded, while it is
/// abroad or when it is already called.
pub fn call_dragon(ctx: &mut Ctx) -> bool {
    let state = &*ctx.state;
    if state.director.founding || state.director.dragon_called || dragon(state).is_some() {
        return false;
    }
    ctx.state.director.dragon_called = true;
    true
}

/// Works out when the dragon is next due, and sends it when it is. Called by the director once the
/// village is settled.
pub fn update_schedule(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let (seed, day, hour) = (ctx.state.seed, ctx.time.day, ctx.time.hour);
    let next = ctx.state.director.next_dragon_tick;
    if next == 0 {
        let first = (FIRST_DAY + i64::from(roll(seed, "first", 0) % FIRST_SPREAD_DAYS)).max(day);
        ctx.state.director.next_dragon_tick = visit_on(seed, first);
        return;
    }
    // A called dragon waits for the first chance and leaves the schedule as it was.
    if ctx.state.director.dragon_called && may_come(ctx, CALLED_MIN_ALIVE) {
        ctx.state.director.dragon_called = false;
        come(ctx);
        return;
    }
    if tick < next {
        return;
    }
    if may_come(ctx, MIN_ALIVE) {
        come(ctx);
        let gap = GAP_DAYS[0] + i64::from(roll(seed, "gap", day) % (GAP_DAYS[1] - GAP_DAYS[0] + 1) as u32);
        ctx.state.director.next_dragon_tick = visit_on(seed, day + gap);
        return;
    }
    // Not now: an hour later while the day allows, else the next day.
    ctx.state.director.next_dragon_tick = if hour < HOURS[0] {
        visit_on(seed, day)
    } else if hour + 1.0 < HOURS[1] {
        tick + minutes_to_ticks(60.0)
    } else {
        visit_on(seed, day + 1)
    };
}

/// The homes it goes for: those still standing nearest where it comes in, lived-in ones first, in
/// the order it reaches them.
fn pick_targets(state: &SimState, from: Vec2, count: usize) -> Vec<String> {
    let mut candidates: Vec<(bool, f64, &str)> = homes()
        .iter()
        .filter(|home| standing(state, &home.id))
        .map(|home| (!living(state).any(|c| c.home_id == home.id), dist(home.door, from), home.id.as_str()))
        .collect();
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(b.2)));
    candidates.truncate(count);
    // Then the nearest of those each time, starting from where it comes in.
    let mut order = Vec::new();
    let mut at = from;
    while !candidates.is_empty() {
        let (index, _) = candidates
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                dist(get_place(a.2).door, at).total_cmp(&dist(get_place(b.2).door, at)).then(a.2.cmp(b.2))
            })
            .expect("not empty");
        let (_, _, home) = candidates.remove(index);
        at = get_place(home).door;
        order.push(home.to_owned());
    }
    order
}

/// The dragon comes over the hills.
pub fn come(ctx: &mut Ctx) -> Option<Id> {
    let angle = ctx.range(0.0, TAU);
    let from = vec2(CENTRE.x + cos(angle) * EDGE, CENTRE.z + sin(angle) * EDGE);
    let away = angle + PI + ctx.range(-0.8, 0.8);
    let exit = vec2(CENTRE.x + cos(away) * EDGE, CENTRE.z + sin(away) * EDGE);
    let room = MAX_FIRES.saturating_sub(burning_sites(ctx.state).len());
    let count = (ctx.int_range(1, 3) as usize).min(room);
    let targets = pick_targets(ctx.state, from, count);
    let first = targets.first()?;
    let facing = facing_to(from, get_place(first).door);
    let id = spawn_monster(ctx, MonsterKind::Dragon, from, facing, MonsterOrigin::Sky);
    let m = ctx.m(&id);
    m.phase = MonsterPhase::Roam;
    m.alt = CRUISE;
    m.raid = Some(Raid {
        phase: RaidPhase::Strafe,
        targets,
        done: 0,
        hover: from,
        exit,
        next_breath_tick: 0,
        until_tick: 0,
        last_tick: 0,
        aim: None,
        aim_home: None,
    });
    set_anim(m, AnimId::DragonFly, true);
    ctx.chronicle(ChronicleKind::World, "A dragon came over the hills, flying for the village!", Vec::new());
    ctx.emit(SimEvent::SavePoint { reason: "dragon".into() });
    Some(id)
}

/// Eases towards a height at the dragon's climbing rate.
fn climb_to(alt: f64, goal: f64) -> f64 {
    let step = CLIMB * TICK_SECONDS;
    if (goal - alt).abs() <= step { goal } else { alt + step * (goal - alt).signum() }
}

/// Someone in the open below to breathe on: whoever last shot at it if they are in reach, else the nearest.
fn prey(state: &SimState, id: &str) -> Option<(Id, Vec2)> {
    let m = &state.monsters[id];
    let shooter = m.target_id.as_deref().and_then(|t| state.characters.get(t));
    if let Some(c) = shooter.filter(|c| c.inside.is_none() && dist(c.pos, m.pos) <= BREATH_REACH * 1.5) {
        return Some((c.id.clone(), c.pos));
    }
    nearby_characters(state, m.pos, BREATH_REACH, None)
        .into_iter()
        .map(|c| (dist(state.characters[&c].pos, m.pos), c))
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, c)| {
            let pos = state.characters[&c].pos;
            (c, pos)
        })
}

/// Rears back to breathe at `aim`; the fire lands `BREATH_LANDS` later.
fn draw_breath(ctx: &mut Ctx, id: &str, raid: &mut Raid, aim: Vec2, home: Option<String>) {
    let tick = ctx.tick();
    let m = ctx.m(id);
    set_anim(m, AnimId::DragonBreath, true);
    m.hit_tick = tick + seconds_to_ticks(BREATH_LANDS);
    raid.aim = Some(aim);
    raid.aim_home = home;
}

/// The fire lands: everyone in the open under it is burned, and a home it swooped on catches.
fn breathe(ctx: &mut Ctx, id: &str, raid: &mut Raid) {
    let tick = ctx.tick();
    let Some(aim) = raid.aim.take() else { return };
    let home = raid.aim_home.take();
    let def = MonsterKind::Dragon.def();
    let from = ctx.state.monsters[id].pos;
    ctx.emit(SimEvent::Attack {
        attacker_id: id.into(),
        target_id: home.clone().unwrap_or_default(),
        ranged: Some(Projectile::Fire),
        from,
        to: aim,
    });
    for victim in nearby_characters(ctx.state, aim, def.range, None) {
        let damage = ctx.int_range(def.damage[0], def.damage[1]);
        let by = Assailant { id: id.into(), label: def.label.into(), character: None };
        damage_character(ctx, &victim, damage, by, FightMode::Monster);
        let slain = !ctx.state.characters.contains_key(&victim);
        let m = ctx.m(id);
        m.blows += 1;
        if slain {
            m.kills += 1;
        }
    }
    raid.next_breath_tick = tick + seconds_to_ticks(def.interval);
    let Some(home) = home else { return };
    if standing(ctx.state, &home) {
        ignite_from_above(ctx, &home);
    }
    raid.done += 1;
    raid.until_tick = tick + seconds_to_ticks(AFTER_PASS);
    if raid.done < raid.targets.len() {
        return;
    }
    // The last home is alight: it hangs over the village a while, between that home and the green, then goes.
    let door = get_place(&home).door;
    let green = get_place("green").door;
    let seconds = ctx.range(HOVER_SECONDS[0], HOVER_SECONDS[1]);
    raid.phase = RaidPhase::Hover;
    raid.hover = vec2((door.x + green.x) / 2.0, (door.z + green.z) / 2.0);
    raid.until_tick = tick + seconds_to_ticks(seconds);
    raid.last_tick = raid.until_tick + seconds_to_ticks(DUEL_SECONDS);
}

/// Whether someone in the open close below is fighting it.
fn harried(state: &SimState, id: &str) -> bool {
    let m = &state.monsters[id];
    living(state).any(|c| {
        c.inside.is_none()
            && dist(c.pos, m.pos) <= BREATH_REACH * 1.5
            && c.intent.as_ref().is_some_and(|i| i.fights(id))
    })
}

/// The dragon is gone over the hills.
fn gone(ctx: &mut Ctx, id: &str) {
    let Some(m) = ctx.state.monsters.shift_remove(id) else { return };
    let max = m.kind.def().max_hp;
    let text = if m.hp >= max {
        "The dragon flew off over the hills, untouched."
    } else if m.hp < max * FLEE_HP {
        "The dragon fled over the hills, badly wounded."
    } else {
        "The dragon flew off over the hills, wounded."
    };
    ctx.chronicle(ChronicleKind::World, text, Vec::new());
    ctx.emit(SimEvent::SavePoint { reason: "dragon gone".into() });
}

/// One tick of the dragon's visit.
pub fn update_dragon(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    let def = MonsterKind::Dragon.def();
    let m = ctx.m(id);
    m.moving = false;
    if m.phase == MonsterPhase::Dying {
        m.alt = (m.alt - FALL * TICK_SECONDS).max(0.0);
        if tick - m.since_tick > seconds_to_ticks(CARCASS_SECONDS) {
            ctx.state.monsters.shift_remove(id);
        }
        return;
    }
    let Some(mut raid) = m.raid.take() else {
        ctx.state.monsters.shift_remove(id);
        return;
    };

    // Hurt badly enough, it gives up on the rest of the village.
    if raid.phase != RaidPhase::Leave && m.hp < def.max_hp * FLEE_HP && m.hit_tick == 0 {
        raid.phase = RaidPhase::Leave;
        raid.aim = None;
    }

    // A breath already drawn lands first; it hangs where it is until then.
    if m.hit_tick > 0 {
        if let Some(aim) = raid.aim {
            face(m, aim);
        }
        if tick >= m.hit_tick {
            m.hit_tick = 0;
            breathe(ctx, id, &mut raid);
        }
        if let Some(m) = ctx.state.monsters.get_mut(id) {
            m.raid = Some(raid);
        }
        return;
    }

    match raid.phase {
        // Holding still over the home it has just set alight, while the breath plays out.
        RaidPhase::Strafe if tick < raid.until_tick => {}
        RaidPhase::Strafe => {
            let Some(home) = raid.targets.get(raid.done).cloned() else {
                raid.phase = RaidPhase::Leave;
                m.raid = Some(raid);
                return;
            };
            // A home that has caught or fallen since is passed over.
            if !standing(ctx.state, &home) {
                raid.done += 1;
                ctx.m(id).raid = Some(raid);
                return;
            }
            let door = get_place(&home).door;
            let m = ctx.m(id);
            let d = dist(m.pos, door);
            m.alt = climb_to(m.alt, if d < SWOOP_FROM { SWOOP } else { CRUISE });
            set_anim(m, AnimId::DragonFly, false);
            if d > 1.0 {
                step_toward(m, door, def.speed * if d < SWOOP_FROM { 0.6 } else { 1.0 });
            } else if (m.alt - SWOOP).abs() < 0.5 {
                draw_breath(ctx, id, &mut raid, door, Some(home));
            }
        }
        RaidPhase::Hover => {
            // A slow circle over the spot, each dragon starting at its own place on it.
            let turn = tick as f64 * TICK_SECONDS * HOVER_SPEED / HOVER_RADIUS + f64::from(hash_string(id) % 360);
            let point = vec2(raid.hover.x + cos(turn) * HOVER_RADIUS, raid.hover.z + sin(turn) * HOVER_RADIUS);
            m.alt = climb_to(m.alt, HOVER);
            step_toward(m, point, def.speed * 0.5);
            set_anim(m, AnimId::DragonHover, false);
            if tick >= raid.until_tick && (tick >= raid.last_tick || !harried(ctx.state, id)) {
                raid.phase = RaidPhase::Leave;
            } else if tick >= raid.next_breath_tick {
                match prey(ctx.state, id) {
                    Some((_, pos)) => draw_breath(ctx, id, &mut raid, pos, None),
                    None => raid.next_breath_tick = tick + seconds_to_ticks(1.0),
                }
            }
        }
        RaidPhase::Leave => {
            let wounded = m.hp < def.max_hp * FLEE_HP;
            m.alt = climb_to(m.alt, if wounded { HOVER } else { CRUISE });
            set_anim(m, AnimId::DragonFly, false);
            if step_toward(m, raid.exit, def.speed * if wounded { WOUNDED_SPEED } else { 1.0 }) {
                gone(ctx, id);
                return;
            }
        }
    }
    if let Some(m) = ctx.state.monsters.get_mut(id) {
        m.raid = Some(raid);
    }
}
