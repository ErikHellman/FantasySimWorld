//! Keeps the world neither empty nor quiet: night attacks, the watch rota, and newcomers.

use fsw_core::jsmath::js_round;
use fsw_core::time::{MINUTES_PER_DAY, START_MINUTE, TICKS_PER_DAY, TICKS_PER_GAME_MINUTE, minutes_to_ticks};
use fsw_core::vec2;
use fsw_world::places::{ActivityId, get_place};

use crate::events::SimEvent;
use crate::state::{ChronicleKind, Id, MemoryKind, MonsterKind};
use crate::systems::arrivals::arrive;
use crate::systems::founding::NEWCOMER_PATIENCE_MINUTES;
use crate::systems::monsters::{MAX_MONSTERS, random_monster_kind, spawn_monster};
use crate::util::{Ctx, adult_count, first_name, join_names, living};

const MAX_WAVE: i64 = 7;

/// Tick at which a given hour of a given day begins.
pub fn tick_at(day: i64, hour: f64) -> i64 {
    js_round(
        ((day - 1) as f64 * MINUTES_PER_DAY as f64 + hour * 60.0 - START_MINUTE as f64) * TICKS_PER_GAME_MINUTE as f64,
    ) as i64
}

fn recent_deaths(ctx: &Ctx, days: i64) -> usize {
    let since = ctx.tick() - days * TICKS_PER_DAY;
    ctx.state.director.death_ticks.iter().filter(|&&tick| tick >= since).count()
}

fn spawn_wave(ctx: &mut Ctx) {
    let (day, hour) = (ctx.time.day, ctx.time.hour);
    let alive = adult_count(ctx.state) as i64;
    let night = if hour < 12.0 { day - 1 } else { day };
    let mut size = 2 + alive / 5 + (night / 4).min(2);
    if night == 1 {
        size = 3;
    }
    // A village still burying its dead gets a little breathing room.
    if recent_deaths(ctx, 3) >= 2 {
        size -= 2;
    }
    if alive < 8 {
        size -= 1;
    }
    size = size.clamp(1, MAX_WAVE);
    // `monsterList` counts the dying too.
    let monsters = ctx.state.monsters.len() as i64;
    size = size.min(MAX_MONSTERS as i64 - monsters);
    if size <= 0 {
        return;
    }

    let first_of_night = (19.0..22.6).contains(&hour);
    let from_ruins = night > 2 && ctx.chance(0.25);
    let place = get_place(if from_ruins { "ruins" } else { "graveyard" });
    let mut points = place.spawn_points.clone();
    // One necromancer abroad at a time: a fallen villager who walks with the dead counts.
    let necromancer_abroad = ctx.state.monsters.values().any(|m| m.lasting);
    let with_necromancer = first_of_night && night >= 4 && night % 4 == 0 && alive >= 9 && !necromancer_abroad;

    for i in 0..size {
        let point = if points.is_empty() {
            place.origin
        } else {
            let index = (ctx.next_float() * points.len() as f64).floor() as usize;
            points.remove(index)
        };
        let x = point.x + ctx.range(-0.6, 0.6);
        let z = point.z + ctx.range(-0.6, 0.6);
        let Some(pos) = ctx.state.nav.nearest_walkable(vec2(x, z), 5) else { continue };
        let kind = if with_necromancer && i == 0 { MonsterKind::Necromancer } else { random_monster_kind(ctx) };
        spawn_monster(ctx, kind, pos, place.facing);
    }
    if first_of_night {
        let text = if with_necromancer {
            "A cold light burns in the graveyard: a necromancer walks among the risen dead tonight."
        } else if from_ruins {
            "Something is stirring in the old ruins."
        } else {
            "As darkness falls, the graves in the old graveyard begin to stir."
        };
        ctx.chronicle(ChronicleKind::World, text, Vec::new());
    }
}

fn schedule_next_wave(ctx: &mut Ctx, spawned: bool) {
    let (day, hour) = (ctx.time.day, ctx.time.hour);
    let still_dark = !(1.5..19.5).contains(&hour);
    // Now and then a second wave follows later the same night.
    if spawned && still_dark && ctx.chance(0.4) {
        let minutes = ctx.range(100.0, 200.0);
        ctx.state.director.next_wave_tick = ctx.tick() + minutes_to_ticks(minutes);
        return;
    }
    // Otherwise the next attack comes on a later evening; some nights are quiet.
    let mut next = if hour < 12.0 { day } else { day + 1 };
    if ctx.chance(0.22) {
        next += 1;
    }
    let at = ctx.range(20.2, 22.4);
    ctx.state.director.next_wave_tick = tick_at(next, at);
}

/// Two of those who can stand watch take tonight's duty, in rotation.
fn assign_watch(ctx: &mut Ctx) {
    let day = ctx.time.day;
    ctx.state.director.watch_day = day;
    let able: Vec<Id> = living(ctx.state)
        .filter(|c| c.jobs.contains(&ActivityId::Watch) && c.hp > 30.0)
        .map(|c| c.id.clone())
        .collect();
    if able.is_empty() {
        return;
    }
    let count = able.len().min(2);
    let mut unique: Vec<Id> = Vec::new();
    for i in 0..count as i64 {
        let step = (able.len() as i64 / 2).max(1);
        let chosen = &able[((day + i * step).rem_euclid(able.len() as i64)) as usize];
        if !unique.contains(chosen) {
            unique.push(chosen.clone());
        }
    }
    for id in &unique {
        let c = ctx.c(id);
        c.watch_until_tick = tick_at(day + 1, 5.0);
        c.sleep_in_until_tick = tick_at(day + 1, 11.0);
        ctx.remember(
            id,
            "I have the night watch at the watchtower tonight. I should rest in the afternoon.",
            3.0,
            MemoryKind::Event,
            Vec::new(),
        );
    }
    let names: Vec<String> = unique.iter().map(|id| first_name(ctx.cr(id)).to_owned()).collect();
    let verb = if unique.len() > 1 { "have" } else { "has" };
    ctx.chronicle(ChronicleKind::World, format!("{} {verb} tonight's watch.", join_names(&names)), unique);
}

pub fn update_director(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let (day, hour) = (ctx.time.day, ctx.time.hour);

    if ctx.state.director.watch_day != day && hour >= 6.5 {
        assign_watch(ctx);
    }

    // A village still being settled is left in peace: no dead, no strangers on the road.
    if ctx.state.director.founding {
        return;
    }

    if tick >= ctx.state.director.next_wave_tick {
        let dark = !(4.0..19.5).contains(&hour);
        if dark {
            spawn_wave(ctx);
        }
        schedule_next_wave(ctx, dark);
    }

    if adult_count(ctx.state) < ctx.state.director.population_target
        && tick >= ctx.state.director.next_arrival_tick
        && (8.0..17.0).contains(&hour)
    {
        let director = &ctx.state.director;
        let deadline = director.newcomer_deadline_tick;
        if director.newcomer_draft.is_some() || (deadline != 0 && tick >= deadline) {
            arrive(ctx);
            let wait = ctx.range(0.8, 1.6);
            ctx.state.director.next_arrival_tick = tick + js_round(wait * TICKS_PER_DAY as f64) as i64;
        } else if deadline == 0 {
            // Ask the server to describe who comes; the road waits a few hours for the answer.
            let deadline_tick = tick + minutes_to_ticks(NEWCOMER_PATIENCE_MINUTES);
            ctx.state.director.newcomer_deadline_tick = deadline_tick;
            ctx.emit(SimEvent::NewcomerWanted { deadline_tick });
        }
    }

    if tick % 200 == 0 {
        let director = &mut ctx.state.director;
        director.death_ticks.retain(|&t| tick - t < TICKS_PER_DAY * 6);
        ctx.state.corpses.retain(|corpse| corpse.until_tick > tick);
    }
}

/// A grave to visit: used when someone who lost a friend has an idle moment.
pub fn has_someone_to_mourn(ctx: &Ctx, character_id: &str) -> bool {
    let Some(c) = ctx.state.characters.get(character_id) else { return false };
    ctx.state.graves.iter().any(|grave| {
        c.relationships.get(&grave.character_id).map_or(0.0, |r| r.affinity) >= 25.0 && ctx.time.day - grave.day <= 4
    })
}
