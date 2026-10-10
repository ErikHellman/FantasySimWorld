//! The Valley Herald: a front page about yesterday each morning, the model's if it writes one by eight, else one
//! the sim sets from the chronicle.

use fsw_core::time::game_time;
use fsw_sim::state::{ChronicleEntry, ChronicleKind, SimState};
use fsw_sim::systems::herald::{HERALD_LINE, stock_issue, wants_issue};
use fsw_sim::{Command, Sim};
use fsw_testing::create_test_state;

/// Runs until the clock reaches `minute` on `day`.
fn run_to(sim: &mut Sim, day: i64, minute: f64) {
    loop {
        let time = game_time(sim.state.tick);
        if time.day > day || (time.day == day && time.minute_of_day >= minute) {
            return;
        }
        sim.step();
    }
}

fn write(day: i64, headline: &str) -> Command {
    Command::WriteHerald {
        day,
        headline: headline.into(),
        stories: vec!["  The mill\nburned. ".into(), String::new()],
        gossip: Some("Who kissed whom?".into()),
    }
}

#[test]
fn the_first_issue_comes_out_on_the_second_morning_and_the_sim_sets_it_if_nobody_writes_it() {
    let mut sim = Sim::new(create_test_state(4));
    run_to(&mut sim, 2, 359.0);
    assert_eq!(wants_issue(&sim.state), None, "wanted before dawn");
    run_to(&mut sim, 2, 361.0);
    assert_eq!(wants_issue(&sim.state), Some(2));
    run_to(&mut sim, 2, 481.0);
    let issue = sim.state.herald.last().expect("no issue by eight");
    assert_eq!(issue.day, 2);
    assert!(!issue.by_model);
    assert!(!issue.headline.is_empty());
    let announced = format!("{HERALD_LINE}{}", issue.headline);
    assert!(sim.state.chronicle.iter().any(|e| e.text == announced));
    assert_eq!(wants_issue(&sim.state), None);
    // Too late for the model's.
    sim.enqueue(write(2, "Late news"));
    sim.step();
    assert_eq!(sim.state.herald.len(), 1);
    // The Herald leaves out the dawn and its own announcements, and survives a save.
    run_to(&mut sim, 3, 481.0);
    assert_eq!(sim.state.herald.len(), 2);
    assert!(sim.state.herald.iter().all(|issue| !issue.headline.starts_with(HERALD_LINE)));
    let saved: SimState = serde_json::from_str(&serde_json::to_string(&sim.state).unwrap()).unwrap();
    assert_eq!(saved.herald, sim.state.herald);
}

#[test]
fn the_models_front_page_is_printed_once_and_tidied() {
    let mut sim = Sim::new(create_test_state(4));
    run_to(&mut sim, 2, 400.0);
    sim.enqueue(write(1, "Wrong day"));
    sim.enqueue(write(2, "Fire at the mill!"));
    sim.enqueue(write(2, "Second thoughts"));
    sim.step();
    assert_eq!(sim.state.herald.len(), 1);
    let issue = &sim.state.herald[0];
    assert_eq!((issue.day, issue.headline.as_str(), issue.by_model), (2, "Fire at the mill!", true));
    assert_eq!(issue.stories, vec!["The mill burned.".to_owned()]);
    assert_eq!(issue.gossip.as_deref(), Some("Who kissed whom?"));
    // The sim does not set another at eight.
    run_to(&mut sim, 2, 490.0);
    assert_eq!(sim.state.herald.len(), 1);
}

#[test]
fn the_sims_own_page_leads_with_the_biggest_news() {
    let mut state = create_test_state(4);
    let day_one = state.tick;
    state.tick = day_one + 30_000;
    let day = game_time(state.tick).day;
    assert_eq!(game_time(day_one).day, day - 1);
    let entry = |kind, text: &str| ChronicleEntry { tick: day_one + 100, kind, text: text.into(), ids: Vec::new() };
    state.chronicle = vec![
        entry(ChronicleKind::World, "Day 1 dawns over the valley."),
        entry(ChronicleKind::Trade, "Bran sold a pie."),
        entry(ChronicleKind::Social, "Edda and Kael shared a joke."),
        entry(ChronicleKind::Death, "Rowan died of old age."),
        entry(ChronicleKind::Fire, "The mill caught fire."),
    ];
    let issue = stock_issue(&state, day);
    assert_eq!(issue.headline, "Rowan died of old age.");
    assert_eq!(issue.stories, vec!["The mill caught fire.".to_owned(), "Bran sold a pie.".to_owned()]);
    assert_eq!(issue.gossip.as_deref(), Some("Edda and Kael shared a joke."));
    state.chronicle.clear();
    assert_eq!(stock_issue(&state, day).headline, "All quiet in the valley");
}
