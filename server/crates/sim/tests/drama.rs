//! Port of `src/sim/drama.test.ts`.

use fsw_core::time::TICKS_PER_DAY;
use fsw_core::vec2;
use fsw_sim::Command;
use fsw_sim::commands::TuningPatch;
use fsw_sim::data::anims::AnimId;
use fsw_sim::data::classes::ClassId;
use fsw_sim::state::{
    Bond, BondKind, ConvoPhase, FightMode, Intent, IntentAction, IntentSource, MemoryKind, MonsterKind, MonsterOrigin,
    NewVillager, Pronouns, Quirk, RelationLabel, TalkTone,
};
use fsw_sim::systems::combat::{Assailant, PANIC_MAX, damage_character, max_hp, panic_chance, run_fight, start_fight};
use fsw_sim::systems::death::kill_character;
use fsw_sim::systems::monsters::spawn_monster;
use fsw_sim::systems::relationships::{RUNG_BLOOD, RUNG_FEUD, RUNG_GRUDGE, Rung, label_for, lethal_unlocked, rung_of};
use fsw_sim::util::{Ctx, adult_count, first_name, living, new_relationship, rel};
use fsw_sim::{Sim, SimEvent};
use fsw_testing::create_test_state;

struct World {
    sim: Sim,
    events: Vec<SimEvent>,
}

impl World {
    fn new(seed: u32) -> Self {
        let mut sim = Sim::new(create_test_state(seed));
        // No small talk: these tests set up their own encounters and must not be interrupted by a chat.
        sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
        Self { sim, events: Vec::new() }
    }

    fn run(&mut self, ticks: i64) {
        self.run_until(ticks, |_| false);
    }

    fn run_until(&mut self, ticks: i64, until: impl Fn(&World) -> bool) {
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

    fn fight(&mut self, id: &str, target: &str, mode: FightMode, say: Option<&str>) {
        let epoch = self.sim.state.characters[id].epoch;
        let mut intent = Intent::new(IntentAction::Fight { target_id: target.into(), mode }, IntentSource::Llm);
        intent.say = say.map(str::to_owned);
        self.sim.enqueue(Command::SetIntents { id: id.into(), epoch, intents: vec![intent] });
    }

    fn lethal_unlocked(&mut self, from: &str, to: &str) -> bool {
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut self.sim.state, &mut events);
        lethal_unlocked(&mut ctx, from, to)
    }
}

#[test]
fn relationship_labels_are_harder_to_lose_than_to_gain() {
    let mut r = new_relationship();
    r.familiarity = 30.0;
    r.affinity = 31.0;
    r.label = label_for(&r, false);
    assert_eq!(r.label, RelationLabel::Friend);
    r.affinity = 22.0; // below the bar for a new friendship, but not low enough to end one
    assert_eq!(label_for(&r, false), RelationLabel::Friend);
    r.affinity = 10.0;
    assert_eq!(label_for(&r, false), RelationLabel::Acquaintance);
    r.label = RelationLabel::Acquaintance;
    r.affinity = 22.0;
    assert_eq!(label_for(&r, false), RelationLabel::Acquaintance);
}

#[test]
fn relationship_labels_put_hostility_and_romance_above_everything_else() {
    let mut r = new_relationship();
    r.affinity = 70.0;
    r.familiarity = 80.0;
    r.kin = true;
    assert_eq!(label_for(&r, false), RelationLabel::Family);
    assert_eq!(label_for(&r, true), RelationLabel::Lover);
    r.grievance = 62.0;
    assert_eq!(label_for(&r, false), RelationLabel::Enemy);
    assert_eq!(rung_of(&r), Rung::Grudge);
}

#[test]
fn the_ladder_only_unlocks_killing_at_the_top_soon_after_an_offence_and_not_twice_in_quick_succession() {
    let mut w = World::new(31);
    w.run(5);
    let (garrick, aldric) = (w.who("Garrick"), w.who("Aldric"));
    let tick = w.sim.state.tick;
    let set = |w: &mut World, grievance: f64, last_offence: i64| {
        let r = rel(w.sim.state.characters.get_mut(&garrick).unwrap(), &aldric);
        r.grievance = grievance;
        r.last_offence_tick = last_offence;
    };
    set(&mut w, RUNG_BLOOD - 1.0, tick);
    assert!(!w.lethal_unlocked(&garrick, &aldric));
    set(&mut w, RUNG_BLOOD + 5.0, tick);
    assert!(w.lethal_unlocked(&garrick, &aldric));
    set(&mut w, RUNG_BLOOD + 5.0, tick - TICKS_PER_DAY * 3);
    assert!(!w.lethal_unlocked(&garrick, &aldric));
    set(&mut w, RUNG_BLOOD + 5.0, tick);
    w.sim.state.director.last_lethal_tick = tick - 100;
    assert!(!w.lethal_unlocked(&garrick, &aldric));
}

#[test]
fn the_ladder_refuses_a_brawl_without_a_feud_and_allows_one_with_it() {
    let mut w = World::new(31);
    w.run(5);
    let (mira, kael) = (w.who("Mira"), w.who("Kael"));
    w.fight(&mira, &kael, FightMode::Brawl, None);
    w.run(2);
    assert!(!w.sim.state.characters[&mira].intent.as_ref().is_some_and(|i| i.is_fight()));
    rel(w.sim.state.characters.get_mut(&mira).unwrap(), &kael).grievance = RUNG_FEUD + 1.0;
    w.fight(&mira, &kael, FightMode::Brawl, None);
    w.run(2);
    let intent = w.sim.state.characters[&mira].intent.clone().unwrap();
    assert!(matches!(intent.action, IntentAction::Fight { mode: FightMode::Brawl, .. }));
}

#[test]
fn a_spiteful_villager_comes_to_blows_over_a_grudge_short_of_a_feud() {
    let mut w = World::new(31);
    w.run(5);
    let (mira, kael) = (w.who("Mira"), w.who("Kael"));
    rel(w.sim.state.characters.get_mut(&mira).unwrap(), &kael).grievance = RUNG_GRUDGE + 10.0;
    w.fight(&mira, &kael, FightMode::Brawl, None);
    w.run(2);
    assert!(!w.sim.state.characters[&mira].intent.as_ref().is_some_and(|i| i.is_fight()));
    w.sim.state.characters.get_mut(&mira).unwrap().profile.spite = 1.0;
    w.fight(&mira, &kael, FightMode::Brawl, None);
    w.run(2);
    let intent = w.sim.state.characters[&mira].intent.clone().unwrap();
    assert!(matches!(intent.action, IntentAction::Fight { mode: FightMode::Brawl, .. }));
}

/// Hostile talks Garrick starts with people he holds no grudge against, over four days of a busy village.
fn quarrels_picked_by_garrick(seed: u32, spite: f64) -> (usize, Sim) {
    let mut sim = Sim::new(create_test_state(seed));
    let garrick = living(&sim.state).find(|c| first_name(c) == "Garrick").unwrap().id.clone();
    sim.state.characters.get_mut(&garrick).unwrap().profile.spite = spite;
    let mut picked = 0;
    for _ in 0..TICKS_PER_DAY * 4 {
        for event in sim.step() {
            if let SimEvent::ConversationStarted { a, b, tone: TalkTone::Hostile, .. } = event
                && a == garrick
                && sim.state.characters[&a].relationships.get(&b).is_none_or(|r| r.grievance < RUNG_GRUDGE)
            {
                picked += 1;
            }
        }
    }
    (picked, sim)
}

#[test]
fn a_spiteful_villager_picks_quarrels_without_a_grudge_but_spares_his_friend() {
    // Over several worlds, since any one of them may keep Garrick busy elsewhere.
    let mut picked = 0;
    for seed in 1..=6 {
        let (calm, _) = quarrels_picked_by_garrick(seed, 0.0);
        let (spiteful, sim) = quarrels_picked_by_garrick(seed, 1.0);
        assert_eq!(calm, 0, "seed {seed}");
        picked += spiteful;
        let state = &sim.state;
        let id = |name: &str| living(state).find(|c| first_name(c) == name).map(|c| c.id.clone());
        // Spite alone does not turn him on his friend; a brawl Dorn starts and wins can, as in any world.
        if let (Some(garrick), Some(dorn)) = (id("Garrick"), id("Dorn")) {
            let wronged = state.characters[&garrick].memories.iter().any(|m| m.text.starts_with("Dorn beat me"));
            if !wronged {
                assert!(state.characters[&garrick].relationships[&dorn].affinity > 0.0, "seed {seed}");
            }
        }
    }
    assert!(picked >= 6, "only {picked} quarrels picked");
}

#[test]
fn a_brawl_ends_with_a_winner_a_loser_and_nobody_dead() {
    let mut w = World::new(31);
    w.run(5);
    let (garrick, dorn) = (w.who("Garrick"), w.who("Dorn"));
    rel(w.sim.state.characters.get_mut(&garrick).unwrap(), &dorn).grievance = RUNG_FEUD + 5.0;
    w.fight(&garrick, &dorn, FightMode::Brawl, Some("Enough!"));
    let blows = |w: &World| w.sim.state.chronicle.iter().any(|entry| entry.text.contains("came to blows"));
    w.run_until(6000, blows);
    assert!(blows(&w));
    assert_eq!(adult_count(&w.sim.state), 13);
    let state = &w.sim.state;
    let (g, d) = (&state.characters[&garrick], &state.characters[&dorn]);
    assert!(g.hp.min(d.hp) > 0.0);
    let (loser, winner) = if g.hp < d.hp { (g, d) } else { (d, g) };
    assert!(loser.memories.iter().any(|m| m.text.contains("beat me")));
    assert!(loser.relationships[&winner.id].grievance > 10.0);
}

fn duel() -> (World, String, String) {
    let mut w = World::new(47);
    w.run(5);
    let (garrick, aldric) = (w.who("Garrick"), w.who("Aldric"));
    let tick = w.sim.state.tick;
    let r = rel(w.sim.state.characters.get_mut(&garrick).unwrap(), &aldric);
    r.grievance = 95.0;
    r.last_offence_tick = tick;
    let a = w.sim.state.characters.get_mut(&aldric).unwrap();
    a.hp = 45.0; // already wounded, so the duel is short and one-sided
    a.inventory.potion = 0;
    w.sim.enqueue(Command::Tuning { tuning: TuningPatch { mercy_wait_ticks: Some(200), ..Default::default() } });
    w.fight(&garrick, &aldric, FightMode::Lethal, None);
    let aldric_id = aldric.clone();
    w.run_until(8000, move |w| {
        w.events.iter().any(|e| matches!(e, SimEvent::MercyRequest { .. }))
            || !w.sim.state.characters.contains_key(&aldric_id)
    });
    (w, garrick, aldric)
}

#[test]
fn the_attacker_may_spare_a_beaten_opponent() {
    let (mut w, garrick, aldric) = duel();
    assert!(w.events.iter().any(|e| matches!(e, SimEvent::MercyRequest { .. })));
    assert!(w.sim.state.characters[&aldric].combat.yielded);
    w.sim.enqueue(Command::Mercy { attacker_id: garrick.clone(), spare: true, say: Some("Go.".into()) });
    w.run(5);
    let a = &w.sim.state.characters[&aldric];
    assert!(!a.combat.yielded);
    assert!(matches!(a.intent.as_ref().map(|i| &i.action), Some(IntentAction::Flee)));
    assert_eq!(a.relationships[&garrick].label, RelationLabel::Enemy);
    assert!(w.sim.state.chronicle.iter().any(|entry| entry.text.contains("spared")));
}

#[test]
fn a_killing_is_permanent_with_a_grave_and_a_village_that_does_not_forgive() {
    let (mut w, garrick, aldric) = duel();
    w.sim.enqueue(Command::Mercy { attacker_id: garrick.clone(), spare: false, say: None });
    w.run(5);
    let state = &w.sim.state;
    assert!(!state.characters.contains_key(&aldric));
    assert_eq!(adult_count(state), 12);
    assert_eq!(state.dead[&aldric].killer_id.as_deref(), Some(garrick.as_str()));
    assert!(state.graves.iter().any(|grave| grave.character_id == aldric));
    assert!(state.characters[&garrick].killer);
    let brenna = &state.characters[&w.who("Brenna")];
    assert_eq!(brenna.relationships[&garrick].label, RelationLabel::Enemy);
    assert!(brenna.memories.iter().any(|m| m.text.contains("Aldric is dead")));
}

#[test]
fn a_duel_resolves_if_no_decision_arrives() {
    let (mut w, _, aldric) = duel();
    w.run(400);
    // Garrick is hot-tempered, so either ending is possible; what matters is that the duel resolves.
    if let Some(a) = w.sim.state.characters.get(&aldric) {
        assert!(!a.combat.yielded);
    }
}

#[test]
fn everyone_has_their_own_chance_to_panic_and_none_above_the_cap() {
    let mut w = World::new(1);
    let mut chances: Vec<f64> = living(&w.sim.state).map(panic_chance).collect();
    assert!(chances.iter().all(|&p| (0.0..=PANIC_MAX).contains(&p)), "{chances:?}");
    chances.sort_by(f64::total_cmp);
    chances.dedup();
    assert!(chances.len() > 5, "{chances:?}");

    let id = w.who("Mira");
    let c = w.sim.state.characters.get_mut(&id).unwrap();
    (c.profile.bravery, c.profile.temper) = (0.0, 0.0);
    assert_eq!(panic_chance(c), PANIC_MAX);
    (c.profile.bravery, c.profile.temper) = (1.0, 1.0);
    assert!(panic_chance(c) < 1e-9);
}

/// Puts a villager with the given nerve in a fight with a skeleton and lets it land up to `blows` light blows.
/// Returns whether the villager ran.
fn take_blows(bravery: f64, temper: f64, blows: usize) -> (World, String, bool) {
    let mut w = World::new(3);
    let id = w.who("Mira");
    let c = w.sim.state.characters.get_mut(&id).unwrap();
    (c.profile.bravery, c.profile.temper) = (bravery, temper);
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut w.sim.state, &mut events);
    let pos = ctx.cr(&id).pos;
    let monster = spawn_monster(&mut ctx, MonsterKind::SkeletonMinion, pos, 0.0, MonsterOrigin::Graveyard);
    start_fight(&mut ctx, &id, &monster, FightMode::Monster, IntentSource::Reflex, None);
    let mut ran = false;
    for _ in 0..blows {
        ctx.c(&id).hp = 60.0;
        let by = Assailant { id: monster.clone(), label: "skeleton".into(), character: None };
        damage_character(&mut ctx, &id, 1, by, FightMode::Monster);
        if matches!(ctx.cr(&id).intent.as_ref().map(|i| &i.action), Some(IntentAction::Flee)) {
            ran = true;
            break;
        }
    }
    (w, id, ran)
}

#[test]
fn a_timid_villager_fighting_the_dead_can_panic_and_run_home() {
    let (w, id, ran) = take_blows(0.0, 0.0, 80);
    assert!(ran, "never panicked");
    let name = &w.sim.state.characters[&id].profile.name;
    assert!(w.sim.state.chronicle.iter().any(|e| e.text.starts_with(&format!("{name} panicked and fled home"))));
    assert!(w.sim.state.characters[&id].memories.iter().any(|m| m.text.starts_with("I panicked")));
}

#[test]
fn the_fearless_do_not_panic() {
    let (_, _, ran) = take_blows(1.0, 1.0, 80);
    assert!(!ran);
}

/// A description of a newcomer, as the server would send it: Torvald's brother, come looking for him.
fn bjarke() -> NewVillager {
    NewVillager {
        class_id: ClassId::Barbarian,
        name: "Bjarke Greymane".into(),
        pronouns: Pronouns::HeHim,
        age: 24,
        occupation: "hill-clan hunter".into(),
        backstory: "Torvald's younger brother, come down from the hills after two winters without a word.".into(),
        traits: vec!["shy".into(), "brave".into(), "kind".into()],
        goals: vec!["find his brother Torvald".into(), "earn a place in the valley".into()],
        speech_style: "Short, quiet and polite; often just a nod.".into(),
        timbre: "low, warm, hesitant".into(),
        temper: 0.3,
        bravery: 0.8,
        sociability: 0.3,
        spite: 0.0,
        allure: 0.0,
        shyness: 0.7,
        looks: 0.6,
        secret: String::new(),
        reason: "looking for his brother".into(),
        quirks: vec![Quirk::Fighter],
        bonds: vec![Bond { to: "Torvald Greymane".into(), relation: BondKind::Sibling, summary: "My brother.".into() }],
    }
}

/// Kills Isolde on day one and runs until the road brings someone, with `on_wanted` run when the sim asks for a
/// description. Returns the world and the newcomer.
fn lose_isolde_and_wait(seed: u32, on_wanted: impl Fn(&mut World)) -> (World, String) {
    // Another death in the meantime puts the newcomer off a day or so, so the road is given six days.
    let mut w = World::new(seed);
    w.run(10);
    let victim = w.who("Isolde");
    {
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut w.sim.state, &mut events);
        kill_character(&mut ctx, &victim, "lost to a fever", None);
    }
    assert_eq!(adult_count(&w.sim.state), 12);
    let mut asked = false;
    for _ in 0..TICKS_PER_DAY * 6 {
        if w.sim.state.director.arrivals > 0 {
            break;
        }
        let events = w.sim.step();
        if events.iter().any(|e| matches!(e, SimEvent::NewcomerWanted { .. })) {
            assert!(!asked, "asked twice");
            asked = true;
            on_wanted(&mut w);
        }
        w.events.extend(events);
    }
    assert!(asked, "the sim never asked for a newcomer");
    assert_eq!(w.sim.state.director.arrivals, 1);
    let newcomer = living(&w.sim.state).find(|c| c.arrived_tick > 0).unwrap().id.clone();
    (w, newcomer)
}

#[test]
fn the_newcomer_the_server_describes_arrives_after_a_death_and_knows_his_kin() {
    let (w, id) = lose_isolde_and_wait(4, |w| w.sim.enqueue(Command::NewcomerDraft(Box::new(bjarke()))));
    let state = &w.sim.state;
    let newcomer = &state.characters[&id];
    assert_eq!(newcomer.profile.name, "Bjarke Greymane");
    assert!(newcomer.home_id.starts_with("home_"));
    assert!(newcomer.has(Quirk::Fighter));
    assert!(newcomer.look.texture.as_deref().is_some_and(|t| t.starts_with("barbarian_texture_alt_")));
    // His brother is still alive, so they know each other at once.
    let torvald = w.who("Torvald");
    assert!(newcomer.relationships[&torvald].kin);
    assert!(state.characters[&torvald].relationships[&id].kin);
    assert!(newcomer.relationships[&torvald].trust >= 80.0);
    assert_eq!(newcomer.relationships[&torvald].summary, "My brother.");
    assert_eq!(state.characters[&torvald].relationships[&id].summary, "Bjarke is my sibling.");
    assert!(
        state
            .chronicle
            .iter()
            .any(|e| e.text == "Bjarke Greymane, hill-clan hunter, arrived by the west road, looking for his brother.")
    );
    assert!(state.director.newcomer_draft.is_none());
    assert_eq!(state.director.newcomer_deadline_tick, 0);
    let rowan = &state.characters[&w.who("Rowan")];
    assert!(rowan.memories.iter().any(|m| m.text.contains("Isolde is dead")));
}

#[test]
fn without_a_description_the_sim_invents_the_newcomer_once_its_patience_runs_out() {
    let (w, id) = lose_isolde_and_wait(4, |_| {});
    let newcomer = &w.sim.state.characters[&id];
    assert_ne!(newcomer.profile.name, "Bjarke Greymane");
    assert!(!fsw_testing::founder_names().contains(&newcomer.profile.name.as_str()));
    assert_eq!(newcomer.profile.traits.len(), 3);
    assert!(!newcomer.profile.backstory.is_empty());
    // The road waited the patience out before giving up on the server.
    let wanted = w.events.iter().find_map(|e| match e {
        SimEvent::NewcomerWanted { deadline_tick } => Some(*deadline_tick),
        _ => None,
    });
    assert!(wanted.is_some_and(|deadline| newcomer.arrived_tick >= deadline));
}

#[test]
fn a_description_whose_name_is_taken_is_dropped_and_told_to_the_sim_arrives_at_once() {
    let (w, id) = lose_isolde_and_wait(4, |w| {
        let mut taken = bjarke();
        taken.name = "Torvald Hale".into();
        w.sim.enqueue(Command::NewcomerDraft(Box::new(taken)));
        // The server then says no description is coming, so the road does not wait the patience out.
        w.sim.enqueue(Command::NewcomerLocal);
    });
    let newcomer = &w.sim.state.characters[&id];
    assert_ne!(newcomer.profile.name, "Torvald Hale");
    let wanted = w.events.iter().find_map(|e| match e {
        SimEvent::NewcomerWanted { deadline_tick } => Some(*deadline_tick),
        _ => None,
    });
    assert!(wanted.is_some_and(|deadline| newcomer.arrived_tick < deadline), "arrived before the patience ran out");
}

#[test]
fn someone_hailed_on_the_way_somewhere_carries_on_there_after_the_chat() {
    let mut w = World::new(8);
    w.run(3);
    let (a, b) = (w.who("Rowan"), w.who("Isolde"));
    let set = |w: &mut World, id: &str, action: IntentAction| {
        let epoch = w.sim.state.characters[id].epoch;
        let intents = vec![Intent::new(action, IntentSource::Llm)];
        w.sim.enqueue(Command::SetIntents { id: id.into(), epoch, intents });
    };
    set(&mut w, &b, IntentAction::GoTo { place_id: "docks".into() });
    w.run(40);
    assert!(!w.sim.state.characters[&b].act.path.is_empty(), "Isolde is on her way to the docks");
    let near = w.sim.state.characters[&b].pos;
    w.sim.state.characters.get_mut(&a).unwrap().pos = vec2(near.x + 1.5, near.z);
    let talk = IntentAction::Talk { target_id: b.clone(), opener: "A word, Isolde.".into(), tone: TalkTone::Friendly };
    set(&mut w, &a, talk);
    let b_id = b.clone();
    w.run_until(6000, move |w| {
        w.events.iter().any(|e| matches!(e, SimEvent::ConversationEnded { b, .. } if *b == b_id))
    });
    let b_id = b.clone();
    w.run_until(6000, move |w| w.sim.state.characters[&b_id].intent.is_none());
    let docks = fsw_world::places::find_place("docks").unwrap().door;
    let isolde = &w.sim.state.characters[&b];
    assert!(isolde.intent.is_none(), "she got there in the end");
    assert!(fsw_core::math2d::dist(isolde.pos, docks) < 3.0, "and is at the docks, not where she was hailed");
}

#[test]
fn conversations_wait_for_lines_then_fall_back_to_local_ones() {
    let mut w = World::new(8);
    w.sim.enqueue(Command::Tuning { tuning: TuningPatch { script_wait_ticks: Some(100), ..Default::default() } });
    w.run(3);
    let (a, b) = (w.who("Rowan"), w.who("Isolde"));
    let epoch = w.sim.state.characters[&a].epoch;
    let talk = Intent::new(
        IntentAction::Talk { target_id: b.clone(), opener: "Walk with me, child.".into(), tone: TalkTone::Friendly },
        IntentSource::Llm,
    );
    w.sim.enqueue(Command::SetIntents { id: a.clone(), epoch, intents: vec![talk] });
    let a_id = a.clone();
    let ended =
        move |w: &World| w.events.iter().any(|e| matches!(e, SimEvent::ConversationEnded { a, .. } if *a == a_id));
    w.run_until(6000, &ended);
    assert!(ended(&w));
    assert!(w.events.iter().any(|e| matches!(e, SimEvent::ConversationWaiting { .. })));
    let lines = w
        .events
        .iter()
        .filter(|e| matches!(e, SimEvent::Speech { speaker_id, to_id: Some(_), .. } if *speaker_id == a || *speaker_id == b))
        .count();
    assert!(lines >= 3);
    let rowan = &w.sim.state.characters[&a];
    assert!(rowan.memories.iter().any(|m| m.kind == MemoryKind::Talk && m.subjects.contains(&b)));
}

#[test]
fn a_talker_who_has_reached_their_partner_stops_walking_and_the_conversation_starts() {
    let mut w = World::new(8);
    w.sim.enqueue(Command::Tuning { tuning: TuningPatch { script_wait_ticks: Some(100), ..Default::default() } });
    w.run(3);
    let (a, b) = (w.who("Rowan"), w.who("Isolde"));
    let epoch = w.sim.state.characters[&a].epoch;
    let talk = Intent::new(
        IntentAction::Talk { target_id: b.clone(), opener: "Walk with me, child.".into(), tone: TalkTone::Friendly },
        IntentSource::Llm,
    );
    w.sim.enqueue(Command::SetIntents { id: a.clone(), epoch, intents: vec![talk] });
    let hailing = |w: &World| w.sim.state.conversations.values().any(|c| c.a == a && c.phase == ConvoPhase::Hail);
    w.run_until(6000, hailing);
    assert!(hailing(&w), "Rowan never hailed Isolde");
    // While hailing, the legs only move when the body does, and standing still means the talk begins:
    // nobody walks on the spot for the fourteen seconds of the hail timeout.
    let mut stood_still = 0;
    for _ in 0..1000 {
        if !hailing(&w) {
            break;
        }
        w.run(1);
        let rowan = &w.sim.state.characters[&a];
        if !rowan.moving {
            stood_still += 1;
            assert_ne!(rowan.anim, AnimId::Walk, "walking on the spot after {stood_still} ticks");
        }
    }
    assert!(!hailing(&w), "the hail never turned into a conversation");
    assert!(stood_still <= 2, "stood still for {stood_still} ticks before the conversation started");
}

/// Puts a villager of middling nerve in a model-ordered stand against a skeleton at the given share of
/// their health, with no potion to drink, and lets the fight run one step. Returns whether they ran.
fn stand_at(hp_share: f64) -> bool {
    let mut w = World::new(3);
    let id = w.who("Mira");
    let c = w.sim.state.characters.get_mut(&id).unwrap();
    c.profile.bravery = 0.5;
    c.inventory.potion = 0;
    let mut events = Vec::new();
    let mut ctx = Ctx::new(&mut w.sim.state, &mut events);
    let pos = ctx.cr(&id).pos;
    let monster = spawn_monster(&mut ctx, MonsterKind::SkeletonMinion, pos, 0.0, MonsterOrigin::Graveyard);
    start_fight(&mut ctx, &id, &monster, FightMode::Monster, IntentSource::Llm, None);
    let cap = max_hp(ctx.cr(&id));
    ctx.c(&id).hp = cap * hp_share;
    run_fight(&mut ctx, &id, &monster, FightMode::Monster, IntentSource::Llm);
    matches!(ctx.cr(&id).intent.as_ref().map(|i| &i.action), Some(IntentAction::Flee))
}

#[test]
fn a_stand_the_model_ordered_breaks_only_at_the_last_resort() {
    // Bravery 0.5 gives up at 23% on instinct; the model's stand holds to half that.
    assert!(!stand_at(0.2), "ran while there was still fight in them");
    assert!(stand_at(0.06), "fought on to the last hit point");
}
