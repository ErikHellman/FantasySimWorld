//! House fires: the dead burn a house while the village sleeps, everyone carries water, and a house
//! that is lost is rebuilt while those who lived there sleep rough.

use fsw_core::math2d::dist;
use fsw_core::time::{TICKS_PER_DAY, game_time};
use fsw_sim::Command;
use fsw_sim::commands::TuningPatch;
use fsw_sim::data::monsters::MonsterKind;
use fsw_sim::describe::describe_activity;
use fsw_sim::state::{
    ActPhase, ActivityId, ChronicleKind, House, HousePhase, Intent, IntentAction, IntentSource, SimState,
};
use fsw_sim::systems::arrivals::free_home;
use fsw_sim::systems::combat::{max_hp, start_flee};
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::fire::{ignite, is_homeless};
use fsw_sim::systems::intents::fresh_act;
use fsw_sim::systems::monsters::spawn_monster;
use fsw_sim::util::{Ctx, living, living_ids};
use fsw_sim::{Sim, create_state};
use fsw_world::places::get_place;

fn quiet(state: SimState) -> Sim {
    let mut sim = Sim::new(state);
    // No small talk, so nobody is held up in a chat.
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    sim
}

/// One in the morning on the second night, everyone in their own bed, no watch set and no dead about.
fn asleep_at_night(seed: u32) -> Sim {
    let mut state = create_state(seed);
    state.tick = tick_at(2, 1.0);
    state.director.watch_day = game_time(state.tick).day;
    state.director.next_wave_tick = i64::MAX;
    let tick = state.tick;
    for id in living_ids(&state) {
        let c = state.characters.get_mut(&id).unwrap();
        c.pos = get_place(&c.home_id).door;
        c.inside = Some(c.home_id.clone());
        c.asleep = true;
        c.watch_until_tick = 0;
        c.plan_block = -1;
        c.intent = Some(Intent::new(
            IntentAction::Activity { activity: ActivityId::Sleep, place_id: None, minutes: Some(280.0) },
            IntentSource::Routine,
        ));
        c.act = fresh_act(tick);
        c.act.phase = ActPhase::Perform;
        c.act.until_tick = tick + 280 * 20;
    }
    quiet(state)
}

fn raise_a_monster(sim: &mut Sim) {
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut sim.state, &mut events);
    let at = get_place("green").door;
    spawn_monster(&mut ctx, MonsterKind::SkeletonMinion, at, 0.0);
}

fn set_alight(sim: &mut Sim, place: &str) {
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut sim.state, &mut events);
    ignite(&mut ctx, place);
}

fn run_until(sim: &mut Sim, ticks: i64, until: impl Fn(&SimState) -> bool) -> i64 {
    for i in 0..ticks {
        if until(&sim.state) {
            return i;
        }
        sim.step();
    }
    ticks
}

fn fire_lines(state: &SimState) -> Vec<&str> {
    state.chronicle.iter().filter(|e| e.kind == ChronicleKind::Fire).map(|e| e.text.as_str()).collect()
}

fn fighting_fire(state: &SimState, id: &str) -> bool {
    matches!(state.characters[id].intent.as_ref().map(|i| &i.action), Some(IntentAction::FightFire { .. }))
}

#[test]
fn the_dead_leave_the_houses_alone_while_anyone_is_awake() {
    let mut sim = asleep_at_night(3);
    // One of them lies awake in bed.
    let awake = sim.state.order[0].clone();
    sim.state.characters.get_mut(&awake).unwrap().asleep = false;
    raise_a_monster(&mut sim);
    for _ in 0..2400 {
        sim.step();
        assert!(sim.state.houses.is_empty());
        assert!(sim.state.monsters.values().all(|m| m.arson.is_none()));
    }
}

#[test]
fn with_everyone_asleep_a_monster_burns_a_house_and_the_village_is_woken() {
    let mut sim = asleep_at_night(3);
    raise_a_monster(&mut sim);
    let took = run_until(&mut sim, 3000, |state| !state.houses.is_empty());
    assert!(took < 3000, "no house was set on fire");
    let (place, house) = sim.state.houses.first().unwrap();
    let place = place.clone();
    assert_eq!(house.phase, HousePhase::Burning);
    assert!(sim.state.director.last_fire_tick > 0);
    // Those who lived there are out on the street, and burnt.
    let residents: Vec<_> = living(&sim.state).filter(|c| c.home_id == place).collect();
    assert!(!residents.is_empty());
    for c in &residents {
        assert!(c.inside.is_none() && !c.asleep, "{}", c.profile.name);
        assert!(c.hp < max_hp(c), "{}", c.profile.name);
    }
    assert!(fire_lines(&sim.state).iter().any(|line| line.contains("set fire to")));

    // With the arsonist gone, every last villager is up and carrying water.
    sim.state.monsters.clear();
    for _ in 0..40 {
        sim.step();
    }
    let mut carrying = 0;
    for id in living_ids(&sim.state) {
        let c = &sim.state.characters[&id];
        // Whoever ran from the monster is still hiding, and the children are not called; everyone else is on the
        // bucket chain.
        if matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::Flee)) || c.is_child() {
            continue;
        }
        assert!(fighting_fire(&sim.state, &id), "{} is {}", c.profile.name, describe_activity(&sim.state, c));
        assert!(!c.asleep && c.inside.is_none());
        assert!(describe_activity(&sim.state, c).contains("water"));
        carrying += 1;
    }
    assert!(carrying >= 9, "only {carrying} carrying water");
}

#[test]
fn nothing_but_a_fight_or_flight_takes_anyone_off_the_bucket_chain() {
    let mut sim = asleep_at_night(3);
    set_alight(&mut sim, "home_1");
    for _ in 0..40 {
        sim.step();
    }
    let id = sim.state.order[0].clone();
    assert!(fighting_fire(&sim.state, &id));
    let epoch = sim.state.characters[&id].epoch;
    let stroll = Intent::new(
        IntentAction::Activity { activity: ActivityId::Stroll, place_id: None, minutes: None },
        IntentSource::Llm,
    );
    sim.enqueue(Command::SetIntents { id: id.clone(), epoch, intents: vec![stroll] });
    sim.step();
    assert!(fighting_fire(&sim.state, &id));
}

#[test]
fn a_whole_village_puts_the_fire_out() {
    for home in ["home_1", "home_5", "home_8"] {
        let mut sim = asleep_at_night(3);
        set_alight(&mut sim, home);
        let mut peak: f64 = 0.0;
        for _ in 0..8000 {
            let Some(house) = sim.state.houses.get(home) else { break };
            assert_eq!(house.phase, HousePhase::Burning, "{home} burned down");
            peak = peak.max(house.fire);
            sim.step();
        }
        assert!(sim.state.houses.is_empty(), "{home} still burning, peak {peak}");
        assert!(fire_lines(&sim.state).iter().any(|line| line.contains("is out")));
        // Nobody is left holding a bucket, and those who helped remember it.
        for _ in 0..5 {
            sim.step();
        }
        assert!(living(&sim.state).all(|c| c.tool.as_deref() != Some("tools/bucket_metal")));
        assert!(living(&sim.state).any(|c| c.memories.iter().any(|m| m.text.contains("saved"))));
    }
}

/// A village of two, one of them living in the house that is alight.
fn two_villagers_and_a_fire() -> (Sim, String, String) {
    let mut sim = asleep_at_night(3);
    let resident = sim.state.order[0].clone();
    let home = sim.state.characters[&resident].home_id.clone();
    let keep: Vec<_> = living(&sim.state).filter(|c| c.home_id != home).map(|c| c.id.clone()).take(1).collect();
    let other = keep[0].clone();
    for id in living_ids(&sim.state) {
        if id != resident && id != other {
            sim.state.characters.shift_remove(&id);
            sim.state.order.retain(|o| o != &id);
        }
    }
    // Nobody new arrives to change the count, and the one who lost the home has working hours to give up.
    sim.state.director.population_target = 2;
    sim.state.characters.get_mut(&resident).unwrap().jobs = vec![ActivityId::Train];
    set_alight(&mut sim, &home);
    (sim, resident, home)
}

#[test]
fn two_villagers_cannot_save_a_house_and_its_owner_sleeps_rough() {
    let (mut sim, resident, home) = two_villagers_and_a_fire();
    let took = run_until(&mut sim, 6000, |state| state.houses[&home.clone()].phase == HousePhase::Ruined);
    assert!(took < 6000, "the house did not burn down");
    assert!(fire_lines(&sim.state).iter().any(|line| line.contains("burned to the ground")));
    assert!(is_homeless(&sim.state, &sim.state.characters[&resident]));
    assert!(sim.state.characters[&resident].memories.iter().any(|m| m.text.contains("burned to the ground")));
    assert_ne!(free_home(&sim.state), home);

    // Running from danger, the homeless hide in the tavern.
    {
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut sim.state, &mut events);
        start_flee(&mut ctx, &resident, IntentSource::Reflex);
    }
    let took = run_until(&mut sim, 3000, |state| state.characters[&resident].inside.is_some());
    assert!(took < 3000);
    assert_eq!(sim.state.characters[&resident].inside.as_deref(), Some("tavern"));

    // The next night is spent on the ground by the campfire.
    let rough = |state: &SimState| {
        let c = &state.characters[&resident];
        c.asleep && c.inside.is_none() && c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Sleep))
    };
    sim.state.director.next_wave_tick = i64::MAX;
    let took = run_until(&mut sim, TICKS_PER_DAY * 2, rough);
    assert!(took < TICKS_PER_DAY * 2, "the resident never slept rough");
    let c = &sim.state.characters[&resident];
    assert!(dist(c.pos, get_place("glade").door) < 12.0);
    assert_eq!(describe_activity(&sim.state, c), "sleeping rough by the campfire");
}

#[test]
fn the_village_rebuilds_a_ruin_and_its_owner_sleeps_indoors_again() {
    let mut sim = quiet(create_state(4));
    sim.state.director.next_wave_tick = i64::MAX;
    let resident = sim.state.order[0].clone();
    let home = sim.state.characters[&resident].home_id.clone();
    sim.state.houses.insert(
        home.clone(),
        House { phase: HousePhase::Ruined, fire: 0.0, built: 0.0, since_tick: 0, helpers: Vec::new() },
    );
    let mut most_builders = 0;
    let mut days = 0;
    for i in 0..TICKS_PER_DAY * 12 {
        sim.step();
        if i % 200 == 0 {
            let builders =
                living(&sim.state).filter(|c| describe_activity(&sim.state, c).starts_with("rebuilding")).count();
            most_builders = most_builders.max(builders);
        }
        if sim.state.houses.is_empty() {
            days = i / TICKS_PER_DAY + 1;
            break;
        }
    }
    assert!(sim.state.houses.is_empty(), "the house was never finished");
    assert!((2..=8).contains(&days), "rebuilt on day {days}");
    assert!(most_builders >= 3);
    assert!(fire_lines(&sim.state).iter().any(|line| line.contains("stands again")));
    assert!(living(&sim.state).count() >= 8, "the village went hungry while it built");

    let indoors = |state: &SimState| {
        state.characters.get(&resident).is_none_or(|c| c.asleep && c.inside.as_deref() == Some(home.as_str()))
    };
    let took = run_until(&mut sim, TICKS_PER_DAY * 2, indoors);
    assert!(took < TICKS_PER_DAY * 2);
}

#[test]
fn a_fire_and_a_ruin_survive_a_save_and_load() {
    let round_trip =
        |state: &SimState| -> SimState { serde_json::from_str(&serde_json::to_string(state).unwrap()).unwrap() };
    // Mid-fire, with a monster on its way to the next house.
    let mut sim = asleep_at_night(7);
    raise_a_monster(&mut sim);
    run_until(&mut sim, 3000, |state| !state.houses.is_empty());
    for _ in 0..300 {
        sim.step();
    }
    assert_eq!(round_trip(&sim.state), sim.state);
    let mut resumed = Sim::new(round_trip(&sim.state));
    for _ in 0..2500 {
        sim.step();
        resumed.step();
    }
    assert!(resumed.state == sim.state, "a reloaded fire went a different way");

    // And through the ruin and the rebuilding.
    let (mut sim, _, home) = two_villagers_and_a_fire();
    run_until(&mut sim, 6000, |state| state.houses[&home.clone()].phase == HousePhase::Ruined);
    let mut resumed = Sim::new(round_trip(&sim.state));
    for _ in 0..TICKS_PER_DAY {
        sim.step();
        resumed.step();
    }
    assert!(resumed.state == sim.state, "a reloaded ruin went a different way");
    assert!(sim.state.houses[&home].built > 0.0);
}
