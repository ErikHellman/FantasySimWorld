//! The valley's animals: given to the village once it is founded, keeping to their haunts, running from the dead,
//! following their keepers, and named by them.

use fsw_core::math2d::dist;
use fsw_core::vec2;
use fsw_sim::Command;
use fsw_sim::Sim;
use fsw_sim::state::{AnimalAnim, MonsterKind, MonsterOrigin, SimState, Species};
use fsw_sim::systems::animals::{FLEE_RADIUS, STRIDE, clean_name, pasture};
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::monsters::spawn_monster;
use fsw_sim::util::{Ctx, first_name};
use fsw_testing::create_test_state;
use fsw_world::places::get_place;

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
fn a_world_without_animals_gets_them_and_its_villagers_draw_as_before() {
    // The animals draw from their own generator: the villagers' world goes on exactly as it would without them.
    let with = create_test_state(5);
    let mut without = with.clone();
    without.fauna.founded = true;
    without.fauna.next_stray_tick = i64::MAX;
    let mut a = Sim::new(with);
    let mut b = Sim::new(without);
    for _ in 0..2400 {
        a.step();
        b.step();
    }
    assert!(!a.state.animals.is_empty());
    assert!(b.state.animals.is_empty());
    assert_eq!(a.state.rng, b.state.rng);
    for (id, c) in &a.state.characters {
        assert_eq!(c.pos, b.state.characters[id].pos, "{id} walked the same");
    }
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
