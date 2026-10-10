//! Nothing stays stuck: over days of village life, no villager, animal, monster or ghost with somewhere to go
//! stands walking in place for long.

use std::collections::BTreeMap;

use fsw_core::Vec2;
use fsw_core::math2d::dist;
use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::state::{ActPhase, SimState};
use fsw_sim::systems::stalls::{STALL_SECONDS, update_stalls};
use fsw_sim::util::Ctx;
use fsw_sim::{Command, Sim, create_state};
use fsw_testing::create_test_state;

/// How often the watch looks, in ticks (once a game minute).
const LOOK_EVERY: i64 = 20;
/// Getting this far counts as getting somewhere.
const PROGRESS: f64 = 0.5;

/// Everything that has a path to walk right now, by id, with where it stands and a few words on what it is doing.
fn walkers(state: &SimState) -> Vec<(String, Vec2, String)> {
    let mut out = Vec::new();
    for c in state.characters.values() {
        if c.inside.is_none() && c.act.phase == ActPhase::Travel && !c.act.path.is_empty() {
            let doing = c.intent.as_ref().map_or("nothing".to_owned(), |i| format!("{:?}", i.action));
            out.push((c.id.clone(), c.pos, format!("{} {doing}", c.profile.name)));
        }
    }
    for m in state.monsters.values() {
        if !m.path.is_empty() && !m.kind.def().flies {
            out.push((m.id.clone(), m.pos, format!("{:?} {:?}", m.kind, m.phase)));
        }
    }
    for g in state.ghosts.values() {
        if !g.path.is_empty() {
            out.push((g.id.clone(), g.pos, format!("ghost of {}", g.name)));
        }
    }
    for a in state.animals.values() {
        if !a.path.is_empty() && a.rider.is_none() {
            out.push((a.id.clone(), a.pos, format!("{:?}", a.species)));
        }
    }
    out
}

/// Runs `sim` for `days` and returns every stretch longer than `limit` ticks that something spent with a path to
/// walk and getting nowhere.
fn stalls(mut sim: Sim, days: i64, limit: i64) -> Vec<String> {
    let mut since: BTreeMap<String, (Vec2, i64)> = BTreeMap::new();
    let mut found = Vec::new();
    for tick in 0..TICKS_PER_DAY * days {
        sim.step();
        if tick % LOOK_EVERY != 0 {
            continue;
        }
        let now = sim.state.tick;
        let moving = walkers(&sim.state);
        since.retain(|id, _| moving.iter().any(|(m, _, _)| m == id));
        for (id, pos, what) in moving {
            let entry = since.entry(id.clone()).or_insert((pos, now));
            if dist(entry.0, pos) > PROGRESS {
                *entry = (pos, now);
            } else if now - entry.1 == limit + LOOK_EVERY - (limit % LOOK_EVERY) {
                let mut line = format!("{id} ({what}) stuck at ({:.1}, {:.1}) on tick {now}", pos.x, pos.z);
                if let Some(c) = sim.state.characters.get(&id) {
                    let near: Vec<String> = sim
                        .state
                        .characters
                        .values()
                        .filter(|o| o.id != id && o.inside.is_none() && dist(o.pos, c.pos) < 2.0)
                        .map(|o| {
                            format!(
                                "{} at {:.2} moving={} path={}",
                                o.id,
                                dist(o.pos, c.pos),
                                o.moving,
                                o.act.path.len()
                            )
                        })
                        .collect();
                    line += &format!(
                        "\n   moving={} walkable={} path={:?} dest={:?} passing={:?} sidestep={:?} convo={:?} near={near:?}",
                        c.moving,
                        sim.state.nav.is_walkable(c.pos.x, c.pos.z),
                        c.act.path,
                        c.act.dest,
                        c.passing,
                        c.sidestep,
                        c.convo_id,
                    );
                }
                found.push(line);
            }
        }
    }
    found
}

fn founded(seed: u32) -> Sim {
    let mut sim = Sim::new(create_state(seed));
    sim.enqueue(Command::FoundVillage);
    sim.step();
    sim
}

#[test]
fn nothing_walks_in_place_for_long_in_the_test_village() {
    let found = stalls(Sim::new(create_test_state(11)), 6, 600);
    assert!(found.is_empty(), "{} stalls:\n{}", found.len(), found.join("\n"));
}

#[test]
fn nothing_walks_in_place_for_long_in_a_founded_village() {
    let found = stalls(founded(5), 6, 600);
    assert!(found.is_empty(), "{} stalls:\n{}", found.len(), found.join("\n"));
}

/// A villager on the green with a path to `goal` and nothing else to do, the clock at a game minute.
fn walker_on_the_green(goal_offset: Vec2) -> (SimState, String) {
    let mut state = create_test_state(2);
    state.tick = state.tick - state.tick % 20 + 20;
    let green = fsw_world::places::get_place("green").origin;
    let id = state.order[0].clone();
    let c = state.characters.get_mut(&id).unwrap();
    c.inside = None;
    c.pos = green;
    c.act.phase = ActPhase::Travel;
    c.act.path = vec![fsw_core::vec2(green.x + goal_offset.x, green.z + goal_offset.z)];
    (state, id)
}

/// Runs the guard alone for `minutes` game minutes, with nobody moving.
fn hold(state: &mut SimState, minutes: i64) {
    for _ in 0..minutes {
        let mut events = Vec::new();
        update_stalls(&mut Ctx::new(state, &mut events));
        state.tick += 20;
    }
}

#[test]
fn held_up_a_step_from_where_they_were_going_a_walker_is_there() {
    let (mut state, id) = walker_on_the_green(fsw_core::vec2(1.0, 0.0));
    hold(&mut state, STALL_SECONDS as i64 - 1);
    assert_eq!(state.characters[&id].act.path.len(), 1, "helped too soon");
    hold(&mut state, 2);
    assert!(state.characters[&id].act.path.is_empty());
}

#[test]
fn held_up_far_from_it_a_walker_steps_round_and_then_gives_up() {
    let (mut state, id) = walker_on_the_green(fsw_core::vec2(0.0, 12.0));
    let start = state.characters[&id].pos;
    let before = state.characters[&id].act.path.clone();
    hold(&mut state, STALL_SECONDS as i64 + 1);
    let after = state.characters[&id].act.path.clone();
    assert_ne!(after, before, "not helped");
    assert_eq!(after.last(), before.last(), "lost the way");
    let aside = after[0];
    assert!(dist(aside, start) > 1.0 && state.nav.is_walkable(aside.x, aside.z), "{aside:?}");
    hold(&mut state, 2 * STALL_SECONDS as i64 + 2);
    assert!(
        state.characters[&id].act.path.is_empty(),
        "never gave up: {:?} {:?}",
        state.characters[&id].act.path,
        state.stalls
    );
    assert!(state.stalls.is_empty());
}
