//! The dragon: it comes by day, sets one to three homes alight, burns whoever it catches in the open and flies
//! off; only bows, crossbows and magic reach it, and they can bring it down.

use fsw_core::math2d::dist;
use fsw_core::time::{TICKS_PER_DAY, game_time};
use fsw_sim::Command;
use fsw_sim::Sim;
use fsw_sim::commands::TuningPatch;
use fsw_sim::data::anims::AnimId;
use fsw_sim::data::classes::WeaponStyle;
use fsw_sim::state::{
    ChronicleKind, HousePhase, Id, IntentAction, LootKind, MonsterKind, MonsterPhase, RaidPhase, SimState,
};
use fsw_sim::systems::combat::max_hp;
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::dragon::{FIRST_DAY, HOURS, come};
use fsw_sim::systems::fire::MAX_FIRES;
use fsw_sim::util::{Ctx, living, living_ids};
use fsw_testing::create_test_state;
use fsw_world::places::get_place;

/// Late morning on day nine, with no dead about, no chatter, and no dragon but the ones the test sends.
fn late_morning(seed: u32) -> Sim {
    let mut state = create_test_state(seed);
    state.tick = tick_at(FIRST_DAY + 1, 11.0);
    state.director.watch_day = game_time(state.tick).day;
    state.director.next_wave_tick = i64::MAX;
    state.director.next_dragon_tick = i64::MAX;
    // No orc raid either, long overdue by this morning: its alarm would bring people out of doors.
    state.director.next_raid_tick = i64::MAX;
    let mut sim = Sim::new(state);
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    sim
}

fn summon(sim: &mut Sim) -> Id {
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut sim.state, &mut events);
    come(&mut ctx).expect("a home to burn")
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

fn lines(state: &SimState, kind: ChronicleKind) -> Vec<&str> {
    state.chronicle.iter().filter(|e| e.kind == kind).map(|e| e.text.as_str()).collect()
}

fn style(state: &SimState, id: &str) -> WeaponStyle {
    state.characters[id].class_id.def().weapon.style
}

/// Everyone indoors at home but `out`, who stands at `at`.
fn everyone_home_but(sim: &mut Sim, out: &[(&Id, fsw_core::Vec2)]) {
    for id in living_ids(&sim.state) {
        let c = sim.state.characters.get_mut(&id).unwrap();
        match out.iter().find(|(o, _)| **o == id) {
            Some((_, at)) => {
                c.pos = *at;
                c.inside = None;
            }
            None => {
                c.pos = get_place(&c.home_id).door;
                c.inside = Some(c.home_id.clone());
            }
        }
    }
}

#[test]
fn a_dragon_comes_by_day_burns_one_to_three_homes_and_is_gone() {
    let mut sim = late_morning(3);
    let id = summon(&mut sim);
    let m = &sim.state.monsters[&id];
    assert_eq!(m.kind, MonsterKind::Dragon);
    assert!(m.alt > 10.0, "it comes in high");
    let targets = m.raid.as_ref().unwrap().targets.len();
    assert!((1..=3).contains(&targets));

    let took = run_until(&mut sim, TICKS_PER_DAY / 4, |state| {
        state.monsters.get(&id).is_none_or(|m| m.phase == MonsterPhase::Dying)
    });
    assert!(took < TICKS_PER_DAY / 4, "the dragon never left");
    let burned = sim.state.houses.len();
    assert!((1..=targets).contains(&burned), "{burned} homes caught for {targets} targets");
    assert!(burned <= MAX_FIRES);
    assert!(lines(&sim.state, ChronicleKind::Fire).iter().any(|l| l.starts_with("The dragon set fire to")));
    let world = lines(&sim.state, ChronicleKind::World);
    assert!(world.iter().any(|l| l.starts_with("A dragon came over the hills")));
    let combat = lines(&sim.state, ChronicleKind::Combat);
    assert!(
        world.iter().any(|l| l.contains("over the hills,"))
            || combat.iter().any(|l| l.contains("brought down the dragon")),
        "it neither left nor fell"
    );
    // It never touched the ground (nor anyone's path) while it lived.
    assert!(sim.state.monsters.values().all(|m| m.kind != MonsterKind::Dragon || m.phase == MonsterPhase::Dying));
}

#[test]
fn its_breath_burns_those_in_the_open_and_spares_those_indoors() {
    let mut sim = late_morning(5);
    let id = summon(&mut sim);
    let target = sim.state.monsters[&id].raid.as_ref().unwrap().targets[0].clone();
    let door = get_place(&target).door;
    // Someone dozing on the grass by the door, and someone at home well away from it.
    let ids = living_ids(&sim.state);
    let dozer = ids.iter().find(|id| sim.state.characters[*id].home_id != target).unwrap().clone();
    let indoors = ids
        .iter()
        .find(|id| **id != dozer && dist(get_place(&sim.state.characters[*id].home_id).door, door) > 20.0)
        .unwrap()
        .clone();
    everyone_home_but(&mut sim, &[(&dozer, door)]);
    let c = sim.state.characters.get_mut(&dozer).unwrap();
    c.asleep = true;
    c.hp = max_hp(c);
    let dozer_hp = c.hp;
    let indoors_hp = sim.state.characters[&indoors].hp;
    // Bring it straight over the door, low enough to breathe.
    let m = sim.state.monsters.get_mut(&id).unwrap();
    m.pos = door;
    m.alt = 7.0;

    run_until(&mut sim, 200, |state| state.houses.contains_key(&target));
    assert_eq!(sim.state.houses[&target].phase, HousePhase::Burning);
    match sim.state.characters.get(&dozer) {
        Some(c) => assert!(dozer_hp - c.hp >= 30.0, "burned for only {}", dozer_hp - c.hp),
        None => assert!(sim.state.dead.values().any(|d| d.cause.contains("dragon"))),
    }
    assert_eq!(sim.state.characters[&indoors].hp, indoors_hp);
    assert_eq!(sim.state.characters[&indoors].inside.as_deref(), Some(sim.state.characters[&indoors].home_id.as_str()));
}

/// The dragon hanging over the green, a fighter with a blade and one with a bow below it.
fn over_the_green(seed: u32) -> (Sim, Id, Id, Id) {
    let mut sim = late_morning(seed);
    let id = summon(&mut sim);
    let green = get_place("green").door;
    let ids = living_ids(&sim.state);
    let adult = |state: &SimState, id: &Id| !state.characters[id].is_child();
    let blade =
        ids.iter().find(|i| adult(&sim.state, i) && style(&sim.state, i) == WeaponStyle::Melee).unwrap().clone();
    let bow = ids.iter().find(|i| adult(&sim.state, i) && style(&sim.state, i) == WeaponStyle::Bow).unwrap().clone();
    everyone_home_but(&mut sim, &[(&blade, green), (&bow, fsw_core::vec2(green.x + 3.0, green.z))]);
    for who in [&blade, &bow] {
        let c = sim.state.characters.get_mut(who).unwrap();
        c.profile.bravery = 1.0;
        c.hp = max_hp(c);
        c.asleep = false;
    }
    let tick = sim.state.tick;
    let m = sim.state.monsters.get_mut(&id).unwrap();
    m.pos = fsw_core::vec2(green.x, green.z + 6.0);
    m.alt = 10.0;
    let raid = m.raid.as_mut().unwrap();
    raid.phase = RaidPhase::Hover;
    raid.hover = m.pos;
    raid.until_tick = tick + TICKS_PER_DAY;
    // Hold its fire, so the test watches the fighting and not the burns.
    raid.next_breath_tick = tick + TICKS_PER_DAY;
    (sim, id, blade, bow)
}

#[test]
fn a_blade_takes_cover_and_a_bow_shoots_it_down_from_the_sky() {
    let (mut sim, id, blade, bow) = over_the_green(2);
    let max = sim.state.monsters[&id].hp;
    run_until(&mut sim, 400, |state| state.monsters[&id].hp < max);
    assert!(sim.state.monsters[&id].hp < max, "nobody hit the dragon");
    let action =
        |state: &SimState, who: &Id| state.characters.get(who).and_then(|c| c.intent.clone()).map(|i| i.action);
    assert!(
        matches!(action(&sim.state, &blade), Some(IntentAction::Flee)) || sim.state.characters[&blade].inside.is_some(),
        "the swordsman stood under the dragon: {:?}",
        action(&sim.state, &blade)
    );
    assert!(matches!(action(&sim.state, &bow), Some(IntentAction::Fight { target_id, .. }) if target_id == id));

    // Wounded nearly to death, it labours off too slowly to get away, and the shots bring it down (from the bow, or
    // from someone else with a bow or a spell who came out at the alarm).
    sim.state.monsters.get_mut(&id).unwrap().hp = 20.0;
    let kills: Vec<(Id, i64)> = living(&sim.state).map(|c| (c.id.clone(), c.kills)).collect();
    run_until(&mut sim, 1200, |state| state.monsters[&id].phase == MonsterPhase::Dying);
    let m = &sim.state.monsters[&id];
    assert_eq!(m.phase, MonsterPhase::Dying);
    assert_eq!(m.anim, AnimId::DragonDeath);
    let (killer, _) = kills.iter().find(|(c, k)| sim.state.characters[c].kills == k + 1).expect("someone has the kill");
    assert_ne!(style(&sim.state, killer), WeaponStyle::Melee);
    let name = sim.state.characters[killer].profile.name.clone();
    assert!(
        lines(&sim.state, ChronicleKind::Combat)
            .iter()
            .any(|l| l.starts_with(&format!("{name} brought down the dragon")))
    );
    assert!(sim.state.loot.values().any(|l| l.kind == LootKind::Relic));
    assert!(sim.state.loot.values().any(|l| matches!(l.kind, LootKind::Coins { count } if count >= 25)));

    // It falls to the ground, lies there a while, and is gone.
    run_until(&mut sim, 200, |state| state.monsters[&id].alt == 0.0);
    assert_eq!(sim.state.monsters[&id].alt, 0.0);
    run_until(&mut sim, 1300, |state| !state.monsters.contains_key(&id));
    assert!(!sim.state.monsters.contains_key(&id));
}

#[test]
fn badly_hurt_it_gives_up_and_flies_off() {
    let (mut sim, id, _, _) = over_the_green(4);
    // With nobody brave enough to shoot at it.
    everyone_home_but(&mut sim, &[]);
    for c in sim.state.characters.values_mut() {
        c.profile.bravery = 0.0;
    }
    sim.state.monsters.get_mut(&id).unwrap().hp = 300.0;
    sim.step();
    assert_eq!(sim.state.monsters[&id].raid.as_ref().unwrap().phase, RaidPhase::Leave);
    run_until(&mut sim, TICKS_PER_DAY / 8, |state| !state.monsters.contains_key(&id));
    assert!(lines(&sim.state, ChronicleKind::World).iter().any(|l| l.contains("badly wounded")));
}

#[test]
fn a_visit_survives_a_save_and_load() {
    let round_trip =
        |state: &SimState| -> SimState { serde_json::from_str(&serde_json::to_string(state).unwrap()).unwrap() };
    let mut sim = late_morning(7);
    summon(&mut sim);
    run_until(&mut sim, 6000, |state| !state.houses.is_empty());
    for _ in 0..100 {
        sim.step();
    }
    assert!(sim.state.monsters.values().any(|m| m.raid.is_some()), "the visit is already over");
    assert_eq!(round_trip(&sim.state), sim.state);
    let mut resumed = Sim::new(round_trip(&sim.state));
    for _ in 0..3000 {
        sim.step();
        resumed.step();
    }
    assert!(resumed.state == sim.state, "a reloaded visit went a different way");
}

#[test]
fn it_is_due_by_day_from_day_eight_and_waiting_for_it_draws_nothing_from_the_rng() {
    let mut waiting = Sim::new(create_test_state(11));
    let mut never = Sim::new(create_test_state(11));
    never.state.director.next_dragon_tick = i64::MAX;
    waiting.step();
    never.step();
    let due = game_time(waiting.state.director.next_dragon_tick);
    assert!((FIRST_DAY..FIRST_DAY + 10).contains(&due.day), "due on day {}", due.day);
    assert!((HOURS[0]..HOURS[1]).contains(&due.hour), "due at {}", due.hour);
    for _ in 0..TICKS_PER_DAY * 2 {
        waiting.step();
        never.step();
        assert_eq!(waiting.state.rng, never.state.rng, "tick {}", waiting.state.tick);
    }
    assert!(living(&waiting.state).count() > 0);
}
