//! Feuds and love triangles: each rung of a grievance shows in what the villagers do to each other, and someone
//! sweet on a villager who is with another speaks up.

use fsw_core::time::{TICKS_PER_GAME_MINUTE, game_time};
use fsw_core::{Vec2, vec2};
use fsw_sim::SimEvent;
use fsw_sim::state::{ChronicleKind, FightMode, IntentAction, SimState};
use fsw_sim::systems::feuds::{SCENE_GAP, WINDOW, heartbroken_at, update_feuds, watch_them_wed};
use fsw_sim::util::{Ctx, rel};
use fsw_testing::create_test_state;
use fsw_world::places::get_place;

/// The test village at ten in the morning, everyone indoors but `n` villagers, who are out on the green and free.
fn morning(n: usize) -> (SimState, Vec<String>) {
    let mut state = create_test_state(6);
    while !(game_time(state.tick).hour >= 10.0 && game_time(state.tick).hour < 11.0) {
        state.tick += TICKS_PER_GAME_MINUTE;
    }
    state.director.founding = false;
    let green = get_place("green").origin;
    let ids = state.order.clone();
    for (at, id) in ids.iter().enumerate() {
        let c = state.characters.get_mut(id).unwrap();
        c.asleep = false;
        c.intent = None;
        c.queue.clear();
        c.convo_id = None;
        c.inside = if at < n { None } else { Some(c.home_id.clone()) };
        c.pos = vec2(green.x + at as f64 * 1.5, green.z);
        c.gold = 20;
    }
    (state, ids[..n].to_vec())
}

fn place(state: &mut SimState, id: &str, pos: Vec2) {
    state.characters.get_mut(id).unwrap().pos = pos;
}

/// Runs the feuds system for a stretch of game minutes, so every pair gets its moment.
fn run(state: &mut SimState, minutes: i64) -> Vec<SimEvent> {
    let mut events = Vec::new();
    for _ in 0..minutes {
        state.tick += TICKS_PER_GAME_MINUTE;
        update_feuds(&mut Ctx::new(state, &mut events));
    }
    events
}

fn chronicled(state: &SimState, kind: ChronicleKind, words: &str) -> usize {
    state.chronicle.iter().filter(|e| e.kind == kind && e.text.contains(words)).count()
}

fn set(state: &mut SimState, from: &str, to: &str, f: impl FnOnce(&mut fsw_sim::state::Relationship)) {
    f(rel(state.characters.get_mut(from).unwrap(), to));
}

#[test]
fn tension_shows_as_a_snub_and_the_snub_stings() {
    let (mut state, ids) = morning(2);
    let (a, b) = (&ids[0], &ids[1]);
    set(&mut state, a, b, |r| r.grievance = 25.0);
    let rng = state.rng;
    let events = run(&mut state, WINDOW);
    assert!(events.iter().any(|e| matches!(e, SimEvent::Speech { speaker_id, .. } if speaker_id == a)));
    assert_eq!(chronicled(&state, ChronicleKind::Conflict, "snubbed"), 1);
    assert!(state.characters[b].relationships[a].grievance >= 2.0, "being snubbed stings");
    assert!(state.characters[a].relationships[b].grievance < 25.0, "and snubbing vents a little");
    assert!(state.characters[b].memories.iter().any(|m| m.text.contains("snubbed me")));
    assert_eq!(state.rng, rng, "nothing here draws from the RNG");
    // Not again until the gap has passed, and later snubs are not news.
    run(&mut state, WINDOW * 3);
    let after = state.characters[b].relationships[a].grievance;
    run(&mut state, SCENE_GAP / TICKS_PER_GAME_MINUTE + WINDOW);
    assert!(state.characters[b].relationships[a].grievance > after, "snubbed again after the gap");
    assert_eq!(chronicled(&state, ChronicleKind::Conflict, "snubbed"), 1);
}

#[test]
fn a_grudge_is_aired_in_front_of_onlookers() {
    let (mut state, ids) = morning(4);
    let (a, b) = (&ids[0], &ids[1]);
    set(&mut state, a, b, |r| r.grievance = 45.0);
    run(&mut state, WINDOW);
    assert_eq!(chronicled(&state, ChronicleKind::Conflict, "shouting match"), 1);
    for onlooker in &ids[2..] {
        assert!(state.characters[onlooker].memories.iter().any(|m| m.text.contains("shouting at each other")));
    }
    assert!(state.characters[b].relationships[a].grievance >= 4.0);
}

#[test]
fn without_an_audience_a_grudge_is_only_a_snub() {
    let (mut state, ids) = morning(2);
    set(&mut state, &ids[0], &ids[1], |r| r.grievance = 45.0);
    run(&mut state, WINDOW);
    assert_eq!(chronicled(&state, ChronicleKind::Conflict, "shouting match"), 0);
    assert_eq!(chronicled(&state, ChronicleKind::Conflict, "snubbed"), 1);
}

#[test]
fn the_hot_headed_challenge_a_feud_to_fists() {
    let (mut state, ids) = morning(4);
    let (a, b) = (&ids[0], &ids[1]);
    state.characters.get_mut(a).unwrap().profile.temper = 0.8;
    set(&mut state, a, b, |r| r.grievance = 70.0);
    run(&mut state, WINDOW);
    assert_eq!(chronicled(&state, ChronicleKind::Conflict, "challenged"), 1);
    let fight = state.characters[a].intent.as_ref().map(|i| i.action.clone());
    assert!(matches!(fight, Some(IntentAction::Fight { ref target_id, mode: FightMode::Brawl }) if target_id == b));
}

#[test]
fn the_cool_headed_rob_the_house_and_the_wrong_one_may_be_blamed() {
    let (mut state, ids) = morning(3);
    let (a, b, c) = (&ids[0], &ids[1], &ids[2]);
    let thief = state.characters.get_mut(a).unwrap();
    (thief.profile.temper, thief.profile.bravery) = (0.2, 0.3);
    assert_ne!(state.characters[a].home_id, state.characters[b].home_id);
    set(&mut state, a, b, |r| r.grievance = 70.0);
    // The one robbed resents someone else more.
    set(&mut state, b, c, |r| r.grievance = 50.0);
    set(&mut state, b, a, |r| r.grievance = 30.0);
    let door = get_place(&state.characters[b].home_id).door;
    place(&mut state, a, door);
    // b and c are out on the west road, well away from the house.
    let road = get_place("west_road").origin;
    place(&mut state, b, road);
    place(&mut state, c, vec2(road.x + 2.0, road.z));
    run(&mut state, WINDOW);
    assert_eq!(chronicled(&state, ChronicleKind::Conflict, "crept into"), 1);
    let (thief, robbed) = (state.characters[a].gold, state.characters[b].gold);
    assert!(thief > 20 && robbed < 20 && thief + robbed == 40, "the coin moved: {thief} and {robbed}");
    let name = fsw_sim::util::first_name(&state.characters[c]).to_owned();
    assert!(state.characters[b].memories.iter().any(|m| m.text.contains(&format!("I'd wager it was {name}"))));
}

/// Three villagers out: `a` is sweet on `b`, who is with `c`, and `c` is away across the valley.
fn triangle() -> (SimState, String, String, String) {
    let (mut state, ids) = morning(3);
    let (a, b, c) = (ids[0].clone(), ids[1].clone(), ids[2].clone());
    for (x, y) in [(&b, &c), (&c, &b)] {
        state.characters.get_mut(x).unwrap().partner_id = Some(y.clone());
        set(&mut state, x, y, |r| (r.affinity, r.attraction) = (50.0, 50.0));
    }
    set(&mut state, &a, &b, |r| (r.affinity, r.attraction) = (40.0, 60.0));
    place(&mut state, &c, get_place("west_road").origin);
    (state, a, b, c)
}

#[test]
fn a_jilted_admirer_speaks_up_and_is_turned_away() {
    let (mut state, a, b, c) = triangle();
    set(&mut state, &b, &a, |r| (r.affinity, r.attraction) = (10.0, 0.0));
    run(&mut state, WINDOW);
    assert_eq!(chronicled(&state, ChronicleKind::Romance, "turned them away"), 1);
    assert_eq!(state.characters[&b].partner_id.as_deref(), Some(c.as_str()));
    assert_eq!(state.characters[&a].relationships[&b].attraction, 48.0, "each refusal cools it");
    assert!(state.characters[&a].relationships[&c].grievance >= 8.0, "and turns it against the one chosen");
    // Not again for a couple of days.
    run(&mut state, WINDOW * 6);
    assert_eq!(chronicled(&state, ChronicleKind::Romance, "turned them away"), 1);
}

#[test]
fn now_and_then_the_admirer_is_chosen_instead() {
    let (mut state, a, b, c) = triangle();
    set(&mut state, &b, &a, |r| (r.affinity, r.attraction) = (70.0, 60.0));
    run(&mut state, WINDOW);
    assert_eq!(chronicled(&state, ChronicleKind::Romance, "has left"), 1);
    assert_eq!(state.characters[&b].partner_id.as_deref(), Some(a.as_str()));
    assert_eq!(state.characters[&c].partner_id, None);
    assert!(state.characters[&c].relationships[&a].grievance >= 25.0);
}

#[test]
fn nobody_speaks_up_with_the_partner_standing_by() {
    let (mut state, a, b, c) = triangle();
    let near = state.characters[&b].pos;
    place(&mut state, &c, vec2(near.x + 3.0, near.z));
    set(&mut state, &b, &a, |r| (r.affinity, r.attraction) = (70.0, 60.0));
    run(&mut state, WINDOW);
    assert_eq!(chronicled(&state, ChronicleKind::Romance, "has left"), 0);
    assert_eq!(state.characters[&b].partner_id.as_deref(), Some(c.as_str()));
}

#[test]
fn a_heartbroken_guest_cannot_cheer_at_the_wedding() {
    let (mut state, a, b, c) = triangle();
    assert_eq!(heartbroken_at(&state, &a, &b, &c), Some(b.as_str()));
    assert_eq!(heartbroken_at(&state, &c, &a, &b), Some(b.as_str()));
    let mut events = Vec::new();
    watch_them_wed(&mut Ctx::new(&mut state, &mut events), &a, &b, &c);
    assert_eq!(chronicled(&state, ChronicleKind::Romance, "with a broken heart"), 1);
    assert!(state.characters[&a].relationships[&c].grievance >= 10.0);
}

#[test]
fn a_grudge_against_the_dead_makes_no_scene() {
    let (mut state, ids) = morning(2);
    set(&mut state, &ids[0], "gone", |r| (r.grievance, r.attraction) = (90.0, 90.0));
    run(&mut state, WINDOW);
    assert!(state.chronicle.iter().all(|e| e.kind != ChronicleKind::Conflict && e.kind != ChronicleKind::Romance));
}

#[test]
fn nobody_is_called_out_in_the_middle_of_their_work() {
    let (mut state, ids) = morning(4);
    let (a, b) = (&ids[0], &ids[1]);
    state.characters.get_mut(a).unwrap().profile.temper = 0.8;
    set(&mut state, a, b, |r| r.grievance = 70.0);
    state.characters.get_mut(b).unwrap().intent =
        Some(fsw_sim::state::Intent::new(IntentAction::Shelter, fsw_sim::state::IntentSource::Routine));
    run(&mut state, WINDOW);
    let name = fsw_sim::util::first_name(&state.characters[a]).to_owned();
    assert_eq!(chronicled(&state, ChronicleKind::Conflict, &format!("{name} challenged")), 0);
    assert_eq!(chronicled(&state, ChronicleKind::Conflict, &format!("{name} and")), 1, "they have it out in words");
}
