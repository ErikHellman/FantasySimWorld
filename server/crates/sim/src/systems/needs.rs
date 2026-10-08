//! Hunger, tiredness, loneliness and slow healing.

use fsw_core::time::{GAME_MINUTES_PER_TICK, minutes_to_ticks};

use crate::state::StimulusKind;
use crate::systems::combat::max_hp;
use crate::systems::weather::{MOOD_PACE, mood};
use crate::util::{Ctx, has_recent_stimulus, living_ids};

/// Change per game minute while awake. A full belly lasts about 15 hours, a night's sleep about 18.
const HUNGER_PER_MINUTE: f64 = -0.11;
const ENERGY_PER_MINUTE: f64 = -0.092;
const SOCIAL_PER_MINUTE: f64 = -0.075;
const HP_PER_MINUTE: f64 = 0.03;
/// Heartened by music (`systems::music`): tiredness comes on slower, and hurts mend faster.
const HEARTENED_ENERGY_FACTOR: f64 = 0.75;
const HEARTENED_HP_PER_MINUTE: f64 = 0.02;

fn warn(ctx: &mut Ctx, id: &str, text: &str) {
    let tick = ctx.tick();
    if !has_recent_stimulus(ctx.cr(id), StimulusKind::Need, tick - minutes_to_ticks(90.0), None) {
        ctx.stimulate(id, StimulusKind::Need, text, 1, None);
    }
}

pub fn update_needs(ctx: &mut Ctx) {
    let dt = GAME_MINUTES_PER_TICK;
    let tick = ctx.tick();
    let lifetime = minutes_to_ticks(100.0);
    for id in living_ids(ctx.state) {
        // Weather they love keeps their spirits up; weather they hate wears them down.
        let weather = 1.0 - MOOD_PACE * mood(ctx.state, ctx.cr(&id));
        let c = ctx.c(&id);
        c.needs.hunger = (c.needs.hunger + HUNGER_PER_MINUTE * dt * if c.asleep { 0.5 } else { 1.0 }).max(0.0);
        let heartened = c.heartened(tick);
        if !c.asleep {
            let pace = if heartened { HEARTENED_ENERGY_FACTOR } else { 1.0 } * weather;
            c.needs.energy = (c.needs.energy + ENERGY_PER_MINUTE * dt * pace).max(0.0);
            let lonely = SOCIAL_PER_MINUTE * dt * (0.5 + c.profile.sociability) * weather;
            c.needs.social = (c.needs.social + lonely).max(0.0);
        }
        let fighting = c.intent.as_ref().is_some_and(|i| i.is_fight());
        if !fighting && c.needs.hunger > 20.0 {
            let heal = HP_PER_MINUTE + if heartened { HEARTENED_HP_PER_MINUTE } else { 0.0 };
            c.hp = max_hp(c).min(c.hp + heal * dt);
        }

        if tick % 40 != 0 {
            continue;
        }
        // What happened hours ago is memory, not news: it no longer calls for a reaction.
        if !c.stimuli.is_empty() {
            c.stimuli.retain(|stimulus| tick - stimulus.tick < lifetime);
        }
        if c.asleep {
            continue;
        }
        if c.needs.hunger < 18.0 {
            warn(ctx, &id, "You are very hungry.");
        } else if c.needs.energy < 14.0 {
            warn(ctx, &id, "You are exhausted and need sleep.");
        } else if c.needs.social < 12.0 && c.profile.sociability > 0.4 {
            warn(ctx, &id, "You feel lonely and want company.");
        }
    }
}
