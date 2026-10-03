//! The Rust prompts against the TypeScript brain's, word for word, for a fixed
//! world (`tests/fixtures/prompts.json`, written by `bun tools/make-prompt-fixture.ts`
//! at the `ts-sim-reference` tag).
//!
//! After a deliberate change to a prompt or to the cast, run this test with
//! `UPDATE_PROMPT_FIXTURE=1` to write the current prompts into the fixture, then
//! read the diff before committing it.

use fsw_brain::prompts::{
    Prompt, conversation_prompt, decide_prompt, mercy_prompt, plan_prompt, prompt_text, react_prompt, reflect_prompt,
    reply_prompt,
};
use fsw_sim::state::{Conversation, ConversationLine, ConvoAuthor, ConvoPhase, Fallback, SimState, TalkTone};
use fsw_sim::{Sim, create_state};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Case {
    tick: i64,
    kind: String,
    ids: Vec<String>,
    budget: usize,
    text: String,
    tools: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct Fixture {
    seed: u32,
    cases: Vec<Case>,
}

fn convo(state: &SimState, a: &str, b: &str) -> Conversation {
    let _ = state;
    Conversation {
        id: "v0".into(),
        a: a.into(),
        b: b.into(),
        tone: TalkTone::Friendly,
        phase: ConvoPhase::Hail,
        lines: vec![ConversationLine { speaker_id: a.into(), text: "Fine morning.".into() }],
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

fn build(state: &SimState, case: &Case) -> Prompt {
    let c = |i: usize| &state.characters[&case.ids[i]];
    match case.kind.as_str() {
        "decide" => decide_prompt(state, c(0), case.budget),
        "plan" => plan_prompt(state, c(0), case.budget),
        "react" => react_prompt(state, c(0)),
        "reflect" => reflect_prompt(state, c(0), case.budget),
        "mercy" => mercy_prompt(state, c(0), c(1)),
        "conversation" => {
            conversation_prompt(state, &convo(state, &case.ids[0], &case.ids[1]), c(0), c(1), case.budget)
        }
        "reply" => {
            let convo = convo(state, &case.ids[1], &case.ids[0]);
            reply_prompt(state, &convo, &convo.lines, c(0), c(1), case.budget)
        }
        other => panic!("unknown prompt kind {other}"),
    }
}

#[test]
fn prompts_match_the_typescript_brain_word_for_word() {
    let path = format!("{}/tests/fixtures/prompts.json", env!("CARGO_MANIFEST_DIR"));
    let mut fixture: Fixture = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut sim = Sim::new(create_state(fixture.seed));
    if std::env::var_os("UPDATE_PROMPT_FIXTURE").is_some() {
        update_fixture(&path, &mut fixture, &mut sim);
        return;
    }
    let mut wrong = Vec::new();
    for case in &fixture.cases {
        while sim.state.tick < case.tick {
            sim.step();
        }
        let prompt = build(&sim.state, case);
        let names: Vec<&str> = prompt.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, case.tools, "{} {} tick {}", case.kind, case.ids[0], case.tick);
        let text = prompt_text(&prompt);
        if text != case.text {
            let at = text.chars().zip(case.text.chars()).take_while(|(a, b)| a == b).count();
            let from = at.saturating_sub(80);
            let got: String = text.chars().skip(from).take(200).collect();
            let want: String = case.text.chars().skip(from).take(200).collect();
            wrong.push(format!("{} {} tick {}:\n  got:  {got:?}\n  want: {want:?}", case.kind, case.ids[0], case.tick));
        }
    }
    assert!(wrong.is_empty(), "{} of {} prompts differ:\n{}", wrong.len(), fixture.cases.len(), wrong.join("\n"));
}

/// Rewrites each case's expected text and tools with what the prompts produce now,
/// keeping the fixture's layout (field order, one-space indent) so the diff shows only what changed.
fn update_fixture(path: &str, fixture: &mut Fixture, sim: &mut Sim) {
    for case in &mut fixture.cases {
        while sim.state.tick < case.tick {
            sim.step();
        }
        let prompt = build(&sim.state, case);
        case.text = prompt_text(&prompt);
        case.tools = prompt.tools.iter().map(|t| t.name.clone()).collect();
    }
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    fixture.serialize(&mut serde_json::Serializer::with_formatter(&mut out, formatter)).unwrap();
    out.push(b'\n');
    std::fs::write(path, out).unwrap();
}
