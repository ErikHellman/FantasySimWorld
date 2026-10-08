//! Coin in the model's view: what the prompts say about gold and loot, and what the interpreter makes
//! of the economy's tools.

use fsw_brain::llm::types::ToolCall;
use fsw_brain::prompts::{decide_prompt, prompt_text};
use fsw_brain::validate::interpret_decision;
use fsw_core::vec2;
use fsw_sim::state::{IntentAction, Loot, LootKind, SimState};
use fsw_testing::create_test_state;
use serde_json::{Map, Value, json};

fn call(name: &str, args: Value) -> ToolCall {
    let args: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    ToolCall { name: name.into(), args }
}

fn drop_near(state: &mut SimState, id: &str, kind: LootKind, dx: f64) -> String {
    let pos = state.characters[id].pos;
    let loot_id = state.new_id("l:");
    state.loot.insert(loot_id.clone(), Loot { kind, pos: vec2(pos.x + dx, pos.z), since_tick: state.tick });
    loot_id
}

#[test]
fn the_header_names_the_villagers_gold() {
    let state = create_test_state(1);
    let c = &state.characters["c1"];
    let text = prompt_text(&decide_prompt(&state, c, 1500, None));
    assert!(text.contains(&format!(", and {} gold.", c.gold)), "{text}");
    assert!(!text.contains("market stalls hold"), "nothing on the stalls, nothing said");
}

#[test]
fn loot_within_reach_is_named_and_the_tool_offered_only_then() {
    let mut state = create_test_state(1);
    let c = state.characters["c1"].clone();
    let prompt = decide_prompt(&state, &c, 1500, None);
    assert!(!prompt.tools.iter().any(|t| t.name == "pick_up"));
    let near = drop_near(&mut state, "c1", LootKind::Coins { count: 4 }, 2.0);
    let far = drop_near(&mut state, "c1", LootKind::Trinket, 6.0);
    drop_near(&mut state, "c1", LootKind::Relic, 200.0);
    let prompt = decide_prompt(&state, &c, 1500, None);
    let text = prompt_text(&prompt);
    assert!(text.contains("On the ground within reach (pick_up): 4 coins near "), "{text}");
    assert!(text.contains("; a trinket near "), "{text}");
    assert!(!text.contains("a relic"), "out of reach: {text}");
    assert!(prompt.tools.iter().any(|t| t.name == "pick_up"));

    let decision = interpret_decision(&state, &c, &[call("pick_up", json!({"what": "the trinket"}))]);
    assert!(
        matches!(&decision.intents[0].action, IntentAction::PickUp { loot_id } if *loot_id == far),
        "{:?}",
        decision.intents
    );
    let decision = interpret_decision(&state, &c, &[call("pick_up", json!({"what": "whatever is nearest"}))]);
    assert!(matches!(&decision.intents[0].action, IntentAction::PickUp { loot_id } if *loot_id == near));

    state.loot.clear();
    let decision = interpret_decision(&state, &c, &[call("pick_up", json!({"what": "the coins"}))]);
    assert!(decision.intents.is_empty());
    assert!(decision.problems.iter().any(|p| p.contains("nothing to pick up")), "{:?}", decision.problems);
}
