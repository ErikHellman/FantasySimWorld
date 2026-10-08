//! Ghosts: some of those the dead cut down walk again for a few nights, frighten and chill the living, and find
//! rest. See `systems::ghosts`.

use fsw_core::math2d::dist;
use fsw_sim::Sim;
use fsw_sim::state::{FightMode, GhostPhase, Id, IntentAction, SimState};
use fsw_sim::systems::combat::{Assailant, damage_character, max_hp};
use fsw_sim::systems::death::kill_character;
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::ghosts::{self, Haunt};
use fsw_sim::util::{Ctx, living};
use fsw_testing::create_test_state;
use fsw_world::places::get_place;

/// A quiet night: no waves of the dead, so only the ghost walks.
fn quiet(seed: u32, day: i64, hour: f64) -> SimState {
    let mut state = create_test_state(seed);
    state.tick = tick_at(day, hour);
    state.director.next_wave_tick = i64::MAX;
    state
}

/// Kills a villager and binds their ghost, whatever the roll would have said.
fn haunted_by(state: &mut SimState, victim: &str) -> Id {
    let mut events = Vec::new();
    let mut ctx = Ctx::new(state, &mut events);
    let haunt = Haunt::of(&ctx, victim);
    kill_character(&mut ctx, victim, "slain by a Skeleton Warrior at the green", None);
    ghosts::bind(&mut ctx, haunt)
}

/// Sends everyone indoors to bed but `awake`, who stands in the open at `at`.
fn alone_outside(state: &mut SimState, awake: &str, at: fsw_core::Vec2) {
    for c in state.characters.values_mut() {
        if c.id == awake {
            c.pos = at;
            c.inside = None;
            c.asleep = false;
            c.intent = None;
            c.queue.clear();
        } else {
            c.inside = Some(c.home_id.clone());
            c.asleep = true;
        }
    }
}

fn first_victim(state: &SimState, returns: bool) -> Id {
    living(state)
        .find(|c| !c.is_child() && ghosts::returns(state.seed, &c.id) == returns)
        .expect("a villager with that roll")
        .id
        .clone()
}

fn monster_blow(state: &mut SimState, victim: &str) {
    let mut events = Vec::new();
    let mut ctx = Ctx::new(state, &mut events);
    ctx.c(victim).hp = 1.0;
    let by = Assailant { id: "m999".into(), label: "Skeleton Warrior".into(), character: None };
    damage_character(&mut ctx, victim, 20, by, FightMode::Monster);
}

#[test]
fn about_one_in_three_of_the_slain_return() {
    let returned = (0..3000).filter(|i| ghosts::returns(7, &format!("c{i}"))).count();
    assert!((850..1150).contains(&returned), "{returned} of 3000 returned");
    // The roll belongs to the world: another seed brings back other people.
    let other = (0..3000).filter(|i| ghosts::returns(8, &format!("c{i}"))).count();
    let both =
        (0..3000).filter(|i| ghosts::returns(7, &format!("c{i}")) && ghosts::returns(8, &format!("c{i}"))).count();
    assert!(both < other.min(returned) / 2, "the seed changes nothing");
}

#[test]
fn only_those_slain_by_the_dead_come_back_and_only_when_the_roll_says_so() {
    let mut state = quiet(1, 2, 23.0);
    let returning = first_victim(&state, true);
    let resting = first_victim(&state, false);
    monster_blow(&mut state, &returning);
    monster_blow(&mut state, &resting);
    assert!(!state.characters.contains_key(&returning) && !state.characters.contains_key(&resting));
    assert_eq!(state.ghosts.len(), 1);
    let ghost = state.ghosts.values().next().unwrap();
    assert_eq!(ghost.was_id, returning);
    assert_eq!(ghost.phase, GhostPhase::Waiting);
    assert!(!ghosts::visible(ghost));
    // It waits by its grave.
    let grave = state.graves.iter().find(|g| g.character_id == returning).expect("a grave");
    let plot = get_place("graveyard").grave_plots[grave.plot as usize].pos;
    assert!(dist(ghost.rest, plot) < 4.0);

    // Murdered by a villager, the same villager would have stayed in the grave.
    let mut state = quiet(1, 2, 23.0);
    let killer = living(&state).find(|c| c.id != returning).unwrap().id.clone();
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut state, &mut events);
    ctx.c(&returning).hp = 1.0;
    let by = Assailant { id: killer.clone(), label: "Someone".into(), character: Some(killer) };
    damage_character(&mut ctx, &returning, 20, by, FightMode::Lethal);
    assert!(state.ghosts.is_empty());
}

#[test]
fn a_child_slain_by_the_dead_rests() {
    let mut state = quiet(1, 2, 23.0);
    let mut picked = None;
    for c in state.characters.values_mut() {
        if !ghosts::returns(1, &c.id) {
            continue;
        }
        c.child =
            Some(fsw_sim::state::Childhood { next_birthday_day: 9, mischief: 0.0, parents: Vec::new(), plan: None });
        picked = Some(c.id.clone());
        break;
    }
    let child = picked.expect("someone whose roll says they return");
    monster_blow(&mut state, &child);
    assert!(!state.characters.contains_key(&child));
    assert!(state.ghosts.is_empty());
}

#[test]
fn a_ghost_walks_by_night_rests_by_day_and_finds_rest_after_two_or_three_nights() {
    let mut state = quiet(3, 2, 10.0);
    let victim = first_victim(&state, true);
    let id = haunted_by(&mut state, &victim);
    let mut sim = Sim::new(state);
    let mut nights = 0;
    let mut was_abroad = false;
    let mut chronicle = Vec::new();
    while sim.state.ghosts.contains_key(&id) {
        assert!(sim.state.tick < tick_at(7, 6.0), "the ghost never found rest");
        for event in sim.step() {
            if let fsw_sim::SimEvent::Chronicle { entry } = event {
                chronicle.push(entry.text);
            }
        }
        let Some(ghost) = sim.state.ghosts.get(&id) else { break };
        let hour = fsw_core::time::game_time(sim.state.tick).hour;
        let abroad = ghost.phase == GhostPhase::Abroad;
        if abroad && !was_abroad {
            nights += 1;
            assert!(!(5.0..20.5).contains(&hour), "rose at {hour}");
        }
        if (6.0..19.0).contains(&hour) {
            assert!(!ghosts::visible(ghost) || ghost.phase == GhostPhase::Fading, "seen by day at {hour}");
        }
        was_abroad = abroad;
        // Nobody ever takes up arms against it.
        for c in living(&sim.state) {
            if let Some(IntentAction::Fight { target_id, .. }) = c.intent.as_ref().map(|i| &i.action) {
                assert_ne!(target_id, &id);
            }
        }
    }
    assert!((2..=3).contains(&nights), "walked {nights} nights");
    let rose = chronicle.iter().filter(|t| t.contains("rose from the grave")).count();
    let rest = chronicle.iter().filter(|t| t.contains("found rest at last")).count();
    assert_eq!((rose, rest), (1, 1), "{chronicle:?}");
    assert!(sim.state.monsters.is_empty(), "a ghost is not one of the dead that fight");
}

/// A ghost already abroad at the green, at night, with one villager awake beside it.
fn face_to_face(seed: u32, bravery: f64, close: bool) -> (SimState, Id, Id) {
    let mut state = quiet(seed, 3, 23.0);
    let victim = first_victim(&state, true);
    let witness = living(&state).find(|c| c.id != victim && !c.is_child()).unwrap().id.clone();
    let id = haunted_by(&mut state, &victim);
    let green = get_place("green").origin;
    alone_outside(&mut state, &witness, green);
    let c = state.characters.get_mut(&witness).unwrap();
    c.profile.bravery = bravery;
    c.quirks.clear();
    let ghost = state.ghosts.get_mut(&id).unwrap();
    ghost.phase = GhostPhase::Abroad;
    ghost.nights_left = 2;
    ghost.pos = fsw_core::vec2(green.x + 5.0, green.z);
    ghost.calm_until_tick = i64::MAX;
    if close {
        ghost.close = vec![witness.clone()];
    } else {
        ghost.close.clear();
    }
    (state, id, witness)
}

#[test]
fn the_timid_run_from_a_ghost_and_those_who_loved_them_grieve() {
    let (state, _, witness) = face_to_face(1, 0.0, false);
    let mut sim = Sim::new(state);
    for _ in 0..20 {
        sim.step();
    }
    let c = &sim.state.characters[&witness];
    assert!(matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::Flee)), "{:?}", c.intent);
    assert!(c.memories.iter().any(|m| m.text.contains("and ran")));

    let (state, _, witness) = face_to_face(1, 0.0, true);
    let mut sim = Sim::new(state);
    for _ in 0..20 {
        sim.step();
    }
    let c = &sim.state.characters[&witness];
    assert!(!matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::Flee)));
    assert!(c.memories.iter().any(|m| m.text.contains("looked straight through me")));
}

#[test]
fn a_ghosts_touch_chills_but_never_kills() {
    let (mut state, id, witness) = face_to_face(1, 1.0, false);
    let c = state.characters.get_mut(&witness).unwrap();
    let cap = max_hp(c);
    c.hp = cap * 0.27;
    c.inventory.potion = 0;
    c.needs.energy = 60.0;
    let pos = c.pos;
    let ghost = state.ghosts.get_mut(&id).unwrap();
    ghost.calm_until_tick = 0;
    ghost.pos = fsw_core::vec2(pos.x + 1.0, pos.z);
    let mut sim = Sim::new(state);
    for _ in 0..20 {
        sim.step();
    }
    let c = &sim.state.characters[&witness];
    assert!(c.hp >= cap * 0.25 - 1e-9 && c.hp < cap * 0.27, "hp {} of {cap}", c.hp);
    assert!(c.needs.energy <= 35.5, "energy {}", c.needs.energy);
    assert!(c.memories.iter().any(|m| m.text.contains("passed right through me")));
    assert!(sim.state.ghosts[&id].calm_until_tick > sim.state.tick);
}

#[test]
fn a_haunting_survives_a_save_and_load_in_the_middle() {
    let (state, _, _) = face_to_face(5, 0.3, false);
    let mut straight = Sim::new(state.clone());
    let mut first = Sim::new(state);
    for _ in 0..3000 {
        straight.step();
    }
    for _ in 0..1500 {
        first.step();
    }
    assert!(!first.state.ghosts.is_empty());
    let saved: SimState = serde_json::from_str(&serde_json::to_string(&first.state).unwrap()).unwrap();
    let mut resumed = Sim::new(saved);
    for _ in 0..1500 {
        resumed.step();
    }
    assert!(resumed.state == straight.state, "a reloaded haunting went a different way");
}
