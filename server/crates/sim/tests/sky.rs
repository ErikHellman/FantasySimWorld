//! Believers and sceptics: hints that someone beyond the sky is watching harden into stances, believers speak to
//! the sky and feel the crowd come and go, and sceptics answer them.

use fsw_core::time::game_time;
use fsw_sim::SimEvent;
use fsw_sim::state::{BoardReaction, ChronicleKind, SimState, Stance};
use fsw_sim::systems::sky::{set_audience, stance_line, took_hint, update_sky};
use fsw_sim::util::Ctx;
use fsw_testing::create_test_state;
use fsw_world::places::get_place;

/// The test village at eight in the morning, with two villagers out on the green and free.
fn morning() -> (SimState, String, String) {
    let mut state = create_test_state(6);
    while !(game_time(state.tick).hour >= 8.0 && game_time(state.tick).hour < 9.0) {
        state.tick += 20;
    }
    let green = get_place("green").origin;
    let (a, b) = (state.order[0].clone(), state.order[1].clone());
    for (id, dx) in [(&a, 0.0), (&b, 2.0)] {
        let c = state.characters.get_mut(id).unwrap();
        c.inside = None;
        c.asleep = false;
        c.intent = None;
        c.queue.clear();
        c.convo_id = None;
        c.pos = fsw_core::vec2(green.x + dx, green.z);
    }
    (state, a, b)
}

fn with<R>(state: &mut SimState, f: impl FnOnce(&mut Ctx) -> R) -> (R, Vec<SimEvent>) {
    let mut events = Vec::new();
    let result = f(&mut Ctx::new(state, &mut events));
    (result, events)
}

fn said(events: &[SimEvent], who: &str) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            SimEvent::Speech { speaker_id, text, .. } if speaker_id == who => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_couple_of_hints_settle_a_stance_for_good() {
    let (mut state, a, _) = morning();
    let c = state.characters.get_mut(&a).unwrap();
    (c.profile.bravery, c.profile.temper) = (0.5, 0.5);
    with(&mut state, |ctx| took_hint(ctx, &a, BoardReaction::Intrigued));
    assert_eq!(state.characters[&a].sky.stance, None, "settled after one hint");
    with(&mut state, |ctx| took_hint(ctx, &a, BoardReaction::Shocked));
    assert_eq!(state.characters[&a].sky.stance, Some(Stance::Believer));
    assert!(state.chronicle.iter().any(|e| e.kind == ChronicleKind::World && e.text.contains("has come to believe")));
    assert!(state.characters[&a].memories.iter().any(|m| m.text.contains("beyond the sky is watching us")));
    // For good: later hints change nothing.
    for _ in 0..3 {
        with(&mut state, |ctx| took_hint(ctx, &a, BoardReaction::Laugh));
    }
    assert_eq!(state.characters[&a].sky.stance, Some(Stance::Believer));
    assert!(stance_line(&state, &state.characters[&a]).unwrap().contains("watching the village"));
}

#[test]
fn the_angry_turn_defiant_and_the_bold_laugh_it_off() {
    let (mut state, a, b) = morning();
    let c = state.characters.get_mut(&a).unwrap();
    (c.profile.bravery, c.profile.temper) = (0.5, 0.8);
    let c = state.characters.get_mut(&b).unwrap();
    (c.profile.bravery, c.profile.temper) = (0.9, 0.3);
    for _ in 0..2 {
        with(&mut state, |ctx| took_hint(ctx, &a, BoardReaction::Angry));
        with(&mut state, |ctx| took_hint(ctx, &b, BoardReaction::Intrigued));
    }
    assert_eq!(state.characters[&a].sky.stance, Some(Stance::Defiant));
    assert_eq!(state.characters[&b].sky.stance, Some(Stance::Sceptic));
}

#[test]
fn a_believer_speaks_to_the_sky_and_a_sceptic_answers() {
    let (mut state, a, b) = morning();
    state.characters.get_mut(&a).unwrap().sky.stance = Some(Stance::Believer);
    state.characters.get_mut(&b).unwrap().sky.stance = Some(Stance::Sceptic);
    let mut spoke = Vec::new();
    let mut answered = Vec::new();
    // Three hours of the guard alone, with nobody moving.
    for _ in 0..180 {
        let ((), events) = with(&mut state, update_sky);
        spoke.extend(said(&events, &a));
        answered.extend(said(&events, &b));
        state.tick += 20;
    }
    assert!(!spoke.is_empty(), "the believer never spoke to the sky");
    assert_eq!(answered.len(), spoke.len(), "the sceptic did not answer every time");
    // A sceptic alone says nothing to the sky.
    state.characters.get_mut(&a).unwrap().sky.stance = Some(Stance::Sceptic);
    for _ in 0..360 {
        let ((), events) = with(&mut state, update_sky);
        assert!(events.is_empty());
        state.tick += 20;
    }
}

#[test]
fn a_believer_feels_a_crowd_gather_once_a_day() {
    let (mut state, a, _) = morning();
    state.characters.get_mut(&a).unwrap().sky.stance = Some(Stance::Believer);
    let ((), events) = with(&mut state, |ctx| set_audience(ctx, 2));
    assert!(said(&events, &a).is_empty(), "a few watching is not a crowd");
    let ((), events) = with(&mut state, |ctx| set_audience(ctx, 3));
    assert_eq!(said(&events, &a), vec!["The sky feels crowded today.".to_owned()]);
    assert!(state.chronicle.iter().any(|e| e.text.contains("looks up at the sky")));
    assert!(stance_line(&state, &state.characters[&a]).unwrap().contains("crowded"));
    // Once a day at most, however often the crowd comes and goes.
    with(&mut state, |ctx| set_audience(ctx, 0));
    let ((), events) = with(&mut state, |ctx| set_audience(ctx, 3));
    assert!(said(&events, &a).is_empty());
    assert_eq!(state.audience.crowd, 3);
}
