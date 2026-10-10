//! Ambitions: every grown villager has one long goal, the chronicle tells when it is won, and after a month
//! without it they give it up.

use fsw_core::time::{TICKS_PER_DAY, game_time};
use fsw_sim::Command;
use fsw_sim::Sim;
use fsw_sim::state::{AmbitionDraft, AmbitionKind, AmbitionOutcome, ChronicleKind, SimState};
use fsw_sim::systems::ambitions::{GIVE_UP_DAYS, WEALTH_MORE, ambition_line, begin, progress, update_ambitions};
use fsw_sim::util::{Ctx, adults, living};
use fsw_testing::create_test_state;

/// Runs the ambitions alone at the top of the next game hour.
fn hour(state: &mut SimState) {
    let hour = 60 * 20;
    state.tick += hour - state.tick.rem_euclid(hour);
    let mut events = Vec::new();
    update_ambitions(&mut Ctx::new(state, &mut events));
}

fn set(state: &mut SimState, id: &str, kind: AmbitionKind, since_day: i64) {
    let draft = AmbitionDraft { kind, words: "I want it.".into() };
    let mut ambition = begin(state, &state.characters[id], draft);
    ambition.since_day = since_day;
    state.characters.get_mut(id).unwrap().ambition = Some(ambition);
}

#[test]
fn every_grown_villager_sets_out_with_an_ambition_and_no_child_does() {
    let state = create_test_state(4);
    for c in living(&state) {
        assert_eq!(c.ambition.is_some(), !c.is_child(), "{}", c.profile.name);
    }
    // Picked from the seed and the name, so the same world gives the same ambitions.
    let again = create_test_state(4);
    for c in adults(&state) {
        assert_eq!(c.ambition, again.characters[&c.id].ambition);
    }
    assert!(ambition_line(&state, adults(&state).next().unwrap()).unwrap().starts_with("What you want most"));
}

#[test]
fn a_fortune_put_by_is_won_and_told() {
    let mut state = create_test_state(4);
    let id = adults(&state).next().unwrap().id.clone();
    set(&mut state, &id, AmbitionKind::Wealth, 0);
    hour(&mut state);
    assert_eq!(state.characters[&id].ambition.as_ref().unwrap().outcome, None);
    state.characters.get_mut(&id).unwrap().gold += WEALTH_MORE / 2;
    let c = &state.characters[&id];
    assert_eq!(progress(&state, c, c.ambition.as_ref().unwrap()), (WEALTH_MORE / 2, WEALTH_MORE));
    state.characters.get_mut(&id).unwrap().gold += WEALTH_MORE;
    hour(&mut state);
    let ambition = state.characters[&id].ambition.clone().unwrap();
    assert_eq!(ambition.outcome, Some(AmbitionOutcome::Won));
    assert_eq!(ambition.outcome_day, game_time(state.tick).day);
    assert!(state.chronicle.iter().any(|e| e.kind == ChronicleKind::Trade && e.text.contains("set out to do")));
    assert!(state.characters[&id].memories.iter().any(|m| m.text.starts_with("I did it.")));
    assert!(ambition_line(&state, &state.characters[&id]).unwrap().contains("achieved"));
}

#[test]
fn a_month_without_it_and_they_give_it_up() {
    let mut state = create_test_state(4);
    let id = adults(&state).next().unwrap().id.clone();
    let today = game_time(state.tick).day;
    set(&mut state, &id, AmbitionKind::Glory, today - GIVE_UP_DAYS);
    hour(&mut state);
    assert_eq!(state.characters[&id].ambition.as_ref().unwrap().outcome, Some(AmbitionOutcome::Abandoned));
    assert!(state.chronicle.iter().any(|e| e.text.contains("has given up on it")));
}

#[test]
fn an_ambition_already_met_the_day_it_is_set_becomes_a_fortune() {
    let mut state = create_test_state(4);
    let id = adults(&state).next().unwrap().id.clone();
    let other = adults(&state).nth(1).unwrap().id.clone();
    state.characters.get_mut(&id).unwrap().spouse_id = Some(other);
    let today = game_time(state.tick).day;
    set(&mut state, &id, AmbitionKind::Love, today);
    hour(&mut state);
    let ambition = state.characters[&id].ambition.clone().unwrap();
    assert_eq!((ambition.kind, ambition.outcome), (AmbitionKind::Wealth, None));
}

#[test]
fn the_models_ambition_comes_with_a_newcomer() {
    let mut sim = Sim::new(create_test_state(4));
    let mut v =
        fsw_sim::systems::founding::invent_villager(&mut Ctx::new(&mut sim.state.clone(), &mut Vec::new()), false);
    v.ambition = Some(AmbitionDraft { kind: AmbitionKind::Friends, words: "Friends for life.".into() });
    let name = v.name.clone();
    sim.enqueue(Command::NewcomerDraft(Box::new(v)));
    // Arrivals come when a bed is free: free one.
    let gone = adults(&sim.state).last().unwrap().id.clone();
    {
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut sim.state, &mut events);
        fsw_sim::systems::death::kill_character(&mut ctx, &gone, "old age", None);
    }
    for _ in 0..TICKS_PER_DAY * 3 {
        sim.step();
        if living(&sim.state).any(|c| c.profile.name == name) {
            break;
        }
    }
    let c = living(&sim.state).find(|c| c.profile.name == name).expect("the newcomer never came");
    let ambition = c.ambition.as_ref().unwrap();
    assert_eq!((ambition.kind, ambition.words.as_str()), (AmbitionKind::Friends, "Friends for life."));
}
