//! Newcomers: the authored ones first, then wanderers made up from name and trait lists.

use fsw_core::math2d::clamp;
use fsw_core::vec2;
use fsw_world::places::{get_place, homes};

use crate::data::anims::AnimId;
use crate::data::cast::{CastEntry, by_key, newcomers};
use crate::data::classes::ClassId;
use crate::events::SimEvent;
use crate::state::{
    Character, ChronicleKind, CombatState, FightMode, Id, Intent, IntentAction, IntentSource, Inventory, Look,
    MemoryKind, Needs, PlanSource, Profile, Pronouns, Quirk, QuirkState, SimState, StimulusKind, VoiceProfile,
};
use crate::systems::intents::fresh_act;
use crate::systems::relationships::{label_for, looks_welcome};
use crate::systems::world::building_whole;
use crate::util::{Ctx, first_name, living, living_ids, new_relationship, rel};

const FIRST_NAMES: [&str; 15] = [
    "Edda", "Bram", "Wren", "Osric", "Tamsin", "Hale", "Ysolt", "Garnet", "Perrin", "Linnea", "Cade", "Marta", "Jory",
    "Senna", "Alder",
];
const LAST_NAMES: [&str; 10] = [
    "Underhill",
    "Blackwater",
    "Fairweather",
    "Stonebridge",
    "Larkspur",
    "Holt",
    "Greenmantle",
    "Ashby",
    "Wolfe",
    "Tanner",
];
const TRAIT_POOL: [&str; 12] = [
    "cheerful",
    "wary",
    "stubborn",
    "generous",
    "sharp-tongued",
    "patient",
    "restless",
    "superstitious",
    "ambitious",
    "soft-hearted",
    "boastful",
    "thoughtful",
];
const WANDERER_CLASSES: [ClassId; 8] = [
    ClassId::Knight,
    ClassId::Barbarian,
    ClassId::Mage,
    ClassId::Ranger,
    ClassId::Rogue,
    ClassId::Druid,
    ClassId::Engineer,
    ClassId::Farmer,
];

/// A home with a free bed (the emptiest one, so newcomers spread out). Nobody is given a house that is
/// burning or in ruins while another stands.
pub fn free_home(state: &SimState) -> String {
    let mut best = homes()[0].id.clone();
    let mut best_free = f64::NEG_INFINITY;
    for home in homes() {
        if state.houses.contains_key(&home.id) || !building_whole(state, &home.id) {
            continue;
        }
        let residents = living(state).filter(|c| c.home_id == home.id).count();
        let free = home.beds as f64 - residents as f64;
        if free > best_free {
            best_free = free;
            best = home.id.clone();
        }
    }
    best
}

/// Builds a character from an authored entry.
pub fn make_character(state: &mut SimState, entry: &CastEntry, home_id: &str) -> Character {
    let def = entry.class_id.def();
    let home = get_place(home_id);
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
        look: Look {
            model: entry.model.clone().unwrap_or_else(|| def.model.into()),
            texture: entry.texture.clone(),
            hide: entry.hide.clone().unwrap_or_default(),
            main_hand: entry.main_hand.clone().unwrap_or_else(|| def.main_hand.map(str::to_owned)),
            off_hand: entry.off_hand.clone().unwrap_or_else(|| def.off_hand.map(str::to_owned)),
        },
        home_id: home_id.to_owned(),
        jobs: entry.jobs.clone().unwrap_or_else(|| def.jobs.to_vec()),
        hobbies: entry.hobbies.clone().unwrap_or_else(|| def.hobbies.to_vec()),
        arrived_tick: 0,
        pos,
        facing: home.facing,
        moving: false,
        hp: def.max_hp,
        needs,
        inventory: Inventory { food: 1, potion: if fighter { 1 } else { 0 }, gift: 0 },
        carried: Default::default(),
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
        quirks: entry.quirks.clone(),
        quirk_state: QuirkState::default(),
    }
}

/// When the authored newcomers run out, wanderers are made up from name and trait lists.
fn wanderer(ctx: &mut Ctx) -> CastEntry {
    let class_id = ctx.pick(&WANDERER_CLASSES);
    let used: Vec<String> = living(ctx.state)
        .map(|c| c.profile.name.clone())
        .chain(ctx.state.dead.values().map(|d| d.name.clone()))
        .collect();
    let mut name = String::new();
    let mut i = 0;
    while i < 20 && (name.is_empty() || used.contains(&name)) {
        let first = ctx.pick(&FIRST_NAMES);
        let last = ctx.pick(&LAST_NAMES);
        name = format!("{first} {last}");
        i += 1;
    }
    let picked = [ctx.pick(&TRAIT_POOL), ctx.pick(&TRAIT_POOL), ctx.pick(&TRAIT_POOL)];
    let mut traits: Vec<String> = Vec::new();
    for t in picked {
        if !traits.iter().any(|existing| existing == t) {
            traits.push(t.to_owned());
        }
    }
    let def = class_id.def();
    let base = def.model.to_lowercase();
    let recolour = ["knight", "barbarian", "mage", "ranger", "rogue", "druid", "engineer"].contains(&base.as_str());
    let key = format!("wanderer{}", ctx.state.director.arrivals);
    let texture = recolour.then(|| format!("{base}_texture_alt_{}", ctx.pick(&["A", "B", "C"])));
    let pronouns = ctx.pick(&[Pronouns::SheHer, Pronouns::HeHim, Pronouns::TheyThem]);
    let age = ctx.int_range(20, 55);
    let temper = clamp(ctx.next_float(), 0.15, 0.85);
    let bravery = clamp(ctx.next_float(), 0.25, 0.9);
    let sociability = clamp(ctx.next_float(), 0.25, 0.9);
    let pitch = ctx.range(0.7, 1.3);
    let rate = ctx.range(0.85, 1.15);
    CastEntry {
        key,
        class_id,
        model: None,
        texture,
        hide: None,
        main_hand: None,
        off_hand: None,
        home_id: None,
        jobs: None,
        hobbies: None,
        profile: Profile {
            name,
            pronouns,
            age,
            occupation: format!("travelling {}", def.label),
            backstory:
                "A traveller who came up the west road looking for a place to settle, and decided to stay a while."
                    .into(),
            traits,
            goals: vec![
                "find a place in the village".into(),
                "make a friend or two".into(),
                "stay alive through the nights".into(),
            ],
            speech_style: "Plain and direct.".into(),
            temper,
            bravery,
            sociability,
            spite: 0.0,
            allure: 0.0,
            shyness: 0.0,
            looks: 0.0,
            voice: VoiceProfile { pitch, rate, timbre: "plain".into() },
            secret: String::new(),
        },
        bonds: Vec::new(),
        quirks: Vec::new(),
    }
}

/// The living character an authored bond means by `key`: the newcomer itself, or
/// whoever was made from that cast entry.
fn bonded(ctx: &Ctx, entry: &CastEntry, newcomer: &str, key: &str) -> Option<Id> {
    if key == entry.key {
        return Some(newcomer.to_owned());
    }
    let name = &by_key(key)?.profile.name;
    living(ctx.state).find(|c| &c.profile.name == name).map(|c| c.id.clone())
}

/// A new face walks in by the west road.
pub fn arrive(ctx: &mut Ctx) -> Id {
    let tick = ctx.tick();
    let arrivals = ctx.state.director.arrivals;
    let entry = match newcomers().get(arrivals as usize) {
        Some(entry) => entry.clone(),
        None => wanderer(ctx),
    };
    ctx.state.director.arrivals += 1;
    let home = free_home(ctx.state);
    let mut c = make_character(ctx.state, &entry, &home);
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
    for bond in &entry.bonds {
        let from = bonded(ctx, &entry, &id, &bond.from);
        let to = bonded(ctx, &entry, &id, &bond.to);
        if let (Some(from), Some(to)) = (from, to) {
            let r = rel(ctx.c(&from), &to);
            bond.apply(r);
            r.label = label_for(r, false);
        }
    }
    ctx.chronicle(ChronicleKind::Arrival, format!("{name}, {occupation}, arrived by the west road."), vec![id.clone()]);
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
