//! Notices danger and reacts on instinct: fight, flee, raise the alarm, answer it.

use fsw_core::math2d::dist;
use fsw_core::time::{minutes_to_ticks, seconds_to_ticks};

use crate::data::monsters::Faction;
use crate::events::{Emote, SimEvent};
use crate::state::{
    ActPhase, Character, ChronicleKind, FightMode, Id, IntentAction, IntentSource, MemoryKind, Monster, MonsterPhase,
    StimulusKind,
};
use crate::systems::combat::{max_hp, start_fight, start_flee};
use crate::systems::conversation::abort_conversation;
use crate::systems::fire::burning;
use crate::systems::intents::leave_building;
use crate::systems::raids::foe_noun;
use crate::systems::relationships::{RelationDelta, adjust};
use crate::util::{
    Ctx, a_noun_capital, first_name, has_recent_stimulus, is_fighting, join_names, living, living_ids, place_name_at,
};

const SIGHT: f64 = 18.0;
const WATCH_SIGHT: f64 = 48.0;
const RESPONDER_BRAVERY: f64 = 0.6;
/// While a boss is abroad, the alarm rallies anyone with this much nerve.
pub const BOSS_RESPONDER_BRAVERY: f64 = 0.45;
const ALARM_RANGE: f64 = 150.0;
const FIRE_COURAGE: f64 = 0.2;
/// Heartened by music, people find a little more nerve.
const MUSIC_COURAGE: f64 = 0.1;

fn nearest_monster<'m>(monsters: &'m [Monster], c: &Character, limit: f64) -> Option<&'m Monster> {
    let mut best = None;
    let mut best_distance = limit;
    for m in monsters {
        let d = dist(m.pos, c.pos);
        if d < best_distance {
            best_distance = d;
            best = Some(m);
        }
    }
    best
}

/// Stand and fight, or run? Instinct only; an LLM "react" decision may overrule it.
fn stands_ground(c: &Character, monsters_near: usize, allies_near: usize, house_burning: bool, tick: i64) -> bool {
    let hp_left = c.hp / max_hp(c);
    let score = c.profile.bravery * 0.75 + hp_left * 0.35 + allies_near as f64 * 0.08
        - (monsters_near as f64 - 1.0).max(0.0) * 0.07;
    // With a house on fire there is more to lose by running: people find courage they did not know they had.
    let fire = if house_burning { FIRE_COURAGE } else { 0.0 };
    let music = if c.heartened(tick) { MUSIC_COURAGE } else { 0.0 };
    score + fire + music > 0.72
}

fn raise_alarm(ctx: &mut Ctx, id: &str, monster: &Monster) {
    let tick = ctx.tick();
    let where_ = place_name_at(monster.pos);
    let raiders = monster.kind.faction() == Faction::Orc;
    if tick >= ctx.state.director.alarm_until_tick {
        let (shout, news) = if raiders {
            (format!("To arms! Orcs at {where_}!"), format!("orc raiders are at {where_}"))
        } else {
            (format!("To arms! The dead are at {where_}!"), format!("the dead are walking at {where_}"))
        };
        ctx.say(id, shout, None, true);
        let name = first_name(ctx.cr(id)).to_owned();
        ctx.chronicle(ChronicleKind::Combat, format!("{name} raised the alarm: {news}."), vec![id.into()]);
        ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Alert });
    }
    ctx.state.director.alarm_until_tick = tick + minutes_to_ticks(40.0);
    ctx.state.director.alarm_pos = Some(monster.pos);
    if raiders {
        ctx.state.director.alarm_raid = true;
    }
}

/// When the last monster falls, those who fought remember it together.
fn stand_down(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let since = (tick - minutes_to_ticks(90.0)).max(1);
    let fighters: Vec<(Id, String)> = living(ctx.state)
        // Those still running from it did not hold the line.
        .filter(|c| {
            c.combat.next_attack_tick > since
                && !c.intent.as_ref().is_some_and(|i| matches!(i.action, IntentAction::Flee))
        })
        .map(|c| (c.id.clone(), first_name(c).to_owned()))
        .collect();
    let raiders = ctx.state.director.alarm_raid;
    ctx.state.director.alarm_until_tick = tick;
    ctx.state.director.alarm_pos = None;
    ctx.state.director.alarm_raid = false;
    if fighters.is_empty() {
        return;
    }
    let (foe, gone) =
        if raiders { ("the orc raiders", "the raiders are gone") } else { ("the dead", "the dead are destroyed") };
    let names: Vec<&str> = fighters.iter().map(|(_, name)| name.as_str()).collect();
    let ids: Vec<Id> = fighters.iter().map(|(id, _)| id.clone()).collect();
    ctx.chronicle(ChronicleKind::Combat, format!("The attack is over. {} held the line.", join_names(&names)), ids);
    for (id, _) in &fighters {
        let others: Vec<&(Id, String)> = fighters.iter().filter(|(o, _)| o != id).collect();
        for (other, _) in &others {
            adjust(ctx, id, other, RelationDelta { affinity: 2.0, trust: 3.0, familiarity: 2.0, ..Default::default() });
        }
        let text = if others.is_empty() {
            format!("Fought off {foe} alone.")
        } else {
            let names: Vec<&str> = others.iter().map(|(_, name)| name.as_str()).collect();
            format!("Fought off {foe} beside {}.", join_names(&names))
        };
        ctx.remember(id, text, 6.0, MemoryKind::Event, others.iter().map(|(o, _)| o.clone()).collect());
        ctx.stimulate(id, StimulusKind::Victory, format!("The fight is over and {gone}."), 1, None);
    }
    ctx.emit(SimEvent::SavePoint { reason: "attack over".into() });
}

pub fn update_perception(ctx: &mut Ctx) {
    let tick = ctx.tick();
    // A snapshot, as the TypeScript sim takes one: monsters keep the positions they had when perception began.
    let monsters: Vec<Monster> =
        ctx.state.monsters.values().filter(|m| m.phase != MonsterPhase::Dying).cloned().collect();
    let alarm = tick < ctx.state.director.alarm_until_tick;
    let house_burning = burning(ctx.state).is_some();
    let bar = if monsters.iter().any(|m| m.kind.def().boss) { BOSS_RESPONDER_BRAVERY } else { RESPONDER_BRAVERY };

    if monsters.is_empty() {
        if ctx.state.director.alarm_pos.is_some() {
            stand_down(ctx);
        }
        return;
    }

    let everyone = living_ids(ctx.state);
    for (i, id) in everyone.iter().enumerate() {
        let Some(c) = ctx.state.characters.get(id) else { continue };
        if (tick + i as i64 * 3) % 10 != 0 || c.combat.yielded {
            continue;
        }
        // Someone asleep on the grass wakes at the alarm, or at the sound of the dead close by.
        if c.asleep && c.inside.is_none() && (alarm || nearest_monster(&monsters, c, SIGHT).is_some()) {
            let c = ctx.c(id);
            c.asleep = false;
            ctx.emit(SimEvent::Emote { id: id.clone(), emote: Emote::Alert });
            continue;
        }
        let awake_outside = c.inside.is_none() && !c.asleep;
        let on_watch = c.intent.as_ref().is_some_and(|i| i.is_activity(fsw_world::places::ActivityId::Watch))
            && c.act.phase == ActPhase::Perform;
        let seen = if awake_outside {
            nearest_monster(&monsters, c, if on_watch { WATCH_SIGHT } else { SIGHT }).cloned()
        } else {
            None
        };

        if let Some(seen) = seen {
            raise_alarm(ctx, id, &seen);
            if let Some(convo) = ctx.cr(id).convo_id.clone() {
                abort_conversation(ctx, &convo);
            }
            if !has_recent_stimulus(ctx.cr(id), StimulusKind::Threat, tick - seconds_to_ticks(30.0), None) {
                let text = format!(
                    "{} is coming at you near {}!",
                    a_noun_capital(seen.kind.def().label),
                    place_name_at(seen.pos)
                );
                ctx.stimulate(id, StimulusKind::Threat, text, 3, Some(&seen.id));
            }
            let c = ctx.cr(id);
            if is_fighting(c) {
                continue;
            }
            let monsters_near = monsters.iter().filter(|m| dist(m.pos, c.pos) < 20.0).count();
            let allies_near = everyone
                .iter()
                .filter_map(|o| ctx.state.characters.get(o))
                .filter(|o| o.id != c.id && o.inside.is_none() && o.intent.as_ref().is_some_and(|i| i.is_fight()))
                .filter(|o| dist(o.pos, c.pos) < 20.0)
                .count();
            if !c.is_child() && (on_watch || stands_ground(c, monsters_near, allies_near, house_burning, tick)) {
                start_fight(ctx, id, &seen.id, FightMode::Monster, IntentSource::Reflex, None);
            } else {
                start_flee(ctx, id, IntentSource::Reflex);
            }
            continue;
        }

        if !alarm || is_fighting(c) {
            continue;
        }
        let responder = !c.is_child() && c.profile.bravery >= bar && c.hp > max_hp(c) * 0.4;
        if responder {
            let Some(target) = nearest_monster(&monsters, c, ALARM_RANGE).cloned() else { continue };
            let nav = ctx.state.nav.clone();
            let c = ctx.c(id);
            if c.asleep || c.inside.is_some() {
                leave_building(&nav, c);
            }
            if !has_recent_stimulus(c, StimulusKind::Alarm, tick - minutes_to_ticks(30.0), None) {
                let foe = foe_noun(target.kind.faction());
                let text =
                    format!("The alarm has been raised: {foe} are attacking near {}.", place_name_at(target.pos));
                ctx.stimulate(id, StimulusKind::Alarm, text, 2, Some(&target.id));
            }
            start_fight(ctx, id, &target.id, FightMode::Monster, IntentSource::Reflex, None);
        } else if awake_outside && !house_burning && nearest_monster(&monsters, c, 45.0).is_some() {
            // (With a house on fire the timid keep carrying water until the dead are right upon them.)
            start_flee(ctx, id, IntentSource::Reflex);
        }
    }
}
