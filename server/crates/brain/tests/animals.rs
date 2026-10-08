//! Animals in the model's view: what the prompts say about pets, horses and the animals about, what the
//! interpreter makes of `pet`, and the name a keeper gives a new pet.

use fsw_brain::llm::types::ToolCall;
use fsw_brain::prompts::{decide_prompt, name_pet_prompt, prompt_text};
use fsw_brain::validate::{interpret_decision, interpret_pet_name};
use fsw_sim::Sim;
use fsw_sim::state::{IntentAction, SimState, Species};
use fsw_testing::create_test_state;
use serde_json::json;

/// The test village with its animals, a moment after its founding.
fn village() -> SimState {
    let mut sim = Sim::new(create_test_state(1));
    sim.step();
    sim.state
}

/// A keeper and their pet, the pet put right beside them.
fn keeper(state: &mut SimState) -> (String, String) {
    let pet = state.animals.values().find(|a| a.owner.is_some()).expect("a pet").id.clone();
    let owner = state.animals[&pet].owner.clone().expect("a keeper");
    let c = state.characters.get_mut(&owner).expect("keeper");
    c.inside = None;
    let pos = c.pos;
    state.animals.get_mut(&pet).expect("pet").pos = fsw_core::vec2(pos.x + 1.0, pos.z);
    (owner, pet)
}

#[test]
fn a_keeper_is_told_of_their_pet_and_may_make_a_fuss_of_it() {
    let mut state = village();
    let (owner, pet) = keeper(&mut state);
    let c = state.characters[&owner].clone();
    let prompt = decide_prompt(&state, &c, 2000, None);
    let text = prompt_text(&prompt);
    let noun = state.animals[&pet].species.def().noun;
    assert!(text.contains(&format!("A {noun} has just come to live with you; it has no name yet.")), "{text}");
    assert!(text.contains("Animals near you (pet):"), "{text}");
    assert!(prompt.tools.iter().any(|t| t.name == "pet"));

    state.animals.get_mut(&pet).expect("pet").name = Some("Biscuit".into());
    let prompt = decide_prompt(&state, &c, 2000, None);
    let text = prompt_text(&prompt);
    assert!(text.contains(&format!("Biscuit, your {noun}, is at your heels.")), "{text}");

    let decision = interpret_decision(&state, &c, &[ToolCall::new("pet", json!({ "animal": "biscuit" }))]);
    assert_eq!(decision.intents.len(), 1, "{:?}", decision.problems);
    assert!(matches!(&decision.intents[0].action, IntentAction::Pet { animal_id } if *animal_id == pet));
    let decision = interpret_decision(&state, &c, &[ToolCall::new("pet", json!({ "animal": "the dragon" }))]);
    assert!(decision.intents.is_empty());
    assert_eq!(decision.problems.len(), 1);
}

#[test]
fn nobody_with_no_animal_near_is_offered_pet() {
    let mut state = village();
    let lonely = state.characters.keys().next().expect("a villager").clone();
    for animal in state.animals.values_mut() {
        animal.pos = fsw_core::vec2(-500.0, -500.0);
    }
    let c = state.characters[&lonely].clone();
    let prompt = decide_prompt(&state, &c, 2000, None);
    assert!(!prompt.tools.iter().any(|t| t.name == "pet"));
    assert!(!prompt_text(&prompt).contains("Animals near you"));
}

#[test]
fn a_rider_is_told_which_horse_they_ride() {
    let mut state = village();
    let horse = state.animals.values().find(|a| a.species == Species::Horse).expect("a horse").id.clone();
    let rider = state.characters.keys().next().expect("a villager").clone();
    state.characters.get_mut(&rider).expect("rider").mount = Some(horse.clone());
    state.animals.get_mut(&horse).expect("horse").rider = Some(rider.clone());
    let name = state.animals[&horse].name.clone().expect("horses are named");
    let c = state.characters[&rider].clone();
    let text = prompt_text(&decide_prompt(&state, &c, 2000, None));
    assert!(text.contains(&format!("You are riding {name}, one of the farm's horses.")), "{text}");
}

#[test]
fn a_keeper_is_asked_to_name_their_pet_and_the_answer_is_cleaned() {
    let mut state = village();
    let (owner, pet) = keeper(&mut state);
    let prompt = name_pet_prompt(&state, &state.characters[&owner], &state.animals[&pet]);
    assert_eq!(prompt.tools.len(), 1);
    assert_eq!(prompt.tools[0].name, "name_pet");
    assert!(prompt_text(&prompt).contains("It needs a name"));
    assert_eq!(
        interpret_pet_name(&[ToolCall::new("name_pet", json!({ "name": " \"old turnip\" " }))]).as_deref(),
        Some("Old turnip")
    );
    assert_eq!(interpret_pet_name(&[ToolCall::new("name_pet", json!({ "name": "R2-D2" }))]), None);
}
