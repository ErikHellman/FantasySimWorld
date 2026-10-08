//! The valley's animals: given to the village once it is founded, keeping to their haunts, running from the dead,
//! following their keepers, and named by them.

use fsw_core::Vec2;
use fsw_core::math2d::dist;
use fsw_core::vec2;
use fsw_sim::Command;
use fsw_sim::Sim;
use fsw_sim::commands::TuningPatch;
use fsw_sim::state::{ActivityId, Intent, IntentAction, IntentSource};
use fsw_sim::state::{AnimalAnim, MonsterKind, MonsterOrigin, SimState, Species};
use fsw_sim::systems::animals::{FLEE_RADIUS, STRIDE, clean_name, pasture, update_animals};
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::intents::fresh_act;
use fsw_sim::systems::monsters::spawn_monster;
use fsw_sim::util::{Ctx, first_name};
use fsw_testing::create_test_state;
use fsw_world::places::{get_place, places};

fn founded(seed: u32) -> Sim {
    let mut sim = Sim::new(create_test_state(seed));
    sim.state.director.next_wave_tick = i64::MAX;
    sim.step();
    sim
}

fn count(state: &SimState, species: Species) -> usize {
    state.animals.values().filter(|a| a.species == species).count()
}

#[test]
fn the_village_gets_its_animals_once_it_is_founded() {
    let sim = founded(1);
    let state = &sim.state;
    assert!(state.fauna.founded);
    assert_eq!(count(state, Species::Cow), 2);
    assert_eq!(count(state, Species::Sheep), 4);
    assert_eq!(count(state, Species::Horse), 3);
    assert_eq!(count(state, Species::Duck), 3);
    for animal in state.animals.values() {
        match animal.species {
            Species::Cow | Species::Sheep | Species::Goat => assert!(dist(animal.pos, pasture()) < 14.0, "{animal:?}"),
            Species::Duck => assert!(dist(animal.pos, get_place("docks").door) < 9.0, "{animal:?}"),
            Species::Horse => assert!(animal.name.is_some(), "a horse has a name"),
            Species::Dog | Species::Cat => {
                let owner = animal.owner.as_deref().expect("a pet has a keeper");
                assert!(!state.characters[owner].is_child());
                assert!(animal.name.is_none(), "a new pet waits for its keeper to name it");
            }
            _ => {}
        }
    }
    assert!(count(state, Species::Dog) + count(state, Species::Cat) > 0, "some homes keep a pet");
}

#[test]
fn animals_draw_nothing_from_the_villagers_generator() {
    // What the animals do on their own never shifts the villagers' draws.
    let mut sim = founded(5);
    let rng = sim.state.rng;
    let mut events = Vec::new();
    for _ in 0..2400 {
        sim.state.tick += 1;
        let mut ctx = Ctx::new(&mut sim.state, &mut events);
        update_animals(&mut ctx);
    }
    assert_eq!(sim.state.rng, rng);
    assert!(sim.state.animals.values().any(|a| a.path.len() + usize::from(a.flight.is_some()) > 0 || a.moving));
}

#[test]
fn animals_keep_to_walkable_ground_for_three_days() {
    let mut sim = founded(3);
    for tick in 0..(3 * 24 * 60 * 20) {
        sim.step();
        if tick % 200 != 0 {
            continue;
        }
        for animal in sim.state.animals.values() {
            if animal.flight.is_some() {
                continue;
            }
            assert!(sim.state.nav.is_walkable(animal.pos.x, animal.pos.z), "{} at {:?}", animal.id, animal.pos);
        }
    }
    // The herd is still about its pasture.
    let away =
        sim.state.animals.values().filter(|a| a.species == Species::Cow && dist(a.pos, pasture()) > 25.0).count();
    assert_eq!(away, 0);
}

#[test]
fn animals_run_from_the_dead_and_the_dead_never_go_for_them() {
    let mut sim = founded(2);
    sim.state.tick = tick_at(2, 22.0);
    let sheep = sim.state.animals.values().find(|a| a.species == Species::Sheep).expect("a sheep").id.clone();
    let at = sim.state.animals[&sheep].pos;
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut sim.state, &mut events);
    let near = ctx.state.nav.nearest_walkable(vec2(at.x + 3.0, at.z), 3).expect("open ground by the sheep");
    let monster = spawn_monster(&mut ctx, MonsterKind::SkeletonMinion, near, 0.0, MonsterOrigin::Graveyard);
    for _ in 0..80 {
        sim.step();
        let m = &sim.state.monsters[&monster];
        assert!(m.target_id.as_deref().is_none_or(|t| !sim.state.animals.contains_key(t)));
    }
    let sheep = &sim.state.animals[&sheep];
    assert!(dist(sheep.pos, at) > FLEE_RADIUS * 0.4, "the sheep ran: {:?} from {at:?}", sheep.pos);
}

#[test]
fn a_pet_keeps_to_its_keeper_while_they_are_out() {
    let mut sim = founded(1);
    sim.state.tick = tick_at(2, 10.0);
    let pet = sim.state.animals.values().find(|a| a.owner.is_some()).expect("a pet").clone();
    let owner = pet.owner.clone().expect("a keeper");
    for _ in 0..1200 {
        sim.step();
    }
    let keeper = &sim.state.characters[&owner];
    let pet = &sim.state.animals[&pet.id];
    if keeper.inside.is_none() {
        assert!(dist(pet.pos, keeper.pos) < 12.0, "{:?} far from its keeper at {:?}", pet.pos, keeper.pos);
    }
}

#[test]
fn a_keeper_names_their_pet_once_and_never_after_a_villager() {
    let mut sim = founded(1);
    let pet = sim.state.animals.values().find(|a| a.owner.is_some()).expect("a pet").id.clone();
    let villager = first_name(sim.state.characters.values().next().expect("a villager")).to_owned();
    sim.enqueue(Command::NameAnimal { animal_id: pet.clone(), name: villager });
    sim.step();
    assert!(sim.state.animals[&pet].name.is_none(), "a villager's name is taken");
    sim.enqueue(Command::NameAnimal { animal_id: pet.clone(), name: "  old   grumbleguts!  ".into() });
    sim.step();
    assert_eq!(sim.state.animals[&pet].name.as_deref(), Some("Old grumbleguts"));
    sim.enqueue(Command::NameAnimal { animal_id: pet.clone(), name: "Pip".into() });
    sim.step();
    assert_eq!(sim.state.animals[&pet].name.as_deref(), Some("Old grumbleguts"), "named once");
    let owner = sim.state.animals[&pet].owner.clone().expect("a keeper");
    assert!(sim.state.characters[&owner].memories.iter().any(|m| m.text.contains("Old grumbleguts")));
}

#[test]
fn an_unnamed_pet_gets_a_name_of_its_own_in_the_end() {
    let mut sim = founded(1);
    for _ in 0..(61 * 20) {
        sim.step();
    }
    for pet in sim.state.animals.values().filter(|a| a.owner.is_some()) {
        assert!(pet.name.is_some(), "{} is named", pet.id);
    }
}

#[test]
fn names_are_cleaned() {
    assert_eq!(clean_name("biscuit").as_deref(), Some("Biscuit"));
    assert_eq!(clean_name("\"Sir Barksalot the Third\"").as_deref(), Some("Sir Barksalot"));
    assert_eq!(clean_name("R2-D2"), None);
    assert_eq!(clean_name("   "), None);
    assert_eq!(clean_name("Bartholomew Fluffington"), None, "too long");
}

#[test]
fn a_pet_goes_to_a_housemate_when_its_keeper_dies() {
    let mut sim = founded(1);
    let state = &sim.state;
    let pet = state
        .animals
        .values()
        .find(|a| {
            let home = a.home_id.as_deref();
            a.owner.is_some()
                && state.characters.values().filter(|c| Some(c.home_id.as_str()) == home && !c.is_child()).count() > 1
        })
        .expect("a pet in a house of two")
        .clone();
    let owner = pet.owner.clone().expect("a keeper");
    sim.state.characters.shift_remove(&owner);
    sim.state.order.retain(|id| *id != owner);
    for _ in 0..STRIDE {
        sim.step();
    }
    let heir = sim.state.animals[&pet.id].owner.clone().expect("someone took it in");
    assert_ne!(heir, owner);
    assert_eq!(Some(sim.state.characters[&heir].home_id.clone()), pet.home_id);
}

#[test]
fn animals_survive_a_save_and_load() {
    let mut sim = founded(4);
    for _ in 0..400 {
        sim.step();
    }
    let json = serde_json::to_string(&sim.state).expect("save");
    let loaded: SimState = serde_json::from_str(&json).expect("load");
    assert_eq!(loaded.animals, sim.state.animals);
    assert_eq!(loaded.fauna, sim.state.fauna);
    let mut a = Sim::new(loaded);
    for _ in 0..400 {
        a.step();
        sim.step();
    }
    assert_eq!(a.state.animals, sim.state.animals);
}

#[test]
fn sleepers_sleep_at_night() {
    let mut sim = founded(6);
    for _ in 0..(16 * 60 * 20) {
        sim.step();
    }
    // Past ten at night most of the herd has bedded down.
    let herd: Vec<_> = sim.state.animals.values().filter(|a| a.species == Species::Sheep).collect();
    let asleep = herd.iter().filter(|a| a.anim == AnimalAnim::Sleep).count();
    assert!(asleep * 2 >= herd.len(), "{asleep} of {} sheep asleep", herd.len());
}

/// Puts someone out in the open, awake, at `pos`, with nothing to do for an hour.
fn idle_at(state: &mut SimState, id: &str, pos: Vec2) {
    let tick = state.tick;
    let c = state.characters.get_mut(id).expect("a villager");
    c.act = fresh_act(tick);
    c.pos = pos;
    c.inside = None;
    c.asleep = false;
    c.slot_id = None;
    c.intent = Some(Intent::new(IntentAction::Wait { minutes: 60.0 }, IntentSource::Routine));
    c.queue.clear();
    c.needs.hunger = 90.0;
    c.needs.energy = 90.0;
}

fn set_intent(sim: &mut Sim, id: &str, action: IntentAction) {
    let epoch = sim.state.characters[id].epoch;
    sim.enqueue(Command::SetIntents { id: id.into(), epoch, intents: vec![Intent::new(action, IntentSource::Llm)] });
}

fn first_adult(state: &SimState) -> String {
    state
        .order
        .iter()
        .find(|id| !state.characters[*id].is_child() && state.characters[*id].convo_id.is_none())
        .expect("a grown villager not talking")
        .clone()
}

/// A village where nobody starts a conversation, which would set aside what a test asks of someone.
fn quiet(seed: u32) -> Sim {
    let mut sim = founded(seed);
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    for _ in 0..1200 {
        sim.step();
    }
    sim
}

#[test]
fn a_farmer_tends_the_herd_and_gets_food_for_it() {
    let mut sim = quiet(1);
    let farmer = first_adult(&sim.state);
    idle_at(&mut sim.state, &farmer, get_place("farm").door);
    sim.state.characters.get_mut(&farmer).expect("farmer").inventory.food = 0;
    let tend = IntentAction::Activity { activity: ActivityId::Tend, place_id: None, minutes: Some(65.0) };
    set_intent(&mut sim, &farmer, tend);
    let mut beside_an_animal = false;
    for _ in 0..(90 * 20) {
        sim.step();
        let c = &sim.state.characters[&farmer];
        let tending = c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Tend));
        if tending && c.act.phase == fsw_sim::state::ActPhase::Perform {
            beside_an_animal |= sim.state.animals.values().any(|a| a.owner.is_none() && dist(a.pos, c.pos) < 4.0);
        }
    }
    assert!(beside_an_animal, "the farmer stood beside one of the herd");
    assert!(sim.state.characters[&farmer].inventory.food >= 1, "tending gave food");
}

#[test]
fn a_keeper_makes_a_fuss_of_their_pet() {
    let mut sim = quiet(1);
    let state = &sim.state;
    let pet = state
        .animals
        .values()
        .find(|a| a.owner.as_deref().is_some_and(|o| state.characters[o].convo_id.is_none()))
        .expect("a pet whose keeper is not talking")
        .clone();
    let owner = pet.owner.clone().expect("a keeper");
    let near = sim.state.nav.nearest_walkable(vec2(pet.pos.x + 2.0, pet.pos.z), 3).expect("open ground");
    idle_at(&mut sim.state, &owner, near);
    set_intent(&mut sim, &owner, IntentAction::Pet { animal_id: pet.id.clone() });
    let mut hearts = 0;
    for _ in 0..400 {
        for event in sim.step() {
            if matches!(&event, fsw_sim::events::SimEvent::Emote { id, emote: fsw_sim::events::Emote::Heart } if *id == owner)
            {
                hearts += 1;
            }
        }
    }
    assert!(sim.state.animals[&pet.id].petted_tick > 0);
    assert_eq!(hearts, 1);
}

#[test]
fn a_villager_rides_a_free_horse_on_a_long_trip_and_it_waits_then_goes_home() {
    let mut sim = quiet(1);
    let horse = sim.state.animals.values().find(|a| a.species == Species::Horse).expect("a horse").clone();
    let rider = first_adult(&sim.state);
    let start = sim.state.nav.nearest_walkable(vec2(horse.pos.x + 4.0, horse.pos.z), 3).expect("open ground");
    idle_at(&mut sim.state, &rider, start);
    // Somewhere far off.
    let far = places()
        .iter()
        .filter(|p| !p.id.starts_with("home"))
        .max_by(|a, b| dist(a.door, start).total_cmp(&dist(b.door, start)))
        .expect("a place");
    let goal = far.door;

    let mut on_foot = sim.state.clone();
    on_foot.animals.retain(|_, a| a.species != Species::Horse);
    let arrive = |mut sim: Sim, rider: &str| -> (i64, bool, Sim) {
        let epoch = sim.state.characters[rider].epoch;
        let intent = Intent::new(IntentAction::GoTo { place_id: far.id.clone() }, IntentSource::Llm);
        sim.enqueue(Command::SetIntents { id: rider.into(), epoch, intents: vec![intent] });
        let mut rode = false;
        for t in 0..6000 {
            sim.step();
            let c = &sim.state.characters[rider];
            rode |= c.mount.is_some();
            if dist(c.pos, goal) < 4.0 {
                return (t, rode, sim);
            }
        }
        panic!("{rider} never got to {}", far.id);
    };
    let (walked, walker_rode, _) = arrive(Sim::new(on_foot), &rider);
    let (ridden, rode, mut sim) = arrive(sim, &rider);
    assert!(!walker_rode);
    assert!(rode, "the rider got on the horse");
    assert!(ridden < walked, "riding ({ridden} ticks) beats walking ({walked})");

    // Got down at the end, and the horse waits there for them while they bide there into the night, so they do not
    // ride it off again.
    set_intent(&mut sim, &rider, IntentAction::Wait { minutes: 900.0 });
    for _ in 0..40 {
        sim.step();
    }
    let h = &sim.state.animals[&horse.id];
    assert_eq!(h.rider, None);
    assert_eq!(sim.state.characters[&rider].mount, None);
    assert_eq!(h.waiting_for.as_deref(), Some(rider.as_str()));
    assert!(dist(h.pos, goal) < 8.0);

    // Come nightfall it gives up on them and goes home to the pasture.
    while sim.state.tick < tick_at(2, 1.0) {
        sim.step();
    }
    let h = &sim.state.animals[&horse.id];
    assert_eq!(h.waiting_for, None);
    assert!(dist(h.pos, pasture()) < 25.0, "the horse is home: {:?}", h.pos);
}
