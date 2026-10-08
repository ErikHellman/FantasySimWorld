//! What the sim keeps about each of the dead, so a viewer who taps one can be told where it came from and what it
//! has done. None of it feeds back into the world.

use fsw_sim::Command;
use fsw_sim::Sim;
use fsw_sim::commands::TuningPatch;
use fsw_sim::state::{MonsterKind, MonsterOrigin, MonsterPhase};
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::monsters::spawn_monster;
use fsw_sim::util::{Ctx, living};
use fsw_testing::create_test_state;
use fsw_world::places::get_place;

#[test]
fn a_monster_remembers_when_and_where_it_rose() {
    let mut state = create_test_state(1);
    state.tick = tick_at(3, 21.5);
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut state, &mut events);
    let id = spawn_monster(&mut ctx, MonsterKind::SkeletonMinion, get_place("ruins").door, 0.0, MonsterOrigin::Ruins);
    let m = &ctx.state.monsters[&id];
    assert_eq!(m.rose_tick, Some(tick_at(3, 21.5)));
    assert_eq!(m.origin, Some(MonsterOrigin::Ruins));
    assert_eq!((m.kills, m.blows), (0, 0));
}

#[test]
fn a_monster_counts_the_blows_it_lands_and_the_villagers_it_slays() {
    let mut state = create_test_state(1);
    // Late at night everyone is home but one, who stands in the open at the green on their last hit point, with no
    // potion to drink.
    state.tick = tick_at(2, 23.0);
    state.director.next_wave_tick = i64::MAX;
    let victim = living(&state).next().expect("a villager").id.clone();
    let green = get_place("green").origin;
    for c in state.characters.values_mut() {
        if c.id == victim {
            c.pos = green;
            c.inside = None;
            c.asleep = false;
            c.hp = 1.0;
            c.inventory.potion = 0;
        } else {
            c.inside = Some(c.home_id.clone());
        }
    }
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut state, &mut events);
    let monster = spawn_monster(&mut ctx, MonsterKind::SkeletonWarrior, green, 0.0, MonsterOrigin::Graveyard);
    let m = ctx.m(&monster);
    m.phase = MonsterPhase::Chase;
    m.target_id = Some(victim.clone());
    // Too much for one villager to cut down before it strikes.
    m.hp = 1000.0;
    let mut sim = Sim::new(state);
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    for _ in 0..600 {
        if !sim.state.characters.contains_key(&victim) {
            break;
        }
        sim.step();
    }
    assert!(!sim.state.characters.contains_key(&victim), "the blow never landed");
    let m = &sim.state.monsters[&monster];
    assert_eq!(m.kills, 1);
    assert!(m.blows >= 1);
}
