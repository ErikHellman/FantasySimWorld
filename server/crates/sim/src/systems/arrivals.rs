//! Newcomers: described by the server (the language model's work) or invented by the sim, and
//! walked in by the west road.

use fsw_core::rng::hash_string;
use fsw_core::time::game_time;
use fsw_core::vec2;
use fsw_world::places::{get_place, homes};

use crate::data::anims::AnimId;
use crate::data::children::{BIRTHDAY_DAYS, CHILD_HOBBIES, CHILD_HP_FRACTION, child_look};
use crate::data::economy::starting_gold;
use crate::events::SimEvent;
use crate::state::{
    Bond, Character, Childhood, ChronicleKind, CombatState, FightMode, Id, Intent, IntentAction, IntentSource,
    Inventory, Look, MemoryKind, Needs, PlanSource, Quirk, QuirkState, SimState, StimulusKind,
};
use crate::systems::founding::{Blueprint, apply_bonds, first_name_taken, invent_villager};
use crate::systems::intents::fresh_act;
use crate::systems::relationships::{label_for, looks_welcome};
use crate::systems::world::{beds_of, building_whole};
use crate::util::{Ctx, adults, first_name, living_ids, new_relationship};

/// A home with a free bed (the emptiest one, so newcomers spread out). Nobody is given a house that is
/// burning or in ruins while another stands.
pub fn free_home(state: &SimState) -> String {
    let mut best = homes()[0].id.clone();
    let mut best_free = f64::NEG_INFINITY;
    for home in homes() {
        if state.houses.contains_key(&home.id) || !building_whole(state, &home.id) {
            continue;
        }
        // Children share their parents' home and take no bed of their own.
        let residents = adults(state).filter(|c| c.home_id == home.id).count();
        let free = beds_of(state, &home.id) as f64 - residents as f64;
        if free > best_free {
            best_free = free;
            best = home.id.clone();
        }
    }
    best
}

/// Builds a character from a blueprint, at their own door.
pub fn make_character(state: &mut SimState, entry: &Blueprint, home_id: &str) -> Character {
    let def = entry.class_id.def();
    let home = get_place(home_id);
    let look = entry.child.as_ref().map(|_| child_look(entry.class_id));
    let rng = &mut state.rng;
    let x = home.door.x + fsw_core::rng::range(rng, -1.5, 1.5);
    let z = home.door.z + fsw_core::rng::range(rng, -0.5, 1.5);
    let pos = state.nav.nearest_walkable(vec2(x, z), 5).unwrap_or(home.door);
    let fighter = entry.profile.bravery >= 0.6;
    let id = state.new_id("c");
    let rng = &mut state.rng;
    let needs = Needs {
        hunger: fsw_core::rng::range(rng, 50.0, 70.0),
        energy: fsw_core::rng::range(rng, 88.0, 100.0),
        social: fsw_core::rng::range(rng, 45.0, 70.0),
    };
    Character {
        id,
        profile: entry.profile.clone(),
        class_id: entry.class_id,
        // A child is drawn small, bare-headed and empty-handed, and has no job.
        look: match look {
            Some(look) => Look {
                model: entry.model.clone().unwrap_or_else(|| look.model.into()),
                texture: entry.texture.clone(),
                hide: if entry.hide.is_empty() {
                    look.hide.iter().map(|&part| part.to_owned()).collect()
                } else {
                    entry.hide.clone()
                },
                main_hand: None,
                off_hand: None,
            },
            None => Look {
                model: entry.model.clone().unwrap_or_else(|| def.model.into()),
                texture: entry.texture.clone(),
                hide: entry.hide.clone(),
                main_hand: entry.main_hand.clone().unwrap_or_else(|| def.main_hand.map(str::to_owned)),
                off_hand: entry.off_hand.clone().unwrap_or_else(|| def.off_hand.map(str::to_owned)),
            },
        },
        home_id: home_id.to_owned(),
        jobs: if look.is_some() { Vec::new() } else { entry.jobs.clone().unwrap_or_else(|| def.jobs.to_vec()) },
        hobbies: match look {
            Some(_) => entry.hobbies.clone().unwrap_or_else(|| CHILD_HOBBIES.to_vec()),
            None => entry.hobbies.clone().unwrap_or_else(|| def.hobbies.to_vec()),
        },
        arrived_tick: 0,
        pos,
        facing: home.facing,
        moving: false,
        hp: if look.is_some() { def.max_hp * CHILD_HP_FRACTION } else { def.max_hp },
        needs,
        inventory: Inventory { food: 1, potion: if fighter { 1 } else { 0 }, gift: 0, trinket: 0, relic: 0 },
        carried: Default::default(),
        // A fixed sum by trade, not a roll, so the founding draws from the RNG as it always did.
        gold: if look.is_some() { 0 } else { starting_gold(entry.class_id) },
        last_arson_tick: 0,
        plan: Vec::new(),
        plan_day: 0,
        plan_source: PlanSource::Routine,
        plan_block: -1,
        intent: None,
        queue: Vec::new(),
        act: fresh_act(state.tick),
        slot_id: None,
        inside: None,
        asleep: false,
        watch_until_tick: 0,
        sleep_in_until_tick: 0,
        anim: AnimId::Idle,
        anim_seq: 0,
        tool: None,
        convo_id: None,
        partner_id: None,
        spouse_id: None,
        partner_since_tick: 0,
        combat: CombatState {
            target_id: None,
            mode: FightMode::Monster,
            next_attack_tick: 0,
            hit_tick: 0,
            hit_target_id: None,
            last_hit_by_tick: -1_000_000_000,
            last_hit_by_id: None,
            yielded: false,
            mercy_until_tick: 0,
            mercy_target_id: None,
            defend_ready_tick: 0,
        },
        epoch: 0,
        thought: String::new(),
        outcome: String::new(),
        stimuli: Vec::new(),
        relationships: Default::default(),
        memories: Vec::new(),
        summary: String::new(),
        last_reflection_day: 0,
        kills: 0,
        killer: false,
        next_social_tick: 0,
        next_perception_tick: 0,
        boards_read: Default::default(),
        sky: Default::default(),
        quirks: entry.quirks.clone(),
        quirk_state: QuirkState::default(),
        child: entry.child.as_ref().map(|child| Childhood {
            next_birthday_day: 1 + BIRTHDAY_DAYS,
            mischief: child.mischief,
            parents: child.parents.clone(),
            plan: None,
        }),
        music: Default::default(),
        mount: None,
        mounting: None,
        heartened_until_tick: 0,
        passing: None,
        exiled_since: None,
        sidestep: None,
    }
}

/// A new face walks in by the west road: the one the server described, if a description is waiting
/// and its name is still free, else one the sim invents.
pub fn arrive(ctx: &mut Ctx) -> Id {
    let draft = ctx.state.director.newcomer_draft.take();
    ctx.state.director.newcomer_deadline_tick = 0;
    let villager = match draft {
        Some(v) if !first_name_taken(ctx.state, &v.name) => *v,
        _ => invent_villager(ctx, false),
    };
    let mut blueprint = Blueprint::from_villager(ctx, &villager);
    if comes_to_kill(ctx.state, &villager.name) {
        blueprint.quirks.push(Quirk::Murderous);
    }
    arrive_with(ctx, &blueprint, &villager.bonds, &villager.reason)
}

/// The first day a newcomer may be a murderer: the village has a few days of peace first.
const MURDERER_FROM_DAY: i64 = 3;
/// One newcomer in this many is one, while nobody else in the valley is.
const MURDERER_ONE_IN: u32 = 8;

/// Whether this newcomer is secretly a murderer: rare, never a founder and never two at a time. Nobody tells
/// the model (its description is of an ordinary traveller), and it is settled by a hash of the world's seed
/// and the name, not the RNG, so a newcomer who is not a murderer arrives exactly as before.
fn comes_to_kill(state: &SimState, name: &str) -> bool {
    game_time(state.tick).day >= MURDERER_FROM_DAY
        && !state.characters.values().any(|c| c.has(Quirk::Murderous))
        && hash_string(&format!("murderer:{}:{name}", state.seed)).is_multiple_of(MURDERER_ONE_IN)
}

/// Walks this person in by the west road, with the ties they bring and, for the chronicle, why they came.
pub fn arrive_with(ctx: &mut Ctx, blueprint: &Blueprint, bonds: &[Bond], reason: &str) -> Id {
    let tick = ctx.tick();
    ctx.state.director.arrivals += 1;
    let home = free_home(ctx.state);
    let mut c = make_character(ctx.state, blueprint, &home);
    let road = get_place("west_road");
    let z = road.origin.z + ctx.range(-1.0, 1.0);
    c.pos = ctx.state.nav.nearest_walkable(vec2(road.origin.x - 2.0, z), 6).unwrap_or(road.door);
    c.arrived_tick = tick;
    c.needs = Needs { hunger: 40.0, energy: 60.0, social: 30.0 };
    c.queue = vec![
        Intent::new(IntentAction::GoTo { place_id: "tavern".into() }, IntentSource::Routine),
        Intent::new(
            IntentAction::Activity { activity: fsw_world::places::ActivityId::Eat, place_id: None, minutes: None },
            IntentSource::Routine,
        ),
    ];
    let id = c.id.clone();
    let (name, occupation, home_name) =
        (c.profile.name.clone(), c.profile.occupation.clone(), get_place(&c.home_id).name);
    let welcome = looks_welcome(&c);
    ctx.state.characters.insert(id.clone(), c);
    ctx.state.order.push(id.clone());

    ctx.remember(
        &id,
        format!(
            "Arrived in the valley by the west road. I know no one here yet. I have been given a bed at {home_name}."
        ),
        7.0,
        MemoryKind::Event,
        Vec::new(),
    );
    let bully = ctx.cr(&id).has(Quirk::Bully);
    for other in living_ids(ctx.state) {
        if other == id {
            continue;
        }
        let mut r = new_relationship();
        r.affinity = welcome;
        // Some people put everyone's back up before they have said a word.
        if bully {
            r.affinity -= fsw_core::jsmath::js_round(ctx.range(5.0, 15.0));
        }
        r.label = label_for(&r, false);
        ctx.c(&other).relationships.insert(id.clone(), r);
        ctx.c(&id).relationships.insert(other.clone(), new_relationship());
        ctx.stimulate(
            &other,
            StimulusKind::Arrival,
            format!("A newcomer has arrived in the village: {name}, {occupation}."),
            0,
            Some(&id),
        );
        if ctx.chance(0.5) {
            ctx.remember(
                &other,
                format!("A newcomer, {name}, has come to the village: {occupation}."),
                4.0,
                MemoryKind::Event,
                vec![id.clone()],
            );
        }
    }
    // Kin and old friends who are still alive know the newcomer at once.
    apply_bonds(ctx, &id, bonds);
    let why = if reason.is_empty() { String::new() } else { format!(", {reason}") };
    ctx.chronicle(
        ChronicleKind::Arrival,
        format!("{name}, {occupation}, arrived by the west road{why}."),
        vec![id.clone()],
    );
    ctx.emit(SimEvent::Arrival { id: id.clone() });
    ctx.emit(SimEvent::SavePoint { reason: "arrival".into() });
    let first = first_name(ctx.cr(&id)).to_owned();
    ctx.stimulate(
        &id,
        StimulusKind::Arrival,
        format!("You have just arrived in the village and know nobody. {first}, find your feet."),
        1,
        None,
    );
    id
}
