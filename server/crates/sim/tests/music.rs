//! Music (`systems::music`): who has a talent, how a performance gathers its audience, and what a good
//! tune does for the people who heard it.

use fsw_core::Vec2;
use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::Command;
use fsw_sim::commands::TuningPatch;
use fsw_sim::state::{ActivityId, Instrument, Intent, IntentAction, IntentSource, SimState, StimulusKind};
use fsw_sim::systems::music::{performance, skill_of, talent};
use fsw_sim::util::{first_name, living, rel_or_default};
use fsw_sim::{Sim, SimEvent};
use fsw_testing::create_test_state;
use fsw_world::places::get_place;

fn who(state: &SimState, name: &str) -> String {
    living(state).find(|c| first_name(c) == name).expect(name).id.clone()
}

/// A quiet world a few ticks in: no small talk, so nothing draws the audience away.
fn quiet(seed: u32) -> Sim {
    let mut sim = Sim::new(create_test_state(seed));
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    for _ in 0..5 {
        sim.step();
    }
    sim
}

/// Puts someone out in the open, awake, at `pos`, with nothing to do for an hour.
fn idle_at(state: &mut SimState, id: &str, pos: Vec2) {
    let tick = state.tick;
    let c = state.characters.get_mut(id).unwrap();
    c.act = fsw_sim::systems::intents::fresh_act(tick);
    c.pos = pos;
    c.inside = None;
    c.asleep = false;
    c.slot_id = None;
    c.anim = fsw_sim::state::AnimId::Idle;
    c.intent = Some(Intent::new(IntentAction::Wait { minutes: 60.0 }, IntentSource::Routine));
    c.queue.clear();
    c.heartened_until_tick = 0;
}

/// Sends `id` to perform at the tavern for `minutes`, as the model would.
fn perform(sim: &mut Sim, id: &str, minutes: f64) {
    let epoch = sim.state.characters[id].epoch;
    let intent = Intent::new(
        IntentAction::Activity {
            activity: ActivityId::Perform,
            place_id: Some("tavern".into()),
            minutes: Some(minutes),
        },
        IntentSource::Llm,
    );
    sim.enqueue(Command::SetIntents { id: id.to_owned(), epoch, intents: vec![intent] });
}

/// Runs until `performer`'s music has started and stopped again (or gives up); returns the listeners the
/// record held and whether anyone stood in the audience.
fn run_performance(sim: &mut Sim, performer: &str) -> (Vec<String>, bool, Vec<SimEvent>) {
    let (mut started, mut listeners, mut stood, mut events) = (false, Vec::new(), false, Vec::new());
    for _ in 0..3000 {
        events.extend(sim.step());
        match performance(&sim.state, performer) {
            Some(p) => {
                started = true;
                listeners = p.listeners.clone();
                stood |= living(&sim.state).any(|c| {
                    matches!(&c.intent, Some(Intent { action: IntentAction::Listen { performer_id }, .. }) if performer_id == performer)
                });
            }
            None if started => return (listeners, stood, events),
            None => {}
        }
    }
    panic!("the performance never ended (started: {started})");
}

#[test]
fn children_have_no_talent_and_some_adults_do() {
    let state = create_test_state(1);
    let musicians: Vec<&str> = living(&state).filter(|c| talent(c).is_some()).map(first_name).collect();
    assert!(living(&state).filter(|c| c.is_child()).all(|c| talent(c).is_none()));
    assert!(musicians.len() >= 2, "{musicians:?}");
    assert_eq!(talent(&state.characters[&who(&state, "Torvald")]).map(|t| t.instrument), Some(Instrument::Voice));
    assert_eq!(talent(&state.characters[&who(&state, "Rowan")]).map(|t| t.instrument), Some(Instrument::Drum));
    assert_eq!(talent(&state.characters[&who(&state, "Aldric")]), None);
    // Anyone can try; without a talent it is a thin voice.
    assert_eq!(skill_of(&state.characters[&who(&state, "Aldric")]), (Instrument::Voice, 0.1));
}

#[test]
fn a_description_that_speaks_of_music_makes_a_musician() {
    let mut state = create_test_state(1);
    let aldric = who(&state, "Aldric");
    let c = state.characters.get_mut(&aldric).unwrap();
    c.profile.occupation = "travelling fiddler".into();
    let t = talent(c).expect("a fiddler has a talent");
    assert_eq!(t.instrument, Instrument::Fiddle);
    assert!(t.skill >= 0.6);
    c.profile.occupation = "captain of the watch".into();
    c.profile.backstory = "Sings in the chapel choir.".into();
    assert_eq!(talent(c).map(|t| t.instrument), Some(Instrument::Voice));
    // Practice counts.
    c.music.performances = 10;
    assert!(skill_of(c).1 > t.skill);
}

#[test]
fn a_good_tune_draws_an_audience_and_heartens_everyone_who_heard_it() {
    let mut sim = quiet(1);
    let state = &mut sim.state;
    let (torvald, aldric, brenna, kael) =
        (who(state, "Torvald"), who(state, "Aldric"), who(state, "Brenna"), who(state, "Kael"));
    let door = get_place("tavern").door;
    idle_at(state, &torvald, door);
    // Enough practice that the tune is superb whatever the luck.
    state.characters.get_mut(&torvald).unwrap().music.performances = 30;
    idle_at(state, &aldric, Vec2 { x: door.x + 6.0, z: door.z + 8.0 });
    idle_at(state, &brenna, Vec2 { x: door.x - 7.0, z: door.z + 9.0 });
    idle_at(state, &kael, Vec2 { x: door.x + 5.0, z: door.z + 12.0 });
    let before = rel_or_default(&state.characters[&aldric], &torvald).affinity;
    perform(&mut sim, &torvald, 20.0);

    let (listeners, stood, events) = run_performance(&mut sim, &torvald);
    assert!(stood, "nobody stood in the audience");
    for id in [&aldric, &brenna, &kael] {
        assert!(listeners.contains(id), "{} did not listen", first_name(&sim.state.characters[id]));
    }
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SimEvent::Emote { id, emote: fsw_sim::events::Emote::Music } if id == &torvald))
    );
    let tick = sim.state.tick;
    let state = &sim.state;
    for id in [&aldric, &brenna, &kael] {
        let c = &state.characters[id];
        assert!(c.heartened(tick), "{} is not heartened", first_name(c));
        assert!(c.memories.iter().any(|m| m.text.contains("lifted my spirits")), "{}", first_name(c));
        assert!(c.stimuli.iter().any(|s| s.kind == StimulusKind::Music), "{}", first_name(c));
    }
    assert!(rel_or_default(&state.characters[&aldric], &torvald).affinity > before);
    assert_eq!(state.characters[&torvald].music.performances, 31);
    assert!(state.characters[&torvald].outcome.contains("they loved it"), "{}", state.characters[&torvald].outcome);
    assert!(state.chronicle.iter().any(|e| e.text.contains("Torvald sang") && e.text.contains("cheered")));
}

#[test]
fn poor_music_heartens_nobody() {
    let mut sim = quiet(1);
    let state = &mut sim.state;
    let (aldric, brenna, kael) = (who(state, "Aldric"), who(state, "Brenna"), who(state, "Kael"));
    let door = get_place("tavern").door;
    idle_at(state, &aldric, door);
    idle_at(state, &brenna, Vec2 { x: door.x - 4.0, z: door.z + 8.0 });
    idle_at(state, &kael, Vec2 { x: door.x + 4.0, z: door.z + 8.0 });
    perform(&mut sim, &aldric, 20.0);

    let (listeners, _, _) = run_performance(&mut sim, &aldric);
    assert!(!listeners.is_empty());
    let tick = sim.state.tick;
    for id in [&brenna, &kael] {
        let c = &sim.state.characters[id];
        assert!(!c.heartened(tick), "{} is heartened by a thin voice", first_name(c));
        assert!(!c.memories.iter().any(|m| m.text.contains("lifted my spirits")));
    }
    assert!(!sim.state.chronicle.iter().any(|e| e.text.contains("cheered")));
}

#[test]
fn a_performance_survives_a_save_and_load() {
    let mut sim = quiet(1);
    let torvald = who(&sim.state, "Torvald");
    idle_at(&mut sim.state, &torvald, get_place("tavern").door);
    perform(&mut sim, &torvald, 30.0);
    for _ in 0..3000 {
        sim.step();
        if performance(&sim.state, &torvald).is_some() {
            break;
        }
    }
    assert!(performance(&sim.state, &torvald).is_some());
    let json = serde_json::to_value(&sim.state).unwrap();
    assert!(json.get("performances").is_some());
    let loaded: SimState = serde_json::from_value(json).unwrap();
    assert_eq!(loaded, sim.state);
    // An untouched world keeps the old save shape.
    let fresh = serde_json::to_value(create_test_state(1)).unwrap();
    assert!(fresh.get("performances").is_none());
    assert!(fresh["characters"].as_object().unwrap().values().all(|c| c.get("music").is_none()));
}

#[test]
fn musicians_strike_up_on_their_own_and_nobody_else_does() {
    let mut performers = Vec::new();
    for seed in 1..=3 {
        let mut sim = Sim::new(create_test_state(seed));
        for _ in 0..TICKS_PER_DAY * 3 {
            for event in sim.step() {
                if let SimEvent::Chronicle { entry } = event
                    && (entry.text.contains("began to sing") || entry.text.contains("struck up"))
                {
                    let id = entry.ids[0].clone();
                    assert!(talent(&sim.state.characters[&id]).is_some(), "{}", entry.text);
                    performers.push(entry.text);
                }
            }
        }
    }
    assert!(performers.len() >= 2, "{performers:?}");
}

#[test]
fn the_heartened_heal_faster_and_tire_slower() {
    let sim = quiet(1);
    let aldric = who(&sim.state, "Aldric");
    let run = |heartened: bool| {
        let mut sim = Sim::new(sim.state.clone());
        let c = sim.state.characters.get_mut(&aldric).unwrap();
        c.hp = 20.0;
        c.heartened_until_tick = if heartened { sim.state.tick + 10_000 } else { 0 };
        for _ in 0..400 {
            sim.step();
        }
        let c = &sim.state.characters[&aldric];
        (c.hp, c.needs.energy)
    };
    let (hp, energy) = run(false);
    let (hp_heartened, energy_heartened) = run(true);
    assert!(hp_heartened > hp);
    assert!(energy_heartened > energy);
}
