//! Allure (`systems::charm`): a charmer flirts for sport, draws admirers
//! without chemistry, sets them against each other and enjoys the show.

use fsw_core::Vec2;
use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::Command;
use fsw_sim::commands::TuningPatch;
use fsw_sim::data::smalltalk::local_script;
use fsw_sim::state::{Feeling, FightMode, Intent, IntentAction, IntentSource, SimState, TalkTone};
use fsw_sim::systems::charm::{JEALOUSY_CAP, stir_jealousy, work_charm};
use fsw_sim::systems::relationships::{RUNG_FEUD, apply_conversation};
use fsw_sim::util::{Ctx, chemistry, first_name, living, rel, rel_or_default};
use fsw_sim::{Sim, SimEvent};
use fsw_testing::create_test_state;

fn who(state: &SimState, name: &str) -> String {
    living(state).find(|c| first_name(c) == name).expect(name).id.clone()
}

/// A quiet world a few ticks in: no small talk, so nothing interrupts what a test sets up.
fn quiet(seed: u32) -> Sim {
    let mut sim = Sim::new(create_test_state(seed));
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    for _ in 0..5 {
        sim.step();
    }
    sim
}

/// Puts someone out in the open, awake, at `pos`.
fn place(state: &mut SimState, id: &str, pos: Vec2) {
    let c = state.characters.get_mut(id).unwrap();
    c.pos = pos;
    c.inside = None;
    c.asleep = false;
}

fn attraction(state: &SimState, from: &str, to: &str) -> f64 {
    rel_or_default(&state.characters[from], to).attraction
}

fn grievance(state: &SimState, from: &str, to: &str) -> f64 {
    rel_or_default(&state.characters[from], to).grievance
}

#[test]
fn mira_is_a_charmer_and_the_rest_of_the_village_is_not() {
    let state = create_test_state(1);
    for c in living(&state) {
        let expected = if first_name(c) == "Mira" { 0.9 } else { 0.0 };
        assert_eq!(c.profile.allure, expected, "{}", c.profile.name);
    }
}

#[test]
fn allure_is_left_out_of_saves_when_zero_and_read_as_zero_when_missing() {
    let state = create_test_state(1);
    let json = serde_json::to_value(&state).unwrap();
    let profiles: Vec<&serde_json::Value> =
        json["characters"].as_object().unwrap().values().map(|c| &c["profile"]).collect();
    assert_eq!(profiles.iter().filter(|p| p.get("allure").is_some()).count(), 1);
    let mut stripped = json.clone();
    for c in stripped["characters"].as_object_mut().unwrap().values_mut() {
        c["profile"].as_object_mut().unwrap().remove("allure");
    }
    let loaded: SimState = serde_json::from_value(stripped).unwrap();
    assert!(living(&loaded).all(|c| c.profile.allure == 0.0));
}

#[test]
fn a_charmer_draws_admirers_without_chemistry_and_rarely_falls_herself() {
    let sim = quiet(31);
    let (mira, garrick) = (who(&sim.state, "Mira"), who(&sim.state, "Garrick"));
    assert!(!chemistry(&sim.state.characters[&mira], &sim.state.characters[&garrick]));
    let gains = |allure: f64| {
        let mut state = sim.state.clone();
        state.characters.get_mut(&mira).unwrap().profile.allure = allure;
        let (his, hers) = (attraction(&state, &garrick, &mira), attraction(&state, &mira, &garrick));
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut state, &mut events);
        apply_conversation(&mut ctx, &mira, &garrick, Feeling::Better, TalkTone::Romantic, true);
        apply_conversation(&mut ctx, &garrick, &mira, Feeling::Better, TalkTone::Romantic, false);
        (attraction(&state, &garrick, &mira) - his, attraction(&state, &mira, &garrick) - hers)
    };
    assert_eq!(gains(0.0), (9.0, 9.0));
    let (his, hers) = gains(0.9);
    assert!(his > 9.0, "his attraction rose by {his}");
    assert!(hers < 9.0, "her attraction rose by {hers}");
}

#[test]
fn a_charmer_flirts_for_sport_teases_those_who_want_her_and_is_rebuffed_by_those_who_do_not() {
    let mut sim = quiet(31);
    let (mira, theron, vex) = (who(&sim.state, "Mira"), who(&sim.state, "Theron"), who(&sim.state, "Vex"));
    let flirt = local_script(&mut sim.state, &mira, &theron, TalkTone::Romantic, None);
    assert_eq!(flirt.feelings, [Feeling::Better, Feeling::Better]);
    let cold = local_script(&mut sim.state, &mira, &vex, TalkTone::Romantic, None);
    assert_eq!(cold.topic, "a cold shoulder");
    assert_eq!(cold.feelings, [Feeling::Same, Feeling::Worse]);
    let tease = local_script(&mut sim.state, &theron, &mira, TalkTone::Romantic, None);
    assert_eq!(tease.topic, "feelings, teased");
    assert_eq!(tease.feelings, [Feeling::Same, Feeling::Better]);
}

/// Romantic talks Mira started in two days of an ordinary village.
fn mira_flirts(seed: u32, allure: f64) -> usize {
    let mut sim = Sim::new(create_test_state(seed));
    let mira = who(&sim.state, "Mira");
    sim.state.characters.get_mut(&mira).unwrap().profile.allure = allure;
    let mut flirts = 0;
    for _ in 0..TICKS_PER_DAY * 2 {
        for event in sim.step() {
            if matches!(&event, SimEvent::ConversationStarted { a, tone: TalkTone::Romantic, .. } if *a == mira) {
                flirts += 1;
            }
        }
    }
    flirts
}

#[test]
fn a_charmer_starts_flirting_on_her_own() {
    // Over several worlds, since in any one of them she may simply be busy (with the animals about, seeds 1 to 4
    // happen to keep her busier: six worlds give her room).
    let seeds = 1..=6;
    assert!(seeds.clone().map(|seed| mira_flirts(seed, 0.9)).sum::<usize>() >= 2);
    assert!(seeds.into_iter().all(|seed| mira_flirts(seed, 0.0) == 0));
}

#[test]
fn an_admirer_who_watches_her_flirt_resents_the_man_she_flirts_with() {
    let mut sim = quiet(31);
    let state = &mut sim.state;
    let (mira, theron, kael, dorn) = (who(state, "Mira"), who(state, "Theron"), who(state, "Kael"), who(state, "Dorn"));
    let here = state.characters[&mira].pos;
    place(state, &mira, here);
    place(state, &theron, Vec2 { x: here.x + 1.5, z: here.z });
    place(state, &kael, Vec2 { x: here.x, z: here.z + 6.0 });
    place(state, &dorn, Vec2 { x: here.x + 100.0, z: here.z });
    rel(state.characters.get_mut(&kael).unwrap(), &mira).attraction = 40.0;
    rel(state.characters.get_mut(&dorn).unwrap(), &mira).attraction = 40.0;
    let (kael_before, dorn_before) = (grievance(state, &kael, &theron), grievance(state, &dorn, &theron));

    let mut events = Vec::new();
    let mut ctx = Ctx::new(state, &mut events);
    stir_jealousy(&mut ctx, &mira, &theron);

    assert!(grievance(state, &kael, &theron) >= kael_before + 9.0);
    assert_eq!(grievance(state, &dorn, &theron), dorn_before);
    let k = &state.characters[&kael];
    assert!(k.memories.iter().any(|m| m.text.contains("Mira flirting with Theron")));
    assert!(k.stimuli.iter().any(|s| s.subject_id.as_deref() == Some(theron.as_str())));
    assert!(state.chronicle.iter().any(|e| e.text.contains("watched Mira flirt with Theron")));
}

#[test]
fn jealousy_alone_never_reaches_a_blood_feud() {
    let mut sim = quiet(31);
    let state = &mut sim.state;
    let (mira, theron, kael) = (who(state, "Mira"), who(state, "Theron"), who(state, "Kael"));
    let here = state.characters[&mira].pos;
    place(state, &mira, here);
    place(state, &kael, Vec2 { x: here.x + 2.0, z: here.z });
    rel(state.characters.get_mut(&kael).unwrap(), &mira).attraction = 100.0;
    rel(state.characters.get_mut(&kael).unwrap(), &theron).grievance = 70.0;
    for _ in 0..5 {
        let mut events = Vec::new();
        stir_jealousy(&mut Ctx::new(state, &mut events), &mira, &theron);
    }
    assert_eq!(grievance(state, &kael, &theron), JEALOUSY_CAP);
}

#[test]
fn a_charmer_turns_an_admirer_against_her_enemy_and_takes_his_gift() {
    let mut sim = quiet(31);
    let state = &mut sim.state;
    let (mira, theron, vex) = (who(state, "Mira"), who(state, "Theron"), who(state, "Vex"));
    state.characters.get_mut(&theron).unwrap().inventory.gift = 1;
    state.characters.get_mut(&mira).unwrap().inventory.gift = 0;
    let before = grievance(state, &theron, &vex);

    let mut events = Vec::new();
    work_charm(&mut Ctx::new(state, &mut events), &mira, &theron);

    assert_eq!(grievance(state, &theron, &vex), before + 6.0);
    assert_eq!(state.characters[&theron].inventory.gift, 0);
    assert_eq!(state.characters[&mira].inventory.gift, 1);
    assert!(state.characters[&theron].memories.iter().any(|m| m.text.contains("confided in me about Vex")));
}

#[test]
fn a_charmer_enjoys_two_admirers_coming_to_blows() {
    let mut sim = quiet(31);
    let (mira, theron, kael) = (who(&sim.state, "Mira"), who(&sim.state, "Theron"), who(&sim.state, "Kael"));
    for admirer in [&theron, &kael] {
        rel(sim.state.characters.get_mut(admirer).unwrap(), &mira).attraction = 40.0;
    }
    rel(sim.state.characters.get_mut(&theron).unwrap(), &kael).grievance = RUNG_FEUD + 5.0;
    let epoch = sim.state.characters[&theron].epoch;
    let fight = Intent::new(IntentAction::Fight { target_id: kael.clone(), mode: FightMode::Brawl }, IntentSource::Llm);
    sim.enqueue(Command::SetIntents { id: theron.clone(), epoch, intents: vec![fight] });
    let over_her = |state: &SimState| state.chronicle.iter().any(|e| e.text.contains("really fighting over: Mira"));
    for _ in 0..6000 {
        if over_her(&sim.state) {
            break;
        }
        sim.step();
    }
    assert!(over_her(&sim.state));
    assert!(sim.state.characters[&mira].memories.iter().any(|m| m.text.contains("came to blows over me")));
}
