//! The murder mystery: a killing nobody saw becomes a case, the village gathers clues and gossips about them,
//! accuses someone, tries them on the green and drives out whoever it finds guilty, right or wrong.

use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::Command;
use fsw_sim::commands::TuningPatch;
use fsw_sim::state::{ChronicleKind, Clue, Intent, IntentAction, IntentSource, Rite, SimState};
use fsw_sim::systems::combat::sneak_attack;
use fsw_sim::util::{Ctx, first_name, living};
use fsw_sim::{Sim, SimEvent};
use fsw_testing::{admit, create_test_state};
use fsw_world::places::get_place;

struct World {
    sim: Sim,
}

impl World {
    fn quiet(seed: u32) -> Self {
        let mut sim = Sim::new(create_test_state(seed));
        sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
        Self { sim }
    }

    fn state(&self) -> &SimState {
        &self.sim.state
    }

    fn run_until(&mut self, ticks: i64, mut until: impl FnMut(&SimState) -> bool) -> bool {
        for _ in 0..ticks {
            if until(&self.sim.state) {
                return true;
            }
            self.sim.step();
        }
        until(&self.sim.state)
    }

    fn who(&self, name: &str) -> String {
        living(&self.sim.state).find(|c| first_name(c) == name).expect(name).id.clone()
    }

    fn with<R>(&mut self, f: impl FnOnce(&mut Ctx) -> R) -> R {
        let mut events: Vec<SimEvent> = Vec::new();
        let mut ctx = Ctx::new(&mut self.sim.state, &mut events);
        f(&mut ctx)
    }

    fn chronicle(&self, kind: ChronicleKind) -> Vec<&str> {
        self.state().chronicle.iter().filter(|e| e.kind == kind).map(|e| e.text.as_str()).collect()
    }

    /// Everyone but these goes indoors for a while, so nobody else sees what happens.
    fn clear_the_streets(&mut self, keep: &[&str]) {
        for c in self.sim.state.characters.values_mut() {
            if !keep.contains(&c.id.as_str()) {
                c.inside = Some(c.home_id.clone());
                c.intent = Some(Intent::new(IntentAction::Wait { minutes: 30.0 }, IntentSource::Routine));
                c.queue.clear();
            }
        }
    }

    /// Lays a clue against `suspect` that every grown villager but the suspect already knows.
    fn everyone_suspects(&mut self, victim: &str, suspect: &str, weight: f64) {
        let known_by: Vec<String> =
            living(self.state()).filter(|c| c.id != suspect && !c.is_child()).map(|c| c.id.clone()).collect();
        let case = self.sim.state.cases.iter_mut().find(|case| case.victim == victim).expect("a case");
        case.clues.push(Clue {
            suspect: suspect.to_owned(),
            fact: "They were seen washing blood from their hands".into(),
            weight,
            known_by,
        });
    }
}

/// Hesper, the authored murderer, kills Kael by the docks with nobody about. Returns them both.
fn unseen_murder(seed: u32) -> (World, String, String) {
    let mut w = World::quiet(seed);
    w.run_until(TICKS_PER_DAY / 3, |_| false);
    let hesper = w.with(|ctx| admit(ctx, "hesper"));
    let kael = w.who("Kael");
    w.clear_the_streets(&[&hesper, &kael]);
    let spot = get_place("docks").door;
    for (id, dx) in [(&kael, 0.0), (&hesper, 1.0)] {
        let c = w.sim.state.characters.get_mut(id).unwrap();
        c.pos = fsw_core::vec2(spot.x + dx, spot.z);
        c.inside = None;
        c.intent = Some(Intent::new(IntentAction::Wait { minutes: 60.0 }, IntentSource::Routine));
    }
    w.sim.state.characters.get_mut(&kael).unwrap().hp = 60.0;
    w.with(|ctx| sneak_attack(ctx, &hesper, &kael));
    assert!(!w.state().characters.contains_key(&kael), "Kael survived");
    (w, hesper, kael)
}

fn verdict(state: &SimState) -> bool {
    state.chronicle.iter().any(|e| e.text.contains("guilty of killing") || e.text.contains("and let them go"))
}

#[test]
fn an_unseen_murder_opens_a_case_and_viewers_are_told_who_did_it() {
    let (mut w, hesper, kael) = unseen_murder(7);
    let case = w.state().cases.iter().find(|case| case.victim == kael).expect("a case");
    assert_eq!(case.killer, hesper);
    assert!(!case.closed);
    assert!(
        w.chronicle(ChronicleKind::Death)
            .iter()
            .any(|t| t.starts_with("Only you know it:") && t.contains("killed Kael"))
    );
    // Nobody in the village was told: no memory names Hesper as the killer.
    for c in living(w.state()).filter(|c| c.id != hesper) {
        assert!(c.memories.iter().all(|m| !m.text.contains("killed Kael")), "{} knows", c.profile.name);
    }
    // Someone comes upon the body before long.
    assert!(w.run_until(TICKS_PER_DAY / 4, |s| s.cases[0].found_by.is_some()), "nobody found the body");
    assert!(w.chronicle(ChronicleKind::Death).iter().any(|t| t.contains("found Kael's body")));
    // The case survives a save.
    let saved: SimState = serde_json::from_str(&serde_json::to_string(w.state()).unwrap()).unwrap();
    assert_eq!(saved.cases, w.state().cases);
}

#[test]
fn the_village_tries_the_murderer_and_drives_them_out() {
    let (mut w, hesper, kael) = unseen_murder(7);
    w.everyone_suspects(&kael, &hesper, 4.0);
    let tried = w.run_until(
        TICKS_PER_DAY * 3,
        |s| matches!(s.ceremony.as_ref().map(|c| &c.rite), Some(Rite::Trial { accused, .. }) if *accused == hesper),
    );
    assert!(tried, "Hesper was never put on trial");
    assert!(w.run_until(TICKS_PER_DAY * 2, verdict), "no verdict");
    assert!(w.chronicle(ChronicleKind::Conflict).iter().any(|t| t.contains("guilty of killing Kael")), "acquitted");
    assert!(w.state().cases[0].closed);
    // Found out, the murderer leaves by the west road, and the chronicle does not repeat what viewers knew.
    assert!(w.run_until(TICKS_PER_DAY / 4, |s| !s.characters.contains_key(&hesper)), "Hesper never left");
    assert!(w.state().exiles.contains_key(&hesper));
    assert!(w.chronicle(ChronicleKind::Death).iter().all(|t| !t.contains("was innocent")));
}

#[test]
fn the_village_can_drive_out_the_wrong_person() {
    let (mut w, hesper, kael) = unseen_murder(11);
    let rowan = w.who("Rowan");
    // Rowan is well liked, so it takes something damning to turn the village against them.
    w.everyone_suspects(&kael, &rowan, 8.0);
    let tried = w.run_until(
        TICKS_PER_DAY * 3,
        |s| matches!(s.ceremony.as_ref().map(|c| &c.rite), Some(Rite::Trial { accused, .. }) if *accused == rowan),
    );
    assert!(tried, "Rowan was never put on trial");
    assert!(w.run_until(TICKS_PER_DAY * 2, verdict), "no verdict");
    assert!(w.chronicle(ChronicleKind::Conflict).iter().any(|t| t.contains("guilty of killing Kael")), "acquitted");
    assert!(w.state().characters[&rowan].exiled_since.is_some());
    assert!(w.state().characters[&hesper].exiled_since.is_none());
    assert!(
        w.chronicle(ChronicleKind::Death).iter().any(|t| t.contains("was innocent") && t.contains("still among them")),
        "{:?}",
        w.chronicle(ChronicleKind::Death)
    );
}

#[test]
fn a_case_nobody_can_solve_goes_cold() {
    let (mut w, _, kael) = unseen_murder(7);
    let case = w.sim.state.cases.iter_mut().find(|case| case.victim == kael).unwrap();
    case.clues.clear();
    case.found_by = Some("nobody".into());
    case.tick -= 7 * TICKS_PER_DAY;
    assert!(w.run_until(TICKS_PER_DAY / 12, |s| s.cases[0].closed), "the case stayed open");
    let deaths = w.chronicle(ChronicleKind::Death);
    assert!(deaths.iter().any(|t| t.contains("The trail has gone cold")));
    assert!(deaths.iter().any(|t| t.starts_with("The valley never learnt") && t.contains("killed Kael")));
}

#[test]
fn a_village_without_a_murderer_never_opens_a_case() {
    let mut w = World::quiet(3);
    w.run_until(TICKS_PER_DAY * 3, |_| false);
    assert!(w.state().cases.is_empty());
    assert!(w.state().sightings.is_empty());
    assert!(w.state().exiles.is_empty());
}
