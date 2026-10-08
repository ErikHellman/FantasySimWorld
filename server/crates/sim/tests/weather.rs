//! Weather: the sky keeps its own course for a seed, villagers take it each in their own way, those who
//! hate the rain go in when it pours, hail sends everyone in (it hurts, but never kills), and rain and
//! wind work on fires.

use fsw_core::time::{TICKS_PER_DAY, TICKS_PER_GAME_MINUTE};
use fsw_sim::Command;
use fsw_sim::Sim;
use fsw_sim::commands::TuningPatch;
use fsw_sim::describe::describe_activity;
use fsw_sim::state::{Ceremony, CeremonyPhase, House, HousePhase, IntentAction, Rite, SimState, Weather, WeatherKind};
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::fire::update_fire;
use fsw_sim::systems::weather::{HAIL_FLOOR, roll, sheltering, taste, update_weather};
use fsw_sim::util::{Ctx, living, living_ids};
use fsw_testing::create_test_state;

/// A world with nobody in it, so only the sky moves.
fn empty(seed: u32) -> SimState {
    let mut state = create_test_state(seed);
    state.characters.clear();
    state.order.clear();
    state
}

/// Runs only the weather, a game minute at a time, and notes each kind as it sets in with the tick.
fn watch_the_sky(state: &mut SimState, days: i64) -> Vec<(i64, WeatherKind)> {
    let mut spells = Vec::new();
    let end = state.tick + days * TICKS_PER_DAY;
    state.tick -= state.tick % TICKS_PER_GAME_MINUTE;
    while state.tick < end {
        state.tick += TICKS_PER_GAME_MINUTE;
        let before = state.weather.kind;
        let mut events = Vec::new();
        update_weather(&mut Ctx::new(state, &mut events));
        if state.weather.kind != before {
            spells.push((state.tick, state.weather.kind));
        }
    }
    spells
}

/// The weather held as it is, the next spell far off.
fn set_weather(state: &mut SimState, kind: WeatherKind, intensity: f64, wind: f64) {
    state.weather = Weather {
        kind,
        intensity,
        target: intensity,
        wind,
        wind_target: wind,
        wind_dir: 0.0,
        since_tick: state.tick,
        until_tick: state.tick + 30 * TICKS_PER_DAY,
        next: None,
    };
}

fn quiet(state: SimState) -> Sim {
    let mut sim = Sim::new(state);
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    sim
}

/// Mid-morning on the second day, nobody about but the villagers.
fn morning(seed: u32) -> SimState {
    let mut state = create_test_state(seed);
    state.tick = tick_at(2, 10.0);
    state.director.next_wave_tick = i64::MAX;
    state
}

#[test]
fn the_sky_keeps_its_own_course_and_leaves_the_rng_alone() {
    let mut a = empty(7);
    let rng = a.rng;
    let spells = watch_the_sky(&mut a, 30);
    assert_eq!(a.rng, rng, "the weather draws nothing from the RNG");

    // The same seed has the same weather, whoever lives there and whatever they do.
    let mut sim = quiet(create_test_state(7));
    let mut lived = Vec::new();
    for _ in 0..2 * TICKS_PER_DAY {
        let before = sim.state.weather.kind;
        sim.step();
        if sim.state.weather.kind != before {
            lived.push((sim.state.tick, sim.state.weather.kind));
        }
    }
    let early: Vec<_> = spells.iter().copied().filter(|(tick, _)| *tick <= sim.state.tick).collect();
    assert!(!early.is_empty());
    assert_eq!(lived, early);

    // A new world starts fair, and all four kinds come round within a month, hail rarely and briefly.
    assert!(spells[0].0 >= 6 * 60 * TICKS_PER_GAME_MINUTE);
    for kind in WeatherKind::ALL {
        assert!(spells.iter().any(|(_, k)| *k == kind), "no {kind:?} in thirty days: {spells:?}");
    }
    let hails: Vec<i64> =
        spells.windows(2).filter(|pair| pair[0].1 == WeatherKind::Hail).map(|pair| pair[1].0 - pair[0].0).collect();
    assert!(hails.len() <= 15, "hail is rare: {} spells", hails.len());
    assert!(hails.iter().all(|ticks| *ticks <= 45 * TICKS_PER_GAME_MINUTE), "hail is brief: {hails:?}");
    let rain = spells.iter().filter(|(_, k)| *k == WeatherKind::Rain).count();
    assert!(rain >= 15, "it rains often enough to matter: {rain} spells");
}

#[test]
fn nobody_is_married_or_buried_in_hail() {
    let mut state = empty(3);
    state.ceremony = None;
    let mut found = None;
    for tick in (0..60 * TICKS_PER_DAY).step_by(TICKS_PER_GAME_MINUTE as usize) {
        if roll(&state, tick).kind == WeatherKind::Hail {
            found = Some(tick);
            break;
        }
    }
    let tick = found.expect("hail is rolled at some point");
    state.ceremony = Some(Ceremony {
        id: "w1".into(),
        rite: Rite::Wedding { a: "c1".into(), b: "c2".into() },
        place_id: "green".into(),
        start_tick: tick + 100,
        phase: CeremonyPhase::Announced,
        step: 0,
        until_tick: tick + 100,
        officiant: "c3".into(),
        speeches: Vec::new(),
        attendees: Vec::new(),
        slips: 0,
    });
    assert_eq!(roll(&state, tick).kind, WeatherKind::Rain);
}

#[test]
fn a_villagers_words_say_how_they_take_the_weather() {
    let state = create_test_state(1);
    let mut c = state.characters.values().next().unwrap().clone();
    c.profile.occupation = "farmer".into();
    assert_eq!(taste(&c).rain, 1);
    assert_eq!(taste(&c).sun, 0, "who loves the rain does not long for the sun");
    c.profile.occupation = "merchant".into();
    c.profile.traits = vec!["vain".into()];
    assert_eq!(taste(&c).rain, -1);
    assert_eq!(taste(&c).sun, 1);
    c.profile.occupation = "sailor".into();
    assert_eq!(taste(&c).wind, 1);

    // The rest are settled by their names: the same each time, and not all alike.
    let tastes: Vec<i8> = living(&state).map(|c| taste(c).rain).collect();
    assert_eq!(tastes, living(&state).map(|c| taste(c).rain).collect::<Vec<_>>());
    assert!(tastes.contains(&1) && tastes.contains(&-1), "{tastes:?}");
}

#[test]
fn hail_sends_everyone_in_and_hurts_but_never_kills() {
    let mut state = morning(4);
    set_weather(&mut state, WeatherKind::Hail, 1.0, 0.6);
    // Someone already badly hurt is not finished off by it.
    let frail = state.order[0].clone();
    state.characters.get_mut(&frail).unwrap().hp = HAIL_FLOOR + 1.0;
    let alive = state.order.len();
    let mut sim = quiet(state);
    let mut struck = false;
    for _ in 0..(20 * TICKS_PER_GAME_MINUTE) {
        sim.step();
        struck |= living(&sim.state).any(|c| c.hp < 100.0 && c.inside.is_none());
        assert!(living(&sim.state).all(|c| c.hp >= HAIL_FLOOR.min(c.hp)));
    }
    assert_eq!(sim.state.order.len(), alive, "hail killed someone");
    assert!(sim.state.characters[&frail].hp >= HAIL_FLOOR);
    assert!(struck, "nobody out in the hail was struck");
    let outside: Vec<String> = living(&sim.state)
        .filter(|c| c.inside.is_none() && !sheltering(c))
        .map(|c| format!("{}: {}", c.profile.name, describe_activity(&sim.state, c)))
        .collect();
    assert!(outside.is_empty(), "still out in the hail: {outside:?}");
    let indoors = living(&sim.state).filter(|c| c.inside.is_some()).count();
    assert!(indoors * 4 >= alive * 3, "only {indoors} of {alive} got indoors");

    // Once it is over they come out again.
    set_weather(&mut sim.state, WeatherKind::Clear, 0.0, 0.0);
    for _ in 0..(5 * TICKS_PER_GAME_MINUTE) {
        sim.step();
    }
    assert!(living(&sim.state).all(|c| !sheltering(c)), "still sheltering after the hail");
}

#[test]
fn those_who_hate_the_rain_go_in_and_those_who_love_it_stay_out() {
    let mut state = morning(2);
    set_weather(&mut state, WeatherKind::Rain, 0.9, 0.3);
    let mut sim = quiet(state);
    let haters: Vec<String> =
        living_ids(&sim.state).into_iter().filter(|id| taste(&sim.state.characters[id]).rain < 0).collect();
    let lovers: Vec<String> =
        living_ids(&sim.state).into_iter().filter(|id| taste(&sim.state.characters[id]).rain > 0).collect();
    assert!(!haters.is_empty() && !lovers.is_empty());
    let mut went_in = std::collections::HashSet::new();
    for _ in 0..(40 * TICKS_PER_GAME_MINUTE) {
        sim.step();
        for id in &haters {
            if sim.state.characters.get(id).is_some_and(sheltering) {
                went_in.insert(id.clone());
            }
        }
        for id in &lovers {
            assert!(!sim.state.characters.get(id).is_some_and(sheltering), "a rain lover went in out of the rain");
        }
    }
    assert!(went_in.len() * 2 >= haters.len(), "only {} of {} rain-haters went in", went_in.len(), haters.len());
    let glad = lovers
        .iter()
        .filter(|id| describe_activity(&sim.state, &sim.state.characters[*id]).contains("glad of the rain"));
    assert!(glad.count() > 0, "no rain lover is glad of it");

    // When it eases they come out again.
    set_weather(&mut sim.state, WeatherKind::Clear, 0.0, 0.0);
    for _ in 0..(10 * TICKS_PER_GAME_MINUTE) {
        sim.step();
    }
    assert!(living(&sim.state).all(|c| !matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::Shelter))));
}

/// How far an untended house fire gets in `minutes` under the given weather.
fn fire_after(kind: WeatherKind, intensity: f64, wind: f64, minutes: i64) -> Option<f64> {
    let mut state = empty(1);
    state.tick = tick_at(2, 12.0);
    set_weather(&mut state, kind, intensity, wind);
    let home = "home_1".to_owned();
    state.houses.insert(
        home.clone(),
        House { phase: HousePhase::Burning, fire: 20.0, built: 0.0, since_tick: state.tick, helpers: Vec::new() },
    );
    for _ in 0..minutes {
        state.tick += TICKS_PER_GAME_MINUTE;
        let mut events = Vec::new();
        update_fire(&mut Ctx::new(&mut state, &mut events));
    }
    state.houses.get(&home).filter(|house| house.phase == HousePhase::Burning).map(|house| house.fire)
}

#[test]
fn rain_keeps_a_fire_down_and_wind_fans_it() {
    let calm = fire_after(WeatherKind::Clear, 0.0, 0.0, 10).unwrap();
    let windy = fire_after(WeatherKind::Wind, 1.0, 1.0, 10).unwrap();
    let drizzle = fire_after(WeatherKind::Rain, 0.4, 0.2, 10).unwrap();
    assert!(windy > calm, "wind {windy} against calm {calm}");
    assert!(drizzle < calm, "drizzle {drizzle} against calm {calm}");
    // A downpour puts a young fire out by itself.
    assert_eq!(fire_after(WeatherKind::Rain, 1.0, 0.3, 120), None);
}

#[test]
fn the_weather_survives_a_save() {
    let mut state = morning(6);
    set_weather(&mut state, WeatherKind::Rain, 0.55, 0.25);
    let json = serde_json::to_string(&state).unwrap();
    let loaded: SimState = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded.weather, state.weather);
    let (mut a, mut b) = (Sim::new(state), Sim::new(loaded));
    for _ in 0..600 {
        a.step();
        b.step();
    }
    assert_eq!(serde_json::to_value(&a.state).unwrap(), serde_json::to_value(&b.state).unwrap());

    // A world that has never seen weather leaves it out of the save.
    let fresh = serde_json::to_value(create_test_state(6)).unwrap();
    assert!(fresh.get("weather").is_none());
}
