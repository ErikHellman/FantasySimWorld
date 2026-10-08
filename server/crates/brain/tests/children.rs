//! A child's mind: what the prompts tell them, what they may choose, and what the
//! interpreter refuses on their behalf.

use fsw_brain::llm::types::ToolCall;
use fsw_brain::prompts::{conversation_prompt, decide_prompt, plan_prompt, prompt_text};
use fsw_brain::validate::interpret_decision;
use fsw_sim::state::{
    Character, Conversation, ConversationLine, ConvoAuthor, ConvoPhase, Fallback, IntentAction, SimState, TalkTone,
};
use fsw_sim::util::living;
use fsw_testing::create_test_state;
use fsw_world::places::ActivityId;
use serde_json::{Map, Value, json};

fn who<'s>(state: &'s SimState, name: &str) -> &'s Character {
    living(state).find(|c| fsw_sim::util::first_name(c) == name).expect(name)
}

fn call(name: &str, args: Value) -> ToolCall {
    let args: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    ToolCall { name: name.into(), args }
}

fn talk(state: &SimState, a: &str, b: &str, tone: TalkTone) -> Conversation {
    let _ = state;
    Conversation {
        id: "v0".into(),
        a: a.into(),
        b: b.into(),
        tone,
        phase: ConvoPhase::Hail,
        lines: vec![ConversationLine { speaker_id: a.into(), text: "Well now.".into() }],
        index: 0,
        line_end_tick: 0,
        started_tick: 0,
        await_until_tick: 0,
        closed: false,
        author: ConvoAuthor::Local,
        topic: String::new(),
        feelings: Default::default(),
        fallback: Fallback { lines: Vec::new(), topic: String::new(), feelings: Default::default() },
        rumour: None,
    }
}

#[test]
fn a_child_is_told_they_are_a_child_and_offered_a_childs_choices() {
    let state = create_test_state(1);
    let pip = who(&state, "Pip");
    let prompt = decide_prompt(&state, pip, 1200, None);
    let text = prompt_text(&prompt);
    assert!(text.contains("You are Pip Vane, a child of 11 (he/him)."), "{text}");
    assert!(!text.contains("You are a knight"), "{text}");
    let tools: Vec<&str> = prompt.tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(tools, ["go_to", "do_activity", "talk_to", "continue"]);
    let activities = text.lines().find(|line| line.starts_with("Activities:")).expect("an activities line");
    for id in ActivityId::CHILD_OFFERED {
        assert!(activities.contains(&format!("{} (", id.as_str())), "{activities}");
    }
    assert!(!activities.contains("smith ("), "{activities}");
    assert!(!activities.contains("train ("), "{activities}");
}

#[test]
fn a_whole_child_is_told_they_are_unhurt() {
    let state = create_test_state(1);
    let pip = who(&state, "Pip");
    let text = prompt_text(&decide_prompt(&state, pip, 1200, None));
    assert!(text.contains("unhurt"), "{text}");
    assert!(!text.contains("hurt ("), "{text}");
}

#[test]
fn a_child_sent_to_work_by_the_model_plays_instead() {
    let state = create_test_state(1);
    let pip = who(&state, "Pip");
    let decision = interpret_decision(&state, pip, &[call("do_activity", json!({"activity": "smith"}))]);
    assert!(
        !decision
            .intents
            .iter()
            .any(|i| matches!(i.action, IntentAction::Activity { activity: ActivityId::Smith, .. })),
        "{:?}",
        decision.intents
    );
    assert!(decision.problems.iter().any(|p| p.contains("child")), "{:?}", decision.problems);
}

#[test]
fn a_grown_up_is_not_offered_play() {
    let state = create_test_state(1);
    let aldric = who(&state, "Aldric");
    let text = prompt_text(&decide_prompt(&state, aldric, 1200, None));
    assert!(text.contains("You are a knight."), "{text}");
    let activities = text.lines().find(|line| line.starts_with("Activities:")).expect("an activities line");
    assert!(activities.contains("smith ("), "{activities}");
    assert!(!activities.contains("play ("), "{activities}");
    let plan = prompt_text(&plan_prompt(&state, aldric, 1200, None));
    let activities = plan.lines().find(|line| line.starts_with("Activities:")).expect("an activities line");
    assert!(!activities.contains("play ("), "{activities}");
}

#[test]
fn a_childs_day_is_for_playing_not_work() {
    let state = create_test_state(1);
    let pip = who(&state, "Pip");
    let text = prompt_text(&plan_prompt(&state, pip, 1200, None));
    assert!(
        text.contains("You have no work: your days are for playing, meals and the people who raise you."),
        "{text}"
    );
    assert!(!text.contains("Your usual work"), "{text}");
    let activities = text.lines().find(|line| line.starts_with("Activities:")).expect("an activities line");
    assert!(activities.contains("play (") && !activities.contains("watch ("), "{activities}");
}

#[test]
fn in_conversation_a_child_is_introduced_as_one_and_a_hard_word_is_a_telling_off() {
    let state = create_test_state(1);
    let (aldric, pip) = (who(&state, "Aldric"), who(&state, "Pip"));
    let friendly = talk(&state, &aldric.id, &pip.id, TalkTone::Friendly);
    let text = prompt_text(&conversation_prompt(&state, &friendly, aldric, pip, 1200));
    assert!(text.contains("Pip Vane, a child of 11 (he/him)."), "{text}");
    let hostile = talk(&state, &aldric.id, &pip.id, TalkTone::Hostile);
    let text = prompt_text(&conversation_prompt(&state, &hostile, aldric, pip, 1200));
    assert!(text.contains("Aldric is telling Pip off."), "{text}");
    let cheek = talk(&state, &pip.id, &aldric.id, TalkTone::Hostile);
    let text = prompt_text(&conversation_prompt(&state, &cheek, pip, aldric, 1200));
    assert!(text.contains("Pip is giving Aldric cheek."), "{text}");
}

#[test]
fn the_interpreter_refuses_a_fight_or_a_gift_from_a_child_but_lets_them_play() {
    let state = create_test_state(1);
    let (pip, garrick) = (who(&state, "Pip"), who(&state, "Garrick"));
    let calls = vec![
        call("brawl", json!({"person": "Garrick"})),
        call("attack", json!({"person": "Garrick"})),
        call("give", json!({"person": "Garrick", "item": "food"})),
        call("do_activity", json!({"activity": "play"})),
    ];
    let decision = interpret_decision(&state, pip, &calls);
    assert_eq!(decision.intents.len(), 1, "{:?}", decision.intents);
    assert!(matches!(decision.intents[0].action, IntentAction::Activity { activity: ActivityId::Play, .. }));
    assert!(decision.problems.iter().any(|p| p.contains("child")), "{:?}", decision.problems);
    let _ = garrick;
}
