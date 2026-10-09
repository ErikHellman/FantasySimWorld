//! Block and dodge: who may turn a blow aside or step out of it, and how often.

use fsw_core::math2d::dist;
use fsw_core::{Vec2, vec2};
use fsw_sim::data::classes::ClassId;
use fsw_sim::events::HitOutcome;
use fsw_sim::state::{MonsterKind, MonsterOrigin, MonsterPhase, Quirk, SimState};
use fsw_sim::systems::combat::{Blow, Guard, defend_chances, monster_defends, villager_defends};
use fsw_sim::systems::monsters::spawn_monster;
use fsw_sim::util::Ctx;
use fsw_testing::create_test_state;
use fsw_world::places::get_place;

const TRIALS: usize = 5000;

/// Open ground on the green.
fn on_the_green(state: &SimState) -> Vec2 {
    let green = get_place("green").door;
    state.nav.nearest_walkable(vec2(green.x, green.z + 4.0), 6).expect("open ground on the green")
}

/// A villager of this class with no nerve to speak of, standing on the green in a fight with someone.
fn fighter(state: &mut SimState, class: ClassId) -> (String, Vec2) {
    let pos = on_the_green(state);
    let id = state.order[0].clone();
    let foe = state.order[1].clone();
    let c = state.characters.get_mut(&id).unwrap();
    c.class_id = class;
    c.profile.bravery = 0.0;
    c.quirks.clear();
    c.hp = class.def().max_hp;
    c.asleep = false;
    c.inside = None;
    c.pos = pos;
    c.combat.target_id = Some(foe);
    c.combat.hit_tick = 0;
    (id, pos)
}

/// A monster of this kind on the green, chasing someone.
fn chaser(state: &mut SimState, kind: MonsterKind) -> (String, Vec2) {
    let pos = on_the_green(state);
    let mut events = Vec::new();
    let mut ctx = Ctx::new(state, &mut events);
    let id = spawn_monster(&mut ctx, kind, pos, 0.0, MonsterOrigin::Graveyard);
    let m = state.monsters.get_mut(&id).unwrap();
    m.phase = MonsterPhase::Chase;
    (id, pos)
}

fn attacker_beside(pos: Vec2) -> Vec2 {
    vec2(pos.x + 1.5, pos.z)
}

/// Defends `TRIALS` times from a fresh start each time, and counts the outcomes.
fn rates(state: &mut SimState, id: &str, pos: Vec2, monster: bool, blow: Blow) -> (f64, f64) {
    let (mut blocked, mut dodged) = (0, 0);
    for _ in 0..TRIALS {
        if monster {
            let m = state.monsters.get_mut(id).unwrap();
            m.defend_ready_tick = 0;
            m.pos = pos;
        } else {
            let c = state.characters.get_mut(id).unwrap();
            c.combat.defend_ready_tick = 0;
            c.pos = pos;
        }
        let mut events = Vec::new();
        let mut ctx = Ctx::new(state, &mut events);
        let outcome = if monster {
            monster_defends(&mut ctx, id, attacker_beside(pos), blow)
        } else {
            villager_defends(&mut ctx, id, attacker_beside(pos), blow)
        };
        match outcome {
            HitOutcome::Blocked => blocked += 1,
            HitOutcome::Dodged => dodged += 1,
            HitOutcome::Hit => {}
        }
    }
    (blocked as f64 / TRIALS as f64, dodged as f64 / TRIALS as f64)
}

fn near(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= 0.025
}

#[test]
fn block_and_dodge_rates_match_their_chances() {
    let mut state = create_test_state(1);
    let (knight, pos) = fighter(&mut state, ClassId::Knight);
    let (blocked, dodged) = rates(&mut state, &knight, pos, false, Blow::Melee);
    assert!(near(blocked, 0.35) && dodged == 0.0, "knight blocked {blocked}, dodged {dodged}");

    let mut state = create_test_state(2);
    let (rogue, pos) = fighter(&mut state, ClassId::Rogue);
    let (blocked, dodged) = rates(&mut state, &rogue, pos, false, Blow::Melee);
    assert!(blocked == 0.0 && near(dodged, 0.25), "rogue blocked {blocked}, dodged {dodged}");

    let mut state = create_test_state(3);
    let (warrior, pos) = chaser(&mut state, MonsterKind::SkeletonWarrior);
    let (blocked, dodged) = rates(&mut state, &warrior, pos, true, Blow::Melee);
    assert!(near(blocked, 0.3) && dodged == 0.0, "warrior blocked {blocked}, dodged {dodged}");

    let mut state = create_test_state(4);
    let (stalker, pos) = chaser(&mut state, MonsterKind::SkeletonRogue);
    let (blocked, dodged) = rates(&mut state, &stalker, pos, true, Blow::Melee);
    assert!(blocked == 0.0 && near(dodged, 0.2), "stalker blocked {blocked}, dodged {dodged}");
}

#[test]
fn the_chance_follows_nerve_a_born_fighter_and_wounds_and_never_passes_a_half() {
    let guard = |block: f64, bravery: f64, fighter: bool, hp_share: f64| Guard {
        block,
        dodge: 0.0,
        bravery: Some(bravery),
        fighter,
        hp_share,
    };
    let block_of = |g: Guard| defend_chances(&g, Blow::Melee).0;
    assert!((block_of(guard(0.35, 0.0, false, 1.0)) - 0.35).abs() < 1e-9);
    assert!((block_of(guard(0.35, 0.5, false, 1.0)) - 0.40).abs() < 1e-9);
    assert!((block_of(guard(0.35, 0.0, true, 1.0)) - 0.43).abs() < 1e-9);
    assert!((block_of(guard(0.35, 0.0, false, 0.2)) - 0.25).abs() < 1e-9);
    assert_eq!(block_of(guard(0.35, 1.0, true, 1.0)), 0.5, "capped at a half");
    // Nerve sharpens a defence the weapon has; it gives none to a bare fist or a staff.
    assert_eq!(block_of(guard(0.0, 1.0, true, 1.0)), 0.0);

    let rogue = Guard { block: 0.0, dodge: 0.25, bravery: Some(0.0), fighter: false, hp_share: 1.0 };
    assert_eq!(defend_chances(&rogue, Blow::Ranged), (0.0, 0.125));
    assert_eq!(defend_chances(&rogue, Blow::Slam), (0.0, 0.375));
    let knight = Guard { block: 0.35, dodge: 0.0, bravery: Some(0.0), fighter: false, hp_share: 1.0 };
    assert_eq!(defend_chances(&knight, Blow::Ranged), (0.0, 0.0), "arrows and spells are never blocked");
    assert_eq!(defend_chances(&knight, Blow::Slam), (0.0, 0.0), "nor is a slam");
}

#[test]
fn nobody_defends_mid_swing_asleep_or_out_of_a_fight() {
    let cases: [fn(&mut SimState, &str); 4] = [
        |s, id| s.characters.get_mut(id).unwrap().combat.hit_tick = s.tick + 5,
        |s, id| s.characters.get_mut(id).unwrap().asleep = true,
        |s, id| s.characters.get_mut(id).unwrap().combat.target_id = None,
        |s, id| s.characters.get_mut(id).unwrap().combat.defend_ready_tick = s.tick + 5,
    ];
    for (n, case) in cases.iter().enumerate() {
        let mut state = create_test_state(5);
        let (knight, pos) = fighter(&mut state, ClassId::Knight);
        case(&mut state, &knight);
        let rng = state.rng;
        for _ in 0..50 {
            let mut events = Vec::new();
            let mut ctx = Ctx::new(&mut state, &mut events);
            assert_eq!(
                villager_defends(&mut ctx, &knight, attacker_beside(pos), Blow::Melee),
                HitOutcome::Hit,
                "case {n}"
            );
        }
        assert_eq!(state.rng, rng, "case {n}: nothing is drawn for a defence that cannot happen");
    }

    // A monster defends only while it chases.
    let mut state = create_test_state(6);
    let (warrior, pos) = chaser(&mut state, MonsterKind::SkeletonWarrior);
    state.monsters.get_mut(&warrior).unwrap().phase = MonsterPhase::Roam;
    let rng = state.rng;
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut state, &mut events);
    assert_eq!(monster_defends(&mut ctx, &warrior, attacker_beside(pos), Blow::Melee), HitOutcome::Hit);
    assert_eq!(state.rng, rng);
}

#[test]
fn ranged_blows_are_only_ever_dodged() {
    let mut state = create_test_state(7);
    let (knight, pos) = fighter(&mut state, ClassId::Knight);
    let rng = state.rng;
    let (blocked, dodged) = rates(&mut state, &knight, pos, false, Blow::Ranged);
    assert_eq!((blocked, dodged), (0.0, 0.0));
    assert_eq!(state.rng, rng, "a knight has no dodge, so an arrow draws nothing");

    let mut state = create_test_state(8);
    let (rogue, pos) = fighter(&mut state, ClassId::Rogue);
    let (blocked, dodged) = rates(&mut state, &rogue, pos, false, Blow::Ranged);
    assert!(blocked == 0.0 && near(dodged, 0.125), "rogue against arrows: blocked {blocked}, dodged {dodged}");
}

#[test]
fn after_a_defence_the_next_must_wait() {
    let mut state = create_test_state(9);
    let (knight, pos) = fighter(&mut state, ClassId::Knight);
    state.characters.get_mut(&knight).unwrap().quirks.push(Quirk::Fighter);
    loop {
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut state, &mut events);
        if villager_defends(&mut ctx, &knight, attacker_beside(pos), Blow::Melee) == HitOutcome::Blocked {
            break;
        }
    }
    let ready = state.characters[&knight].combat.defend_ready_tick;
    assert_eq!(ready, state.tick + 12, "0.6 seconds at 20 ticks a second");
    let rng = state.rng;
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut state, &mut events);
    assert_eq!(villager_defends(&mut ctx, &knight, attacker_beside(pos), Blow::Melee), HitOutcome::Hit);
    assert_eq!(state.rng, rng);
}

#[test]
fn a_dodge_steps_aside_onto_walkable_ground() {
    let mut state = create_test_state(10);
    let (rogue, pos) = fighter(&mut state, ClassId::Rogue);
    let mut dodges = 0;
    for _ in 0..400 {
        {
            let c = state.characters.get_mut(&rogue).unwrap();
            c.pos = pos;
            c.combat.defend_ready_tick = 0;
        }
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut state, &mut events);
        if villager_defends(&mut ctx, &rogue, attacker_beside(pos), Blow::Melee) == HitOutcome::Dodged {
            dodges += 1;
            let now = state.characters[&rogue].pos;
            assert!((dist(now, pos) - 1.2).abs() < 1e-9, "stepped {}", dist(now, pos));
            assert!(state.nav.is_walkable(now.x, now.z));
            assert!(dist(now, attacker_beside(pos)) > 1.5 - 1e-9, "never towards the blow");
        }
    }
    assert!(dodges > 50, "{dodges} dodges");
}
