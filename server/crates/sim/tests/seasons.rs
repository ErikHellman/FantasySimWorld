//! The village's year and its elder: the seasons turn, the harvest and midwinter are kept on the green, and the
//! village chooses an elder by what its people feel about those who stand.

use fsw_sim::Command;
use fsw_sim::commands::TuningPatch;
use fsw_sim::state::{CeremonyPhase, FestivalKind, Rite, Season, SimState};
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::seasons::{
    candidates, day_of_season, election_due, festival_due, season_of, tally, votes_words, winner, year_of,
};
use fsw_sim::util::{living_ids, rel};
use fsw_sim::{Sim, SimEvent};
use fsw_testing::create_test_state;

/// A quiet village at the given hour of the given day.
fn village(day: i64, hour: f64) -> Sim {
    let mut state = create_test_state(3);
    state.tick = tick_at(day, hour);
    state.director.founding = false;
    state.director.next_wave_tick = i64::MAX;
    let mut sim = Sim::new(state);
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    sim
}

/// Steps until `until` holds or the clock reaches `target`.
fn run_until(sim: &mut Sim, target: i64, mut until: impl FnMut(&SimState) -> bool) -> Vec<SimEvent> {
    let mut events = Vec::new();
    while sim.state.tick < target {
        if until(&sim.state) {
            break;
        }
        events.extend(sim.step());
    }
    events
}

fn chronicled(state: &SimState, words: &str) -> bool {
    state.chronicle.iter().any(|e| e.text.contains(words))
}

#[test]
fn a_year_is_four_seasons_of_seven_days() {
    assert_eq!((season_of(1), day_of_season(1), year_of(1)), (Season::Spring, 1, 1));
    assert_eq!((season_of(7), day_of_season(7)), (Season::Spring, 7));
    assert_eq!((season_of(8), day_of_season(8)), (Season::Summer, 1));
    assert_eq!(season_of(15), Season::Autumn);
    assert_eq!((season_of(28), day_of_season(28), year_of(28)), (Season::Winter, 7, 1));
    assert_eq!((season_of(29), year_of(29)), (Season::Spring, 2));
}

#[test]
fn the_feasts_fall_late_in_autumn_and_in_the_depth_of_winter_once_a_year() {
    let mut state = create_test_state(3);
    assert_eq!(festival_due(&state, 19), None);
    assert_eq!(festival_due(&state, 20), Some(FestivalKind::Harvest));
    assert_eq!(festival_due(&state, 24), None);
    assert_eq!(festival_due(&state, 25), Some(FestivalKind::Midwinter));
    state.village.harvest_year = 1;
    assert_eq!(festival_due(&state, 21), None, "kept already this year");
    assert_eq!(festival_due(&state, 48), Some(FestivalKind::Harvest), "but due again the next");
}

/// The test village's grown-ups, in its own order.
fn grown(state: &SimState) -> Vec<String> {
    state.order.iter().filter(|id| !state.characters[*id].is_child()).cloned().collect()
}

#[test]
fn votes_follow_regard_and_a_tie_keeps_the_first_named() {
    let mut state = create_test_state(3);
    let ids = grown(&state);
    let (a, b) = (ids[0].clone(), ids[1].clone());
    // Everyone but the two thinks much better of `b`.
    for voter in &ids[2..] {
        let c = state.characters.get_mut(voter).unwrap();
        (rel(c, &a).affinity, rel(c, &a).trust) = (0.0, 0.0);
        (rel(c, &b).affinity, rel(c, &b).trust) = (90.0, 90.0);
    }
    // Children have no vote.
    let votes = tally(&state, &[a.clone(), b.clone()], &state.order.clone());
    assert_eq!(votes, vec![1, ids.len() - 1]);
    assert_eq!(winner(&votes), 1);
    assert_eq!(winner(&[3, 3]), 0);
    assert_eq!(votes_words(&[2, 5]), "5 votes to 2");
    assert_eq!(votes_words(&[1, 0]), "1 vote to 0");
}

#[test]
fn the_most_respected_stand_and_a_sitting_elder_always_does() {
    let mut state = create_test_state(3);
    let ids = grown(&state);
    for voter in state.order.clone() {
        let c = state.characters.get_mut(&voter).unwrap();
        if voter != ids[3] {
            rel(c, &ids[3]).affinity = 100.0;
        }
        if voter != ids[4] {
            rel(c, &ids[4]).trust = 150.0;
        }
    }
    assert_eq!(candidates(&state), vec![ids[3].clone(), ids[4].clone()]);
    state.village.elder = Some(ids[0].clone());
    assert_eq!(candidates(&state), vec![ids[0].clone(), ids[3].clone()]);
}

#[test]
fn an_election_is_due_with_no_elder_and_each_new_year() {
    let mut state = create_test_state(3);
    state.director.founding = false;
    assert!(!election_due(&state, 2), "not in the first days");
    assert!(election_due(&state, 3));
    state.village.elder = Some(state.order[0].clone());
    state.village.election_year = 1;
    assert!(!election_due(&state, 10));
    assert!(election_due(&state, 29), "the first day of spring");
    state.village.election_year = 2;
    assert!(!election_due(&state, 29), "but only once");
}

#[test]
fn a_village_without_an_elder_chooses_one_on_the_green() {
    let mut sim = village(2, 23.0);
    run_until(&mut sim, tick_at(3, 1.0), |_| false);
    let Some(Rite::Election { candidates }) = sim.state.ceremony.as_ref().map(|c| c.rite.clone()) else {
        panic!("an election is arranged: {:?}", sim.state.ceremony);
    };
    assert_eq!(candidates.len(), 2);
    let officiant = sim.state.ceremony.as_ref().unwrap().officiant.clone();
    assert!(!candidates.contains(&officiant), "neither candidate runs it");
    assert!(chronicled(&sim.state, "The village chooses its elder"));
    let events = run_until(&mut sim, tick_at(3, 16.0), |s| s.village.elder.is_some());
    let elder = sim.state.village.elder.clone().expect("an elder is chosen by the afternoon");
    assert!(candidates.contains(&elder));
    assert_eq!(sim.state.village.elder_day, 3);
    assert_eq!(sim.state.village.election_year, 1);
    assert!(chronicled(&sim.state, "as its elder"));
    assert!(events.iter().any(|e| matches!(e, SimEvent::Speech { speaker_id, .. } if candidates.contains(speaker_id))));
    let loser = candidates.iter().find(|id| **id != elder).unwrap();
    assert!(sim.state.characters[loser].relationships[&elder].grievance > 0.0, "losing stings");
}

#[test]
fn the_harvest_is_kept_on_the_green_and_leaves_the_village_heartened() {
    let mut sim = village(19, 22.0);
    sim.state.village.elder = Some(sim.state.order[0].clone());
    sim.state.village.election_year = 1;
    run_until(&mut sim, tick_at(20, 1.0), |_| false);
    let ceremony = sim.state.ceremony.clone().expect("the harvest is arranged");
    assert!(matches!(ceremony.rite, Rite::Festival { feast: FestivalKind::Harvest }));
    assert_eq!(ceremony.officiant, sim.state.order[0], "the elder speaks");
    run_until(&mut sim, tick_at(20, 23.0), |s| s.village.harvest_year == 1);
    assert_eq!(sim.state.village.harvest_year, 1);
    assert!(chronicled(&sim.state, "kept its harvest festival"));
    let tick = sim.state.tick;
    let heartened =
        living_ids(&sim.state).iter().filter(|id| sim.state.characters[*id].heartened_until_tick > tick).count();
    assert!(heartened >= 2, "those who came are in good spirits ({heartened})");
    assert!(matches!(sim.state.ceremony.as_ref().map(|c| c.phase), Some(CeremonyPhase::Feast) | None));
}

#[test]
fn midwinter_sets_old_quarrels_down() {
    let mut sim = village(24, 22.0);
    sim.state.village.elder = Some(sim.state.order[0].clone());
    sim.state.village.election_year = 1;
    sim.state.village.harvest_year = 1;
    let (a, b) = (sim.state.order[1].clone(), sim.state.order[2].clone());
    rel(sim.state.characters.get_mut(&a).unwrap(), &b).grievance = 15.0;
    run_until(&mut sim, tick_at(25, 23.0), |s| s.village.midwinter_year == 1);
    assert_eq!(sim.state.village.midwinter_year, 1);
    assert!(chronicled(&sim.state, "set old quarrels down"));
    let attended = sim.state.chronicle.iter().any(|e| e.text.contains("kept midwinter"));
    assert!(attended);
    assert!(sim.state.characters[&a].relationships[&b].grievance < 15.0);
}
