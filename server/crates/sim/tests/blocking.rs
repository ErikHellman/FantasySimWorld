//! Someone standing in a villager's way: the walker stops and says so, the one in the way steps
//! aside (or is squeezed past), and a hot-tempered walker gets angry about it.

use fsw_core::math2d::dist;
use fsw_core::time::seconds_to_ticks;
use fsw_core::{Vec2, vec2};
use fsw_sim::commands::TuningPatch;
use fsw_sim::events::Emote;
use fsw_sim::state::{Id, Intent, IntentAction, IntentSource, SimState};
use fsw_sim::util::{first_name, rel, rel_or_default};
use fsw_sim::{Command, Sim, SimEvent};
use fsw_testing::create_test_state;
use fsw_world::places::find_place;

/// A village of two adults, `a` on the green and `b` idle, with no monsters and no chatter.
struct Scene {
    sim: Sim,
    a: Id,
    b: Id,
}

fn scene(seed: u32) -> Scene {
    let mut state = create_test_state(seed);
    let adults: Vec<Id> = state.order.iter().filter(|id| !state.characters[*id].is_child()).cloned().collect();
    let (a, b) = (adults[0].clone(), adults[1].clone());
    // Only the two of them, so nobody else wanders into the scene.
    state.order.retain(|id| id == &a || id == &b);
    state.characters.retain(|id, _| id == &a || id == &b);
    for id in [&a, &b] {
        let c = state.characters.get_mut(id).unwrap();
        c.partner_id = None;
        c.spouse_id = None;
        c.inside = None;
        c.asleep = false;
        c.relationships.clear();
        c.profile.temper = 0.0;
        c.profile.spite = 0.0;
    }
    state.director.next_wave_tick = i64::MAX;
    let green = find_place("green").unwrap();
    let start = state.nav.nearest_walkable(green.door, 4).unwrap();
    state.characters.get_mut(&a).unwrap().pos = start;
    state.characters.get_mut(&b).unwrap().pos = vec2(start.x + 30.0, start.z + 30.0);
    let mut sim = Sim::new(state);
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    sim.step();
    Scene { sim, a, b }
}

fn set_intent(sim: &mut Sim, id: &str, action: IntentAction) {
    let epoch = sim.state.characters[id].epoch;
    sim.enqueue(Command::SetIntents { id: id.into(), epoch, intents: vec![Intent::new(action, IntentSource::Llm)] });
}

/// The point `along` units down a route that starts at `from`.
fn point_along(from: Vec2, path: &[Vec2], along: f64) -> Vec2 {
    let mut left = along;
    let mut at = from;
    for &next in path {
        let d = dist(at, next);
        if d >= left {
            return vec2(at.x + (next.x - at.x) / d * left, at.z + (next.z - at.z) / d * left);
        }
        left -= d;
        at = next;
    }
    at
}

/// Sends `a` off to the docks and stands `b` squarely in the way, two steps ahead.
fn block_the_way(scene: &mut Scene) {
    let Scene { sim, a, b } = scene;
    set_intent(sim, b, IntentAction::Wait { minutes: 240.0 });
    set_intent(sim, a, IntentAction::GoTo { place_id: "docks".into() });
    sim.step();
    sim.step();
    let walker = &sim.state.characters[a.as_str()];
    assert!(!walker.act.path.is_empty(), "a has a route to the docks");
    let spot = point_along(walker.pos, &walker.act.path, 2.0);
    sim.state.characters.get_mut(b.as_str()).unwrap().pos = spot;
}

/// Runs until `a` has finished walking to the docks, gathering every event on the way.
fn walk(scene: &mut Scene, seconds: f64) -> (bool, Vec<SimEvent>) {
    let mut events = Vec::new();
    for _ in 0..seconds_to_ticks(seconds) {
        events.extend(scene.sim.step());
        if scene.sim.state.characters[scene.a.as_str()].intent.is_none() {
            return (true, events);
        }
    }
    (false, events)
}

fn lines_to<'e>(events: &'e [SimEvent], speaker: &str, to: &str) -> Vec<(&'e str, bool)> {
    events
        .iter()
        .filter_map(|event| match event {
            SimEvent::Speech { speaker_id, to_id: Some(to_id), text, loud, .. }
                if speaker_id == speaker && to_id == to =>
            {
                Some((text.as_str(), *loud))
            }
            _ => None,
        })
        .collect()
}

fn angry(events: &[SimEvent], id: &str) -> bool {
    events.iter().any(|event| matches!(event, SimEvent::Emote { id: who, emote: Emote::Anger } if who == id))
}

fn round_trip(state: &SimState) -> SimState {
    serde_json::from_str(&serde_json::to_string(state).unwrap()).unwrap()
}

#[test]
fn a_calm_walker_asks_politely_and_the_other_steps_aside() {
    let mut scene = scene(3);
    block_the_way(&mut scene);
    let stood_at = scene.sim.state.characters[scene.b.as_str()].pos;
    let (arrived, events) = walk(&mut scene, 120.0);
    assert!(arrived, "the walker gets where they were going");
    let lines = lines_to(&events, &scene.a, &scene.b);
    assert_eq!(lines.len(), 1, "one word, not a stream of them: {lines:?}");
    let blocker_name = first_name(&scene.sim.state.characters[scene.b.as_str()]).to_owned();
    assert!(lines[0].0.contains(&blocker_name), "the line names the one in the way: {}", lines[0].0);
    assert!(!lines[0].1, "a polite word is not shouted");
    assert!(!angry(&events, &scene.a));
    let walker = &scene.sim.state.characters[scene.a.as_str()];
    assert_eq!(rel_or_default(walker, &scene.b).grievance, 0.0, "nothing to hold against anyone");
    let blocker = &scene.sim.state.characters[scene.b.as_str()];
    assert!(dist(blocker.pos, stood_at) > 0.8, "the one in the way stepped aside");
    assert!(blocker.sidestep.is_none(), "and has finished stepping");
}

#[test]
fn a_hot_tempered_walker_shouts_and_holds_it_against_them() {
    let mut scene = scene(3);
    {
        let state = &mut scene.sim.state;
        let walker = state.characters.get_mut(scene.a.as_str()).unwrap();
        walker.profile.temper = 1.0;
        walker.profile.spite = 1.0;
        rel(walker, &scene.b).grievance = 30.0;
    }
    block_the_way(&mut scene);
    let (arrived, events) = walk(&mut scene, 120.0);
    assert!(arrived);
    let lines = lines_to(&events, &scene.a, &scene.b);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].1, "an angry word is shouted");
    assert!(angry(&events, &scene.a), "and shows");
    let state = &scene.sim.state;
    assert!(rel_or_default(&state.characters[scene.a.as_str()], &scene.b).grievance > 30.0);
    assert!(rel_or_default(&state.characters[scene.b.as_str()], &scene.a).grievance > 0.0, "being shouted at stings");
    let memory = state.characters[scene.a.as_str()].memories.last().unwrap();
    assert!(memory.subjects.contains(&scene.b), "the walker remembers who was in the way");
}

#[test]
fn someone_who_cannot_move_is_squeezed_past() {
    let mut scene = scene(3);
    block_the_way(&mut scene);
    scene.sim.state.characters.get_mut(scene.b.as_str()).unwrap().slot_id = Some("pinned-for-the-test".into());
    let stood_at = scene.sim.state.characters[scene.b.as_str()].pos;
    let (arrived, events) = walk(&mut scene, 120.0);
    assert!(arrived, "the walker squeezes past");
    assert_eq!(lines_to(&events, &scene.a, &scene.b).len(), 1);
    assert!(dist(scene.sim.state.characters[scene.b.as_str()].pos, stood_at) < 0.3, "the one in the way stayed put");
}

#[test]
fn the_same_person_in_the_way_again_soon_is_passed_without_a_word() {
    let mut scene = scene(3);
    block_the_way(&mut scene);
    // Stop just after the first word, then stand in the way again.
    let mut events = Vec::new();
    for _ in 0..seconds_to_ticks(10.0) {
        events.extend(scene.sim.step());
        if !lines_to(&events, &scene.a, &scene.b).is_empty() {
            break;
        }
    }
    assert_eq!(lines_to(&events, &scene.a, &scene.b).len(), 1);
    for _ in 0..seconds_to_ticks(2.0) {
        scene.sim.step();
    }
    let walker = &scene.sim.state.characters[scene.a.as_str()];
    let spot = point_along(walker.pos, &walker.act.path, 2.0);
    let blocker = scene.sim.state.characters.get_mut(scene.b.as_str()).unwrap();
    blocker.pos = spot;
    blocker.sidestep = None;
    let (arrived, events) = walk(&mut scene, 120.0);
    assert!(arrived);
    assert!(lines_to(&events, &scene.a, &scene.b).is_empty(), "no second word so soon");
}

#[test]
fn a_block_survives_a_save_mid_way() {
    let mut scene = scene(3);
    block_the_way(&mut scene);
    let mut saved = None;
    for _ in 0..seconds_to_ticks(20.0) {
        scene.sim.step();
        let b = &scene.sim.state.characters[scene.b.as_str()];
        if b.sidestep.is_some() {
            saved = Some(round_trip(&scene.sim.state));
            break;
        }
    }
    let saved = saved.expect("the one in the way started to step aside");
    assert_eq!(saved, scene.sim.state);
    let mut resumed = Sim::new(saved);
    for _ in 0..seconds_to_ticks(30.0) {
        assert_eq!(resumed.step(), scene.sim.step());
    }
    assert_eq!(resumed.state, scene.sim.state);
}
