//! Children: the authored five, how they are kept safe, and how they grow up.

use fsw_sim::state::RelationLabel;
use fsw_sim::util::{adult_count, children, living, rel_or_default};
use fsw_sim::{Sim, create_state};

fn by_first_name<'s>(state: &'s fsw_sim::state::SimState, name: &str) -> &'s fsw_sim::state::Character {
    living(state).find(|c| fsw_sim::util::first_name(c) == name).expect(name)
}

#[test]
fn the_village_starts_with_five_children_who_do_not_count_as_adults() {
    let state = create_state(1);
    assert_eq!(state.order.len(), 18);
    assert_eq!(adult_count(&state), 13);
    assert_eq!(state.director.population_target, 13);
    let kids: Vec<_> = children(&state).collect();
    assert_eq!(kids.len(), 5);
    for child in &kids {
        assert!(child.is_child());
        assert!(child.jobs.is_empty(), "{}", child.profile.name);
        assert_eq!(child.profile.occupation, "child");
        assert!(child.look.model.starts_with("Child_"), "{}", child.look.model);
        assert!(child.look.main_hand.is_none() && child.look.off_hand.is_none(), "{}", child.profile.name);
        assert!((5..16).contains(&child.profile.age), "{}", child.profile.name);
    }
    // The founders keep their ids: the children come after them.
    assert_eq!(state.order[0], "c1");
    assert!(state.order[13..].iter().all(|id| state.characters[id].is_child()));
}

#[test]
fn authored_children_are_kin_to_the_people_who_raise_them() {
    let state = create_state(1);
    let (pip, aldric) = (by_first_name(&state, "Pip"), by_first_name(&state, "Aldric"));
    assert_eq!(pip.child.as_ref().unwrap().parents, vec![aldric.id.clone()]);
    assert_eq!(pip.home_id, aldric.home_id);
    assert_eq!(rel_or_default(pip, &aldric.id).label, RelationLabel::Family);
    assert_eq!(rel_or_default(aldric, &pip.id).label, RelationLabel::Family);
    let moss = by_first_name(&state, "Moss");
    assert!(moss.child.as_ref().unwrap().parents.is_empty(), "the village raises Moss");
    assert!(moss.relationships.values().all(|r| !r.kin));
}

#[test]
fn a_new_world_with_children_round_trips_through_a_save() {
    let state = create_state(1);
    let json = serde_json::to_string(&state).unwrap();
    let back: fsw_sim::state::SimState = serde_json::from_str(&json).unwrap();
    assert_eq!(back, state);
    // An adult's record says nothing about childhood, so older saves load as before.
    let aldric = serde_json::to_value(by_first_name(&state, "Aldric")).unwrap();
    assert!(aldric.get("child").is_none());
    let _ = Sim::new(back);
}

#[test]
fn a_child_plans_a_day_of_play_and_is_home_before_the_dead_rise() {
    use fsw_world::places::ActivityId;
    assert_eq!(ActivityId::parse("play"), Some(ActivityId::Play));
    assert!(ActivityId::OFFERED.contains(&ActivityId::Play));
    let mut sim = Sim::new(create_state(1));
    sim.step();
    for child in children(&sim.state) {
        let plan = &child.plan;
        assert!(plan.iter().any(|block| block.activity == ActivityId::Play), "{}: {plan:?}", child.profile.name);
        for block in plan {
            assert!(
                ActivityId::CHILD_OFFERED.contains(&block.activity),
                "{}: {:?}",
                child.profile.name,
                block.activity
            );
        }
        let bed = plan.iter().find(|block| block.activity == ActivityId::Sleep).expect("a bedtime");
        assert!(bed.hour < 20.0, "{}: bed at {}", child.profile.name, bed.hour);
    }
    for adult in fsw_sim::util::adults(&sim.state) {
        assert!(!adult.plan.iter().any(|block| block.activity == ActivityId::Play), "{}", adult.profile.name);
    }
}

#[test]
fn a_child_at_play_is_described_as_playing() {
    use fsw_sim::describe::describe_activity;
    use fsw_world::places::ActivityId;
    let mut sim = Sim::new(create_state(1));
    let mut seen = false;
    for _ in 0..fsw_core::time::TICKS_PER_DAY {
        sim.step();
        if let Some(child) =
            children(&sim.state).find(|c| c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Play)))
        {
            assert!(
                describe_activity(&sim.state, child).starts_with("playing"),
                "{}",
                describe_activity(&sim.state, child)
            );
            seen = true;
            break;
        }
    }
    assert!(seen, "no child played all day");
}

// --- Protection -----------------------------------------------------------------

mod protection {
    use fsw_core::time::TICKS_PER_DAY;
    use fsw_core::vec2;
    use fsw_sim::commands::TuningPatch;
    use fsw_sim::state::{FightMode, Intent, IntentAction, IntentSource, MonsterKind, RelationLabel};
    use fsw_sim::systems::combat::max_hp;
    use fsw_sim::systems::fire::ignite;
    use fsw_sim::systems::monsters::{find_target, spawn_monster};
    use fsw_sim::systems::relationships::{RelationDelta, adjust};
    use fsw_sim::util::{Ctx, adults, children, open_to, rel, rel_or_default};
    use fsw_sim::{Command, Sim, create_state};

    use super::by_first_name;

    #[test]
    fn a_child_has_half_the_hit_points_of_their_class() {
        let state = create_state(1);
        let pip = by_first_name(&state, "Pip");
        assert_eq!(max_hp(pip), pip.class_id.def().max_hp * 0.5);
        assert_eq!(pip.hp, max_hp(pip));
        let aldric = by_first_name(&state, "Aldric");
        assert_eq!(max_hp(aldric), aldric.class_id.def().max_hp);
    }

    #[test]
    fn grievance_toward_or_from_a_child_never_reaches_the_ladder() {
        let mut state = create_state(1);
        let (pip, garrick) = (by_first_name(&state, "Pip").id.clone(), by_first_name(&state, "Garrick").id.clone());
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut state, &mut events);
        let fury = RelationDelta { grievance: 100.0, affinity: -100.0, ..Default::default() };
        adjust(&mut ctx, &garrick, &pip, fury);
        adjust(&mut ctx, &pip, &garrick, fury);
        for (from, to) in [(&garrick, &pip), (&pip, &garrick)] {
            let r = rel_or_default(ctx.cr(from), to);
            assert!(r.grievance <= 30.0, "{from} -> {to}: {}", r.grievance);
            assert!(r.affinity >= -20.0, "{from} -> {to}: {}", r.affinity);
            assert!(!matches!(r.label, RelationLabel::Rival | RelationLabel::Enemy), "{from} -> {to}: {:?}", r.label);
        }
    }

    #[test]
    fn nobody_falls_for_a_child_and_a_child_falls_for_nobody() {
        let state = create_state(1);
        let (moss, theron) = (by_first_name(&state, "Moss"), by_first_name(&state, "Theron"));
        assert!(!open_to(theron, moss));
        assert!(!open_to(moss, theron));
        assert!(open_to(theron, by_first_name(&state, "Mira")));
    }

    #[test]
    fn a_fight_with_a_child_is_refused_however_the_ladder_stands() {
        let mut sim = Sim::new(create_state(1));
        let (pip, garrick) =
            (by_first_name(&sim.state, "Pip").id.clone(), by_first_name(&sim.state, "Garrick").id.clone());
        // The ladder is forced open by hand, past the cap a real grievance would hit.
        rel(sim.state.characters.get_mut(&garrick).unwrap(), &pip).grievance = 100.0;
        rel(sim.state.characters.get_mut(&pip).unwrap(), &garrick).grievance = 100.0;
        for (from, to) in [(&garrick, &pip), (&pip, &garrick)] {
            let epoch = sim.state.characters[from].epoch;
            let intent =
                Intent::new(IntentAction::Fight { target_id: to.clone(), mode: FightMode::Brawl }, IntentSource::Llm);
            sim.enqueue(Command::SetIntents { id: from.clone(), epoch, intents: vec![intent] });
            sim.step();
            assert!(!sim.state.characters[from].intent.as_ref().is_some_and(|i| i.is_fight()), "{from} fought {to}");
        }
    }

    #[test]
    fn the_dead_go_for_an_adult_before_a_child() {
        let mut state = create_state(1);
        let (pip, aldric) = (by_first_name(&state, "Pip").id.clone(), by_first_name(&state, "Aldric").id.clone());
        let door = state.characters[&pip].pos;
        // Both stand in the open; the child is nearer the monster.
        state.characters.get_mut(&pip).unwrap().pos = door;
        state.characters.get_mut(&aldric).unwrap().pos = vec2(door.x + 6.0, door.z);
        for c in state.characters.values_mut() {
            if c.id != pip && c.id != aldric {
                c.inside = Some(c.home_id.clone());
            }
        }
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut state, &mut events);
        let monster = spawn_monster(&mut ctx, MonsterKind::SkeletonMinion, vec2(door.x - 2.0, door.z), 0.0);
        let monster = ctx.state.monsters[&monster].clone();
        assert_eq!(find_target(ctx.state, &monster, 30.0).as_deref(), Some(aldric.as_str()));
        ctx.state.characters.get_mut(&aldric).unwrap().inside = Some("home_2".into());
        assert_eq!(
            find_target(ctx.state, &monster, 30.0).as_deref(),
            Some(pip.as_str()),
            "a child alone is still prey"
        );
    }

    #[test]
    fn children_never_fight_and_never_carry_water() {
        let mut sim = Sim::new(create_state(3));
        sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
        let kids: Vec<String> = children(&sim.state).map(|c| c.id.clone()).collect();
        let mut fought = false;
        for tick in 0..TICKS_PER_DAY * 3 {
            // Every evening the dead come up the green, where the children play.
            if tick % TICKS_PER_DAY == TICKS_PER_DAY * 13 / 24 + 1200 {
                let mut events = Vec::new();
                let mut ctx = Ctx::new(&mut sim.state, &mut events);
                let green = fsw_world::places::get_place("green").door;
                for i in 0..4 {
                    spawn_monster(&mut ctx, MonsterKind::SkeletonMinion, vec2(green.x + i as f64, green.z + 3.0), 0.0);
                }
            }
            if tick == TICKS_PER_DAY + 1000 {
                let mut events = Vec::new();
                let mut ctx = Ctx::new(&mut sim.state, &mut events);
                ignite(&mut ctx, "home_3");
            }
            sim.step();
            for id in &kids {
                let Some(c) = sim.state.characters.get(id) else { continue };
                if let Some(intent) = &c.intent {
                    assert!(!intent.is_fight(), "{} fought: {:?}", c.profile.name, intent);
                    assert!(
                        !matches!(intent.action, IntentAction::FightFire { .. }),
                        "{} carried water",
                        c.profile.name
                    );
                    assert!(!matches!(intent.action, IntentAction::Stalk { .. }), "{} stalked", c.profile.name);
                }
            }
            fought |= adults(&sim.state).any(|c| c.intent.as_ref().is_some_and(|i| i.is_fight()));
        }
        assert!(fought, "the adults never fought the dead, so the test proved nothing");
    }

    #[test]
    fn a_child_sits_on_the_ground_to_eat() {
        use fsw_sim::data::anims::AnimId;
        use fsw_sim::state::ActPhase;
        use fsw_world::places::ActivityId;
        let mut sim = Sim::new(create_state(1));
        let mut seen = false;
        for _ in 0..TICKS_PER_DAY {
            sim.step();
            for c in children(&sim.state) {
                if c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Eat)) && c.act.phase == ActPhase::Perform
                {
                    assert_ne!(c.anim, AnimId::SitChair, "{}", c.profile.name);
                    seen = true;
                }
            }
            if seen {
                break;
            }
        }
        assert!(seen);
    }
}

// --- Company -------------------------------------------------------------------

mod company {
    use fsw_core::time::TICKS_PER_DAY;
    use fsw_sim::state::{Intent, IntentAction, IntentSource, TalkTone};
    use fsw_sim::util::children;
    use fsw_sim::{Command, Sim, SimEvent, create_state};

    use super::by_first_name;

    #[test]
    fn children_mostly_keep_company_with_other_children_and_their_own_people() {
        let mut sim = Sim::new(create_state(2));
        let kids: Vec<String> = children(&sim.state).map(|c| c.id.clone()).collect();
        let (mut with_their_own, mut total) = (0, 0);
        for _ in 0..TICKS_PER_DAY * 2 {
            for event in sim.step() {
                let SimEvent::ConversationStarted { a, b, .. } = event else { continue };
                let (child, other) = if kids.contains(&a) {
                    (&a, &b)
                } else if kids.contains(&b) {
                    (&b, &a)
                } else {
                    continue;
                };
                total += 1;
                let own = kids.contains(other)
                    || sim.state.characters[child].child.as_ref().is_some_and(|k| k.parents.contains(other));
                with_their_own += own as i32;
            }
        }
        assert!(total >= 6, "only {total} talks involved a child");
        assert!(with_their_own * 2 >= total, "{with_their_own} of {total} talks were with other children or family");
    }

    #[test]
    fn a_hard_word_at_a_child_is_a_telling_off() {
        let mut sim = Sim::new(create_state(1));
        let (garrick, tansy) =
            (by_first_name(&sim.state, "Garrick").id.clone(), by_first_name(&sim.state, "Tansy").id.clone());
        let epoch = sim.state.characters[&garrick].epoch;
        let talk = IntentAction::Talk { target_id: tansy.clone(), opener: String::new(), tone: TalkTone::Hostile };
        sim.enqueue(Command::SetIntents {
            id: garrick.clone(),
            epoch,
            intents: vec![Intent::new(talk, IntentSource::Llm)],
        });
        for _ in 0..6000 {
            sim.step();
            if sim.state.chronicle.iter().any(|entry| entry.text.contains("told Tansy off")) {
                break;
            }
        }
        let entry = sim.state.chronicle.iter().find(|entry| entry.ids.contains(&tansy) && entry.ids.contains(&garrick));
        assert!(entry.is_some_and(|e| e.text.contains("told Tansy off")), "{entry:?}");
        let tansy = &sim.state.characters[&tansy];
        assert!(tansy.memories.iter().any(|m| m.text.contains("told me off")), "{:?}", tansy.memories);
    }
}

// --- Growing up ------------------------------------------------------------------

mod growing_up {
    use fsw_core::time::TICKS_PER_DAY;
    use fsw_sim::util::{adult_count, children};
    use fsw_sim::{Sim, create_state};

    use super::by_first_name;

    fn run_to_day(sim: &mut Sim, day: i64) {
        while fsw_core::time::game_time(sim.state.tick).day < day {
            sim.step();
        }
    }

    #[test]
    fn a_child_turns_a_year_older_every_three_days() {
        let mut sim = Sim::new(create_state(1));
        let pip = by_first_name(&sim.state, "Pip").id.clone();
        assert_eq!(sim.state.characters[&pip].profile.age, 11);
        assert_eq!(sim.state.characters[&pip].child.as_ref().unwrap().next_birthday_day, 4);
        // Brought forward so the test does not hang on who survives the first nights.
        sim.state.characters.get_mut(&pip).unwrap().child.as_mut().unwrap().next_birthday_day = 2;
        run_to_day(&mut sim, 2);
        let child = &sim.state.characters[&pip];
        assert_eq!(child.profile.age, 12);
        assert_eq!(child.child.as_ref().unwrap().next_birthday_day, 5);
        assert!(child.memories.iter().any(|m| m.text.contains("twelve")), "{:?}", child.memories);
        let aldric = by_first_name(&sim.state, "Aldric");
        assert!(
            aldric.memories.iter().any(|m| m.text.contains("Pip") && m.text.contains("twelve")),
            "{:?}",
            aldric.memories
        );
    }

    #[test]
    fn at_sixteen_a_child_comes_of_age_and_keeps_who_they_are() {
        let mut sim = Sim::new(create_state(1));
        let pip = by_first_name(&sim.state, "Pip").id.clone();
        let before = sim.state.characters[&pip].clone();
        {
            let c = sim.state.characters.get_mut(&pip).unwrap();
            c.profile.age = 15;
            c.child.as_mut().unwrap().next_birthday_day = 2;
        }
        run_to_day(&mut sim, 2);
        sim.step();
        let grown = &sim.state.characters[&pip];
        assert!(!grown.is_child());
        assert_eq!(grown.profile.age, 16);
        assert_eq!(grown.profile.name, before.profile.name);
        assert_eq!(grown.profile.occupation, "young knight");
        assert_eq!(grown.look.model, "Knight");
        assert!(grown.look.hide.is_empty());
        assert!(grown.look.main_hand.is_some(), "a knight carries a sword");
        assert_eq!(grown.jobs, grown.class_id.def().jobs.to_vec());
        assert_eq!(grown.hp, grown.class_id.def().max_hp);
        assert!(grown.home_id.starts_with("home_"));
        assert!(grown.relationships.len() >= before.relationships.len(), "bonds are kept");
        assert!(grown.memories.len() > before.memories.len());
        assert!(grown.profile.backstory.contains("came of age"));
        assert_eq!(adult_count(&sim.state), 14);
        assert_eq!(children(&sim.state).count(), 4);
        assert_eq!(sim.state.director.population_target, 13);
        assert!(sim.state.chronicle.iter().any(|e| e.text.contains("Pip") && e.text.contains("come of age")));
        let aldric = by_first_name(&sim.state, "Aldric");
        assert!(
            aldric.memories.iter().any(|m| m.text.contains("Pip") && m.text.contains("of age")),
            "{:?}",
            aldric.memories
        );
        // A later start from this save leaves the grown-up alone: the cast only describes the child.
        let json = serde_json::to_string(&sim.state).unwrap();
        let reloaded = Sim::new(serde_json::from_str(&json).unwrap());
        let again = &reloaded.state.characters[&pip];
        assert!(again.profile.backstory.contains("came of age"));
        assert_eq!(again.profile.occupation, "young knight");
    }

    #[test]
    fn a_grown_child_plans_an_adult_day() {
        use fsw_world::places::ActivityId;
        let mut sim = Sim::new(create_state(1));
        let pip = by_first_name(&sim.state, "Pip").id.clone();
        {
            let c = sim.state.characters.get_mut(&pip).unwrap();
            c.profile.age = 15;
            c.child.as_mut().unwrap().next_birthday_day = 2;
        }
        run_to_day(&mut sim, 2);
        for _ in 0..TICKS_PER_DAY / 4 {
            sim.step();
        }
        let plan = &sim.state.characters[&pip].plan;
        assert!(!plan.iter().any(|b| b.activity == ActivityId::Play), "{plan:?}");
        assert!(plan.iter().any(|b| matches!(b.activity, ActivityId::Watch | ActivityId::Train)), "{plan:?}");
    }
}

// --- Births ----------------------------------------------------------------------

mod births {
    use fsw_core::time::TICKS_PER_DAY;
    use fsw_sim::state::RelationLabel;
    use fsw_sim::systems::relationships::{become_lovers, break_up};
    use fsw_sim::util::{Ctx, children, rel, rel_or_default};
    use fsw_sim::{Sim, create_state};

    use super::by_first_name;

    fn run_days(sim: &mut Sim, days: i64) {
        for _ in 0..TICKS_PER_DAY * days {
            sim.step();
        }
    }

    #[test]
    fn falling_in_love_records_when_and_parting_forgets_it() {
        let mut state = create_state(1);
        let (mira, theron) = (by_first_name(&state, "Mira").id.clone(), by_first_name(&state, "Theron").id.clone());
        state.tick = 500;
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut state, &mut events);
        become_lovers(&mut ctx, &mira, &theron);
        assert_eq!(ctx.cr(&mira).partner_since_tick, 500);
        assert_eq!(ctx.cr(&theron).partner_since_tick, 500);
        break_up(&mut ctx, &mira, &theron, "a test");
        assert_eq!(ctx.cr(&mira).partner_since_tick, 0);
        assert_eq!(ctx.cr(&theron).partner_since_tick, 0);
    }

    #[test]
    fn lovers_long_together_have_a_child_who_knows_them_both() {
        let mut sim = Sim::new(create_state(5));
        // The world is nine days old and the two have been together since its first morning.
        sim.state.tick = 9 * TICKS_PER_DAY;
        let (mira, theron) =
            (by_first_name(&sim.state, "Mira").id.clone(), by_first_name(&sim.state, "Theron").id.clone());
        for (me, them) in [(&mira, &theron), (&theron, &mira)] {
            let c = sim.state.characters.get_mut(me).unwrap();
            c.partner_id = Some(them.clone());
            c.partner_since_tick = 1;
            // Fond enough that the daily reckoning does not part them.
            let r = rel(c, them);
            (r.affinity, r.attraction) = (70.0, 70.0);
        }
        let before = children(&sim.state).count();
        let mut born_on = None;
        for day in 1..=12 {
            run_days(&mut sim, 1);
            if children(&sim.state).count() > before {
                born_on = Some(day);
                break;
            }
        }
        let day = born_on.expect("no child was born in twelve days");
        let child = children(&sim.state).find(|c| c.arrived_tick > 0).expect("the newborn");
        let parents = &child.child.as_ref().unwrap().parents;
        assert!(parents.contains(&mira) && parents.contains(&theron), "{parents:?}");
        assert_eq!(child.profile.age, 5);
        assert!(child.look.model.starts_with("Child_"), "{}", child.look.model);
        assert!(child.jobs.is_empty());
        assert!(
            child.home_id == sim.state.characters[&mira].home_id
                || child.home_id == sim.state.characters[&theron].home_id
        );
        for parent in [&mira, &theron] {
            assert_eq!(rel_or_default(child, parent).label, RelationLabel::Family);
            assert_eq!(rel_or_default(&sim.state.characters[parent], &child.id).label, RelationLabel::Family);
            assert!(sim.state.characters[parent].memories.iter().any(|m| m.text.contains(&child.profile.name)));
        }
        assert!(sim.state.chronicle.iter().any(|e| e.text.contains(&child.profile.name) && e.text.contains("child")));
        let first_birth = sim.state.director.last_birth_tick;
        assert!(first_birth > 0);
        // One is enough for now: the village is full of children and births are spaced out.
        run_days(&mut sim, 12 - day);
        assert_eq!(sim.state.director.last_birth_tick, first_birth, "a second child was born");
    }

    #[test]
    fn a_village_without_lovers_has_no_births() {
        let mut sim = Sim::new(create_state(6));
        let authored = children(&sim.state).count();
        run_days(&mut sim, 10);
        assert_eq!(sim.state.director.last_birth_tick, 0);
        assert!(children(&sim.state).count() <= authored);
        assert!(children(&sim.state).all(|c| c.arrived_tick == 0));
    }
}
