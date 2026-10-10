//! The billboards: what they show, people reading them, and reactions from the model or the stock lines.

use std::collections::HashSet;

use fsw_core::rng::hash_string;
use fsw_core::time::{TICKS_PER_DAY, game_time};
use fsw_sim::Command;
use fsw_sim::commands::TuningPatch;
use fsw_sim::data::billboards::WATCHER_DECK;
use fsw_sim::describe::describe_activity;
use fsw_sim::state::{
    ActivityId, BoardNotice, BoardReaction, BoardTone, ChronicleKind, Intent, IntentAction, IntentSource, MemoryKind,
    SimState,
};
use fsw_sim::systems::billboards::{
    TEXT_CAP, clean_text, day_tone, hints_at_watchers, shown, stock_reaction, wants_words, watcher_reaction,
};
use fsw_sim::util::{first_name, living};
use fsw_sim::{Sim, SimEvent};
use fsw_testing::create_test_state;
use fsw_world::places::billboards;

struct World {
    sim: Sim,
    events: Vec<SimEvent>,
}

impl World {
    fn new(seed: u32) -> Self {
        let mut sim = Sim::new(create_test_state(seed));
        // No small talk, so nobody is pulled into a chat on the way.
        sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
        Self { sim, events: Vec::new() }
    }

    fn run_until(&mut self, ticks: i64, until: impl Fn(&World) -> bool) {
        for _ in 0..ticks {
            if until(self) {
                return;
            }
            let events = self.sim.step();
            self.events.extend(events);
        }
    }

    fn who(&self, name: &str) -> String {
        living(&self.sim.state).find(|c| first_name(c) == name).expect(name).id.clone()
    }

    /// Sends someone to read the green's board.
    fn send_to_read(&mut self, id: &str) {
        let epoch = self.sim.state.characters[id].epoch;
        let read = Intent::new(
            IntentAction::Activity { activity: ActivityId::ReadBoard, place_id: Some("green".into()), minutes: None },
            IntentSource::Llm,
        );
        self.sim.enqueue(Command::SetIntents { id: id.into(), epoch, intents: vec![read] });
    }

    fn started_reading(&self, id: &str) -> bool {
        self.events.iter().any(|e| matches!(e, SimEvent::BillboardRead { id: who, .. } if who == id))
    }

    fn lines_from(&self, id: &str) -> Vec<String> {
        self.events
            .iter()
            .filter_map(|e| match e {
                SimEvent::Speech { speaker_id, text, .. } if speaker_id == id => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}

fn green_index() -> usize {
    billboards().iter().position(|b| b.id == "green_board").unwrap()
}

#[test]
fn someone_sent_to_a_board_reads_it_and_reacts_without_a_model() {
    let mut w = World::new(3);
    w.run_until(3, |_| false);
    let id = w.who("Rowan");
    w.send_to_read(&id);
    let reader = id.clone();
    w.run_until(4000, move |w| w.started_reading(&reader));
    assert!(w.started_reading(&id), "never got to the board");
    let c = &w.sim.state.characters[&id];
    assert_eq!(describe_activity(&w.sim.state, c), "reading the billboard at the village green");

    let text = shown(&w.sim.state, green_index()).text;
    let reader = id.clone();
    w.run_until(2000, move |w| w.sim.state.characters[&reader].intent.is_none());
    let c = &w.sim.state.characters[&id];
    assert!(c.intent.is_none(), "still reading");
    assert_eq!(w.lines_from(&id).len(), 1, "one remark: {:?}", w.lines_from(&id));
    assert!(w.events.iter().any(|e| matches!(e, SimEvent::Emote { id: who, .. } if *who == id)));
    assert_eq!(c.boards_read.get("green_board"), Some(&hash_string(&text)));
    let memory = c.memories.iter().find(|m| m.text.starts_with("Read the billboard")).expect("a memory of it");
    assert_eq!(memory.kind, MemoryKind::Event);
    assert!(memory.text.contains(&text), "{}", memory.text);
    assert!(c.outcome.contains("billboard"), "{}", c.outcome);
}

#[test]
fn a_reaction_from_the_model_replaces_the_stock_one() {
    let mut w = World::new(3);
    w.sim.enqueue(Command::Tuning { tuning: TuningPatch { script_wait_ticks: Some(400), ..Default::default() } });
    w.run_until(3, |_| false);
    let id = w.who("Rowan");
    w.send_to_read(&id);
    let reader = id.clone();
    w.run_until(4000, move |w| w.started_reading(&reader));
    assert!(w.started_reading(&id));
    // Waits for the model rather than reacting after the usual couple of minutes.
    w.run_until(200, |_| false);
    assert!(w.lines_from(&id).is_empty());

    w.sim.enqueue(Command::BillboardReaction {
        id: id.clone(),
        board_id: "green_board".into(),
        reaction: BoardReaction::Annoyed,
        say: Some("  Who paints\nthese things?  ".into()),
        thought: Some("Someone wastes good paint.".into()),
        follow: None,
    });
    let reader = id.clone();
    w.run_until(1000, move |w| w.sim.state.characters[&reader].intent.is_none());
    assert_eq!(w.lines_from(&id), vec!["Who paints these things?".to_owned()]);
    let c = &w.sim.state.characters[&id];
    assert_eq!(c.thought, "Someone wastes good paint.");
    assert!(c.memories.iter().any(|m| m.text.ends_with("It annoyed me.")));

    // Too late now: they have moved on.
    w.sim.enqueue(Command::BillboardReaction {
        id: id.clone(),
        board_id: "green_board".into(),
        reaction: BoardReaction::Laugh,
        say: Some("Ha!".into()),
        thought: None,
        follow: None,
    });
    w.run_until(5, |_| false);
    assert_eq!(w.lines_from(&id).len(), 1);
}

/// Puts `text` up on the green's board, sends Rowan to read it, and answers for the model with `reaction` and
/// `follow` once Rowan stands there. Returns the world once Rowan has finished at the board, and Rowan's id.
fn read_with(text: &str, reaction: BoardReaction, follow: Option<bool>) -> (World, String) {
    let mut w = World::new(3);
    w.sim.enqueue(Command::Tuning { tuning: TuningPatch { script_wait_ticks: Some(400), ..Default::default() } });
    w.sim.enqueue(Command::SetBillboard {
        board_id: "green_board".into(),
        notice: Some(BoardNotice { text: text.into(), tone: BoardTone::Wholesome, day: None }),
    });
    w.run_until(3, |_| false);
    let id = w.who("Rowan");
    w.send_to_read(&id);
    let reader = id.clone();
    w.run_until(4000, move |w| w.started_reading(&reader));
    assert!(w.started_reading(&id), "never got to the board");
    w.sim.enqueue(Command::BillboardReaction {
        id: id.clone(),
        board_id: "green_board".into(),
        reaction,
        say: None,
        thought: None,
        follow,
    });
    let reader = id.clone();
    w.run_until(1000, move |w| {
        !w.sim.state.characters[&reader].intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::ReadBoard))
    });
    (w, id)
}

#[test]
fn a_reader_who_takes_the_advice_goes_and_does_it() {
    let (w, id) = read_with("Pray at the shrine today!", BoardReaction::Love, Some(true));
    let c = &w.sim.state.characters[&id];
    let next = c.intent.as_ref().or(c.queue.first()).expect("something to do next");
    assert!(next.is_activity(ActivityId::Pray), "{next:?}");
    let memory = c.memories.iter().find(|m| m.text.starts_with("Read the billboard")).expect("a memory of it");
    assert!(memory.text.ends_with("It cheered me up. I decided to pray or meditate in quiet."), "{}", memory.text);
}

#[test]
fn the_advice_can_be_turned_down() {
    let (w, id) = read_with("Pray at the shrine today!", BoardReaction::Love, Some(false));
    let c = &w.sim.state.characters[&id];
    assert!(!c.intent.iter().chain(&c.queue).any(|i| i.is_activity(ActivityId::Pray)));
    assert!(c.memories.iter().any(|m| m.text.ends_with("It cheered me up.")));

    // Without a model's word it is left to chance, and the annoyed never take a board's advice.
    let (w, id) = read_with("Pray at the shrine today!", BoardReaction::Annoyed, None);
    let c = &w.sim.state.characters[&id];
    assert!(!c.intent.iter().chain(&c.queue).any(|i| i.is_activity(ActivityId::Pray)));
}

#[test]
fn a_reader_urged_to_rest_goes_home_to_do_it() {
    let (w, id) = read_with("Take a nap. You've earned it.", BoardReaction::Love, Some(true));
    let c = &w.sim.state.characters[&id];
    let next = c.intent.as_ref().or(c.queue.first()).expect("something to do next");
    let IntentAction::Activity { activity: ActivityId::Rest, place_id, .. } = &next.action else {
        panic!("{next:?}");
    };
    assert_eq!(place_id.as_ref(), Some(&c.home_id));
}

#[test]
fn some_villagers_follow_the_slogans_on_their_own() {
    let mut sim = Sim::new(create_test_state(5));
    for _ in 0..TICKS_PER_DAY * 3 {
        sim.step();
    }
    let followed = living(&sim.state)
        .flat_map(|c| &c.memories)
        .filter(|m| m.text.starts_with("Read the billboard") && m.text.contains(" I decided to "))
        .count();
    assert!(followed >= 1, "nobody took a board's advice in three days");
}

#[test]
fn without_an_answer_the_stock_reaction_comes_at_the_deadline() {
    let mut w = World::new(3);
    w.sim.enqueue(Command::Tuning { tuning: TuningPatch { script_wait_ticks: Some(300), ..Default::default() } });
    w.run_until(3, |_| false);
    let id = w.who("Rowan");
    w.send_to_read(&id);
    let reader = id.clone();
    w.run_until(4000, move |w| w.started_reading(&reader));
    let started = w.sim.state.tick;
    let reader = id.clone();
    w.run_until(1000, move |w| !w.lines_from(&reader).is_empty());
    assert_eq!(w.lines_from(&id).len(), 1);
    assert!(w.sim.state.tick - started >= 295, "reacted after {} ticks", w.sim.state.tick - started);
}

#[test]
fn admins_put_text_up_and_take_it_down() {
    let mut w = World::new(3);
    let index = green_index();
    let slogan = shown(&w.sim.state, index);
    assert!(!slogan.custom);

    w.sim.enqueue(Command::SetBillboard {
        board_id: "green_board".into(),
        notice: Some(BoardNotice {
            text: "  Fresh\n\tbread  at the market! ".into(),
            tone: BoardTone::Pushy,
            day: None,
        }),
    });
    w.sim.enqueue(Command::SetBillboard {
        board_id: "no_such_board".into(),
        notice: Some(BoardNotice { text: "Hello".into(), tone: BoardTone::Absurd, day: None }),
    });
    w.run_until(1, |_| false);
    let now = shown(&w.sim.state, index);
    assert_eq!(now.text, "Fresh bread at the market!");
    assert_eq!(now.tone, BoardTone::Pushy);
    assert!(now.custom);
    assert_eq!(w.sim.state.billboards.len(), 1);

    let long = "word ".repeat(40);
    w.sim.enqueue(Command::SetBillboard {
        board_id: "green_board".into(),
        notice: Some(BoardNotice { text: long, tone: BoardTone::Absurd, day: None }),
    });
    w.run_until(1, |_| false);
    assert_eq!(shown(&w.sim.state, index).text.encode_utf16().count(), TEXT_CAP);

    w.sim.enqueue(Command::SetBillboard { board_id: "green_board".into(), notice: None });
    w.run_until(1, |_| false);
    assert_eq!(shown(&w.sim.state, index), slogan);
    assert!(w.sim.state.billboards.is_empty());
}

#[test]
fn text_is_cleaned_before_it_goes_up() {
    assert_eq!(clean_text("  two\n lines "), Some("two lines".into()));
    assert_eq!(clean_text(" \n\t "), None);
    assert_eq!(clean_text("bell\u{7}"), None);
}

#[test]
fn the_slogans_differ_between_boards_and_change_every_day() {
    let mut state: SimState = create_test_state(11);
    let boards = billboards().len();
    let today: Vec<String> = (0..boards).map(|i| shown(&state, i).text).collect();
    assert_eq!(today.iter().collect::<HashSet<_>>().len(), boards);
    state.tick += TICKS_PER_DAY;
    for (i, before) in today.iter().enumerate() {
        assert_ne!(&shown(&state, i).text, before, "board {i}");
    }
}

#[test]
fn villagers_read_boards_on_their_own_but_never_the_same_text_twice() {
    let mut sim = Sim::new(create_test_state(5));
    let mut reads: Vec<(String, String, String)> = Vec::new();
    for _ in 0..TICKS_PER_DAY * 2 {
        for event in sim.step() {
            if let SimEvent::BillboardRead { id, board_id } = event {
                let index = billboards().iter().position(|b| b.id == board_id).unwrap();
                reads.push((id, board_id, shown(&sim.state, index).text));
            }
        }
    }
    let readers: HashSet<&str> = reads.iter().map(|(id, _, _)| id.as_str()).collect();
    assert!(readers.len() >= 3, "only {} people read a board: {reads:?}", readers.len());
    let unique: HashSet<_> = reads.iter().collect();
    assert_eq!(unique.len(), reads.len(), "someone read the same thing twice");
}

#[test]
fn a_world_with_a_notice_up_survives_a_save_and_load() {
    let mut sim = Sim::new(create_test_state(9));
    sim.enqueue(Command::SetBillboard {
        board_id: "market_board".into(),
        notice: Some(BoardNotice { text: "Cheese! Now with holes!".into(), tone: BoardTone::Absurd, day: None }),
    });
    for _ in 0..TICKS_PER_DAY / 2 {
        sim.step();
    }
    let json = serde_json::to_string(&sim.state).unwrap();
    assert!(json.contains("\"billboards\""));
    let loaded: SimState = serde_json::from_str(&json).unwrap();
    assert!(loaded == sim.state);
}

#[test]
fn the_model_writes_a_board_for_the_day_and_it_comes_down_at_midnight() {
    let mut w = World::new(3);
    let index = green_index();
    let day = game_time(w.sim.state.tick).day;
    let slogan = shown(&w.sim.state, index);
    assert_eq!(wants_words(&w.sim.state, index), Some(day));

    w.sim.enqueue(Command::WriteBillboard { board_id: "green_board".into(), day, text: " Pray\nfor rain. ".into() });
    w.run_until(1, |_| false);
    let now = shown(&w.sim.state, index);
    assert_eq!(now.text, "Pray for rain.");
    assert_eq!(now.tone, day_tone(&w.sim.state, index));
    assert_eq!(now.tone, slogan.tone, "the model writes in the day's tone");
    assert!(!now.custom);
    assert_eq!(wants_words(&w.sim.state, index), None);

    // Once written, the day's words stay; and words for another day are refused.
    w.sim.enqueue(Command::WriteBillboard { board_id: "green_board".into(), day, text: "Again!".into() });
    w.sim.enqueue(Command::WriteBillboard { board_id: "market_board".into(), day: day - 1, text: "Late!".into() });
    w.run_until(1, |_| false);
    assert_eq!(shown(&w.sim.state, index).text, "Pray for rain.");
    let market = billboards().iter().position(|b| b.id == "market_board").unwrap();
    assert_ne!(shown(&w.sim.state, market).text, "Late!");

    // At midnight they come down, and the board waits for the new day's words.
    let mut tomorrow = w.sim.state.clone();
    tomorrow.tick += TICKS_PER_DAY;
    assert_ne!(shown(&tomorrow, index).text, "Pray for rain.");
    assert_eq!(wants_words(&tomorrow, index), Some(day + 1));
}

#[test]
fn an_admin_notice_wins_over_the_models_words() {
    let mut w = World::new(3);
    let index = green_index();
    let day = game_time(w.sim.state.tick).day;
    w.sim.enqueue(Command::SetBillboard {
        board_id: "green_board".into(),
        notice: Some(BoardNotice { text: "Market day!".into(), tone: BoardTone::Pushy, day: Some(day) }),
    });
    w.run_until(1, |_| false);
    assert_eq!(wants_words(&w.sim.state, index), None);
    w.sim.enqueue(Command::WriteBillboard { board_id: "green_board".into(), day, text: "Something else".into() });
    w.run_until(1, |_| false);
    let now = shown(&w.sim.state, index);
    assert_eq!(now.text, "Market day!");
    assert!(now.custom, "an admin's notice never comes down on its own");
    let mut later = w.sim.state.clone();
    later.tick += TICKS_PER_DAY * 3;
    assert_eq!(shown(&later, index).text, "Market day!");
}

#[test]
fn now_and_then_a_board_hints_at_the_watchers() {
    let mut state: SimState = create_test_state(11);
    let start = game_time(state.tick).day;
    let mut hints = 0;
    for day in start..start + 60 {
        for index in 0..billboards().len() {
            let shown = shown(&state, index);
            assert_eq!(shown.watchers, hints_at_watchers(state.seed, day, index));
            if shown.watchers {
                hints += 1;
                assert!(WATCHER_DECK.iter().any(|(text, _, _)| *text == shown.text), "{}", shown.text);
            }
        }
        state.tick += TICKS_PER_DAY;
    }
    // About one board-day in fourteen: some, but not every day.
    assert!((10..=60).contains(&hints), "{hints} hints in 60 days");
}

#[test]
fn the_watchers_shock_the_timid_anger_the_hot_headed_and_intrigue_the_rest() {
    let state = create_test_state(3);
    let mut profile = state.characters.values().next().unwrap().profile.clone();
    profile.temper = 0.9;
    assert_eq!(watcher_reaction(&profile), BoardReaction::Angry);
    profile.temper = 0.2;
    profile.bravery = 0.1;
    assert_eq!(watcher_reaction(&profile), BoardReaction::Shocked);
    profile.bravery = 0.6;
    assert_eq!(watcher_reaction(&profile), BoardReaction::Intrigued);
    let ordinary = shown(&state, green_index());
    if !ordinary.watchers {
        assert_ne!(stock_reaction(&profile, &ordinary), BoardReaction::Intrigued);
    }
}

/// A seed whose first day has the green's board hinting at the watchers.
fn watched_seed() -> u32 {
    (1..2000)
        .find(|&seed| {
            let state = create_test_state(seed);
            hints_at_watchers(state.seed, game_time(state.tick).day, green_index())
        })
        .expect("some seed hints on its first day")
}

#[test]
fn a_reader_of_a_hint_at_the_watchers_takes_it_to_heart() {
    let mut w = World::new(watched_seed());
    w.sim.enqueue(Command::Tuning { tuning: TuningPatch { script_wait_ticks: Some(400), ..Default::default() } });
    w.run_until(3, |_| false);
    assert!(shown(&w.sim.state, green_index()).watchers);
    let id = w.who("Rowan");
    w.send_to_read(&id);
    let reader = id.clone();
    w.run_until(4000, move |w| w.started_reading(&reader));
    assert!(w.started_reading(&id), "never got to the board");
    w.sim.enqueue(Command::BillboardReaction {
        id: id.clone(),
        board_id: "green_board".into(),
        reaction: BoardReaction::Intrigued,
        say: Some("Hello up there!".into()),
        thought: None,
        follow: Some(false),
    });
    let reader = id.clone();
    w.run_until(1000, move |w| w.sim.state.characters[&reader].intent.is_none());
    assert_eq!(w.lines_from(&id), vec!["Hello up there!".to_owned()]);
    let c = &w.sim.state.characters[&id];
    let memory = c.memories.iter().find(|m| m.text.starts_with("Read the billboard")).expect("a memory of it");
    assert!(memory.text.contains("someone beyond the world is watching us"), "{}", memory.text);
    assert!(memory.importance >= 5);
    assert_eq!(c.thought, "Who is watching us, and what do they want to see?");
    let entry = w.sim.state.chronicle.iter().rev().find(|e| e.ids.contains(&id)).expect("a chronicle entry");
    assert_eq!(entry.kind, ChronicleKind::World);
    assert!(entry.text.contains("is intrigued"), "{}", entry.text);
}

#[test]
fn only_a_hint_at_the_watchers_can_shock_anyone() {
    let (w, id) = read_with("Fresh bread at the market!", BoardReaction::Shocked, Some(false));
    let c = &w.sim.state.characters[&id];
    assert!(c.memories.iter().any(|m| m.text.ends_with("It left me uneasy.")));
    assert!(!w.sim.state.chronicle.iter().any(|e| e.text.contains("beyond the world")));
}
