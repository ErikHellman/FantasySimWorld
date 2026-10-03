//! Permadeath: graves, corpses, and everyone's reaction.

use fsw_core::math2d::dist;
use fsw_core::time::{TICKS_PER_DAY, minutes_to_ticks};
use fsw_world::places::get_place;

use crate::data::anims::AnimId;
use crate::events::{Emote, SimEvent};
use crate::state::{ChronicleKind, Corpse, DeadRecord, Grave, MemoryKind, Quirk, StimulusKind};
use crate::systems::combat::witnesses;
use crate::systems::conversation::abort_conversation;
use crate::systems::relationships::{RelationDelta, adjust};
use crate::util::{Ctx, first_name, living_ids, place_name_at, rel};

const CORPSE_MINUTES: f64 = 50.0;

/// Releases whatever a character is holding on to in the world (seat, conversation).
pub fn detach(ctx: &mut Ctx, id: &str) {
    if let Some(convo) = ctx.cr(id).convo_id.clone() {
        abort_conversation(ctx, &convo);
    }
    if let Some(slot) = ctx.cr(id).slot_id.clone() {
        if ctx.state.slots.get(&slot).map(String::as_str) == Some(id) {
            ctx.state.slots.shift_remove(&slot);
        }
        ctx.c(id).slot_id = None;
    }
}

pub fn free_grave_plot(ctx: &mut Ctx) -> i64 {
    let plots = get_place("graveyard").grave_plots.len() as i64;
    for i in 0..plots {
        if !ctx.state.graves.iter().any(|grave| grave.plot == i) {
            return i;
        }
    }
    // The graveyard is full: the oldest grave gives up its place.
    if ctx.state.graves.is_empty() { 0 } else { ctx.state.graves.remove(0).plot }
}

/// Whether a killing went unseen: a murderer at work, with nobody awake near enough to see it.
fn unseen_murder(ctx: &Ctx, victim_id: &str, killer: Option<&str>) -> bool {
    let Some(killer) = killer else { return false };
    ctx.cr(killer).has(Quirk::Murderous) && witnesses(ctx, ctx.cr(victim_id).pos, &[killer, victim_id]).is_empty()
}

/// Permadeath. The character leaves the living, gets a grave, and everyone who
/// knew them reacts. `killer` is set when another villager did it; when a
/// murderer killed unseen, nobody but the murderer knows who it was.
pub fn kill_character(ctx: &mut Ctx, victim_id: &str, cause: &str, killer: Option<&str>) {
    let unseen = unseen_murder(ctx, victim_id, killer);
    let (hidden, killer) = if unseen { (killer, None) } else { (None, killer) };
    let unseen_cause;
    let cause = if unseen {
        unseen_cause = format!("found struck down from behind at {}", place_name_at(ctx.cr(victim_id).pos));
        unseen_cause.as_str()
    } else {
        cause
    };
    detach(ctx, victim_id);
    let tick = ctx.tick();
    let day = ctx.time.day;
    let victim = ctx.state.characters.shift_remove(victim_id).expect("the victim is alive");
    let name = first_name(&victim).to_owned();
    let where_ = place_name_at(victim.pos);
    let killer_name = killer.map(|k| first_name(ctx.cr(k)).to_owned());

    ctx.state.corpses.push(Corpse {
        id: victim.id.clone(),
        look: victim.look.clone(),
        pos: victim.pos,
        facing: victim.facing,
        anim: AnimId::Death,
        until_tick: tick + minutes_to_ticks(CORPSE_MINUTES),
    });
    // The plot is chosen before the grave joins the list (TypeScript evaluates the object literal first).
    let plot = free_grave_plot(ctx);
    ctx.state.graves.push(Grave {
        plot,
        character_id: victim.id.clone(),
        name: victim.profile.name.clone(),
        day,
        cause: cause.to_owned(),
    });
    ctx.state.dead.insert(
        victim.id.clone(),
        DeadRecord {
            name: victim.profile.name.clone(),
            day,
            cause: cause.to_owned(),
            killer_id: killer.map(str::to_owned),
            hidden_killer_id: hidden.map(str::to_owned),
        },
    );
    ctx.state.order.retain(|id| id != victim_id);

    for other in living_ids(ctx.state) {
        let c = ctx.c(&other);
        if c.combat.target_id.as_deref() == Some(victim_id) {
            c.combat.target_id = None;
        }
        // Whoever had yielded to the dead character is free again.
        if c.combat.yielded && c.combat.last_hit_by_id.as_deref() == Some(victim_id) {
            c.combat.yielded = false;
        }
        if c.combat.mercy_target_id.as_deref() == Some(victim_id) {
            c.combat.mercy_target_id = None;
            c.combat.mercy_until_tick = 0;
        }
        let was_partner = c.partner_id.as_deref() == Some(victim_id);
        if was_partner {
            c.partner_id = None;
        }
        let r = rel(c, victim_id).clone();
        let close = was_partner || r.kin || r.affinity >= 25.0;
        let saw = c.inside.is_none() && dist(c.pos, victim.pos) < 28.0;
        let by_whom = match (killer, &killer_name) {
            (Some(k), _) if k == other => "killed by my own hand".to_owned(),
            (Some(_), Some(kn)) => format!("killed by {kn}"),
            _ => cause.to_owned(),
        };
        let subjects = vec![victim.id.clone()];

        if hidden == Some(other.as_str()) {
            ctx.remember(
                &other,
                format!("I killed {name} at {where_}, and nobody saw. Nobody will ever know."),
                10.0,
                MemoryKind::Event,
                subjects,
            );
            continue;
        }
        if killer == Some(other.as_str()) {
            ctx.remember(
                &other,
                format!("I killed {name} at {where_}. There is no taking it back."),
                10.0,
                MemoryKind::Event,
                subjects,
            );
            continue;
        }
        if close {
            let loss = if was_partner {
                "My love is gone."
            } else if r.kin {
                "My own blood."
            } else {
                "I have lost a friend."
            };
            let weight = if was_partner || r.kin { 10.0 } else { 8.0 };
            ctx.remember(&other, format!("{name} is dead, {by_whom}. {loss}"), weight, MemoryKind::Event, subjects);
            ctx.stimulate(
                &other,
                StimulusKind::Death,
                format!("{name}, who was dear to you, has died: {by_whom}."),
                2,
                Some(victim_id),
            );
            ctx.emit(SimEvent::Emote { id: other.clone(), emote: Emote::Grief });
        } else if saw {
            ctx.remember(&other, format!("Saw {name} die at {where_}, {by_whom}."), 7.0, MemoryKind::Event, subjects);
            ctx.stimulate(&other, StimulusKind::Death, format!("You saw {name} die: {by_whom}."), 1, Some(victim_id));
        } else if r.affinity <= -25.0 {
            ctx.remember(
                &other,
                format!("{name} is dead, {by_whom}. I feel less than I thought I would."),
                5.0,
                MemoryKind::Event,
                subjects,
            );
        } else {
            ctx.remember(&other, format!("Heard that {name} died, {by_whom}."), 5.0, MemoryKind::Event, subjects);
        }

        if let Some(killer) = killer {
            // The whole village turns on a killer; those who loved the victim most of all.
            let delta = if close {
                RelationDelta { affinity: -70.0, trust: -60.0, grievance: 70.0, ..Default::default() }
            } else if saw {
                RelationDelta { affinity: -45.0, trust: -50.0, grievance: 35.0, ..Default::default() }
            } else {
                RelationDelta { affinity: -28.0, trust: -35.0, grievance: 12.0, ..Default::default() }
            };
            adjust(ctx, &other, killer, delta);
        }
    }

    if let Some(killer) = killer.or(hidden) {
        ctx.c(killer).killer = true;
        ctx.state.director.last_lethal_tick = tick;
    }
    let (text, ids) = match killer {
        Some(k) => (
            format!("{} was killed by {} at {where_}.", victim.profile.name, ctx.cr(k).profile.name),
            vec![victim.id.clone(), k.to_owned()],
        ),
        None if unseen => {
            (format!("{} was {cause}. Nobody saw who did it.", victim.profile.name), vec![victim.id.clone()])
        }
        None => (format!("{} has died, {cause}.", victim.profile.name), vec![victim.id.clone()]),
    };
    ctx.chronicle(ChronicleKind::Death, text, ids);
    ctx.emit(SimEvent::Death {
        id: victim.id.clone(),
        name: victim.profile.name.clone(),
        pos: victim.pos,
        monster: false,
    });
    ctx.emit(SimEvent::SavePoint { reason: "death".into() });

    ctx.state.director.death_ticks.push(tick);
    schedule_arrival_after_loss(ctx);
}

/// The village is a soul short: someone new will come up the road before long.
pub fn schedule_arrival_after_loss(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let wait = if ctx.state.order.len() < 8 { ctx.range(0.4, 0.9) } else { ctx.range(1.2, 2.4) };
    let director = &mut ctx.state.director;
    director.next_arrival_tick =
        director.next_arrival_tick.max(tick + fsw_core::jsmath::js_round(wait * TICKS_PER_DAY as f64) as i64);
}
