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

#[test]
fn deals_within_reach_are_named_and_the_trade_tool_offered_only_then() {
    use fsw_sim::state::ItemId;
    let mut state = create_test_state(1);
    // Alone, there is nobody to deal with and the tool is not offered.
    let spot = state.characters["c1"].pos;
    for c in state.characters.values_mut() {
        if c.id != "c1" {
            c.pos = vec2(spot.x + 100.0, spot.z + 100.0);
        }
    }
    let c = state.characters["c1"].clone();
    let prompt = decide_prompt(&state, &c, 1500, None);
    assert!(!prompt.tools.iter().any(|t| t.name == "trade"), "nobody within reach");

    // A neighbour at hand with food to spare, and a trinket of our own they have the coin for.
    {
        let me = state.characters.get_mut("c1").unwrap();
        me.inventory.food = 0;
        me.inventory.trinket = 1;
        me.gold = 10;
        me.inside = None;
        let other = state.characters.get_mut("c2").unwrap();
        other.pos = vec2(spot.x + 2.0, spot.z);
        other.inside = None;
        other.inventory.food = 3;
        other.inventory.potion = 0;
        other.needs.hunger = 90.0;
        other.gold = 20;
    }
    let c = state.characters["c1"].clone();
    let other = fsw_sim::util::first_name(&state.characters["c2"]).to_owned();
    let prompt = decide_prompt(&state, &c, 1500, None);
    let text = prompt_text(&prompt);
    assert!(text.contains(&format!("You could trade (trade): buy food from {other} (")), "{text}");
    assert!(text.contains(&format!("sell a trinket to {other} (")), "{text}");
    assert!(prompt.tools.iter().any(|t| t.name == "trade"));

    let decision =
        interpret_decision(&state, &c, &[call("trade", json!({"person": other, "item": "food", "deal": "buy"}))]);
    assert!(
        matches!(&decision.intents[0].action, IntentAction::Trade { target_id, item: ItemId::Food, buy: true } if target_id == "c2"),
        "{:?}",
        decision.intents
    );
    let decision =
        interpret_decision(&state, &c, &[call("trade", json!({"person": other, "item": "potion", "deal": "buy"}))]);
    assert!(decision.intents.is_empty());
    assert!(decision.problems.iter().any(|p| p.contains("nothing to spare")), "{:?}", decision.problems);

    // A child neither buys nor sells.
    let child = fsw_sim::util::living(&state).find(|c| c.is_child()).cloned().expect("a child");
    let decision =
        interpret_decision(&state, &child, &[call("trade", json!({"person": other, "item": "food", "deal": "buy"}))]);
    assert!(decision.intents.is_empty());
    assert!(decision.problems.iter().any(|p| p.contains("child")), "{:?}", decision.problems);
}

#[test]
fn what_a_villager_can_pay_for_is_named_and_the_commission_tool_offered_only_then() {
    use fsw_sim::data::economy::EXPAND_PRICE;
    let mut state = create_test_state(1);
    let id = fsw_sim::util::living(&state)
        .find(|c| {
            !c.is_child() && fsw_world::places::find_place(&c.home_id).unwrap().models[0].model.contains("home_A")
        })
        .map(|c| c.id.clone())
        .expect("someone in a small cottage");
    state.characters.get_mut(&id).unwrap().gold = 0;
    let c = state.characters[&id].clone();
    let prompt = decide_prompt(&state, &c, 1500, None);
    assert!(!prompt.tools.iter().any(|t| t.name == "commission"), "penniless");
    assert!(!prompt_text(&prompt).contains("(commission)"));

    state.characters.get_mut(&id).unwrap().gold = EXPAND_PRICE;
    let c = state.characters[&id].clone();
    let prompt = decide_prompt(&state, &c, 1500, None);
    let text = prompt_text(&prompt);
    assert!(
        text.contains(&format!("You could pay for (commission): a new room for your cottage ({EXPAND_PRICE} gold); ")),
        "{text}"
    );
    assert!(text.contains("in your yard ("), "{text}");
    assert!(prompt.tools.iter().any(|t| t.name == "commission"));

    let decision = interpret_decision(&state, &c, &[call("commission", json!({"what": "the new room"}))]);
    assert!(
        matches!(&decision.intents[0].action, IntentAction::Commission { what } if what == "expand"),
        "{:?}",
        decision.intents
    );
    let decision = interpret_decision(&state, &c, &[call("commission", json!({"what": "a hay bale"}))]);
    assert!(
        matches!(&decision.intents[0].action, IntentAction::Commission { what } if what.ends_with(":haybale")),
        "{:?}",
        decision.intents
    );

    state.characters.get_mut(&id).unwrap().gold = 0;
    let c = state.characters[&id].clone();
    let decision = interpret_decision(&state, &c, &[call("commission", json!({"what": "a wagon"}))]);
    assert!(decision.intents.is_empty());
    assert!(decision.problems.iter().any(|p| p.contains("nothing to pay for")), "{:?}", decision.problems);
}
