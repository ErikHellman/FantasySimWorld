//! Children: the authored five, how they are kept safe, and how they grow up.

use fsw_sim::Sim;
use fsw_sim::state::RelationLabel;
use fsw_sim::util::{adult_count, children, living, rel_or_default};
use fsw_testing::create_test_state;

fn by_first_name<'s>(state: &'s fsw_sim::state::SimState, name: &str) -> &'s fsw_sim::state::Character {
    living(state).find(|c| fsw_sim::util::first_name(c) == name).expect(name)
}

#[test]
fn the_village_starts_with_five_children_who_do_not_count_as_adults() {
    let state = create_test_state(1);
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
    let state = create_test_state(1);
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
    let state = create_test_state(1);
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
    let mut sim = Sim::new(create_test_state(1));
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
    let mut sim = Sim::new(create_test_state(1));
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

#[test]
fn a_child_covers_ground_at_the_pace_of_their_short_legs() {
    use fsw_core::math2d::dist;
    use fsw_core::time::{TICK_SECONDS, TICKS_PER_DAY};
    use fsw_sim::data::children::CHILD_SCALE;
    use fsw_sim::data::classes::WALK_SPEED;
    use fsw_sim::state::AnimId;
    use std::collections::HashMap;
    // How often each length of step was taken at a walk, in micrometres, children and adults apart.
    let mut steps: [HashMap<i64, u32>; 2] = Default::default();
    let mut sim = Sim::new(create_test_state(1));
    let mut last: Vec<_> = living(&sim.state).map(|c| (c.id.clone(), c.pos)).collect();
    for _ in 0..TICKS_PER_DAY / 2 {
        sim.step();
        for (id, before) in &mut last {
            let c = &sim.state.characters[id.as_str()];
            let step = dist(*before, c.pos);
            *before = c.pos;
            if step > 0.0 && matches!(c.anim, AnimId::Walk | AnimId::WalkB | AnimId::WalkC) {
                *steps[usize::from(c.is_child())].entry((step * 1e6).round() as i64).or_default() += 1;
            }
        }
    }
    // Most ticks of a walk are a full stride along a straight leg of the route; corners and arrivals are shorter.
    let stride = |who: usize| *steps[who].iter().max_by_key(|(_, n)| **n).expect("nobody walked").0;
    let micrometres = |speed: f64| (speed * TICK_SECONDS * 1e6).round() as i64;
    assert_eq!(stride(0), micrometres(WALK_SPEED), "an adult's stride");
    assert_eq!(stride(1), micrometres(WALK_SPEED * CHILD_SCALE), "a child's stride");
}

// --- Protection -----------------------------------------------------------------

mod protection {
    use fsw_core::time::TICKS_PER_DAY;
    use fsw_core::vec2;
    use fsw_sim::commands::TuningPatch;
    use fsw_sim::state::{FightMode, Intent, IntentAction, IntentSource, MonsterKind, MonsterOrigin, RelationLabel};
    use fsw_sim::systems::combat::max_hp;
    use fsw_sim::systems::fire::ignite;
    use fsw_sim::systems::monsters::{find_target, spawn_monster};
    use fsw_sim::systems::relationships::{RelationDelta, adjust};
    use fsw_sim::util::{Ctx, adults, children, open_to, rel, rel_or_default};
    use fsw_sim::{Command, Sim};
    use fsw_testing::create_test_state;

    use super::by_first_name;

    #[test]
    fn a_child_has_half_the_hit_points_of_their_class() {
        let state = create_test_state(1);
        let pip = by_first_name(&state, "Pip");
        assert_eq!(max_hp(pip), pip.class_id.def().max_hp * 0.5);
        assert_eq!(pip.hp, max_hp(pip));
        let aldric = by_first_name(&state, "Aldric");
        assert_eq!(max_hp(aldric), aldric.class_id.def().max_hp);
    }

    #[test]
    fn grievance_toward_or_from_a_child_never_reaches_the_ladder() {
        let mut state = create_test_state(1);
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
        let state = create_test_state(1);
        let (moss, theron) = (by_first_name(&state, "Moss"), by_first_name(&state, "Theron"));
        assert!(!open_to(theron, moss));
        assert!(!open_to(moss, theron));
        assert!(open_to(theron, by_first_name(&state, "Mira")));
    }

    #[test]
    fn a_fight_with_a_child_is_refused_however_the_ladder_stands() {
        let mut sim = Sim::new(create_test_state(1));
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
    fn a_command_cannot_put_a_child_to_work() {
        let mut sim = Sim::new(create_test_state(1));
        let pip = by_first_name(&sim.state, "Pip").id.clone();
        let epoch = sim.state.characters[&pip].epoch;
        let work =
            IntentAction::Activity { activity: fsw_world::places::ActivityId::Smith, place_id: None, minutes: None };
        sim.enqueue(Command::SetIntents {
            id: pip.clone(),
            epoch,
            intents: vec![Intent::new(work, IntentSource::Llm)],
        });
        sim.step();
        let intent = &sim.state.characters[&pip].intent;
        assert!(!intent.as_ref().is_some_and(|i| i.is_activity(fsw_world::places::ActivityId::Smith)), "{intent:?}");
    }

    #[test]
    fn the_dead_go_for_an_adult_before_a_child() {
        let mut state = create_test_state(1);
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
        let monster = spawn_monster(
            &mut ctx,
            MonsterKind::SkeletonMinion,
            vec2(door.x - 2.0, door.z),
            0.0,
            MonsterOrigin::Graveyard,
        );
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
        let mut sim = Sim::new(create_test_state(3));
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
                    spawn_monster(
                        &mut ctx,
                        MonsterKind::SkeletonMinion,
                        vec2(green.x + i as f64, green.z + 3.0),
                        0.0,
                        MonsterOrigin::Graveyard,
                    );
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
    fn a_child_sits_on_the_seat_to_eat() {
        use fsw_sim::data::anims::AnimId;
        use fsw_sim::state::ActPhase;
        use fsw_world::places::ActivityId;
        let mut sim = Sim::new(create_test_state(1));
        let mut seen = false;
        for _ in 0..TICKS_PER_DAY {
            sim.step();
            for c in children(&sim.state) {
                if c.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Eat)) && c.act.phase == ActPhase::Perform
                {
                    assert_eq!(c.anim, AnimId::SitChair, "{}", c.profile.name);
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
    use fsw_sim::{Command, Sim, SimEvent};
    use fsw_testing::create_test_state;

    use super::by_first_name;

    #[test]
    fn children_mostly_keep_company_with_other_children_and_their_own_people() {
        // Pooled over a few villages: two days of one village is too few talks to tell a pull from luck.
        let (mut with_their_own, mut total) = (0, 0);
        for seed in 1..=4 {
            let mut sim = Sim::new(create_test_state(seed));
            let kids: Vec<String> = children(&sim.state).map(|c| c.id.clone()).collect();
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
        }
        assert!(total >= 24, "only {total} talks involved a child");
        // Children and their own people are a quarter of the village; the pull shows as well over that.
        assert!(
            with_their_own * 5 >= total * 2,
            "{with_their_own} of {total} talks were with other children or family"
        );
    }

    #[test]
    fn a_hard_word_at_a_child_is_a_telling_off() {
        let mut sim = Sim::new(create_test_state(1));
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
    use fsw_sim::Sim;
    use fsw_sim::util::{adult_count, children};
    use fsw_testing::create_test_state;

    use super::by_first_name;

    fn run_to_day(sim: &mut Sim, day: i64) {
        while fsw_core::time::game_time(sim.state.tick).day < day {
            sim.step();
        }
    }

    #[test]
    fn a_child_turns_a_year_older_every_three_days() {
        let mut sim = Sim::new(create_test_state(1));
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
        let mut sim = Sim::new(create_test_state(1));
        let pip = by_first_name(&sim.state, "Pip").id.clone();
        let before = sim.state.characters[&pip].clone();
        {
            let c = sim.state.characters.get_mut(&pip).unwrap();
            c.profile.age = 15;
            c.child.as_mut().unwrap().next_birthday_day = 2;
        }
        run_to_day(&mut sim, 2);
        // The routine restarts everyone's sleep at the turn of the day; whoever was out walks home. Within the
        // hour all is still again.
        for _ in 0..TICKS_PER_DAY / 24 {
            let c = &sim.state.characters[&pip];
            if c.asleep && c.inside.as_deref() == Some(c.home_id.as_str()) {
                break;
            }
            sim.step();
        }
        let grown = &sim.state.characters[&pip];
        assert!(!grown.is_child());
        assert_eq!(grown.profile.age, 16);
        // It is night: they sleep in their own bed and take up the day in the morning.
        assert_eq!(grown.inside.as_deref(), Some(grown.home_id.as_str()), "{:?}", grown.intent);
        assert!(grown.asleep);
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
        let mut sim = Sim::new(create_test_state(1));
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
    use fsw_sim::Sim;
    use fsw_sim::state::RelationLabel;
    use fsw_sim::systems::children::maybe_birth;
    use fsw_sim::systems::relationships::{become_lovers, break_up};
    use fsw_sim::util::{Ctx, children, rel, rel_or_default};
    use fsw_testing::create_test_state;

    use super::by_first_name;

    fn run_days(sim: &mut Sim, days: i64) {
        for _ in 0..TICKS_PER_DAY * days {
            sim.step();
        }
    }

    #[test]
    fn falling_in_love_records_when_and_parting_forgets_it() {
        let mut state = create_test_state(1);
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
        let mut sim = Sim::new(create_test_state(5));
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
        // The dawn roll is one in six; ask it directly rather than wait on the luck of a seed.
        let mut rolls = 0;
        while children(&sim.state).count() == before && rolls < 60 {
            let mut events = Vec::new();
            let mut ctx = Ctx::new(&mut sim.state, &mut events);
            maybe_birth(&mut ctx);
            rolls += 1;
        }
        assert!(rolls < 60, "no child in sixty dawns");
        let child = children(&sim.state).find(|c| c.arrived_tick > 0).expect("the newborn");
        let parents = &child.child.as_ref().unwrap().parents;
        assert!(parents.contains(&mira) && parents.contains(&theron), "{parents:?}");
        assert_eq!(child.profile.age, 5);
        assert!(child.look.model.starts_with("Child_"), "{}", child.look.model);
        assert!(child.jobs.is_empty());
        // Born in the night, indoors and asleep, not standing on the step for the dead to find.
        assert_eq!(child.inside.as_deref(), Some(child.home_id.as_str()));
        assert!(child.asleep);
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
        // Births are spaced out: nothing for six days, whatever the dawn rolls say.
        run_days(&mut sim, 5);
        assert_eq!(sim.state.director.last_birth_tick, first_birth, "a second child was born within the cooldown");
    }

    #[test]
    fn a_village_without_lovers_has_no_births() {
        let mut sim = Sim::new(create_test_state(6));
        let authored = children(&sim.state).count();
        run_days(&mut sim, 10);
        assert_eq!(sim.state.director.last_birth_tick, 0);
        assert!(children(&sim.state).count() <= authored);
        assert!(children(&sim.state).all(|c| c.arrived_tick == 0));
    }
}

// --- Mischief --------------------------------------------------------------------

mod mischief {
    use fsw_core::time::{TICKS_PER_DAY, game_time};
    use fsw_core::vec2;
    use fsw_sim::commands::TuningPatch;
    use fsw_sim::events::Projectile;
    use fsw_sim::state::{ActPhase, Intent, IntentAction, IntentSource, Mischief, MischiefPlan, TalkTone};
    use fsw_sim::util::{children, first_name, living, rel_or_default};
    use fsw_sim::{Command, Sim, SimEvent};
    use fsw_testing::create_test_state;
    use fsw_world::places::{ActivityId, get_place};

    use super::by_first_name;

    fn quiet(seed: u32) -> Sim {
        let mut sim = Sim::new(create_test_state(seed));
        sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
        sim
    }

    fn hour(sim: &Sim) -> f64 {
        game_time(sim.state.tick).hour
    }

    fn run_to_hour(sim: &mut Sim, target: f64) {
        while (hour(sim) - target).abs() > 0.02 {
            sim.step();
        }
    }

    fn plan(sim: &mut Sim, id: &str, kind: Mischief) {
        let child = sim.state.characters.get_mut(id).unwrap().child.as_mut().unwrap();
        child.plan = Some(MischiefPlan { kind, place_id: None, done_tick: 0 });
    }

    /// Stands two people out in the open side by side with nothing to do.
    fn side_by_side(sim: &mut Sim, a: &str, b: &str, place: &str) {
        let spot = get_place(place).door;
        for (id, dx) in [(a, 0.0), (b, 1.5)] {
            let c = sim.state.characters.get_mut(id).unwrap();
            c.pos = vec2(spot.x + dx, spot.z + 1.0);
            c.inside = None;
            c.asleep = false;
            c.convo_id = None;
            c.intent = Some(Intent::new(IntentAction::Wait { minutes: 240.0 }, IntentSource::Routine));
            c.queue.clear();
            c.act = fsw_sim::systems::intents::fresh_act(sim.state.tick);
        }
    }

    fn run(sim: &mut Sim, ticks: i64) -> Vec<SimEvent> {
        let mut events = Vec::new();
        for _ in 0..ticks {
            events.extend(sim.step());
        }
        events
    }

    fn chronicled(sim: &Sim, text: &str) -> bool {
        sim.state.chronicle.iter().any(|e| e.text.contains(text))
    }

    #[test]
    fn the_naughty_plan_trouble_at_dawn_and_the_good_do_not() {
        let mut sim = quiet(1);
        let kids: Vec<String> = children(&sim.state).map(|c| c.id.clone()).collect();
        for id in &kids {
            sim.state.characters.get_mut(id).unwrap().child.as_mut().unwrap().mischief = 1.0;
        }
        for _ in 0..TICKS_PER_DAY {
            sim.step();
        }
        assert!(children(&sim.state).any(|c| c.child.as_ref().unwrap().plan.is_some()), "nobody planned any trouble");
        let mut sim = quiet(1);
        for id in &kids {
            sim.state.characters.get_mut(id).unwrap().child.as_mut().unwrap().mischief = 0.0;
        }
        for _ in 0..TICKS_PER_DAY {
            sim.step();
        }
        assert!(children(&sim.state).all(|c| c.child.as_ref().unwrap().plan.is_none()));
    }

    #[test]
    fn a_child_pinches_food_from_a_grown_up_who_holds_it_against_them_a_little() {
        let mut sim = quiet(1);
        let (tansy, kael) =
            (by_first_name(&sim.state, "Tansy").id.clone(), by_first_name(&sim.state, "Kael").id.clone());
        run_to_hour(&mut sim, 10.0);
        side_by_side(&mut sim, &tansy, &kael, "green");
        sim.state.characters.get_mut(&kael).unwrap().inventory.food = 2;
        sim.state.characters.get_mut(&tansy).unwrap().inventory.food = 0;
        plan(&mut sim, &tansy, Mischief::PinchFood);
        run(&mut sim, 200);
        assert_eq!(sim.state.characters[&kael].inventory.food, 1);
        assert_eq!(sim.state.characters[&tansy].inventory.food, 1);
        let r = rel_or_default(&sim.state.characters[&kael], &tansy);
        assert!(r.grievance > 0.0 && r.grievance <= 30.0, "{r:?}");
        assert!(chronicled(&sim, "Tansy pinched"), "{:?}", sim.state.chronicle.last());
        assert!(sim.state.characters[&tansy].child.as_ref().unwrap().plan.as_ref().unwrap().done_tick > 0);
        let garrick = by_first_name(&sim.state, "Garrick");
        assert!(garrick.memories.iter().any(|m| m.text.contains("Tansy")), "her uncle hears of it");
        assert!(garrick.stimuli.iter().any(|s| s.text.contains("Tansy")));
    }

    #[test]
    fn a_child_throws_food_at_a_grown_up_and_runs_home() {
        let mut sim = quiet(1);
        let (tansy, kael) =
            (by_first_name(&sim.state, "Tansy").id.clone(), by_first_name(&sim.state, "Kael").id.clone());
        run_to_hour(&mut sim, 10.0);
        side_by_side(&mut sim, &tansy, &kael, "green");
        sim.state.characters.get_mut(&tansy).unwrap().inventory.food = 1;
        plan(&mut sim, &tansy, Mischief::ThrowFood);
        let events = run(&mut sim, 200);
        assert!(events.iter().any(|e| matches!(e, SimEvent::Attack { ranged: Some(Projectile::Food), attacker_id, .. } if attacker_id == &tansy)));
        assert_eq!(sim.state.characters[&tansy].inventory.food, 0);
        assert_eq!(
            sim.state.characters[&kael].hp,
            fsw_sim::systems::combat::max_hp(&sim.state.characters[&kael]),
            "no harm done"
        );
        let r = rel_or_default(&sim.state.characters[&kael], &tansy);
        assert!(r.grievance >= 10.0 && r.grievance <= 30.0, "{r:?}");
        assert!(!sim.state.characters[&kael].intent.as_ref().is_some_and(|i| i.is_fight()), "nobody fights a child");
        let home = sim.state.characters[&tansy].home_id.clone();
        assert!(
            matches!(&sim.state.characters[&tansy].intent, Some(Intent { action: IntentAction::GoTo { place_id }, .. }) if place_id == &home),
            "{:?}",
            sim.state.characters[&tansy].intent
        );
        assert!(chronicled(&sim, "threw"), "{:?}", sim.state.chronicle.last());
    }

    #[test]
    fn a_child_hides_a_tool_and_the_work_runs_long() {
        let mut sim = quiet(1);
        let (hollis, garrick) =
            (by_first_name(&sim.state, "Hollis").id.clone(), by_first_name(&sim.state, "Garrick").id.clone());
        run_to_hour(&mut sim, 10.0);
        side_by_side(&mut sim, &hollis, &garrick, "lumber_camp");
        let tick = sim.state.tick;
        {
            let c = sim.state.characters.get_mut(&garrick).unwrap();
            c.intent = Some(Intent::new(
                IntentAction::Activity { activity: ActivityId::ChopWood, place_id: None, minutes: Some(60.0) },
                IntentSource::Routine,
            ));
            c.act.phase = ActPhase::Perform;
            c.act.until_tick = tick + 1200;
            c.tool = Some("tools/axe".into());
        }
        plan(&mut sim, &hollis, Mischief::HideTool);
        run(&mut sim, 100);
        let g = &sim.state.characters[&garrick];
        assert!(g.tool.is_none(), "the axe is gone");
        assert!(g.act.until_tick > tick + 1200, "the work runs long");
        assert!(rel_or_default(g, &hollis).grievance > 0.0);
        assert!(chronicled(&sim, "Hollis hid"), "{:?}", sim.state.chronicle.last());
        assert!(g.stimuli.iter().any(|s| s.text.contains("axe")), "{:?}", g.stimuli);
    }

    #[test]
    fn a_child_with_a_tall_tale_tells_it_in_conversation() {
        let mut sim = quiet(1);
        let (hollis, kael) =
            (by_first_name(&sim.state, "Hollis").id.clone(), by_first_name(&sim.state, "Kael").id.clone());
        run_to_hour(&mut sim, 10.0);
        side_by_side(&mut sim, &hollis, &kael, "green");
        plan(&mut sim, &hollis, Mischief::TallTale);
        let epoch = sim.state.characters[&hollis].epoch;
        let talk = IntentAction::Talk { target_id: kael.clone(), opener: String::new(), tone: TalkTone::Friendly };
        sim.enqueue(Command::SetIntents {
            id: hollis.clone(),
            epoch,
            intents: vec![Intent::new(talk, IntentSource::Llm)],
        });
        for _ in 0..6000 {
            sim.step();
            if chronicled(&sim, "Hollis told Kael") {
                break;
            }
        }
        assert!(chronicled(&sim, "Hollis told Kael a made-up story"), "{:?}", sim.state.chronicle);
        assert!(sim.state.characters[&hollis].child.as_ref().unwrap().plan.as_ref().unwrap().done_tick > 0);
    }

    #[test]
    fn a_child_who_wanders_off_at_dusk_is_fetched_home() {
        let mut sim = quiet(1);
        let (moss, rowan) =
            (by_first_name(&sim.state, "Moss").id.clone(), by_first_name(&sim.state, "Rowan").id.clone());
        run_to_hour(&mut sim, 18.0);
        plan(&mut sim, &moss, Mischief::WanderOff);
        side_by_side(&mut sim, &moss, &rowan, "green");
        // Wide awake, so the evening's nap does not keep Moss in.
        sim.state.characters.get_mut(&moss).unwrap().needs.energy = 100.0;
        // Rowan raises Moss for the test (the cast gives Moss nobody), so someone will go after Moss whichever way
        // they slip off. Rowan has the evening free and the raids hold off; Rowan or someone nearer may be the one to
        // go after Moss, depending on how the day went.
        sim.state.characters.get_mut(&moss).unwrap().child.as_mut().unwrap().parents = vec![rowan.clone()];
        sim.state.director.next_wave_tick = sim.state.tick + TICKS_PER_DAY;
        let mut wandered = false;
        let mut fetcher = None;
        for _ in 0..3000 {
            sim.step();
            let m = &sim.state.characters[&moss];
            if matches!(&m.intent, Some(Intent { action: IntentAction::GoTo { place_id }, .. }) if ["glade", "graveyard", "ruins"].contains(&place_id.as_str()))
            {
                wandered = true;
            }
            // Standing right beside Moss, the fetcher may catch up within the tick; the chronicle sees it either way.
            fetcher = fetcher.or_else(|| {
                living(&sim.state)
                    .find(|o| matches!(&o.intent, Some(Intent { action: IntentAction::Fetch { target_id }, .. }) if target_id == &moss))
                    .map(|o| first_name(o).to_owned())
            });
            fetcher = fetcher.or_else(|| {
                sim.state
                    .chronicle
                    .iter()
                    .find_map(|e| e.text.split_once(" fetched Moss").map(|(who, _)| who.to_owned()))
            });
            if wandered && fetcher.is_some() && m.intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Sleep)) {
                break;
            }
        }
        assert!(wandered, "Moss never wandered off");
        let fetcher = fetcher.expect("nobody went after Moss");
        assert!(
            sim.state.characters[&moss].intent.as_ref().is_some_and(|i| i.is_activity(ActivityId::Sleep)),
            "{:?}",
            sim.state.characters[&moss].intent
        );
        assert!(chronicled(&sim, &format!("{fetcher} fetched Moss")), "{:?}", sim.state.chronicle.last());
        assert!(sim.state.characters[&moss].memories.iter().any(|m| m.text.contains(&fetcher)));
    }

    #[test]
    fn the_naughtiest_child_may_set_a_house_alight_by_day() {
        let mut sim = quiet(1);
        let hollis = by_first_name(&sim.state, "Hollis").id.clone();
        run_to_hour(&mut sim, 11.0);
        plan(&mut sim, &hollis, Mischief::PlayWithFire);
        for _ in 0..TICKS_PER_DAY / 4 {
            sim.step();
            if !sim.state.houses.is_empty() {
                break;
            }
        }
        let (place, _) = sim.state.houses.first().expect("a house caught");
        assert_ne!(place, &sim.state.characters[&hollis].home_id, "not her own home");
        assert!(chronicled(&sim, "Hollis set fire"), "{:?}", sim.state.chronicle.last());
        let dorn = by_first_name(&sim.state, "Dorn");
        assert!(dorn.memories.iter().any(|m| m.text.contains("fire")), "{:?}", dorn.memories);
        for _ in 0..40 {
            sim.step();
        }
        assert!(
            children(&sim.state)
                .all(|c| !matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::FightFire { .. })))
        );
        assert!(
            fsw_sim::util::adults(&sim.state)
                .any(|c| matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::FightFire { .. }))),
            "the village answers"
        );
    }
}

// --- Founding ------------------------------------------------------------------

mod founding {
    use fsw_sim::util::{adult_count, adults, children};
    use fsw_sim::{Command, Sim, create_state};
    use fsw_testing::create_test_state;

    #[test]
    fn a_village_the_sim_founds_has_children_of_its_own() {
        let mut sim = Sim::new(create_state(7));
        sim.enqueue(Command::FoundVillage);
        sim.step();
        assert!(!sim.state.director.founding);
        assert_eq!(adult_count(&sim.state), 13);
        let kids: Vec<_> = children(&sim.state).collect();
        assert!((4..=5).contains(&kids.len()), "{} children", kids.len());
        let grown: Vec<String> = adults(&sim.state).map(|c| c.id.clone()).collect();
        let mut foundlings = 0;
        for child in &kids {
            let parents = &child.child.as_ref().unwrap().parents;
            assert!(
                parents.len() <= 2 && parents.iter().all(|p| grown.contains(p)),
                "{}: {parents:?}",
                child.profile.name
            );
            assert!((6..=12).contains(&child.profile.age), "{}", child.profile.name);
            assert!(child.look.model.starts_with("Child_"), "{}", child.look.model);
            if let Some(parent) = parents.first() {
                assert_eq!(
                    child.home_id, sim.state.characters[parent].home_id,
                    "{} lives with a parent",
                    child.profile.name
                );
                assert!(child.relationships[parent].kin);
                assert!(sim.state.characters[parent].relationships[&child.id].kin);
            } else {
                foundlings += 1;
            }
        }
        assert_eq!(foundlings, 1, "one child the village raises");
        assert!(sim.state.chronicle.iter().any(|e| e.text.starts_with("The village's children:")));
        // Their first names are all different, and none is a grown-up's.
        let mut firsts: Vec<&str> = sim.state.characters.values().map(fsw_sim::util::first_name).collect();
        firsts.sort_unstable();
        firsts.dedup();
        assert_eq!(firsts.len(), sim.state.characters.len());
    }

    #[test]
    fn the_test_cast_brings_its_own_children_and_gets_no_more() {
        let state = create_test_state(1);
        assert_eq!(children(&state).count(), 5);
        assert_eq!(adult_count(&state), 13);
    }
}
