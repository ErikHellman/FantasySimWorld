//! The authored characters' quirks: what the sim itself makes them do, with or without a model.

use fsw_core::time::{TICKS_PER_DAY, game_time};
use fsw_sim::Command;
use fsw_sim::commands::TuningPatch;
use fsw_sim::data::activities::{ActivityKind, activity};
use fsw_sim::state::{FightMode, Intent, IntentAction, IntentSource, Pronouns, Quirk, RelationLabel, Rumour, TalkTone};
use fsw_sim::systems::combat::{SNEAK_ATTACK_MULTIPLIER, damage_monster, panic_chance, sneak_attack};
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::intents::fresh_act;
use fsw_sim::systems::quirks::{NAP_HOURS, NAP_NOTE, after_drink, fall_to_necromancy, pass_on_rumour, raise_the_dead};
use fsw_sim::systems::relationships::{RelationDelta, adjust};
use fsw_sim::util::{Ctx, can_fall_for, drawn_to, first_name, living, rel};
use fsw_sim::{Sim, SimEvent};
use fsw_testing::{admit, create_test_state};
use fsw_world::places::get_place;

struct World {
    sim: Sim,
    events: Vec<SimEvent>,
}

impl World {
    fn new(seed: u32) -> Self {
        Self { sim: Sim::new(create_test_state(seed)), events: Vec::new() }
    }

    fn quiet(seed: u32) -> Self {
        let mut w = Self::new(seed);
        w.sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
        w
    }

    fn run(&mut self, ticks: i64) {
        self.run_until(ticks, |_| false);
    }

    fn run_until(&mut self, ticks: i64, mut until: impl FnMut(&World) -> bool) {
        for _ in 0..ticks {
            if until(self) {
                return;
            }
            let events = self.sim.step();
            self.events.extend(events);
        }
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

    /// Brings one of the authored newcomers into the village now.
    fn admit(&mut self, key: &str) -> String {
        self.with(|ctx| admit(ctx, key))
    }

    /// Everyone but these goes indoors, so nobody else can see what happens.
    fn clear_the_streets(&mut self, keep: &[&str]) {
        for c in self.sim.state.characters.values_mut() {
            if !keep.contains(&c.id.as_str()) {
                c.inside = Some(c.home_id.clone());
                c.intent = Some(Intent::new(IntentAction::Wait { minutes: 600.0 }, IntentSource::Routine));
                c.queue.clear();
            }
        }
    }
}

#[test]
fn the_authored_cast_carries_its_quirks() {
    let w = World::new(1);
    let quirks = |name: &str| w.sim.state.characters[&w.who(name)].quirks.clone();
    assert_eq!(quirks("Dorn"), vec![Quirk::Drunkard]);
    assert_eq!(quirks("Elowen"), vec![Quirk::Occultist]);
    assert_eq!(quirks("Vex"), vec![Quirk::Rumourmonger]);
    assert!(quirks("Aldric").is_empty());
    assert!(!w.sim.state.characters[&w.who("Elowen")].profile.secret.is_empty());
}

#[test]
fn a_coward_panics_half_the_time_and_a_born_fighter_never_does() {
    let mut w = World::new(2);
    let tobin = w.admit("tobin");
    let zariel = w.admit("zariel");
    assert_eq!(panic_chance(&w.sim.state.characters[&tobin]), 0.5);
    assert_eq!(panic_chance(&w.sim.state.characters[&zariel]), 0.0);
}

#[test]
fn a_lazy_villager_skips_work_for_naps_and_a_diligent_one_works_through() {
    let mut w = World::quiet(4);
    let (tobin, zariel, rowan) = (w.admit("tobin"), w.admit("zariel"), w.who("Rowan"));
    let is_work = |a| activity(a).kind == ActivityKind::Work;
    let (mut days, mut tobin_mornings, mut tobin_naps, mut rowan_mornings) = (0, 0, 0, 0);
    for _ in 0..10 {
        w.run(TICKS_PER_DAY);
        let state = &w.sim.state;
        let (Some(t), Some(r)) = (state.characters.get(&tobin), state.characters.get(&rowan)) else { break };
        days += 1;
        tobin_mornings += usize::from(is_work(t.plan[1].activity));
        tobin_naps += t.plan.iter().filter(|b| b.note.as_deref() == Some(NAP_NOTE)).count();
        rowan_mornings += usize::from(is_work(r.plan[1].activity));
        if let Some(z) = state.characters.get(&zariel) {
            // The afternoon and the idle hours both go on work.
            assert!(z.jobs.contains(&z.plan[3].activity) && z.jobs.contains(&z.plan[4].activity), "{:?}", z.plan);
            assert!(z.plan.iter().all(|b| b.note.as_deref() != Some(NAP_NOTE)));
        }
    }
    assert!(days >= 5, "Tobin or Rowan died too soon");
    assert_eq!(rowan_mornings, days);
    assert!(tobin_mornings * 3 <= days * 2, "Tobin worked {tobin_mornings} mornings of {days}");
    assert!(tobin_naps >= 2, "Tobin planned {tobin_naps} naps");
}

#[test]
fn people_nap_on_the_grass_in_plain_sight_and_only_by_day() {
    let mut w = World::new(6);
    let tobin = w.admit("tobin");
    let mut seen = 0;
    let mut tobin_seen = false;
    for _ in 0..(TICKS_PER_DAY * 6 / 100) {
        w.run(100);
        let hour = game_time(w.sim.state.tick).hour;
        for c in living(&w.sim.state) {
            if c.asleep && c.inside.is_none() {
                assert!(matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::Nap { .. })));
                assert!((NAP_HOURS.start..20.5).contains(&hour), "{} napping at {hour}", c.profile.name);
                seen += 1;
                tobin_seen |= c.id == tobin;
            }
        }
    }
    assert!(seen > 0, "nobody napped");
    assert!(tobin_seen, "Tobin never napped");
}

/// Hesper next to a lone villager with nobody else about; the victim is weak enough for one blow to kill.
fn ambush(seed: u32, witness: bool) -> (World, String, String, Option<String>) {
    let mut w = World::quiet(seed);
    w.run(TICKS_PER_DAY / 3);
    let hesper = w.admit("hesper");
    let victim = w.who("Kael");
    let onlooker = witness.then(|| w.who("Rowan"));
    let mut keep = vec![hesper.as_str(), victim.as_str()];
    if let Some(o) = &onlooker {
        keep.push(o);
    }
    w.clear_the_streets(&keep);
    let spot = get_place("docks").door;
    for (id, dx) in [(&victim, 0.0), (&hesper, 1.0)] {
        let c = w.sim.state.characters.get_mut(id).unwrap();
        c.pos = fsw_core::vec2(spot.x + dx, spot.z);
        c.inside = None;
        c.intent = Some(Intent::new(IntentAction::Wait { minutes: 60.0 }, IntentSource::Routine));
    }
    if let Some(o) = &onlooker {
        let c = w.sim.state.characters.get_mut(o).unwrap();
        c.pos = fsw_core::vec2(spot.x + 6.0, spot.z);
        c.inside = None;
        c.asleep = false;
    }
    w.sim.state.characters.get_mut(&victim).unwrap().hp = 60.0;
    (w, hesper, victim, onlooker)
}

#[test]
fn a_sneak_attack_does_five_times_the_damage_and_an_unseen_murder_names_nobody() {
    let (mut w, hesper, victim, _) = ambush(7, false);
    let aldric = w.who("Aldric");
    let before = rel(w.sim.state.characters.get_mut(&aldric).unwrap(), &hesper).clone();
    w.with(|ctx| sneak_attack(ctx, &hesper, &victim));
    let min = 15 * SNEAK_ATTACK_MULTIPLIER;
    let blow = w.events.iter().find_map(|e| match e {
        SimEvent::Hit { attacker_id, damage, .. } if *attacker_id == hesper => Some(*damage),
        _ => None,
    });
    assert!(blow.is_some_and(|d| d >= min as f64), "{blow:?}");
    assert!(!w.sim.state.characters.contains_key(&victim));
    let record = &w.sim.state.dead[&victim];
    assert_eq!(record.killer_id, None);
    assert_eq!(record.hidden_killer_id.as_deref(), Some(hesper.as_str()));
    assert!(record.cause.contains("from behind"), "{}", record.cause);
    let after = rel(w.sim.state.characters.get_mut(&aldric).unwrap(), &hesper).clone();
    assert_eq!(before.affinity, after.affinity);
    assert!(w.sim.state.chronicle.iter().any(|e| e.text.contains("Nobody saw who did it")));
    let h = &w.sim.state.characters[&hesper];
    assert!(h.killer);
    assert!(h.memories.iter().any(|m| m.text.contains("nobody saw")));
}

#[test]
fn a_murder_someone_sees_is_known_to_all() {
    let (mut w, hesper, victim, onlooker) = ambush(7, true);
    w.with(|ctx| sneak_attack(ctx, &hesper, &victim));
    let record = &w.sim.state.dead[&victim];
    assert_eq!(record.killer_id.as_deref(), Some(hesper.as_str()));
    assert_eq!(record.hidden_killer_id, None);
    let o = onlooker.unwrap();
    assert!(rel(w.sim.state.characters.get_mut(&o).unwrap(), &hesper).affinity < -20.0);
}

#[test]
fn a_murderer_left_to_herself_hunts_someone_down() {
    let mut w = World::quiet(9);
    w.run(TICKS_PER_DAY / 3);
    let hesper = w.admit("hesper");
    w.sim.state.characters.get_mut(&hesper).unwrap().arrived_tick -= TICKS_PER_DAY * 3;
    w.run_until(TICKS_PER_DAY * 12, |w| {
        w.events.iter().any(
            |e| matches!(e, SimEvent::Hit { attacker_id, damage, .. } if *attacker_id == hesper && *damage >= 75.0),
        )
    });
    let struck = w
        .events
        .iter()
        .any(|e| matches!(e, SimEvent::Hit { attacker_id, damage, .. } if *attacker_id == hesper && *damage >= 75.0));
    assert!(struck, "Hesper never struck");
}

#[test]
fn accused_the_murderer_denies_everything() {
    let mut w = World::quiet(10);
    let hesper = w.admit("hesper");
    let mira = w.who("Mira");
    let script = fsw_sim::data::smalltalk::local_script(&mut w.sim.state, &mira, &hesper, TalkTone::Hostile, None);
    assert!(["an accusation", "suspicion"].contains(&script.topic.as_str()), "{}", script.topic);
}

#[test]
fn nobody_warms_to_a_bully_and_the_bully_warms_to_nobody() {
    let mut w = World::new(12);
    let zariel = w.admit("zariel");
    let brenna = w.who("Brenna");
    w.with(|ctx| {
        adjust(ctx, &brenna, &zariel, RelationDelta { affinity: 40.0, ..Default::default() });
        adjust(ctx, &zariel, &brenna, RelationDelta { affinity: 40.0, ..Default::default() });
    });
    assert!(rel(w.sim.state.characters.get_mut(&brenna).unwrap(), &zariel).affinity <= 0.0);
    assert!(rel(w.sim.state.characters.get_mut(&zariel).unwrap(), &brenna).affinity <= 20.0);
    w.run(TICKS_PER_DAY * 8);
    let friendly = [RelationLabel::Friend, RelationLabel::CloseFriend, RelationLabel::Lover];
    if let Some(z) = w.sim.state.characters.get(&zariel) {
        assert!(z.relationships.values().all(|r| !friendly.contains(&r.label)));
        for c in living(&w.sim.state).filter(|c| c.id != zariel) {
            if let Some(r) = c.relationships.get(&zariel) {
                assert!(!friendly.contains(&r.label), "{} likes Zariel", c.profile.name);
            }
        }
    }
}

#[test]
fn ulfa_falls_only_for_women_and_most_people_fall_for_her() {
    let mut w = World::new(13);
    let ulfa = w.admit("ulfa");
    let state = &w.sim.state;
    let u = &state.characters[&ulfa];
    let mut drawn = 0;
    // Grown-ups only: nobody falls for a child, nor a child for anyone.
    let others: Vec<_> = living(state).filter(|c| c.id != ulfa && !c.is_child()).collect();
    for c in &others {
        let expected = c.profile.pronouns == Pronouns::SheHer && drawn_to(u, c);
        assert_eq!(can_fall_for(u, c), expected, "Ulfa and {}", c.profile.name);
        drawn += usize::from(can_fall_for(c, u));
    }
    assert!(drawn * 4 >= others.len() * 3, "{drawn} of {}", others.len());
    let (kael, aldric) = (w.who("Kael"), w.who("Aldric"));
    w.with(|ctx| {
        adjust(ctx, &kael, &ulfa, RelationDelta { attraction: 4.0, ..Default::default() });
        adjust(ctx, &ulfa, &aldric, RelationDelta { attraction: 10.0, ..Default::default() });
    });
    let kael_for_ulfa = rel(w.sim.state.characters.get_mut(&kael).unwrap(), &ulfa).attraction;
    assert!(kael_for_ulfa == 0.0 || kael_for_ulfa > 4.0, "{kael_for_ulfa}");
    assert_eq!(rel(w.sim.state.characters.get_mut(&ulfa).unwrap(), &aldric).attraction, 0.0);
}

#[test]
fn halvard_never_falls_for_anyone_and_courting_him_only_confuses_him() {
    let mut w = World::quiet(14);
    w.run(TICKS_PER_DAY / 3);
    let halvard = w.admit("halvard");
    let brenna = w.who("Brenna");
    w.with(|ctx| adjust(ctx, &halvard, &brenna, RelationDelta { attraction: 30.0, ..Default::default() }));
    assert_eq!(rel(w.sim.state.characters.get_mut(&halvard).unwrap(), &brenna).attraction, 0.0);
    let script = fsw_sim::data::smalltalk::local_script(&mut w.sim.state, &brenna, &halvard, TalkTone::Romantic, None);
    assert!(["a misunderstanding", "an evening walk"].contains(&script.topic.as_str()), "{}", script.topic);

    // A romantic approach from start to finish: no couple, no hard feelings, and a puzzled chronicle.
    let (b, h) = (brenna.clone(), halvard.clone());
    for (id, other) in [(&b, &h), (&h, &b)] {
        let c = w.sim.state.characters.get_mut(id).unwrap();
        c.inside = None;
        c.asleep = false;
        rel(c, other).affinity = 50.0;
    }
    let pos = w.sim.state.characters[&h].pos;
    w.sim.state.characters.get_mut(&b).unwrap().pos = fsw_core::vec2(pos.x + 1.0, pos.z);
    let epoch = w.sim.state.characters[&b].epoch;
    let talk = Intent::new(
        IntentAction::Talk { target_id: h.clone(), opener: String::new(), tone: TalkTone::Romantic },
        IntentSource::Llm,
    );
    w.sim.enqueue(Command::SetIntents { id: b.clone(), epoch, intents: vec![talk] });
    let ended = |w: &World| w.events.iter().any(|e| matches!(e, SimEvent::ConversationEnded { a, .. } if *a == b));
    w.run_until(4000, ended);
    assert!(w.sim.state.characters[&h].partner_id.is_none());
    assert_eq!(rel(w.sim.state.characters.get_mut(&b).unwrap(), &h).grievance, 0.0);
    assert!(w.sim.state.chronicle.iter().any(|e| e.text.contains("did not seem to understand")));
}

#[test]
fn the_secret_necromancer_raises_the_dead_by_her_own_door() {
    let mut w = World::new(15);
    w.sim.state.tick = tick_at(1, 21.0);
    let elowen = w.who("Elowen");
    let before = w.sim.state.monsters.len();
    w.with(|ctx| raise_the_dead(ctx, &elowen));
    assert!(w.sim.state.monsters.len() > before);
    let door = get_place(&w.sim.state.characters[&elowen].home_id).door;
    for m in w.sim.state.monsters.values() {
        assert!(fsw_core::math2d::dist(m.pos, door) < 22.0);
    }
    let theron = w.who("Theron");
    assert!(w.sim.state.characters[&theron].memories.iter().any(|m| m.text.contains("those books")));
}

#[test]
fn the_secret_necromancer_can_fall_for_good_and_be_laid_to_rest() {
    let mut w = World::new(16);
    w.sim.state.tick = tick_at(8, 22.0);
    let elowen = w.who("Elowen");
    w.with(|ctx| fall_to_necromancy(ctx, &elowen));
    assert!(!w.sim.state.characters.contains_key(&elowen));
    assert_eq!(w.sim.state.dead[&elowen].cause, "lost to necromancy");
    let fallen = w.sim.state.monsters.values().find(|m| m.lasting).expect("a lasting necromancer").id.clone();
    assert_eq!(w.sim.state.monsters[&fallen].name.as_deref(), Some("Elowen the Necromancer"));

    // Daylight does not turn her to dust: she keeps to the ruins until nightfall. (No raid that morning: its
    // alarm would bring the village out against her.)
    w.sim.state.tick = tick_at(9, 9.0);
    w.sim.state.director.next_raid_tick = i64::MAX;
    w.clear_the_streets(&[]);
    let lair = get_place("ruins").door;
    let far = fsw_core::math2d::dist(w.sim.state.monsters[&fallen].pos, lair);
    w.run(600);
    let m = &w.sim.state.monsters[&fallen];
    assert_ne!(m.phase, fsw_sim::state::MonsterPhase::Dying);
    let near = fsw_core::math2d::dist(m.pos, lair);
    assert!(near < far || near < 6.0, "{far} -> {near}");

    let kael = w.who("Kael");
    let graves = w.sim.state.graves.len();
    w.with(|ctx| damage_monster(ctx, &fallen, 10_000, &kael));
    assert_eq!(w.sim.state.graves.len(), graves + 1);
    assert_eq!(w.sim.state.graves.last().unwrap().name, "Elowen Mistral");
}

#[test]
fn a_gossip_spreads_stories_and_listeners_think_less_of_the_one_they_are_about() {
    let mut w = World::new(17);
    let (vex, rowan, mira) = (w.who("Vex"), w.who("Rowan"), w.who("Mira"));
    let before = rel(w.sim.state.characters.get_mut(&rowan).unwrap(), &mira).affinity;
    let rumour = Rumour {
        about: mira.clone(),
        text: "Mira keeps stolen trinkets under the floorboards.".into(),
        truthful: false,
    };
    w.with(|ctx| pass_on_rumour(ctx, &vex, &rowan, &rumour));
    assert!(rel(w.sim.state.characters.get_mut(&rowan).unwrap(), &mira).affinity < before);
    assert!(w.sim.state.characters[&rowan].memories.iter().any(|m| m.text.starts_with("Vex told me that")));

    w.run(TICKS_PER_DAY * 6);
    let told: Vec<&str> =
        w.sim.state.chronicle.iter().filter(|e| e.text.starts_with("Vex told ")).map(|e| e.text.as_str()).collect();
    assert!(told.len() >= 3, "{told:?}");
    let made_up = told.iter().filter(|t| t.contains("made-up")).count();
    assert!(made_up * 2 >= told.len(), "{told:?}");
}

#[test]
fn the_drunk_gets_drunk_and_starts_fights() {
    let mut w = World::quiet(18);
    w.run(TICKS_PER_DAY / 2);
    let (dorn, kael) = (w.who("Dorn"), w.who("Kael"));
    w.with(|ctx| {
        for _ in 0..40 {
            if ctx.cr(&dorn).drunk(ctx.tick()) {
                break;
            }
            after_drink(ctx, &dorn);
        }
    });
    let tick = w.sim.state.tick;
    assert!(w.sim.state.characters[&dorn].drunk(tick));
    // A long bender, so the fight is a matter of when.
    w.sim.state.characters.get_mut(&dorn).unwrap().quirk_state.drunk_until_tick = tick + TICKS_PER_DAY / 3;
    w.clear_the_streets(&[&dorn, &kael]);
    // No raid tonight: an alarm would send the drunk home to bed before he picks his fight.
    w.sim.state.director.next_wave_tick = tick + TICKS_PER_DAY;
    let spot = get_place("green").door;
    for (id, dx) in [(&dorn, 0.0), (&kael, 1.5)] {
        let c = w.sim.state.characters.get_mut(id).unwrap();
        c.pos = fsw_core::vec2(spot.x + dx, spot.z);
        c.inside = None;
        c.asleep = false;
        // Kael is told to stay put, as the model might tell him: a routine wait would let a tune outside
        // the tavern draw him off to listen, out of the drunk's reach.
        let source = if *id == kael { IntentSource::Llm } else { IntentSource::Routine };
        c.intent = Some(Intent::new(IntentAction::Wait { minutes: 600.0 }, source));
        // A fresh clock, or the wait is measured from whatever they were doing before and ends at once.
        c.act = fresh_act(tick);
    }
    w.run_until(TICKS_PER_DAY / 3, |w| {
        w.sim.state.characters[&dorn]
            .intent
            .as_ref()
            .is_some_and(|i| matches!(i.action, IntentAction::Fight { mode: FightMode::Brawl, .. }))
    });
    assert!(
        w.sim.state.characters[&dorn]
            .intent
            .as_ref()
            .is_some_and(|i| matches!(i.action, IntentAction::Fight { mode: FightMode::Brawl, .. })),
        "Dorn never started a fight"
    );
}
