//! The bosses: the skeleton golem on a grim night, the orc warchief at the head of a war band, and the village
//! rallying to bring them down.

use fsw_core::math2d::dist;
use fsw_core::time::game_time;
use fsw_core::vec2;
use fsw_sim::Command;
use fsw_sim::Sim;
use fsw_sim::SimEvent;
use fsw_sim::commands::TuningPatch;
use fsw_sim::data::classes::ClassId;
use fsw_sim::events::HitOutcome;
use fsw_sim::state::{
    Intent, IntentAction, IntentSource, MemoryKind, MonsterKind, MonsterOrigin, MonsterPhase, SimState,
};
use fsw_sim::systems::combat::damage_monster;
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::monsters::spawn_monster;
use fsw_sim::systems::raids::next_raid_tick;
use fsw_sim::util::{Ctx, adult_count};
use fsw_testing::{admit, create_test_state, newcomer_keys};
use fsw_world::places::get_place;

fn quiet(seed: u32) -> Sim {
    let mut sim = Sim::new(create_test_state(seed));
    sim.state.director.next_wave_tick = i64::MAX;
    sim.state.director.next_raid_tick = i64::MAX;
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    sim
}

fn with<R>(sim: &mut Sim, f: impl FnOnce(&mut Ctx) -> R) -> R {
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut sim.state, &mut events);
    f(&mut ctx)
}

/// Enough villagers for a boss to bother with.
fn at_least_ten(sim: &mut Sim) {
    for key in newcomer_keys() {
        if adult_count(&sim.state) >= 10 {
            break;
        }
        with(sim, |ctx| admit(ctx, key));
    }
    assert!(adult_count(&sim.state) >= 10);
}

fn run(sim: &mut Sim, ticks: i64) -> Vec<SimEvent> {
    let mut events = Vec::new();
    for _ in 0..ticks {
        events.extend(sim.step());
    }
    events
}

fn kinds(state: &SimState, kind: MonsterKind) -> Vec<String> {
    state.monsters.values().filter(|m| m.kind == kind).map(|m| m.id.clone()).collect()
}

#[test]
fn a_golem_rises_on_a_grim_night_and_lies_low_by_day() {
    let mut sim = quiet(41);
    at_least_ten(&mut sim);
    // Night 12 is a grim night (a necromancer's, every fourth) and late enough for a golem.
    sim.state.tick = tick_at(12, 20.5);
    sim.state.director.next_wave_tick = sim.state.tick + 1;
    run(&mut sim, 3);
    let golems = kinds(&sim.state, MonsterKind::SkeletonGolem);
    assert_eq!(golems.len(), 1, "a golem rises in the necromancer's place");
    assert!(kinds(&sim.state, MonsterKind::Necromancer).is_empty());
    let golem = golems[0].clone();
    assert!(sim.state.monsters[&golem].lasting);
    assert!(sim.state.chronicle.iter().any(|e| e.text.contains("a skeleton golem has risen")));
    let next = sim.state.director.next_golem_night;
    assert!((20..=24).contains(&next), "the next not before night 20: {next}");
    assert!(sim.state.tick < sim.state.director.alarm_until_tick, "the village is rallied");

    // By day it does not crumble: it keeps to the ruins.
    for c in sim.state.characters.values_mut() {
        c.inside = Some(c.home_id.clone());
        c.intent = Some(Intent::new(IntentAction::Wait { minutes: 900.0 }, IntentSource::Routine));
    }
    sim.state.tick = tick_at(13, 9.0);
    let lair = get_place("ruins").door;
    let far = dist(sim.state.monsters[&golem].pos, lair);
    run(&mut sim, 1200);
    let m = &sim.state.monsters[&golem];
    assert_ne!(m.phase, MonsterPhase::Dying, "the dawn does not take it");
    let near = dist(m.pos, lair);
    assert!(near < far || near < 6.0, "{far} -> {near}");

    // And no second one while it is abroad, even on the next grim night it is due.
    sim.state.director.next_golem_night = 0;
    sim.state.tick = tick_at(16, 20.5);
    sim.state.director.next_wave_tick = sim.state.tick + 1;
    run(&mut sim, 3);
    assert_eq!(kinds(&sim.state, MonsterKind::SkeletonGolem).len(), 1);
}

#[test]
fn its_slam_catches_everyone_near_where_it_lands_and_is_never_blocked() {
    let mut sim = quiet(42);
    sim.state.tick = tick_at(2, 22.0);
    let green = get_place("green").door;
    let at = sim.state.nav.nearest_walkable(vec2(green.x, green.z + 4.0), 6).unwrap();
    let golem = with(&mut sim, |ctx| spawn_monster(ctx, MonsterKind::SkeletonGolem, at, 0.0, MonsterOrigin::Graveyard));
    let ids: Vec<String> = sim.state.order.iter().take(4).cloned().collect();
    // Three knights in front of it, close together; one far off. Knights block, but not this.
    let spots =
        [vec2(at.x, at.z + 2.4), vec2(at.x + 1.0, at.z + 2.6), vec2(at.x - 1.0, at.z + 2.2), vec2(at.x, at.z + 14.0)];
    for (id, spot) in ids.iter().zip(spots) {
        let c = sim.state.characters.get_mut(id).unwrap();
        c.class_id = ClassId::Knight;
        c.hp = 120.0;
        c.pos = spot;
        c.inside = None;
        c.asleep = false;
        c.profile.bravery = 1.0;
        c.combat.target_id = Some(golem.clone());
        c.intent = Some(Intent::new(IntentAction::Wait { minutes: 600.0 }, IntentSource::Routine));
    }
    {
        let m = sim.state.monsters.get_mut(&golem).unwrap();
        m.phase = MonsterPhase::Chase;
        m.target_id = Some(ids[0].clone());
        m.facing = 0.0;
    }
    // Pin everyone in place, and let it swing a few times.
    let mut hits = Vec::new();
    for _ in 0..(10 * 20) {
        for (id, spot) in ids.iter().zip(spots) {
            if let Some(c) = sim.state.characters.get_mut(id) {
                c.pos = spot;
            }
        }
        if let Some(m) = sim.state.monsters.get_mut(&golem) {
            m.pos = at;
            m.facing = 0.0;
        }
        for event in sim.step() {
            if let SimEvent::Hit { attacker_id, target_id, outcome, .. } = event
                && attacker_id == golem
            {
                hits.push((target_id, outcome));
            }
        }
    }
    assert!(!hits.is_empty(), "it swung");
    for id in &ids[..3] {
        assert!(hits.iter().any(|(t, _)| t == id), "{id} was caught under the slam");
    }
    assert!(!hits.iter().any(|(t, _)| *t == ids[3]), "nobody far off is hit");
    assert!(hits.iter().all(|(_, outcome)| *outcome != HitOutcome::Blocked), "a slam is never blocked");
}

#[test]
fn the_rally_brings_out_the_moderately_brave_for_a_boss() {
    let answers = |boss: bool| {
        let mut sim = quiet(43);
        sim.state.tick = tick_at(2, 22.0);
        let ruins = get_place("ruins").door;
        let kind = if boss { MonsterKind::SkeletonGolem } else { MonsterKind::SkeletonWarrior };
        let monster = with(&mut sim, |ctx| spawn_monster(ctx, kind, ruins, 0.0, MonsterOrigin::Ruins));
        sim.state.monsters.get_mut(&monster).unwrap().phase = MonsterPhase::Roam;
        sim.state.monsters.get_mut(&monster).unwrap().lasting = true;
        sim.state.director.alarm_until_tick = sim.state.tick + 20 * 60 * 40;
        sim.state.director.alarm_pos = Some(ruins);
        let id = sim.state.order[0].clone();
        {
            let c = sim.state.characters.get_mut(&id).unwrap();
            c.profile.bravery = 0.5;
            c.quirks.clear();
            c.inside = None;
            c.asleep = false;
            c.pos = get_place("green").door;
        }
        run(&mut sim, 20);
        let c = &sim.state.characters[&id];
        c.intent.as_ref().is_some_and(|i| matches!(i.action, IntentAction::Fight { .. }))
    };
    assert!(answers(true), "bravery 0.5 answers the alarm when a boss is abroad");
    assert!(!answers(false), "but not for the ordinary dead");
}

#[test]
fn the_warchief_leads_every_third_raid_from_day_12_and_his_fall_breaks_the_band() {
    let mut sim = quiet(44);
    at_least_ten(&mut sim);
    sim.state.director.raids = 2;
    sim.state.director.next_raid_tick = tick_at(12, 10.0);
    sim.state.tick = next_raid_tick(&sim.state) - 2;
    for c in sim.state.characters.values_mut() {
        c.profile.bravery = 0.0;
        c.inside = Some(c.home_id.clone());
        c.intent = Some(Intent::new(IntentAction::Wait { minutes: 900.0 }, IntentSource::Routine));
    }
    run(&mut sim, 5);
    let band = sim.state.raid.clone().expect("a band");
    let chief = band.warchief.clone().expect("led by a warchief");
    assert_eq!(sim.state.monsters[&chief].kind, MonsterKind::OrcWarchief);
    assert!(band.members.contains(&chief));
    assert_eq!(band.members.len() as i64, band.size + 1, "the warchief does not count in the band's size");

    let killer = sim.state.order[0].clone();
    with(&mut sim, |ctx| damage_monster(ctx, &chief, 10_000, &killer));
    let band = sim.state.raid.as_ref().unwrap();
    assert!(band.leaving, "with the warchief fallen the band breaks");
    assert_eq!(band.fallen, 0, "and he is not one of its fallen");

    // A raid that is not every third, or comes too early, has no warchief.
    let mut sim = quiet(45);
    at_least_ten(&mut sim);
    sim.state.director.raids = 1;
    sim.state.director.next_raid_tick = tick_at(12, 10.0);
    sim.state.tick = next_raid_tick(&sim.state) - 2;
    run(&mut sim, 5);
    assert!(sim.state.raid.as_ref().unwrap().warchief.is_none());
    let mut sim = quiet(46);
    at_least_ten(&mut sim);
    sim.state.director.raids = 2;
    sim.state.director.next_raid_tick = tick_at(8, 10.0);
    sim.state.tick = next_raid_tick(&sim.state) - 2;
    run(&mut sim, 5);
    assert!(sim.state.raid.as_ref().unwrap().warchief.is_none(), "not before day 12");
    assert!(game_time(sim.state.tick).day < 12);
}

#[test]
fn everyone_who_fought_a_boss_remembers_bringing_it_down() {
    let mut sim = quiet(47);
    sim.state.tick = tick_at(3, 22.0);
    let ruins = get_place("ruins").door;
    let golem = with(&mut sim, |ctx| spawn_monster(ctx, MonsterKind::SkeletonGolem, ruins, 0.0, MonsterOrigin::Ruins));
    let fighters: Vec<String> = sim.state.order.iter().take(3).cloned().collect();
    for id in &fighters[..2] {
        with(&mut sim, |ctx| damage_monster(ctx, &golem, 5, id));
    }
    assert_eq!(sim.state.monsters[&golem].foes, fighters[..2].to_vec());
    let saves = |events: &[SimEvent]| events.iter().any(|e| matches!(e, SimEvent::SavePoint { .. }));
    let mut events = Vec::new();
    {
        let mut ctx = Ctx::new(&mut sim.state, &mut events);
        damage_monster(&mut ctx, &golem, 10_000, &fighters[2]);
    }
    assert!(saves(&events), "the server saves");
    let line = sim.state.chronicle.iter().rev().find(|e| e.text.contains("skeleton golem fell")).expect("the news");
    for id in &fighters {
        let first = sim.state.characters[id].profile.name.split(' ').next().unwrap().to_owned();
        assert!(line.text.contains(&first), "{} names {first}", line.text);
        let memory = sim.state.characters[id]
            .memories
            .iter()
            .find(|m| m.text.contains("brought down the skeleton golem"))
            .expect("a memory");
        assert_eq!(memory.importance, 9);
        assert_eq!(memory.kind, MemoryKind::Event);
    }
}
