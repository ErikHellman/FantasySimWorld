//! Carries out each character's current intent, one tick at a time.

use fsw_core::math2d::{dist, facing_to};
use fsw_core::time::{GAME_MINUTES_PER_TICK, TICKS_PER_GAME_MINUTE, minutes_to_ticks, seconds_to_ticks};
use fsw_core::{Vec2, vec2};
use fsw_world::navgrid::nearest_walkable;
use fsw_world::places::{ActivityId, Slot, billboards, find_place, get_place, get_slot, slots_for};

use crate::data::activities::{ActivityKind, ITEM_CAP, activity};
use crate::data::anims::AnimId;
use crate::data::classes::{RUN_SPEED, WALK_SPEED, WeaponStyle};
use crate::events::{Emote, SimEvent};
use crate::state::{
    Act, ActPhase, Character, ChronicleKind, HousePhase, Intent, IntentAction, IntentSource, ItemId, MemoryKind,
    MonsterPhase, Quirk, SimState, StimulusKind, TalkTone,
};
use crate::systems::billboards::{has_read, react, reading, shown, template_reaction};
use crate::systems::combat::{max_hp, run_fight, sneak_attack, witnesses};
use crate::systems::conversation::start_conversation;
use crate::systems::fire::{self, home_usable, rough_spot, run_fight_fire, shelter_of};
use crate::systems::movement::{advance, route_to, step_toward};
use crate::systems::quirks::{after_drink, end_hunt};
use crate::systems::relationships::{RelationDelta, adjust};
use crate::systems::viewer_events::pray;
use crate::util::{Ctx, first_name, is_approachable, living, living_ids, rel, set_anim};

const TALK_REACH: f64 = 9.0;
/// Close enough to strike from behind.
const STRIKE_REACH: f64 = 1.8;
const GIVE_REACH: f64 = 2.3;
/// Energy a game minute of sleep on the bare ground gives less than a bed does.
const ROUGH_SLEEP_PENALTY: f64 = 0.1;
const STROLL_STOPS: [&str; 8] =
    ["green", "tavern", "market", "smithy", "chapel", "training_yard", "docks", "west_road"];

pub fn fresh_act(tick: i64) -> Act {
    Act {
        phase: ActPhase::Start,
        path: Vec::new(),
        dest: None,
        since_tick: tick,
        until_tick: 0,
        repath_tick: 0,
        stuck: 0,
    }
}

fn walk_anim(c: &Character, tick: i64) -> AnimId {
    if c.drunk(tick) {
        return AnimId::WalkDrunk;
    }
    let last = c.id.encode_utf16().last().unwrap_or(0);
    match last % 3 {
        0 => AnimId::Walk,
        1 => AnimId::WalkB,
        _ => AnimId::WalkC,
    }
}

fn speed_of(c: &Character, tick: i64) -> f64 {
    WALK_SPEED * if c.needs.energy < 15.0 || c.drunk(tick) { 0.8 } else { 1.0 }
}

/// Steps out of a building back onto the street (or gets up off the grass).
pub fn leave_building(c: &mut Character) {
    let Some(inside) = c.inside.take() else {
        c.asleep = false;
        return;
    };
    if let Some(place) = find_place(&inside) {
        c.pos = nearest_walkable(place.door, 4).unwrap_or(place.door);
    }
    c.asleep = false;
}

fn release_slot(ctx: &mut Ctx, id: &str) {
    if let Some(slot) = ctx.c(id).slot_id.take()
        && ctx.state.slots.get(&slot).map(String::as_str) == Some(id)
    {
        ctx.state.slots.shift_remove(&slot);
    }
}

pub(crate) fn finish(ctx: &mut Ctx, id: &str, outcome: impl Into<String>) {
    release_slot(ctx, id);
    let c = ctx.c(id);
    c.tool = None;
    c.outcome = outcome.into();
    c.act.phase = ActPhase::Done;
}

/// Sets a route; false when there is none.
pub(crate) fn travel_to(tick: i64, c: &mut Character, dest: Vec2) -> bool {
    leave_building(c);
    let Some(route) = route_to(c.pos, dest) else { return false };
    c.act.path = route;
    c.act.dest = Some(dest);
    c.act.phase = ActPhase::Travel;
    c.act.since_tick = tick;
    true
}

/// Walks the current route; true on arrival.
pub(crate) fn travel(tick: i64, c: &mut Character, speed: Option<f64>) -> bool {
    if c.act.path.is_empty() {
        return true;
    }
    let speed = speed.unwrap_or_else(|| speed_of(c, tick));
    let mut path = std::mem::take(&mut c.act.path);
    let arrived = advance(c, &mut path, speed);
    c.act.path = path;
    let anim = if speed > WALK_SPEED * 1.3 { AnimId::Run } else { walk_anim(c, tick) };
    set_anim(c, anim, false);
    arrived
}

fn pick_slot(ctx: &Ctx, c: &Character, activity_id: ActivityId, place_id: Option<&str>) -> Option<&'static Slot> {
    let ranged = c.class_id.def().weapon.style != WeaponStyle::Melee;
    let mut best = None;
    let mut best_score = f64::INFINITY;
    for slot in slots_for(activity_id) {
        if let Some(holder) = ctx.state.slots.get(&slot.id)
            && holder != &c.id
        {
            continue;
        }
        // A billboard is read where it stands; another board is another errand.
        if activity_id == ActivityId::ReadBoard && place_id.is_some_and(|p| slot.place_id != p) {
            continue;
        }
        let mut score = dist(c.pos, slot.pos);
        if place_id.is_some_and(|p| slot.place_id != p) {
            score += 400.0;
        }
        // At home, people use their own bench.
        if slot.place_id.starts_with("home_") && slot.place_id != c.home_id {
            score += 150.0;
        }
        if activity_id == ActivityId::Train && slot.ranged != ranged {
            score += 60.0;
        }
        if score < best_score {
            best_score = score;
            best = Some(slot);
        }
    }
    best
}

fn give(c: &mut Character, item: ItemId) {
    let count = c.inventory.get_mut(item);
    *count = ITEM_CAP.min(*count + 1);
}

/// Per-game-minute effects of doing an activity.
fn activity_minute(c: &mut Character, activity_id: ActivityId, minutes_done: i64) {
    let def = activity(activity_id);
    if let Some(needs) = def.needs {
        c.needs.hunger = (c.needs.hunger + needs.hunger).clamp(0.0, 100.0);
        c.needs.energy = (c.needs.energy + needs.energy).clamp(0.0, 100.0);
        c.needs.social = (c.needs.social + needs.social).clamp(0.0, 100.0);
    }
    if def.heal != 0.0 {
        c.hp = max_hp(c).min(c.hp + def.heal);
    }
    if let Some((item, every)) = def.yields
        && minutes_done > 0
        && minutes_done % every == 0
    {
        give(c, item);
    }
    if matches!(activity_id, ActivityId::Pray | ActivityId::Mourn) {
        for r in c.relationships.values_mut() {
            if r.grievance > 0.0 {
                r.grievance = (r.grievance - 0.12).max(0.0);
            }
        }
    }
}

fn stroll_goal(ctx: &mut Ctx) -> Vec2 {
    let stop = get_place(STROLL_STOPS[ctx.pick_index(STROLL_STOPS.len())]);
    let x = stop.door.x + ctx.range(-5.0, 5.0);
    let z = stop.door.z + ctx.range(-5.0, 5.0);
    nearest_walkable(vec2(x, z), 6).unwrap_or(stop.door)
}

fn run_activity(ctx: &mut Ctx, id: &str, activity_id: ActivityId, place_id: Option<String>, minutes: Option<f64>) {
    let tick = ctx.tick();
    let def = activity(activity_id);
    let minutes = match minutes {
        Some(m) => m,
        None => ctx.int_range(def.minutes[0], def.minutes[1]) as f64,
    };
    let phase = ctx.cr(id).act.phase;

    if phase == ActPhase::Start {
        if let Some(Intent { action: IntentAction::Activity { minutes: m, .. }, .. }) = &mut ctx.c(id).intent {
            *m = Some(minutes);
        }
        if activity_id == ActivityId::Sleep {
            let c = ctx.c(id);
            if c.inside.as_deref() == Some(c.home_id.as_str()) {
                c.act.phase = ActPhase::Perform;
                c.act.since_tick = tick;
                c.act.until_tick = tick + minutes_to_ticks(minutes);
                c.asleep = true;
                return;
            }
            // Those whose home is burning or in ruins sleep on the ground by the campfire.
            let usable = home_usable(ctx.state, ctx.cr(id));
            let c = ctx.c(id);
            let bed = if usable { get_place(&c.home_id).door } else { rough_spot(c) };
            if !travel_to(tick, c, bed) {
                finish(ctx, id, if usable { "You could not find a way home." } else { "You found nowhere to sleep." });
            }
            return;
        }
        if activity_id == ActivityId::Rebuild {
            let ruined = |state: &SimState, place: &str| {
                state.houses.get(place).is_some_and(|house| house.phase == HousePhase::Ruined)
            };
            let ruin =
                place_id.filter(|place| ruined(ctx.state, place)).or_else(|| fire::ruin(ctx.state).map(str::to_owned));
            let Some(ruin) = ruin else {
                finish(ctx, id, "There was nothing to rebuild.");
                return;
            };
            let spot = fire::build_spot(get_place(&ruin), id);
            let c = ctx.c(id);
            if let Some(Intent { action: IntentAction::Activity { place_id, .. }, .. }) = &mut c.intent {
                *place_id = Some(ruin);
            }
            if !travel_to(tick, c, spot) {
                finish(ctx, id, "You could not get to the ruin.");
            }
            return;
        }
        if activity_id == ActivityId::Stroll {
            let goal = stroll_goal(ctx);
            let c = ctx.c(id);
            c.act.until_tick = tick + minutes_to_ticks(minutes);
            if !travel_to(tick, c, goal) {
                finish(ctx, id, "You wandered a little.");
            }
            return;
        }
        let Some(slot) = pick_slot(ctx, ctx.cr(id), activity_id, place_id.as_deref()) else {
            finish(ctx, id, format!("There was no room to {}.", def.label));
            return;
        };
        ctx.state.slots.insert(slot.id.clone(), id.to_owned());
        let c = ctx.c(id);
        c.slot_id = Some(slot.id.clone());
        let approach = nearest_walkable(slot.pos, 4).unwrap_or(slot.pos);
        if !travel_to(tick, c, approach) {
            finish(ctx, id, format!("You could not get to {}.", get_place(&slot.place_id).name));
        }
        return;
    }

    if phase == ActPhase::Travel {
        if !travel(tick, ctx.c(id), None) {
            return;
        }
        if activity_id == ActivityId::Sleep {
            let usable = home_usable(ctx.state, ctx.cr(id));
            let c = ctx.c(id);
            if usable {
                c.inside = Some(c.home_id.clone());
            } else {
                set_anim(c, AnimId::LieDown, true);
            }
            c.asleep = true;
            c.act.phase = ActPhase::Perform;
            c.act.since_tick = tick;
            c.act.until_tick = tick + minutes_to_ticks(minutes);
            ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Sleep });
            return;
        }
        if activity_id == ActivityId::Rebuild {
            let c = ctx.c(id);
            if let Some(place) = place_id.as_deref().and_then(find_place) {
                c.facing = facing_to(c.pos, place.origin);
            }
            c.act.phase = ActPhase::Perform;
            c.act.since_tick = tick;
            c.act.until_tick = tick + minutes_to_ticks(minutes);
            c.tool = def.tool.map(str::to_owned);
            set_anim(c, def.anim, true);
            return;
        }
        if activity_id == ActivityId::Stroll {
            let c = ctx.c(id);
            set_anim(c, AnimId::Idle, false);
            if tick >= c.act.until_tick {
                finish(ctx, id, "You finished your stroll.");
            } else if c.act.dest.is_some() {
                // Arrived at a stop: linger a moment before wandering on.
                c.act.dest = None;
                let linger = ctx.range(3.0, 8.0);
                ctx.c(id).act.repath_tick = tick + seconds_to_ticks(linger);
            } else if tick >= c.act.repath_tick {
                let goal = stroll_goal(ctx);
                if !travel_to(tick, ctx.c(id), goal) {
                    finish(ctx, id, "You finished your stroll.");
                }
            }
            return;
        }
        // The last stretch onto the seat or up to the bench ignores the nav grid.
        let Some(slot) = ctx.cr(id).slot_id.as_deref().map(get_slot) else {
            finish(ctx, id, "You lost your place.");
            return;
        };
        let c = ctx.c(id);
        let speed = speed_of(c, tick);
        if !step_toward(c, slot.pos, speed) {
            let anim = walk_anim(c, tick);
            set_anim(c, anim, false);
            return;
        }
        c.facing = slot.facing;
        c.act.phase = ActPhase::Perform;
        c.act.since_tick = tick;
        // The idle find a reason to knock off early.
        let minutes = if c.has(Quirk::Lazy) && def.kind == ActivityKind::Work { minutes * 0.5 } else { minutes };
        c.act.until_tick = tick + minutes_to_ticks(minutes);
        c.tool = def.tool.map(str::to_owned);
        let anim = slot.anim.and_then(AnimId::parse).unwrap_or(def.anim);
        set_anim(c, anim, true);
        if activity_id == ActivityId::Pray {
            pray(ctx, id);
        }
        if activity_id == ActivityId::ReadBoard {
            // With a model about, give it time to say how this person takes the board; the stock reaction
            // comes at the end either way if it does not answer.
            let wait = ctx.state.tuning.script_wait_ticks.min(600);
            let c = ctx.c(id);
            c.act.until_tick = c.act.until_tick.max(tick + wait);
            if let Some(index) = reading(ctx.cr(id)) {
                ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Thinking });
                ctx.emit(SimEvent::BillboardRead { id: id.into(), board_id: billboards()[index].id.clone() });
            }
        }
        return;
    }

    // Performing.
    let c = ctx.c(id);
    let elapsed = tick - c.act.since_tick;
    if elapsed > 0 && elapsed % TICKS_PER_GAME_MINUTE == 0 {
        activity_minute(c, activity_id, elapsed / TICKS_PER_GAME_MINUTE);
    }

    if activity_id == ActivityId::Rebuild && elapsed > 0 && elapsed % TICKS_PER_GAME_MINUTE == 0 {
        let place = place_id.unwrap_or_default();
        if fire::add_work(ctx, id, &place) {
            set_anim(ctx.c(id), AnimId::Idle, false);
            finish(ctx, id, "The house stands again.");
            return;
        }
    }
    let c = ctx.c(id);
    // Asleep on the ground with no home to go to: a poorer night, and anything may wake you.
    if activity_id == ActivityId::Sleep && c.inside.is_none() {
        if !c.asleep {
            set_anim(c, AnimId::Idle, false);
            finish(ctx, id, "You were woken where you lay.");
            return;
        }
        if c.anim == AnimId::LieDown && elapsed >= seconds_to_ticks(AnimId::LieDown.seconds()) {
            set_anim(c, AnimId::Lie, false);
        }
        if elapsed > 0 && elapsed % TICKS_PER_GAME_MINUTE == 0 {
            c.needs.energy = (c.needs.energy - ROUGH_SLEEP_PENALTY).max(0.0);
        }
    }

    if activity_id == ActivityId::Train {
        let weapon = &c.class_id.def().weapon;
        let every = seconds_to_ticks(weapon.interval + 1.2);
        if elapsed % every == 0 {
            let anim = ctx.pick(weapon.anims);
            set_anim(ctx.c(id), anim, true);
        }
    } else if matches!(activity_id, ActivityId::Forage | ActivityId::Shop) && elapsed % seconds_to_ticks(5.0) == 0 {
        set_anim(c, activity(activity_id).anim, true);
    }

    let c = ctx.c(id);
    let rested = activity_id == ActivityId::Sleep
        && c.needs.energy >= 99.0
        && elapsed > minutes_to_ticks(240.0)
        && tick >= c.sleep_in_until_tick;
    if tick < c.act.until_tick && !rested {
        return;
    }

    match activity_id {
        ActivityId::Sleep => {
            let rough = c.inside.is_none();
            leave_building(c);
            set_anim(c, AnimId::Idle, false);
            if rough {
                let text = "You have woken cold and stiff on the ground by the campfire: you have no home to sleep in.";
                ctx.stimulate(id, StimulusKind::Woke, text, 1, None);
                let text = "Slept on the ground by the campfire, with no home to go to.";
                ctx.remember(id, text, 4.0, MemoryKind::Event, Vec::new());
            } else {
                ctx.stimulate(id, StimulusKind::Woke, "You have just woken up.", 0, None);
            }
            ctx.emit(SimEvent::Woke { id: id.into() });
            finish(
                ctx,
                id,
                if rough { "You slept rough and feel only half rested." } else { "You slept and feel rested." },
            );
        }
        ActivityId::ReadBoard => {
            let reader = ctx.cr(id);
            match reading(reader) {
                Some(index) if !has_read(ctx.state, reader, index) => {
                    let reaction = template_reaction(&reader.profile, shown(ctx.state, index).tone);
                    let ticks = react(ctx, id, index, reaction, None, None);
                    // Stay a moment for the line before moving on.
                    ctx.c(id).act.until_tick = tick + ticks.min(160);
                }
                Some(index) => {
                    let place = get_place(&billboards()[index].place_id).name;
                    finish(ctx, id, format!("You read the billboard at {place}."));
                }
                None => finish(ctx, id, "You read the billboard."),
            }
        }
        ActivityId::Shop => {
            let item = if c.inventory.food < 2 { ItemId::Food } else { ItemId::Gift };
            give(c, item);
            let what = if item == ItemId::Food { "some food" } else { "a small gift" };
            finish(ctx, id, format!("You picked up {what} at the market."));
        }
        ActivityId::Drink => {
            set_anim(c, AnimId::Idle, false);
            finish(ctx, id, format!("You finished {}.", def.verb));
            after_drink(ctx, id);
        }
        _ => {
            set_anim(c, AnimId::Idle, false);
            finish(ctx, id, format!("You finished {}.", def.verb));
        }
    }
}

/// Somewhere on the grass not far off, or on the village green.
fn nap_spot(ctx: &mut Ctx, from: Vec2) -> Vec2 {
    let near = if ctx.chance(0.5) { get_place("green").door } else { from };
    let x = near.x + ctx.range(-7.0, 7.0);
    let z = near.z + ctx.range(-7.0, 7.0);
    nearest_walkable(vec2(x, z), 6).unwrap_or(from)
}

/// Lies down on the grass and sleeps a while, in plain sight.
fn run_nap(ctx: &mut Ctx, id: &str, minutes: f64) {
    let tick = ctx.tick();
    match ctx.cr(id).act.phase {
        ActPhase::Start => {
            let from = ctx.cr(id).pos;
            let spot = nap_spot(ctx, from);
            let c = ctx.c(id);
            if !travel_to(tick, c, spot) {
                finish(ctx, id, "You could not find a quiet spot to lie down.");
            }
        }
        ActPhase::Travel => {
            let c = ctx.c(id);
            if !travel(tick, c, None) {
                return;
            }
            c.act.phase = ActPhase::Perform;
            c.act.since_tick = tick;
            c.act.until_tick = tick + minutes_to_ticks(minutes);
            c.asleep = true;
            set_anim(c, AnimId::LieDown, true);
            ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Sleep });
        }
        _ => {
            let c = ctx.c(id);
            if !c.asleep {
                // Woken by something: up and about again.
                set_anim(c, AnimId::Idle, false);
                finish(ctx, id, "You were woken from your nap.");
                return;
            }
            let elapsed = tick - c.act.since_tick;
            if c.anim == AnimId::LieDown && elapsed >= seconds_to_ticks(AnimId::LieDown.seconds()) {
                set_anim(c, AnimId::Lie, false);
            }
            if elapsed > 0 && elapsed % TICKS_PER_GAME_MINUTE == 0 {
                activity_minute(c, ActivityId::Sleep, 0);
            }
            if tick >= c.act.until_tick || c.needs.energy >= 99.0 {
                c.asleep = false;
                set_anim(c, AnimId::Idle, false);
                ctx.stimulate(id, StimulusKind::Woke, "You have just woken from a nap on the grass.", 0, None);
                finish(ctx, id, "You napped on the grass and feel rested.");
            }
        }
    }
}

/// Follows someone at a stroll, and strikes once nobody else is about.
fn run_stalk(ctx: &mut Ctx, id: &str, target_id: &str) {
    let tick = ctx.tick();
    let hour = ctx.time.hour;
    let Some(target) = ctx.state.characters.get(target_id) else {
        end_hunt(ctx, id);
        finish(ctx, id, "You lost track of them.");
        return;
    };
    let (target_pos, gone) = (target.pos, target.inside.is_some() || target.convo_id.is_some());
    let c = ctx.c(id);
    if c.act.phase == ActPhase::Start {
        leave_building(c);
        c.act.phase = ActPhase::Travel;
        c.act.since_tick = tick;
        c.act.until_tick = tick + minutes_to_ticks(90.0);
        c.act.repath_tick = 0;
    }
    if gone || tick >= c.act.until_tick || !(8.0..19.4).contains(&hour) {
        end_hunt(ctx, id);
        let c = ctx.c(id);
        set_anim(c, AnimId::Idle, false);
        finish(ctx, id, "You gave up following them, for today.");
        return;
    }
    let d = dist(c.pos, target_pos);
    if d <= STRIKE_REACH {
        c.act.path.clear();
        if witnesses(ctx, target_pos, &[id, target_id]).is_empty() {
            sneak_attack(ctx, id, target_id);
        } else {
            // Too many eyes: loiter as if admiring the view.
            set_anim(ctx.c(id), AnimId::Idle, false);
        }
        return;
    }
    if c.act.path.is_empty() || tick >= c.act.repath_tick {
        c.act.path = route_to(c.pos, target_pos).unwrap_or_default();
        c.act.repath_tick = tick + seconds_to_ticks(1.5);
    }
    travel(tick, c, None);
}

fn run_talk(ctx: &mut Ctx, id: &str, target_id: &str, tone: TalkTone, opener: &str) {
    let tick = ctx.tick();
    let Some(target) = ctx.state.characters.get(target_id) else {
        finish(ctx, id, "The person you wanted to talk to is gone.");
        return;
    };
    let (target_pos, target_inside, target_asleep, approachable, target_name) =
        (target.pos, target.inside.clone(), target.asleep, is_approachable(target), first_name(target).to_owned());
    let c = ctx.c(id);
    if c.act.phase == ActPhase::Start {
        leave_building(c);
        c.act.phase = ActPhase::Travel;
        c.act.since_tick = tick;
        c.act.until_tick = tick + minutes_to_ticks(45.0);
        c.act.repath_tick = 0;
    }
    let d = dist(c.pos, target_pos);
    if d <= TALK_REACH && approachable {
        c.act.path.clear();
        start_conversation(ctx, id, target_id, tone, opener);
        return;
    }
    if tick >= c.act.until_tick || (d <= TALK_REACH && target_asleep) {
        finish(ctx, id, format!("You could not get a word with {target_name}."));
        return;
    }
    if let Some(inside) = &target_inside {
        // Wait near their door for a while rather than walking through walls.
        let door = get_place(inside).door;
        if dist(c.pos, door) < 4.0 {
            set_anim(c, AnimId::Idle, false);
            if tick - c.act.since_tick > minutes_to_ticks(8.0) {
                finish(ctx, id, format!("{target_name} is indoors and did not come out."));
            }
            return;
        }
    }
    if c.act.path.is_empty() || tick >= c.act.repath_tick {
        let goal = target_inside.as_deref().map_or(target_pos, |inside| get_place(inside).door);
        c.act.path = route_to(c.pos, goal).unwrap_or_default();
        c.act.repath_tick = tick + seconds_to_ticks(1.5);
        if c.act.path.is_empty() && d > TALK_REACH {
            finish(ctx, id, format!("You could not reach {target_name}."));
            return;
        }
    }
    travel(tick, c, None);
}

fn describe_item(item: ItemId) -> &'static str {
    match item {
        ItemId::Food => "some food",
        ItemId::Potion => "a healing potion",
        ItemId::Gift => "a small gift",
    }
}

fn run_give(ctx: &mut Ctx, id: &str, target_id: &str, item: ItemId, said: bool) {
    let tick = ctx.tick();
    let has_item = ctx.cr(id).inventory.get(item) > 0;
    let Some(target) = ctx.state.characters.get(target_id).filter(|_| has_item) else {
        let outcome = if ctx.alive(target_id) {
            format!("You had no {} to give.", item.as_str())
        } else {
            "The person you wanted to give something to is gone.".into()
        };
        finish(ctx, id, outcome);
        return;
    };
    let (target_pos, target_inside, approachable, target_name) =
        (target.pos, target.inside.is_some(), is_approachable(target), first_name(target).to_owned());
    let c = ctx.c(id);
    if c.act.phase == ActPhase::Start {
        leave_building(c);
        c.act.phase = ActPhase::Travel;
        c.act.since_tick = tick;
        c.act.until_tick = tick + minutes_to_ticks(40.0);
    }
    if c.act.phase == ActPhase::Perform {
        if tick >= c.act.until_tick {
            set_anim(c, AnimId::Idle, false);
            finish(ctx, id, format!("You gave {target_name} {}.", describe_item(item)));
        }
        return;
    }
    let d = dist(c.pos, target_pos);
    if d <= GIVE_REACH && approachable {
        c.act.path.clear();
        c.facing = facing_to(c.pos, target_pos);
        *c.inventory.get_mut(item) -= 1;
        set_anim(c, AnimId::Interact, true);
        c.act.phase = ActPhase::Perform;
        c.act.until_tick = tick + seconds_to_ticks(1.6);
        let name = first_name(c).to_owned();
        give(ctx.c(target_id), item);
        let romantic = item == ItemId::Gift && rel(ctx.c(id), target_id).attraction >= 35.0;
        let attraction = if romantic && !rel(ctx.c(target_id), id).kin { 4.0 } else { 0.0 };
        adjust(
            ctx,
            target_id,
            id,
            RelationDelta { affinity: 8.0, trust: 4.0, grievance: -10.0, attraction, ..Default::default() },
        );
        adjust(ctx, id, target_id, RelationDelta { affinity: 3.0, ..Default::default() });
        let what = describe_item(item);
        ctx.remember(target_id, format!("{name} gave me {what}."), 5.0, MemoryKind::Event, vec![id.into()]);
        ctx.remember(id, format!("Gave {target_name} {what}."), 3.0, MemoryKind::Event, vec![target_id.into()]);
        ctx.stimulate(target_id, StimulusKind::Gift, format!("{name} just gave you {what}."), 1, Some(id));
        ctx.emit(SimEvent::Emote { id: target_id.into(), emote: Emote::Gift });
        if !said {
            let line = if item == ItemId::Potion { "Take this. You may need it." } else { "Here. This is for you." };
            ctx.say(id, line, Some(target_id), false);
        }
        if item == ItemId::Gift {
            ctx.chronicle(
                ChronicleKind::Social,
                format!("{name} gave {target_name} a gift."),
                vec![id.into(), target_id.into()],
            );
        }
        return;
    }
    if tick >= c.act.until_tick || target_inside {
        finish(ctx, id, format!("You could not find {target_name} to give them anything."));
        return;
    }
    if c.act.path.is_empty() || tick >= c.act.repath_tick {
        c.act.path = route_to(c.pos, target_pos).unwrap_or_default();
        c.act.repath_tick = tick + seconds_to_ticks(1.5);
    }
    if d > GIVE_REACH * 0.8 {
        travel(tick, c, None);
    }
}

fn run_flee(ctx: &mut Ctx, id: &str) {
    let tick = ctx.tick();
    let c = ctx.cr(id);
    // Home, or the tavern for those whose home is burning or gone.
    let home = shelter_of(ctx.state, c);
    match c.act.phase {
        ActPhase::Start => {
            let c = ctx.c(id);
            if c.inside.is_some() {
                c.act.phase = ActPhase::Perform;
                c.act.since_tick = tick;
            } else if !travel_to(tick, c, home.door) {
                finish(ctx, id, "There was nowhere to run.");
            }
        }
        ActPhase::Travel => {
            let c = ctx.c(id);
            if !travel(tick, c, Some(RUN_SPEED)) {
                return;
            }
            c.inside = Some(home.id.clone());
            c.act.phase = ActPhase::Perform;
            c.act.since_tick = tick;
        }
        _ => {
            // Hiding indoors until the danger has passed.
            let waited = tick - c.act.since_tick;
            // With a house on fire nobody hides for long: out again as soon as the dead have moved off.
            let (reach, patience) = if fire::burning(ctx.state).is_some() { (20.0, 3.0) } else { (45.0, 12.0) };
            let danger =
                ctx.state.monsters.values().any(|m| m.phase != MonsterPhase::Dying && dist(m.pos, home.door) < reach);
            let hunted = living(ctx.state).any(|other| other.intent.as_ref().is_some_and(|i| i.fights(id)));
            if waited > minutes_to_ticks(patience) && !danger && !hunted {
                let c = ctx.c(id);
                leave_building(c);
                set_anim(c, AnimId::Idle, false);
                finish(ctx, id, "You hid until it was safe to come out.");
            }
        }
    }
}

/// Says the line and records the thought that came with a new intent.
fn begin(ctx: &mut Ctx, id: &str, intent: &Intent) {
    if let Some(thought) = intent.thought.as_ref().filter(|t| !t.is_empty()) {
        ctx.c(id).thought = thought.clone();
        if intent.source == IntentSource::Llm {
            ctx.remember(id, thought.clone(), 3.0, MemoryKind::Thought, Vec::new());
        }
    }
    if let Some(say) = intent.say.as_ref().filter(|s| !s.is_empty())
        && !matches!(intent.action, IntentAction::Talk { .. })
    {
        let target = intent.action.target_id().filter(|t| ctx.alive(t)).cloned();
        let loud = intent.is_fight();
        ctx.say(id, say.clone(), target.as_deref(), loud);
    }
}

fn complete_intent(ctx: &mut Ctx, id: &str, source: IntentSource) {
    let c = ctx.c(id);
    c.intent = None;
    if c.queue.is_empty() && source == IntentSource::Llm {
        let text = format!("You finished what you set out to do: {}", c.outcome);
        ctx.stimulate(id, StimulusKind::BlockEnded, text, 0, None);
    }
}

/// Carries out each character's current intent, one tick at a time.
pub fn update_intents(ctx: &mut Ctx) {
    let tick = ctx.tick();
    for id in living_ids(ctx.state) {
        // Characters killed earlier in this tick are skipped.
        if !ctx.alive(&id) {
            continue;
        }
        let c = ctx.c(&id);
        c.moving = false;
        if c.convo_id.is_some() || c.combat.yielded {
            continue;
        }

        // Finished elsewhere (a conversation ending, a fight resolving): clear it before picking the next.
        if c.act.phase == ActPhase::Done
            && let Some(source) = c.intent.as_ref().map(|i| i.source)
        {
            complete_intent(ctx, &id, source);
        }
        let c = ctx.c(&id);
        if c.intent.is_none() {
            if c.queue.is_empty() {
                continue;
            }
            c.intent = Some(c.queue.remove(0));
            c.act = fresh_act(tick);
        }
        let intent = c.intent.clone().expect("an intent to run");
        if c.act.phase == ActPhase::Start {
            begin(ctx, &id, &intent);
        }

        match &intent.action {
            IntentAction::GoTo { place_id } => match find_place(place_id) {
                None => finish(ctx, &id, "You did not know the way."),
                Some(place) => {
                    let c = ctx.c(&id);
                    if c.act.phase == ActPhase::Start {
                        if !travel_to(tick, c, place.door) {
                            finish(ctx, &id, format!("You could not find a way to {}.", place.name));
                        }
                    } else if travel(tick, c, None) {
                        set_anim(c, AnimId::Idle, false);
                        finish(ctx, &id, format!("You arrived at {}.", place.name));
                    }
                }
            },
            IntentAction::Activity { activity, place_id, minutes } => {
                run_activity(ctx, &id, *activity, place_id.clone(), *minutes);
            }
            IntentAction::Talk { target_id, opener, tone } => run_talk(ctx, &id, target_id, *tone, opener),
            IntentAction::Give { target_id, item } => {
                run_give(ctx, &id, target_id, *item, intent.say.as_ref().is_some_and(|s| !s.is_empty()))
            }
            IntentAction::Fight { target_id, mode } => run_fight(ctx, &id, target_id, *mode, intent.source),
            IntentAction::Flee => run_flee(ctx, &id),
            IntentAction::Nap { minutes } => run_nap(ctx, &id, *minutes),
            IntentAction::Stalk { target_id } => run_stalk(ctx, &id, target_id),
            IntentAction::FightFire { place_id } => run_fight_fire(ctx, &id, place_id),
            IntentAction::Wait { minutes } => {
                let c = ctx.c(&id);
                if c.act.phase == ActPhase::Start {
                    c.act.phase = ActPhase::Perform;
                    c.act.until_tick = tick + minutes_to_ticks(*minutes);
                    let anim = c.anim;
                    if !anim.loops() || anim.as_str().starts_with("walk") || anim == AnimId::Run {
                        set_anim(c, AnimId::Idle, false);
                    }
                } else if tick >= c.act.until_tick {
                    if c.anim == AnimId::SitFloor {
                        set_anim(c, AnimId::Idle, false);
                    }
                    finish(ctx, &id, "You waited a while.");
                }
            }
        }

        // The intent may have been replaced (a reflex, a conversation) while it ran.
        if !ctx.alive(&id) {
            continue;
        }
        let c = ctx.cr(&id);
        if c.act.phase == ActPhase::Done && same_intent(c.intent.as_ref(), &intent) {
            complete_intent(ctx, &id, intent.source);
        }
    }
}

/// Whether the intent that just ran is still the current one. The TypeScript sim
/// compares object identity; here the run may have filled in `minutes`, so the
/// comparison ignores that field.
fn same_intent(current: Option<&Intent>, ran: &Intent) -> bool {
    let Some(current) = current else { return false };
    match (&current.action, &ran.action) {
        (
            IntentAction::Activity { activity: a, place_id: pa, .. },
            IntentAction::Activity { activity: b, place_id: pb, .. },
        ) => {
            a == b
                && pa == pb
                && current.source == ran.source
                && current.say == ran.say
                && current.thought == ran.thought
        }
        _ => current == ran,
    }
}

/// Game minutes an activity has been going, for status lines.
pub fn minutes_into(tick: i64, c: &Character) -> f64 {
    (tick - c.act.since_tick) as f64 * GAME_MINUTES_PER_TICK
}

/// Used when a character is wounded and idle: is there a potion worth drinking?
pub fn drink_potion_if_hurt(ctx: &mut Ctx, id: &str) -> bool {
    let c = ctx.cr(id);
    if c.inventory.potion > 0 && c.hp < max_hp(c) * 0.5 && ctx.chance(0.5) {
        let c = ctx.c(id);
        c.inventory.potion -= 1;
        c.hp = max_hp(c).min(c.hp + 55.0);
        set_anim(c, AnimId::UseItem, true);
        return true;
    }
    false
}
