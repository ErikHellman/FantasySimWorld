//! Founding a village one described person at a time, and the commands around it.

use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::data::classes::ClassId;
use fsw_sim::state::{Bond, BondKind, NewVillager, Pronouns, Quirk, SaveFile};
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::founding::ALLOWED_QUIRKS;
use fsw_sim::util::living;
use fsw_sim::{Command, FOUNDERS, Sim, create_state};
use fsw_world::assets::asset_manifest;

fn villager(name: &str, class_id: ClassId) -> NewVillager {
    NewVillager {
        class_id,
        name: name.into(),
        pronouns: Pronouns::TheyThem,
        age: 30,
        occupation: "settler".into(),
        backstory: "Came up the road.".into(),
        traits: vec!["steady".into(), "quiet".into()],
        goals: vec!["stay alive".into(), "make a friend".into()],
        speech_style: "Plain.".into(),
        timbre: "even".into(),
        temper: 0.5,
        bravery: 0.5,
        sociability: 0.5,
        spite: 0.0,
        allure: 0.0,
        shyness: 0.0,
        looks: 0.0,
        secret: String::new(),
        reason: String::new(),
        quirks: Vec::new(),
        bonds: Vec::new(),
    }
}

fn settle(sim: &mut Sim, v: NewVillager) {
    sim.enqueue(Command::Settle(Box::new(v)));
    sim.step();
}

#[test]
fn settlers_arrive_one_by_one_know_each_other_and_the_thirteenth_ends_the_founding() {
    let mut sim = Sim::new(create_state(3));
    settle(&mut sim, villager("Agnes Vane", ClassId::Knight));
    let mut second = villager("Bram Holt", ClassId::Farmer);
    second.bonds.push(Bond {
        to: "Agnes Vane".into(),
        relation: BondKind::Friend,
        summary: "We came up the road together.".into(),
    });
    second.quirks = vec![Quirk::Diligent, Quirk::Murderous];
    settle(&mut sim, second);
    let state = &sim.state;
    assert_eq!(state.order.len(), 2);
    assert!(state.director.founding);
    let (agnes, bram) = (&state.characters["c1"], &state.characters["c2"]);
    assert_eq!(agnes.profile.name, "Agnes Vane");
    assert!(agnes.relationships["c2"].familiarity >= 70.0 && bram.relationships["c1"].familiarity >= 70.0);
    assert_eq!(bram.relationships["c1"].summary, "We came up the road together.");
    assert_eq!(agnes.relationships["c2"].summary, "Bram is my friend.");
    assert!(agnes.look.texture.as_deref().is_some_and(|t| t.starts_with("knight_texture_alt_")));
    assert!(bram.look.texture.is_none(), "farmers have no recolour");
    // The deadly quirks never come in from outside.
    assert_eq!(bram.quirks, vec![Quirk::Diligent]);
    assert!(state.chronicle.iter().any(|e| e.text.starts_with("Bram Holt, settler, has settled at ")));

    // A taken first name, in any case, is refused; so is a second settle of the same person.
    settle(&mut sim, villager("agnes Holloway", ClassId::Mage));
    assert_eq!(sim.state.order.len(), 2);

    for i in 2..FOUNDERS {
        settle(&mut sim, villager(&format!("Settler{i} Ashby"), ClassId::Ranger));
    }
    let state = &sim.state;
    assert_eq!(state.order.len(), FOUNDERS);
    assert!(!state.director.founding);
    assert!(state.chronicle.iter().any(|e| e.text.starts_with("The village is founded")));
    for c in living(state) {
        assert_eq!(c.relationships.len(), FOUNDERS - 1, "{}", c.profile.name);
    }
    // The first wave comes the evening of the day the founding ended.
    assert_eq!(state.director.next_wave_tick, tick_at(1, 20.6));

    // Once founded, more settlers are turned away.
    settle(&mut sim, villager("Late Comer", ClassId::Cleric));
    assert_eq!(sim.state.order.len(), FOUNDERS);
}

#[test]
fn found_village_fills_the_rest_after_some_have_settled() {
    let mut sim = Sim::new(create_state(5));
    for i in 0..5 {
        settle(&mut sim, villager(&format!("Early{i} Farrow"), ClassId::Druid));
    }
    sim.enqueue(Command::FoundVillage);
    sim.step();
    assert_eq!(sim.state.order.len(), FOUNDERS);
    assert!(!sim.state.director.founding);
    assert!(living(&sim.state).filter(|c| c.profile.name.starts_with("Early")).count() == 5);
    // Not twice.
    sim.enqueue(Command::FoundVillage);
    sim.step();
    assert_eq!(sim.state.order.len(), FOUNDERS);
}

#[test]
fn founding_takes_as_long_as_it_takes_and_the_first_night_follows_the_last_settler() {
    let mut sim = Sim::new(create_state(8));
    // A day and a half passes before anyone comes.
    for _ in 0..TICKS_PER_DAY + TICKS_PER_DAY / 2 {
        sim.step();
    }
    assert!(sim.state.monsters.is_empty());
    sim.enqueue(Command::FoundVillage);
    sim.step();
    let tick = sim.state.tick;
    let next = sim.state.director.next_wave_tick;
    assert!(next > tick, "the wave is still ahead");
    assert_eq!(next, tick_at(2, 20.6));
}

#[test]
fn newcomer_drafts_are_held_dropped_when_the_name_is_taken_and_survive_a_save() {
    let mut sim = Sim::new(create_state(9));
    sim.enqueue(Command::FoundVillage);
    sim.step();
    let taken = sim.state.characters["c1"].profile.name.clone();
    sim.enqueue(Command::NewcomerDraft(Box::new(villager(&taken, ClassId::Rogue))));
    sim.step();
    assert!(sim.state.director.newcomer_draft.is_none());
    sim.enqueue(Command::NewcomerDraft(Box::new(villager("Fresh Face", ClassId::Rogue))));
    sim.step();
    assert_eq!(sim.state.director.newcomer_draft.as_ref().map(|v| v.name.as_str()), Some("Fresh Face"));
    let json = serde_json::to_string(&sim.state).unwrap();
    assert!(json.contains("\"newcomerDraft\""));
    let back: fsw_sim::SimState = serde_json::from_str(&json).unwrap();
    assert_eq!(back, sim.state);
    // An unasked-for "nothing is coming" changes nothing.
    sim.enqueue(Command::NewcomerLocal);
    sim.step();
    assert_eq!(sim.state.director.newcomer_deadline_tick, 0);
}

#[test]
fn old_saves_load_as_founded_villages() {
    for name in ["export-day-1", "export-day-2"] {
        let text =
            std::fs::read_to_string(format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let save: SaveFile = serde_json::from_str(&text).unwrap();
        assert!(!save.state.director.founding, "{name}");
        assert!(save.state.director.newcomer_draft.is_none(), "{name}");
        assert_eq!(save.state.director.newcomer_deadline_tick, 0, "{name}");
    }
}

#[test]
fn every_class_model_and_recolour_is_in_the_asset_manifest() {
    let manifest = asset_manifest();
    for class in ClassId::ALL {
        let model = class.def().model;
        let character = manifest.characters.get(model).unwrap_or_else(|| panic!("{class:?}: no model {model}"));
        assert!(manifest.rigs.contains_key(&character.rig), "{model}: no rig {}", character.rig);
    }
    // Every recolour the founding may pick exists (the Rust manifest leaves the textures out, so read the file).
    let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/asset-manifest.json")).unwrap();
    let textures: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut sim = Sim::new(create_state(11));
    sim.enqueue(Command::FoundVillage);
    sim.step();
    for c in living(&sim.state) {
        if let Some(texture) = &c.look.texture {
            assert!(textures["textures"].get(texture).is_some(), "{}: no texture {texture}", c.profile.name);
        }
    }
    assert!(ALLOWED_QUIRKS.iter().all(|q| !matches!(q, Quirk::Murderous | Quirk::Occultist)));
}
