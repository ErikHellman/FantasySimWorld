//! Port of `src/sim/drama.test.ts`.

use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::Command;
use fsw_sim::commands::TuningPatch;
use fsw_sim::state::{FightMode, Intent, IntentAction, IntentSource, MemoryKind, MonsterKind, RelationLabel, TalkTone};
use fsw_sim::systems::combat::{Assailant, PANIC_MAX, damage_character, panic_chance, start_fight};
use fsw_sim::systems::death::kill_character;
use fsw_sim::systems::monsters::spawn_monster;
use fsw_sim::systems::relationships::{RUNG_BLOOD, RUNG_FEUD, RUNG_GRUDGE, Rung, label_for, lethal_unlocked, rung_of};
use fsw_sim::util::{Ctx, first_name, living, new_relationship, rel};
use fsw_sim::{Sim, SimEvent, create_state};

struct World {
    sim: Sim,
    events: Vec<SimEvent>,
}

impl World {
    fn new(seed: u32) -> Self {
        let mut sim = Sim::new(create_state(seed));
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
    let mut sim = Sim::new(create_state(seed));
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
    // Worlds where Garrick and Dorn both live through the four days.
    for seed in [5, 3] {
        let (calm, _) = quarrels_picked_by_garrick(seed, 0.0);
        let (spiteful, sim) = quarrels_picked_by_garrick(seed, 1.0);
        assert_eq!(calm, 0, "seed {seed}");
        assert!(spiteful >= 2, "seed {seed}: only {spiteful} quarrels picked");
        let state = &sim.state;
        let id = |name: &str| living(state).find(|c| first_name(c) == name).expect(name).id.clone();
        assert!(state.characters[&id("Garrick")].relationships[&id("Dorn")].affinity > 0.0, "seed {seed}");
    }
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
    assert_eq!(w.sim.state.order.len(), 13);
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
    assert_eq!(state.order.len(), 12);
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
    let monster = spawn_monster(&mut ctx, MonsterKind::SkeletonMinion, pos, 0.0);
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

#[test]
fn a_newcomer_arrives_after_a_death() {
    let mut w = World::new(5);
    w.run(10);
    let victim = w.who("Isolde");
    {
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut w.sim.state, &mut events);
        kill_character(&mut ctx, &victim, "lost to a fever", None);
    }
    assert_eq!(w.sim.state.order.len(), 12);
    w.run_until(TICKS_PER_DAY * 4, |w| w.sim.state.director.arrivals > 0);
    assert_eq!(w.sim.state.director.arrivals, 1);
    let newcomer = living(&w.sim.state).find(|c| c.arrived_tick > 0).unwrap();
    assert_eq!(newcomer.profile.name, "Bjarke Greymane");
    assert!(newcomer.home_id.starts_with("home_"));
    // His brother is still alive, so they know each other at once.
    let torvald = w.who("Torvald");
    let state = &w.sim.state;
    assert!(newcomer.relationships[&torvald].kin);
    assert!(state.characters[&torvald].relationships[&newcomer.id].kin);
    assert!(newcomer.relationships[&torvald].trust >= 80.0);
    let rowan = &w.sim.state.characters[&w.who("Rowan")];
    assert!(rowan.memories.iter().any(|m| m.text.contains("Isolde is dead")));
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
