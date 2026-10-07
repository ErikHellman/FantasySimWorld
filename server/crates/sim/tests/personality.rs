//! Shyness and looks: the Greymane brothers' traits, and that everyone else is untouched by them.

use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::state::{Character, Profile, Pronouns, SimState};
use fsw_sim::systems::relationships::{RelationDelta, adjust, looks_welcome};
use fsw_sim::util::{Ctx, chemistry, drawn_to, first_name, living, openness, rel};
use fsw_sim::{Sim, SimEvent, create_state};

fn who(state: &SimState, name: &str) -> String {
    living(state).find(|c| first_name(c) == name).unwrap().id.clone()
}

fn with_trust(state: &mut SimState, from: &str, to: &str, trust: f64) {
    rel(state.characters.get_mut(from).unwrap(), to).trust = trust;
}

#[test]
fn the_shy_open_up_to_those_they_trust_and_least_to_women() {
    let mut state = create_state(1);
    let (torvald, brenna, aldric) = (who(&state, "Torvald"), who(&state, "Brenna"), who(&state, "Aldric"));
    let open = |state: &SimState, to: &str| openness(&state.characters[&torvald], &state.characters[to]);

    with_trust(&mut state, &torvald, &brenna, 0.0);
    with_trust(&mut state, &torvald, &aldric, 0.0);
    assert!(open(&state, &brenna) < open(&state, &aldric), "worse with women");
    assert!(open(&state, &brenna) < 0.2);

    let mut last = 0.0;
    for trust in [0.0, 10.0, 20.0, 30.0, 40.0] {
        with_trust(&mut state, &torvald, &brenna, trust);
        let now = open(&state, &brenna);
        assert!(now > last, "opens up as trust grows");
        last = now;
    }
    assert_eq!(last, 1.0, "with someone he trusts he talks like anyone");

    // Nobody else is shy at all.
    let garrick = &state.characters[&who(&state, "Garrick")];
    assert_eq!(openness(garrick, &state.characters[&brenna]), 1.0);
}

/// Conversations a very sociable Torvald starts with women over a few days.
fn chats_with_women(seed: u32, shyness: f64) -> usize {
    let mut state = create_state(seed);
    let torvald = who(&state, "Torvald");
    let profile = &mut state.characters.get_mut(&torvald).unwrap().profile;
    profile.shyness = shyness;
    profile.sociability = 1.0;
    let mut sim = Sim::new(state);
    let mut started = 0;
    for _ in 0..TICKS_PER_DAY * 3 {
        for event in sim.step() {
            if let SimEvent::ConversationStarted { a, b, .. } = event
                && a == torvald
                && sim.state.characters.get(&b).is_some_and(|c| c.profile.pronouns == Pronouns::SheHer && !c.is_child())
            {
                started += 1;
            }
        }
    }
    started
}

#[test]
fn a_shy_villager_seldom_starts_a_conversation_with_a_woman() {
    // Eight seeds: a few chats a seed is too few to tell shy from unlucky over four.
    let total = |shyness: f64| (1..=8).map(|seed| chats_with_women(seed, shyness)).sum::<usize>();
    let (shy, at_ease) = (total(0.85), total(0.0));
    assert!(shy * 2 <= at_ease, "shy {shy}, at ease {at_ease}");
}

#[test]
fn everyone_warms_to_a_good_looking_person_and_keeps_warming() {
    let mut state = create_state(1);
    let (torvald, garrick, kael) = (who(&state, "Torvald"), who(&state, "Garrick"), who(&state, "Kael"));
    // Kael has no authored bond with him: the head start comes from his looks alone.
    let welcome = looks_welcome(&state.characters[&torvald]);
    assert!(welcome >= 19.0);
    assert!(state.characters[&kael].relationships[&torvald].affinity >= welcome);
    assert_eq!(looks_welcome(&state.characters[&garrick]), 0.0);

    let before = |state: &SimState, to: &str| state.characters[&kael].relationships[to].affinity;
    let (to_torvald, to_garrick) = (before(&state, &torvald), before(&state, &garrick));
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut state, &mut events);
    let warm = RelationDelta { affinity: 4.0, ..Default::default() };
    adjust(&mut ctx, &kael, &torvald, warm);
    adjust(&mut ctx, &kael, &garrick, warm);
    assert!(before(&state, &torvald) - to_torvald > 5.5);
    assert_eq!(before(&state, &garrick) - to_garrick, 4.0);
}

#[test]
fn a_good_looking_person_turns_most_heads() {
    let state = create_state(1);
    let torvald = &state.characters[&who(&state, "Torvald")];
    let others: Vec<&Character> = living(&state).filter(|c| c.id != torvald.id).collect();
    let drawn = others.iter().filter(|c| drawn_to(c, torvald)).count();
    assert!(drawn * 10 >= others.len() * 8, "{drawn} of {}", others.len());
    // Without looks, being drawn to someone is just the pair's chemistry.
    for a in &others {
        for b in &others {
            if a.id != b.id {
                assert_eq!(drawn_to(a, b), chemistry(a, b));
            }
        }
    }
}

#[test]
fn profiles_without_shyness_or_looks_read_and_write_as_before() {
    let state = create_state(1);
    let garrick = &state.characters[&who(&state, "Garrick")].profile;
    let json = serde_json::to_value(garrick).unwrap();
    assert!(json.get("shyness").is_none() && json.get("looks").is_none());
    let back: Profile = serde_json::from_value(json).unwrap();
    assert_eq!((back.shyness, back.looks), (0.0, 0.0));

    let torvald = &state.characters[&who(&state, "Torvald")].profile;
    let json = serde_json::to_value(torvald).unwrap();
    assert_eq!(json["shyness"], 0.85);
    assert_eq!(&serde_json::from_value::<Profile>(json).unwrap(), torvald);
}
