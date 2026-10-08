//! Weddings and funerals: a couple who have lasted are married on the green before the whole village and
//! move in together; the dead are buried the next morning with everyone at the graveside.

use fsw_core::time::{TICKS_PER_DAY, game_time};
use fsw_sim::Command;
use fsw_sim::commands::TuningPatch;
use fsw_sim::data::monsters::MonsterKind;
use fsw_sim::describe::describe_activity;
use fsw_sim::state::{
    ActivityId, CeremonyPhase, ChronicleKind, Intent, IntentAction, IntentSource, MonsterOrigin, RelationLabel, Rite,
    SimState,
};
use fsw_sim::systems::arrivals::free_home;
use fsw_sim::systems::ceremonies::{BETROTHAL_DAYS, FUNERAL_HOUR, WEDDING_HOUR, attending};
use fsw_sim::systems::combat::{damage_monster, max_hp};
use fsw_sim::systems::death::kill_character;
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::fire::ignite;
use fsw_sim::systems::monsters::spawn_monster;
use fsw_sim::systems::quirks::fall_to_necromancy;
use fsw_sim::systems::relationships::{daily_relationships, marry};
use fsw_sim::util::{Ctx, first_name, living, living_ids, rel};
use fsw_sim::{Sim, SimEvent};
use fsw_testing::{admit, create_test_state};
use fsw_world::places::get_place;

struct World {
    sim: Sim,
    events: Vec<SimEvent>,
}

impl World {
    /// A village with no small talk and no monsters, on the evening of day 4.
    fn quiet(seed: u32) -> Self {
        let mut state = create_test_state(seed);
        state.tick = tick_at(4, 20.0);
        state.director.next_wave_tick = i64::MAX;
        let mut sim = Sim::new(state);
        sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
        Self { sim, events: Vec::new() }
    }

    fn state(&self) -> &SimState {
        &self.sim.state
    }

    fn run(&mut self, ticks: i64) {
        self.run_until(ticks, |_| false);
    }

    /// Steps until `until` holds or `ticks` have passed; returns how many were taken.
    fn run_until(&mut self, ticks: i64, mut until: impl FnMut(&SimState) -> bool) -> i64 {
        for i in 0..ticks {
            if until(&self.sim.state) {
                return i;
            }
            let events = self.sim.step();
            self.events.extend(events);
        }
        ticks
    }

    /// Steps to the given hour of the given day.
    fn run_to(&mut self, day: i64, hour: f64) {
        let target = tick_at(day, hour);
        let now = self.sim.state.tick;
        assert!(target > now, "already past day {day} {hour}");
        self.run(target - now);
    }

    fn who(&self, name: &str) -> String {
        living(&self.sim.state).find(|c| first_name(c) == name).expect(name).id.clone()
    }

    fn with<R>(&mut self, f: impl FnOnce(&mut Ctx) -> R) -> R {
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut self.sim.state, &mut events);
        let result = f(&mut ctx);
        self.events.extend(events);
        result
    }

    /// Makes two villagers a fond couple who got together `days_ago`.
    fn couple(&mut self, a: &str, b: &str, days_ago: i64) {
        let since = self.sim.state.tick - days_ago * TICKS_PER_DAY;
        assert!(since > 0);
        for (me, other) in [(a, b), (b, a)] {
            let c = self.sim.state.characters.get_mut(me).unwrap();
            c.partner_id = Some(other.to_owned());
            c.partner_since_tick = since;
            let r = rel(c, other);
            r.affinity = 60.0;
            r.trust = 40.0;
            r.attraction = 70.0;
            r.grievance = 0.0;
        }
    }

    fn kill(&mut self, id: &str) {
        self.with(|ctx| kill_character(ctx, id, "slain by a skeleton", None));
    }

    fn admit(&mut self, key: &str) -> String {
        self.with(|ctx| admit(ctx, key))
    }

    fn chronicle(&self, kind: ChronicleKind) -> Vec<&str> {
        self.state().chronicle.iter().filter(|e| e.kind == kind).map(|e| e.text.as_str()).collect()
    }

    fn said(&self, speaker: &str) -> Vec<&str> {
        self.events
            .iter()
            .filter_map(|e| match e {
                SimEvent::Speech { speaker_id, text, .. } if speaker_id == speaker => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn scheduled(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, SimEvent::CeremonyScheduled { .. })).count()
    }
}

fn round_trip(state: &SimState) -> SimState {
    serde_json::from_str(&serde_json::to_string(state).unwrap()).unwrap()
}

/// Sylvi and Kael: not kin, in different homes (Sylvi shares hers with Isolde; Kael is alone).
fn betrothed(seed: u32) -> (World, String, String) {
    let mut w = World::quiet(seed);
    let (sylvi, kael) = (w.who("Sylvi"), w.who("Kael"));
    w.couple(&sylvi, &kael, BETROTHAL_DAYS);
    w.run_to(5, 0.1);
    assert!(matches!(w.state().ceremony.as_ref().map(|c| &c.rite), Some(Rite::Wedding { .. })));
    (w, sylvi, kael)
}

#[test]
fn a_world_that_never_had_a_ceremony_saves_without_the_new_keys() {
    let mut sim = Sim::new(create_test_state(1));
    for _ in 0..2000 {
        sim.step();
    }
    let json = serde_json::to_string(&sim.state).unwrap();
    for key in ["\"ceremony\"", "\"spouseId\"", "\"partnerSinceTick\"", "\"awaitingFuneral\"", "\"attend\""] {
        assert!(!json.contains(key), "{key} in an untouched world's save");
    }
}

#[test]
fn a_couple_is_betrothed_after_two_days_and_not_before() {
    let mut w = World::quiet(3);
    let (sylvi, kael) = (w.who("Sylvi"), w.who("Kael"));
    w.couple(&sylvi, &kael, BETROTHAL_DAYS - 1);
    w.run_to(5, 0.1);
    assert!(w.state().ceremony.is_none(), "betrothed after a day");

    let (w, sylvi, kael) = betrothed(3);
    let ceremony = w.state().ceremony.as_ref().unwrap();
    assert_eq!(ceremony.rite, Rite::Wedding { a: sylvi.clone(), b: kael.clone() });
    assert_eq!(ceremony.start_tick, tick_at(5, WEDDING_HOUR));
    assert_eq!(ceremony.phase, CeremonyPhase::Announced);
    assert_eq!(ceremony.officiant, w.who("Rowan"));
    assert_eq!(w.scheduled(), 1);
    assert!(w.chronicle(ChronicleKind::Romance).iter().any(|t| t.contains("are to be married")));
    assert!(w.state().characters[&sylvi].memories.iter().any(|m| m.text.contains("I am to marry Kael")));
}

#[test]
fn kin_and_the_quarrelling_are_not_betrothed() {
    let mut w = World::quiet(3);
    let (elowen, theron) = (w.who("Elowen"), w.who("Theron"));
    w.couple(&elowen, &theron, BETROTHAL_DAYS);
    w.run_to(5, 0.1);
    assert!(w.state().ceremony.is_none(), "kin were betrothed");

    let mut w = World::quiet(3);
    let (sylvi, kael) = (w.who("Sylvi"), w.who("Kael"));
    w.couple(&sylvi, &kael, BETROTHAL_DAYS);
    rel(w.sim.state.characters.get_mut(&kael).unwrap(), &sylvi).grievance = 30.0;
    w.run_to(5, 0.1);
    assert!(w.state().ceremony.is_none(), "a quarrelling couple were betrothed");
}

#[test]
fn the_wedding_is_held_and_everyone_attends() {
    let (mut w, sylvi, kael) = betrothed(3);
    w.run_to(5, 13.6);
    assert_eq!(w.state().ceremony.as_ref().unwrap().phase, CeremonyPhase::Gathering);
    let there = living(w.state()).filter(|c| attending(c)).count();
    assert!(there >= 11, "only {there} on their way to the wedding");
    for c in living(w.state()).filter(|c| attending(c)) {
        assert!(describe_activity(w.state(), c).contains("wedding"), "{}", describe_activity(w.state(), c));
    }

    w.run_to(5, 14.6);
    let (s, k) = (&w.state().characters[&sylvi], &w.state().characters[&kael]);
    assert_eq!(s.spouse_id.as_deref(), Some(kael.as_str()));
    assert_eq!(k.spouse_id.as_deref(), Some(sylvi.as_str()));
    assert_eq!(s.relationships[&kael].label, RelationLabel::Spouse);
    assert_eq!(k.relationships[&sylvi].label, RelationLabel::Spouse);
    assert!(w.chronicle(ChronicleKind::Romance).iter().any(|t| t.contains("were married on the village green")));
    let rowan = w.who("Rowan");
    assert!(w.said(&rowan).iter().any(|t| t.contains("gathered") || t.contains("come near") || t.contains("sees")));
    assert!(!w.said(&sylvi).is_empty() && !w.said(&kael).is_empty(), "no vows were spoken");
    assert!(living(w.state()).all(|c| c.memories.iter().any(|m| m.text.contains("married"))));
    assert_eq!(w.state().ceremony.as_ref().unwrap().phase, CeremonyPhase::Feast);
    let feasting = living(w.state())
        .filter(|c| c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Drink)) || c.inside.is_some())
        .count();
    assert!(feasting >= 6, "only {feasting} went to the feast");

    w.run_to(5, 15.6);
    assert!(w.state().ceremony.is_none());
}

#[test]
fn nothing_but_a_fight_or_flight_takes_anyone_from_a_ceremony() {
    let (mut w, _, _) = betrothed(3);
    w.run_to(5, 13.6);
    let id = living(w.state()).find(|c| attending(c)).unwrap().id.clone();
    let epoch = w.state().characters[&id].epoch;
    let stroll = Intent::new(
        IntentAction::Activity { activity: ActivityId::Stroll, place_id: None, minutes: None },
        IntentSource::Llm,
    );
    w.sim.enqueue(Command::SetIntents { id: id.clone(), epoch, intents: vec![stroll] });
    w.run(1);
    assert!(attending(&w.state().characters[&id]));
    // A plan block boundary (14:00 is one for most) does not end it either.
    w.run_to(5, 14.05);
    assert!(attending(&w.state().characters[&id]));
    let epoch = w.state().characters[&id].epoch;
    w.sim.enqueue(Command::SetIntents {
        id: id.clone(),
        epoch,
        intents: vec![Intent::new(IntentAction::Flee, IntentSource::Llm)],
    });
    w.run(1);
    assert!(!attending(&w.state().characters[&id]));
}

#[test]
fn spouses_move_in_together_and_a_bed_is_freed() {
    let (mut w, sylvi, kael) = betrothed(3);
    let (home_s, home_k) = (w.state().characters[&sylvi].home_id.clone(), w.state().characters[&kael].home_id.clone());
    assert_ne!(home_s, home_k);
    let free_before = |state: &SimState, home: &str| {
        get_place(home).beds as i64 - living(state).filter(|c| c.home_id == home).count() as i64
    };
    let was_free = free_before(w.state(), &home_s);
    w.run_to(5, 14.6);
    // Kael lived alone; Sylvi shared with Isolde, so she moves to him.
    assert_eq!(w.state().characters[&sylvi].home_id, home_k);
    assert_eq!(w.state().characters[&kael].home_id, home_k);
    assert_eq!(free_before(w.state(), &home_s), was_free + 1);
    assert!(w.state().characters[&sylvi].memories.iter().any(|m| m.text.starts_with("Moved into")));
    assert_eq!(free_home(w.state()), home_s);
}

#[test]
fn a_wedding_slips_a_day_for_a_fire_or_the_dead_and_is_called_off_after_three() {
    let (mut w, _, _) = betrothed(3);
    w.run_to(5, 13.2);
    w.with(|ctx| ignite(ctx, "home_1"));
    w.run_to(5, 13.5);
    let ceremony = w.state().ceremony.as_ref().unwrap();
    assert_eq!(ceremony.phase, CeremonyPhase::Announced);
    assert_eq!(ceremony.start_tick, tick_at(6, WEDDING_HOUR));
    assert_eq!(ceremony.slips, 1);
    assert!(w.chronicle(ChronicleKind::Romance).iter().any(|t| t.contains("put off until tomorrow")));

    // One of the dead that will not crumble in daylight stands by the well.
    // Just before the village is called at 13:20, so it has not yet slunk off to the ruins.
    let (mut w, _, _) = betrothed(3);
    w.run_to(5, 13.33);
    w.with(|ctx| {
        spawn_monster(ctx, MonsterKind::SkeletonWarrior, get_place("green").origin, 0.0, MonsterOrigin::Ruins)
    });
    w.sim.state.monsters.values_mut().for_each(|m| m.lasting = true);
    w.run(10);
    assert_eq!(w.state().ceremony.as_ref().unwrap().slips, 1);
    // One lying low at the ruins puts nothing off.
    let (mut w, _, _) = betrothed(3);
    w.run_to(5, 13.2);
    w.with(|ctx| spawn_monster(ctx, MonsterKind::Necromancer, get_place("ruins").door, 0.0, MonsterOrigin::Ruins));
    w.sim.state.monsters.values_mut().for_each(|m| m.lasting = true);
    w.run_to(5, 13.5);
    assert_eq!(w.state().ceremony.as_ref().unwrap().phase, CeremonyPhase::Gathering);

    let (mut w, _, _) = betrothed(3);
    for day in 5..8 {
        w.run_to(day, 13.2);
        if w.state().houses.is_empty() {
            w.with(|ctx| ignite(ctx, "home_1"));
        } else {
            // Keep it burning: a two-house village would have put it out.
            w.sim.state.houses.values_mut().for_each(|h| h.fire = 50.0);
        }
        w.run_to(day, 13.5);
    }
    assert!(w.state().ceremony.is_none(), "still pending after three slips");
    assert!(w.chronicle(ChronicleKind::Romance).iter().any(|t| t.contains("called off")));
}

#[test]
fn a_death_in_the_couple_cancels_the_wedding_and_a_death_in_the_village_displaces_it() {
    let (mut w, sylvi, _) = betrothed(3);
    w.run_to(5, 8.0);
    w.kill(&sylvi);
    assert!(w.chronicle(ChronicleKind::Romance).iter().any(|t| t.contains("will not be held")));
    let ceremony = w.state().ceremony.as_ref().unwrap();
    assert_eq!(ceremony.rite, Rite::Funeral { dead: vec![sylvi.clone()] });
    assert_eq!(ceremony.start_tick, tick_at(6, FUNERAL_HOUR));

    let (mut w, _, _) = betrothed(3);
    w.run_to(5, 8.0);
    let dorn = w.who("Dorn");
    w.kill(&dorn);
    assert!(w.chronicle(ChronicleKind::Romance).iter().any(|t| t.contains("in mourning")));
    assert!(matches!(w.state().ceremony.as_ref().map(|c| &c.rite), Some(Rite::Funeral { .. })));
    // The couple are betrothed again once the village has buried him.
    w.run_to(7, 0.1);
    assert!(matches!(w.state().ceremony.as_ref().map(|c| &c.rite), Some(Rite::Wedding { .. })));
}

#[test]
fn a_funeral_is_held_the_next_morning_for_everyone_who_died_since_the_last() {
    let mut w = World::quiet(3);
    let (sylvi, kael, dorn, isolde) = (w.who("Sylvi"), w.who("Kael"), w.who("Dorn"), w.who("Isolde"));
    // Sylvi and Kael are married; Isolde is Sylvi's housemate and friend.
    w.with(|ctx| marry(ctx, &sylvi, &kael));
    rel(w.sim.state.characters.get_mut(&isolde).unwrap(), &sylvi).affinity = 50.0;
    w.run_to(4, 22.0);
    w.kill(&sylvi);
    w.run_to(5, 1.5);
    w.kill(&dorn);
    let ceremony = w.state().ceremony.as_ref().unwrap();
    assert_eq!(ceremony.rite, Rite::Funeral { dead: vec![sylvi.clone(), dorn.clone()] });
    assert_eq!(ceremony.start_tick, tick_at(5, FUNERAL_HOUR));
    assert_eq!(w.scheduled(), 1);
    assert!(w.chronicle(ChronicleKind::Death).iter().any(|t| t.contains("will bury Sylvi")));

    w.run_to(5, 8.6);
    let there = living(w.state()).filter(|c| attending(c)).count();
    assert!(there >= 9, "only {there} on their way to the funeral");
    let k = &w.state().characters[&kael];
    assert!(
        describe_activity(w.state(), k).contains("funeral") || describe_activity(w.state(), k).contains("graveside")
    );
    let trust_before = w.state().characters[&kael].relationships[&isolde].trust;

    // The officiant may have been at the far end of the valley when the village was called, so the
    // rite can start as late as the grace allows; by ten the dead are in the ground either way.
    w.run_to(5, 10.0);
    let rowan = w.who("Rowan");
    assert!(w.said(&rowan).len() >= 3, "the officiant said {:?}", w.said(&rowan));
    assert!(w.said(&kael).iter().any(|t| t.contains("Sylvi")), "the widower said nothing: {:?}", w.said(&kael));
    let k = &w.state().characters[&kael];
    assert!(k.memories.iter().any(|m| m.text.contains("I will carry this")));
    assert!(w.state().characters[&isolde].memories.iter().any(|m| m.text.contains("spoke well")));
    assert!(k.relationships[&isolde].trust > trust_before);
    assert!(w.chronicle(ChronicleKind::Death).iter().any(|t| t.contains("buried Sylvi and Dorn")));
    assert!(w.state().director.awaiting_funeral.is_empty());
    assert_eq!(w.state().ceremony.as_ref().unwrap().phase, CeremonyPhase::Feast);
    w.run_to(5, 11.0);
    assert!(w.state().ceremony.is_none());
}

#[test]
fn funerals_are_held_by_day_only_and_an_early_death_is_buried_the_same_morning() {
    let mut w = World::quiet(3);
    let dorn = w.who("Dorn");
    w.run_to(5, 6.5);
    w.kill(&dorn);
    assert_eq!(w.state().ceremony.as_ref().unwrap().start_tick, tick_at(5, FUNERAL_HOUR));

    let mut w = World::quiet(3);
    let dorn = w.who("Dorn");
    w.run_to(5, 20.0);
    w.kill(&dorn);
    assert_eq!(w.state().ceremony.as_ref().unwrap().start_tick, tick_at(6, FUNERAL_HOUR));
    w.run_to(6, 8.0);
    assert_eq!(w.state().ceremony.as_ref().unwrap().phase, CeremonyPhase::Announced);
    assert!(w.chronicle(ChronicleKind::Death).iter().all(|t| !t.starts_with("The village buried")));
}

#[test]
fn a_necromancy_victim_is_buried_only_once_a_grave_exists() {
    let mut w = World::quiet(3);
    let vex = w.who("Vex");
    w.run_to(4, 21.0);
    w.with(|ctx| fall_to_necromancy(ctx, &vex));
    assert!(w.state().ceremony.is_none());
    assert!(w.state().director.awaiting_funeral.is_empty());
    let necromancer = w.state().monsters.keys().next().unwrap().clone();
    let aldric = w.who("Aldric");
    w.with(|ctx| damage_monster(ctx, &necromancer, 10_000, &aldric));
    assert!(w.state().graves.iter().any(|g| g.character_id == vex));
    let ceremony = w.state().ceremony.as_ref().unwrap();
    assert_eq!(ceremony.rite, Rite::Funeral { dead: vec![vex.clone()] });
}

#[test]
fn the_officiant_is_a_cleric_or_paladin_before_a_druid_and_the_eldest_otherwise() {
    let mut w = World::quiet(3);
    let maren = w.admit("maren");
    let dorn = w.who("Dorn");
    w.run_to(4, 21.0);
    w.kill(&dorn);
    assert_eq!(w.state().ceremony.as_ref().unwrap().officiant, maren);

    let mut w = World::quiet(3);
    for name in ["Rowan", "Isolde"] {
        let id = w.who(name);
        w.sim.state.characters.shift_remove(&id);
        w.sim.state.order.retain(|o| o != &id);
    }
    let torvald = w.who("Torvald");
    w.run_to(4, 21.0);
    w.kill(&torvald);
    // Dorn, at 45, is the eldest left.
    assert_eq!(w.state().ceremony.as_ref().unwrap().officiant, w.who("Dorn"));
}

#[test]
fn a_held_speech_is_spoken_at_the_rite_and_a_stale_or_wrong_one_is_dropped() {
    let (mut w, sylvi, kael) = betrothed(3);
    let ceremony_id = w.state().ceremony.as_ref().unwrap().id.clone();
    // Stage directions are the brain's to strip; the sim tidies the whitespace and keeps the words.
    let vow = "Kael,  I have loved you\nsince the first night watch we shared.";
    w.sim.enqueue(Command::CeremonySpeech {
        ceremony_id: ceremony_id.clone(),
        speaker_id: sylvi.clone(),
        text: vow.into(),
    });
    let rowan = w.who("Rowan");
    w.sim.enqueue(Command::CeremonySpeech {
        ceremony_id: ceremony_id.clone(),
        speaker_id: rowan.clone(),
        text: "Not mine to say.".into(),
    });
    w.sim.enqueue(Command::CeremonySpeech {
        ceremony_id: "y999".into(),
        speaker_id: kael.clone(),
        text: "Stale.".into(),
    });
    w.run(1);
    let speeches = &w.state().ceremony.as_ref().unwrap().speeches;
    assert_eq!(speeches.len(), 1);
    assert_eq!(speeches[0].speaker_id, sylvi);
    assert_eq!(speeches[0].text, "Kael, I have loved you since the first night watch we shared.");

    w.run_to(5, 14.6);
    assert!(w.said(&sylvi).iter().any(|t| t.contains("first night watch")), "{:?}", w.said(&sylvi));
    assert!(w.said(&kael).iter().any(|t| t.contains("Sylvi")), "no scripted vow: {:?}", w.said(&kael));
}

#[test]
fn the_world_is_the_same_through_a_save_in_the_middle_of_a_wedding() {
    let (mut w, _, _) = betrothed(7);
    w.run_to(5, 13.6);
    assert_eq!(round_trip(w.state()), *w.state());
    let mut resumed = Sim::new(round_trip(w.state()));
    for _ in 0..3000 {
        w.sim.step();
        resumed.step();
    }
    assert!(resumed.state == *w.state(), "a reloaded wedding went a different way");
    assert!(living(w.state()).filter(|c| c.married()).count() == 2);
}

#[test]
fn married_couples_do_not_split_at_the_lover_threshold_and_a_widow_is_free() {
    let mut w = World::quiet(3);
    let (sylvi, kael) = (w.who("Sylvi"), w.who("Kael"));
    w.with(|ctx| marry(ctx, &sylvi, &kael));
    for (me, other) in [(&sylvi, &kael), (&kael, &sylvi)] {
        rel(w.sim.state.characters.get_mut(me).unwrap(), other).affinity = 5.0;
    }
    w.with(daily_relationships);
    assert!(w.state().characters[&sylvi].married(), "a marriage ended at a lover's threshold");
    for (me, other) in [(&sylvi, &kael), (&kael, &sylvi)] {
        rel(w.sim.state.characters.get_mut(me).unwrap(), other).affinity = -15.0;
    }
    w.with(daily_relationships);
    assert!(!w.state().characters[&sylvi].married() && !w.state().characters[&kael].married());
    assert!(w.state().characters[&sylvi].partner_id.is_none());
    assert!(w.chronicle(ChronicleKind::Romance).iter().any(|t| t.contains("their marriage is over")));

    let mut w = World::quiet(3);
    let (sylvi, kael) = (w.who("Sylvi"), w.who("Kael"));
    w.with(|ctx| marry(ctx, &sylvi, &kael));
    w.kill(&sylvi);
    let k = &w.state().characters[&kael];
    assert!(k.spouse_id.is_none() && k.partner_id.is_none());
    assert_eq!(w.state().dead[&sylvi].spouse_id.as_deref(), Some(kael.as_str()));
    let _ = (game_time(w.state().tick), max_hp(k), living_ids(w.state()));
}
