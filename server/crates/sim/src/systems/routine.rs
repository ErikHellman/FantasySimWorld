//! The local brain. It gives every character a daily plan and, whenever they
//! have nothing to do, the next thing to do. It never waits for anything, so
//! the village lives with no LLM at all; LLM decisions simply replace its
//! choices when they arrive.

use fsw_core::math2d::dist;
use fsw_core::rng;
use fsw_core::time::{TICKS_PER_DAY, minutes_to_ticks};
use fsw_world::places::ActivityId;

use crate::data::activities::activity;
use crate::state::{FightMode, Id, Intent, IntentAction, IntentSource, PlanBlock, PlanSource, StimulusKind, TalkTone};
use crate::systems::charm;
use crate::systems::combat::{max_hp, start_fight};
use crate::systems::conversation::{chatter_roll, talked_recently};
use crate::systems::intents::{drink_potion_if_hurt, fresh_act};
use crate::systems::relationships::{RUNG_GRUDGE, brawl_unlocked, lethal_unlocked};
use crate::util::{Ctx, first_name, is_approachable, living, living_ids, rel, rel_or_default};

const SOCIAL_RADIUS: f64 = 15.0;

/// A plausible day for this character, from their jobs and hobbies.
pub fn routine_plan(ctx: &mut Ctx, id: &str) -> Vec<PlanBlock> {
    let c = ctx.cr(id);
    let jobs = if c.jobs.is_empty() { vec![ActivityId::Stroll] } else { c.jobs.clone() };
    let hobbies = if c.hobbies.is_empty() { vec![ActivityId::Stroll] } else { c.hobbies.clone() };
    let sociability = c.profile.sociability;
    let wake = 5.8 + ctx.next_float() * 1.4;
    let morning_job = ctx.pick(&jobs);
    let afternoon_job = if ctx.chance(0.6) { ctx.pick(&jobs) } else { ctx.pick(&hobbies) };
    let leisure = ctx.pick(&hobbies);
    let evening = if sociability > 0.35 || ctx.chance(0.4) { ActivityId::Drink } else { ctx.pick(&hobbies) };
    let block = |hour: f64, activity: ActivityId, note: Option<&str>| PlanBlock {
        hour,
        activity,
        place_id: None,
        note: note.map(str::to_owned),
    };
    let midday = 12.0 + ctx.next_float() * 0.6;
    let afternoon = 13.2 + ctx.next_float() * 0.4;
    let late = 16.6 + ctx.next_float() * 0.6;
    let supper = 18.4 + ctx.next_float() * 0.5;
    let night = 19.3 + ctx.next_float() * 0.4;
    let bed = 21.4 + ctx.next_float() * 1.1;
    vec![
        block(wake, ActivityId::Eat, Some("breakfast")),
        block(wake + 0.9, morning_job, None),
        block(midday, ActivityId::Eat, Some("midday meal")),
        block(afternoon, afternoon_job, None),
        block(late, leisure, None),
        block(supper, ActivityId::Eat, Some("supper")),
        block(night, evening, None),
        block(bed, ActivityId::Sleep, None),
    ]
}

/// Index of the plan block in force at `hour` (-1 before the first one).
pub fn block_at(plan: &[PlanBlock], hour: f64) -> i64 {
    let mut index = -1;
    for (i, block) in plan.iter().enumerate() {
        if block.hour <= hour {
            index = i as i64;
        }
    }
    index
}

fn minutes_until_next_block(plan: &[PlanBlock], index: i64, hour: f64) -> f64 {
    // The last block (sleep) runs past midnight.
    let end_hour = plan.get((index + 1) as usize).map_or(30.0, |next| next.hour);
    ((end_hour - hour) * 60.0).max(10.0)
}

fn act(activity: ActivityId, minutes: Option<f64>, place_id: Option<String>) -> Intent {
    Intent::new(IntentAction::Activity { activity, place_id, minutes }, IntentSource::Routine)
}

/// Someone nearby worth walking over to, and in what spirit.
fn social_intent(ctx: &mut Ctx, id: &str) -> Option<Intent> {
    let tick = ctx.tick();
    if tick < ctx.cr(id).next_social_tick || !chatter_roll(ctx, id) {
        return None;
    }
    let pos = ctx.cr(id).pos;
    let near: Vec<Id> = living(ctx.state)
        .filter(|o| o.id != id && is_approachable(o) && dist(o.pos, pos) <= SOCIAL_RADIUS && tick >= o.next_social_tick)
        .map(|o| o.id.clone())
        .collect();
    let mut candidates = Vec::new();
    for other in near {
        if !talked_recently(ctx, id, &other, 180.0) {
            candidates.push(other);
        }
    }
    let allure = ctx.cr(id).profile.allure;
    let weights: Vec<f64> = candidates
        .iter()
        .map(|o| {
            let c = ctx.c(id);
            let spite = c.profile.spite;
            let r = rel(c, o);
            // The spiteful are drawn twice as hard to the people they resent.
            let weight = 1.0 + r.affinity.abs() / 25.0 + r.attraction / 30.0 + r.grievance * (1.0 + spite) / 12.0;
            // A charmer seeks out whoever wants her. Added only for one, so nobody else's weights change.
            if allure > 0.0 { weight + allure * rel_or_default(ctx.cr(o), id).attraction / 20.0 } else { weight }
        })
        .collect();
    let index = rng::weighted_pick_index(&mut ctx.state.rng, &weights, |w| *w)?;
    let other = candidates[index].clone();

    let c = ctx.c(id);
    let partner = c.partner_id.clone();
    let (temper, spite) = (c.profile.temper, c.profile.spite);
    let r = rel(c, &other).clone();
    let mut tone = TalkTone::Friendly;
    if r.grievance >= RUNG_GRUDGE && ctx.chance(0.35 + temper * 0.5) {
        tone = TalkTone::Hostile;
    } else if spite > 0.0 && !r.kin && r.affinity < 40.0 && ctx.chance(0.5 * spite) {
        // Picking a quarrel with anyone who is not a real friend, grudge or no grudge. Only rolled
        // for the spiteful, so everyone else draws from the RNG exactly as before.
        tone = TalkTone::Hostile;
    } else if r.grievance >= 22.0
        && r.affinity > -30.0
        && spite < 1.0
        && ctx.chance((0.12 + (1.0 - temper) * 0.25) * (1.0 - spite))
    {
        tone = TalkTone::Apology;
    } else if !r.kin
        && r.attraction >= 50.0
        && r.affinity >= 35.0
        && partner.as_ref().is_none_or(|p| p == &other)
        && ctx.chance(0.3)
    {
        tone = TalkTone::Romantic;
    } else if allure > 0.0
        && !r.kin
        && r.grievance < RUNG_GRUDGE
        && rel_or_default(ctx.cr(&other), id).attraction >= charm::MARK
        && ctx.chance(allure * 0.5)
    {
        // A charmer flirts with anyone already a little drawn to her, partners or no partners.
        // Checked last and only for a charmer, so nobody else draws from the RNG differently.
        tone = TalkTone::Romantic;
    }
    ctx.c(id).next_social_tick = tick + minutes_to_ticks(40.0);
    Some(Intent::new(IntentAction::Talk { target_id: other, opener: String::new(), tone }, IntentSource::Routine))
}

/// Without an LLM to hold them back, a long feud eventually comes to blows on its own.
fn hostile_intent(ctx: &mut Ctx, id: &str) -> bool {
    let tick = ctx.tick();
    let c = ctx.cr(id);
    if c.hp < max_hp(c) * 0.6 {
        return false;
    }
    let (pos, temper) = (c.pos, c.profile.temper);
    for other in living_ids(ctx.state) {
        let o = ctx.cr(&other);
        if other == id || !is_approachable(o) || dist(o.pos, pos) > 18.0 {
            continue;
        }
        let name = first_name(o).to_owned();
        let c = ctx.c(id);
        let spite = c.profile.spite;
        let r = rel(c, &other).clone();
        if !brawl_unlocked(c, &other) {
            continue;
        }
        if lethal_unlocked(ctx, id, &other) && ctx.chance(0.04 * temper) {
            start_fight(
                ctx,
                id,
                &other,
                FightMode::Lethal,
                IntentSource::Routine,
                Some(format!("{name}! This ends now!")),
            );
            return true;
        }
        if tick - r.last_offence_tick < TICKS_PER_DAY * 2 && ctx.chance(0.07 * temper + 0.06 * spite) {
            let line = format!("I have had enough of you, {name}!");
            start_fight(ctx, id, &other, FightMode::Brawl, IntentSource::Routine, Some(line));
            return true;
        }
    }
    false
}

/// Picks what a character with nothing to do does next; `None` when it started a fight directly.
fn choose(ctx: &mut Ctx, id: &str) -> Option<Intent> {
    let tick = ctx.tick();
    let hour = ctx.time.hour;
    let c = ctx.cr(id);

    let has_watch_tonight = tick < c.watch_until_tick;
    let on_watch = has_watch_tonight && !(5.2..20.5).contains(&hour);

    // Starving comes before anything, even duty. A guard with food in the pack eats before the next
    // stretch of watch rather than after: one stretch can burn more than the last 12 points of hunger.
    if c.needs.hunger < 12.0 || (c.needs.hunger < 24.0 && (!on_watch || c.inventory.food > 0)) {
        let hunger = c.needs.hunger;
        if c.inventory.food > 0 && (on_watch || hunger < 10.0 || ctx.chance(0.4)) {
            let c = ctx.c(id);
            c.inventory.food -= 1;
            c.needs.hunger = (c.needs.hunger + 45.0).min(100.0);
            return Some(Intent::new(IntentAction::Wait { minutes: 6.0 }, IntentSource::Routine));
        }
        return Some(act(ActivityId::Eat, None, None));
    }
    if on_watch {
        let minutes = ((c.watch_until_tick - tick) as f64 / 20.0).clamp(5.0, 150.0);
        return Some(act(ActivityId::Watch, Some(minutes), None));
    }
    // After a night on watch, sleep in; before one, take an afternoon nap.
    if !has_watch_tonight && tick < c.sleep_in_until_tick && c.needs.energy < 90.0 {
        return Some(act(ActivityId::Sleep, Some(((c.sleep_in_until_tick - tick) as f64 / 20.0).max(20.0)), None));
    }
    if has_watch_tonight && (13.5..18.5).contains(&hour) && c.needs.energy < 88.0 {
        return Some(act(ActivityId::Sleep, Some((18.6 - hour) * 60.0), None));
    }
    if c.needs.energy < 9.0 {
        return Some(act(ActivityId::Sleep, Some(300.0), None));
    }
    if c.hp < max_hp(c) * 0.5 {
        drink_potion_if_hurt(ctx, id);
        let c = ctx.cr(id);
        if c.hp < max_hp(c) * 0.5 {
            return Some(act(ActivityId::Rest, Some(45.0), Some(c.home_id.clone())));
        }
    }

    if hostile_intent(ctx, id) {
        return None;
    }
    if let Some(social) = social_intent(ctx, id) {
        return Some(social);
    }

    let c = ctx.cr(id);
    let index = block_at(&c.plan, hour);
    let first_hour = c.plan.first().map_or(6.0, |block| block.hour);
    if index < 0 {
        // Before the day's first block: stay asleep, or potter about if already up.
        return Some(if hour < 5.0 {
            act(ActivityId::Sleep, Some(first_hour * 60.0 - hour * 60.0), None)
        } else {
            act(ActivityId::Rest, Some(20.0), None)
        });
    }
    let block = c.plan[index as usize].clone();
    let remaining = minutes_until_next_block(&c.plan, index, hour);
    if block.activity == ActivityId::Sleep {
        let minutes = (((24.0 - hour) % 24.0) * 60.0 + first_hour * 60.0).max(60.0);
        return Some(act(ActivityId::Sleep, Some(minutes), None));
    }
    if block.activity == ActivityId::Eat && c.needs.hunger > 78.0 {
        return Some(act(ActivityId::Stroll, Some(remaining.min(25.0)), None));
    }
    let def = activity(block.activity);
    let minutes = remaining.min(ctx.int_range(def.minutes[0], def.minutes[1]) as f64);
    Some(act(block.activity, Some(minutes), block.place_id))
}

pub fn update_routine(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let (day, hour) = (ctx.time.day, ctx.time.hour);
    for id in living_ids(ctx.state) {
        if !ctx.alive(&id) {
            continue;
        }
        // A fresh plan each morning (an LLM plan for the day, if one arrives, replaces it).
        if ctx.cr(&id).plan_day != day && hour >= 4.5 {
            let plan = routine_plan(ctx, &id);
            let c = ctx.c(&id);
            c.plan = plan;
            c.plan_day = day;
            c.plan_source = PlanSource::Routine;
            c.plan_block = -1;
        }

        let c = ctx.c(&id);
        let index = block_at(&c.plan, hour);
        if index != c.plan_block {
            c.plan_block = index;
            let block = usize::try_from(index).ok().and_then(|i| c.plan.get(i)).cloned();
            if let Some(block) = &block
                && !c.asleep
            {
                let note =
                    block.note.as_ref().filter(|n| !n.is_empty()).map_or(String::new(), |note| format!(" ({note})"));
                let text = format!(
                    "It is time for the next part of your day: you had planned to {}{note}.",
                    activity(block.activity).label
                );
                ctx.stimulate(&id, StimulusKind::BlockEnded, text, 0, None);
            }
            // A routine activity from the previous block gives way to the new one.
            let c = ctx.c(&id);
            if let Some(Intent {
                action: IntentAction::Activity { activity: current, .. },
                source: IntentSource::Routine,
                ..
            }) = &c.intent
                && c.act.phase == crate::state::ActPhase::Perform
                && c.convo_id.is_none()
            {
                let current = *current;
                let next = block.as_ref().map(|b| b.activity);
                let sleeping_on =
                    current == ActivityId::Sleep && next != Some(ActivityId::Sleep) && c.needs.energy < 60.0;
                let on_duty = current == ActivityId::Watch && tick < c.watch_until_tick;
                if !sleeping_on && !on_duty && Some(current) != next {
                    c.act.until_tick = c.act.until_tick.min(tick);
                }
            }
        }

        // Watch duty starts on time, whatever the plan said.
        let c = ctx.c(&id);
        let due_on_watch = tick < c.watch_until_tick && !(5.2..20.5).contains(&hour);
        if due_on_watch
            && let Some(Intent {
                action: IntentAction::Activity { activity, .. }, source: IntentSource::Routine, ..
            }) = &c.intent
            && *activity != ActivityId::Watch
            && c.act.phase == crate::state::ActPhase::Perform
        {
            c.act.until_tick = c.act.until_tick.min(tick);
        }

        if c.intent.is_some() || !c.queue.is_empty() || c.convo_id.is_some() || c.combat.yielded {
            continue;
        }
        let Some(intent) = choose(ctx, &id) else { continue };
        let c = ctx.c(&id);
        if c.intent.is_some() {
            continue; // choose() may have started a fight directly
        }
        c.intent = Some(intent);
        c.act = fresh_act(tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_at_finds_the_block_in_force() {
        let block = |hour| PlanBlock { hour, activity: ActivityId::Eat, place_id: None, note: None };
        let plan = vec![block(6.0), block(9.0), block(22.0)];
        assert_eq!(block_at(&plan, 5.0), -1);
        assert_eq!(block_at(&plan, 9.0), 1);
        assert_eq!(block_at(&plan, 23.0), 2);
    }
}
