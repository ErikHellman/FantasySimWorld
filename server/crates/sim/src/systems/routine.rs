//! The local brain. It gives every character a daily plan and, whenever they
//! have nothing to do, the next thing to do. It never waits for anything, so
//! the village lives with no LLM at all; LLM decisions simply replace its
//! choices when they arrive.

use fsw_core::math2d::dist;
use fsw_core::rng;
use fsw_core::time::{TICKS_PER_DAY, minutes_to_ticks};
use fsw_world::places::ActivityId;

use crate::data::activities::{ActivityKind, activity};
use crate::state::{
    FightMode, Id, Intent, IntentAction, IntentSource, PlanBlock, PlanSource, Quirk, StimulusKind, TalkTone, WorldJob,
};
use crate::systems::billboards::wants_to_read;
use crate::systems::charm;
use crate::systems::combat::{max_hp, start_fight};
use crate::systems::conversation::{chatter_roll, talked_recently};
use crate::systems::fire::ruin_to_rebuild;
use crate::systems::intents::{drink_potion_if_hurt, fresh_act};
use crate::systems::quirks::{NAP_HOURS, NAP_NOTE, STUDY_NOTE};
use crate::systems::relationships::{RUNG_GRUDGE, RUNG_TENSION, brawl_unlocked, lethal_unlocked};
use crate::systems::world::{building_id, job_accepts, job_for_block};
use crate::util::{Ctx, first_name, is_approachable, living, living_ids, openness, rel, rel_or_default};

const SOCIAL_RADIUS: f64 = 15.0;

/// A plausible day for this character, from their jobs and hobbies.
pub fn routine_plan(ctx: &mut Ctx, id: &str) -> Vec<PlanBlock> {
    if ctx.cr(id).is_child() {
        return child_plan(ctx, id);
    }
    let c = ctx.cr(id);
    let jobs = if c.jobs.is_empty() { vec![ActivityId::Stroll] } else { c.jobs.clone() };
    let hobbies = if c.hobbies.is_empty() { vec![ActivityId::Stroll] } else { c.hobbies.clone() };
    let sociability = c.profile.sociability;
    let (lazy, diligent, drunkard, occultist) =
        (c.has(Quirk::Lazy), c.has(Quirk::Diligent), c.has(Quirk::Drunkard), c.has(Quirk::Occultist));
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
    let mut plan = vec![
        block(wake, ActivityId::Eat, Some("breakfast")),
        block(wake + 0.9, morning_job, None),
        block(midday, ActivityId::Eat, Some("midday meal")),
        block(afternoon, afternoon_job, None),
        block(late, leisure, None),
        block(supper, ActivityId::Eat, Some("supper")),
        block(night, evening, None),
        block(bed, ActivityId::Sleep, None),
    ];
    let nap = |hour: f64| PlanBlock { hour, activity: ActivityId::Rest, place_id: None, note: Some(NAP_NOTE.into()) };

    if diligent {
        // The afternoon and the hours others spend idle both go on work.
        plan[3].activity = ctx.pick(&jobs);
        plan[4].activity = ctx.pick(&jobs);
    } else if lazy {
        // Most of the work never gets done: a nap or a sit by the fire instead.
        for index in [1, 3] {
            if activity(plan[index].activity).kind == ActivityKind::Work && ctx.chance(0.8) {
                let hour = plan[index].hour;
                plan[index] = if ctx.chance(0.65) { nap(hour) } else { block(hour, ActivityId::Relax, None) };
            }
        }
    } else if ctx.chance(0.05) {
        // Anyone might doze off on the grass of an afternoon.
        let index = if activity(plan[3].activity).kind == ActivityKind::Work { 4 } else { 3 };
        let hour = plan[index].hour;
        plan[index] = nap(hour);
    }
    if drunkard {
        plan[6].activity = ActivityId::Drink;
        if ctx.chance(0.3) {
            plan[4].activity = ActivityId::Drink;
        }
    }
    if occultist && ctx.chance(0.4) {
        // A long night over forbidden books: home early, and up by candlelight long after.
        plan[7].hour = plan[6].hour + 0.4;
        plan[7].note = Some(STUDY_NOTE.into());
        let day = ctx.time.day;
        ctx.c(id).quirk_state.study_day = day;
    }
    plan
}

/// A child's day: meals, play, and home before the dead rise. Drawn from the RNG
/// only for children, so the adults' plans fall as they always did.
fn child_plan(ctx: &mut Ctx, id: &str) -> Vec<PlanBlock> {
    let c = ctx.cr(id);
    let home = c.home_id.clone();
    let hobbies = if c.hobbies.is_empty() { vec![ActivityId::Play] } else { c.hobbies.clone() };
    let block = |hour: f64, activity: ActivityId, place_id: Option<String>, note: Option<&str>| PlanBlock {
        hour,
        activity,
        place_id,
        note: note.map(str::to_owned),
    };
    let wake = 6.2 + ctx.next_float() * 1.2;
    let afternoon = ctx.pick(&hobbies);
    let midday = 12.0 + ctx.next_float() * 0.6;
    vec![
        block(wake, ActivityId::Eat, None, Some("breakfast")),
        block(wake + 0.8, ActivityId::Play, None, None),
        block(midday, ActivityId::Eat, None, Some("midday meal")),
        block(midday + 1.0, afternoon, None, None),
        block(15.4 + ctx.next_float() * 0.6, ActivityId::Play, None, Some("with the other children")),
        block(17.6, ActivityId::Eat, None, Some("supper")),
        block(18.6, ActivityId::Rest, Some(home), Some("at home before bed")),
        block(19.4 + ctx.next_float() * 0.5, ActivityId::Sleep, None, None),
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
    let bully = ctx.cr(id).has(Quirk::Bully);
    let weights: Vec<f64> = candidates
        .iter()
        .map(|o| {
            let open = openness(ctx.cr(id), ctx.cr(o));
            // A bully goes looking for the timid.
            let timid = if bully { (1.0 - ctx.cr(o).profile.bravery) * 3.0 } else { 0.0 };
            let (me_child, o_child) = (ctx.cr(id).is_child(), ctx.cr(o).is_child());
            let c = ctx.c(id);
            let spite = c.profile.spite;
            let r = rel(c, o);
            // Children seek out other children and their own people; grown-ups look in on their own children
            // and seldom go out of their way for someone else's.
            let (family, aloof) = match (me_child, o_child, r.kin) {
                (true, true, _) => (4.0, 1.0),
                (true, false, true) | (false, true, true) => (2.0, 1.0),
                (false, true, false) => (0.0, 0.3),
                _ => (0.0, 1.0),
            };
            // The spiteful are drawn twice as hard to the people they resent.
            let weight = (1.0
                + r.affinity.abs() / 25.0
                + r.attraction / 30.0
                + r.grievance * (1.0 + spite) / 12.0
                + timid
                + family)
                * aloof;
            // A charmer seeks out whoever wants her. Added only for one, so nobody else's weights change.
            let weight =
                if allure > 0.0 { weight + allure * rel_or_default(ctx.cr(o), id).attraction / 20.0 } else { weight };
            // The shy keep to the people they trust (1 for everyone else).
            weight * open
        })
        .collect();
    let index = rng::weighted_pick_index(&mut ctx.state.rng, &weights, |w| *w)?;
    let other = candidates[index].clone();
    // The shy often think better of it at the last moment, unless it is someone they trust.
    if ctx.cr(id).profile.shyness > 0.0 {
        let open = openness(ctx.cr(id), ctx.cr(&other));
        if !ctx.chance(open) {
            ctx.c(id).next_social_tick = tick + minutes_to_ticks(40.0);
            return None;
        }
    }

    let c = ctx.c(id);
    let partner = c.partner_id.clone();
    let drunk = c.drunk(tick);
    // Drink shortens the fuse.
    let temper = if drunk { (c.profile.temper + 0.3).min(1.0) } else { c.profile.temper };
    let grudge = if drunk { RUNG_TENSION } else { RUNG_GRUDGE };
    let spite = c.profile.spite;
    let r = rel(c, &other).clone();
    let mut tone = TalkTone::Friendly;
    // A bully needs no grievance to pick on someone.
    if (bully && !r.kin && ctx.chance(0.6)) || (r.grievance >= grudge && ctx.chance(0.35 + temper * 0.5)) {
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
        // The spiteful, at the same rung, sometimes take a torch to the other's home by night instead.
        let hour = ctx.time.hour;
        if spite >= 0.5 && !(5.6..19.4).contains(&hour) && lethal_unlocked(ctx, id, &other) {
            let target = building_id(&ctx.cr(&other).home_id);
            if job_accepts(ctx.state, WorldJob::Burn, &target) && ctx.chance(0.01) {
                let c = ctx.c(id);
                c.intent = Some(Intent::new(
                    IntentAction::WorldWork { job: WorldJob::Burn, target, minutes: None },
                    IntentSource::Routine,
                ));
                c.queue.clear();
                c.act = fresh_act(tick);
                return true;
            }
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
    let child = c.is_child();

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

    if !child && hostile_intent(ctx, id) {
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
    if block.note.as_deref() == Some(NAP_NOTE) && NAP_HOURS.contains(&hour) && c.needs.energy < 90.0 {
        let minutes = remaining.min(ctx.range(40.0, 90.0));
        return Some(Intent::new(IntentAction::Nap { minutes }, IntentSource::Routine));
    }
    let def = activity(block.activity);
    if remaining >= 10.0
        && let Some(place) = wants_to_read(ctx, id, def.kind == ActivityKind::Leisure)
    {
        return Some(act(ActivityId::ReadBoard, None, Some(place)));
    }
    let minutes = remaining.min(ctx.int_range(def.minutes[0], def.minutes[1]) as f64);
    // While a house lies in ruins, working hours go to building it again.
    if def.kind == ActivityKind::Work
        && let Some(ruin) = ruin_to_rebuild(ctx.state, ctx.cr(id), ctx.time.day)
    {
        return Some(act(ActivityId::Rebuild, Some(minutes), Some(ruin)));
    }
    // Woodcutters fell real trees and miners break real rocks when there are any within reach.
    if let Some(work) = job_for_block(ctx.state, ctx.cr(id), block.activity, minutes, ctx.time.day) {
        return Some(work);
    }
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
            // So does work in the woods or at the rocks; what is carried is still put down first.
            if let Some(Intent { action: IntentAction::WorldWork { job, .. }, source: IntentSource::Routine, .. }) =
                &c.intent
                && *job != crate::state::WorldJob::Deliver
                && c.act.phase == crate::state::ActPhase::Perform
                && c.convo_id.is_none()
            {
                c.act.until_tick = c.act.until_tick.min(tick);
            }
        }

        // Watch duty starts on time, whatever the plan said, but a meal is finished first: a starving
        // guard sent to eat would otherwise be called back the moment they sat down, and starve.
        let c = ctx.c(&id);
        let due_on_watch = tick < c.watch_until_tick && !(5.2..20.5).contains(&hour);
        if due_on_watch
            && let Some(Intent {
                action: IntentAction::Activity { activity, .. }, source: IntentSource::Routine, ..
            }) = &c.intent
            && !matches!(activity, ActivityId::Watch | ActivityId::Eat)
            && c.act.phase == crate::state::ActPhase::Perform
        {
            c.act.until_tick = c.act.until_tick.min(tick);
        }
        // Hunger is otherwise weighed only between stretches of watch, and a stretch of up to 150 minutes
        // burns about 16 points: a starving guard leaves the post now and `choose` sends them to eat.
        if c.needs.hunger < 12.0
            && c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Watch))
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
