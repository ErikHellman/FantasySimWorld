//! Founding the village and taking in newcomers from a description of who they are.
//!
//! A new world starts empty and `founding`: the server sends one `Command::Settle` per villager the
//! language model invents, or one `Command::FoundVillage` to fill the village from the lists in
//! `data::names` when there is no model. Either way the sim builds the character, draws its look and
//! voice from its own RNG, and acquaints it with everyone already here, so a run stays reproducible
//! for a given command stream. Newcomers later go through the same `Blueprint`.

use fsw_core::jsmath::js_round;
use fsw_core::math2d::clamp;
use fsw_core::rng::{hash_string, range};
use fsw_world::places::{ActivityId, get_place};

use crate::data::classes::ClassId;
use crate::data::names::*;
use crate::events::SimEvent;
use crate::state::{
    AmbitionDraft, Bond, BondKind, ChronicleKind, Id, NewVillager, Profile, Pronouns, Quirk, RelationLabel, SimState,
    VoiceProfile,
};
use crate::systems::arrivals::{free_home, make_character};
use crate::systems::children::found_children;
use crate::systems::director::tick_at;
use crate::systems::relationships::{label_for, looks_welcome};
use crate::util::{Ctx, first_name, living, new_relationship, rel};

/// Everything `make_character` needs: who someone is, how they look and where they live.
#[derive(Clone, Debug)]
pub struct Blueprint {
    pub class_id: ClassId,
    /// model override (defaults to the class model)
    pub model: Option<String>,
    pub texture: Option<String>,
    pub hide: Vec<String>,
    /// `None` keeps the class weapon; `Some(None)` is empty-handed
    pub main_hand: Option<Option<String>>,
    pub off_hand: Option<Option<String>>,
    /// `None` takes the emptiest standing home
    pub home_id: Option<String>,
    pub jobs: Option<Vec<ActivityId>>,
    pub hobbies: Option<Vec<ActivityId>>,
    pub profile: Profile,
    pub quirks: Vec<Quirk>,
    /// set for a child; the class is the one they will grow into
    pub child: Option<ChildSeed>,
    /// the one long goal they come with, if the model gave one; grown villagers without get one from the sim
    pub ambition: Option<AmbitionDraft>,
}

/// A child as described: how naughty they are and who raises them.
#[derive(Clone, Debug)]
pub struct ChildSeed {
    /// 0 never up to anything .. 1 trouble every day
    pub mischief: f64,
    /// ids of the people who raise them; empty for a child the village raises
    pub parents: Vec<Id>,
}

/// The models that come with alternative textures (`<base>_texture_alt_{A,B,C}`).
const RECOLOURED: [&str; 7] = ["knight", "barbarian", "mage", "ranger", "rogue", "druid", "engineer"];

/// The quirks an invented or model-written villager may have: those that make a village lively, not
/// those that make it deadly (no murderers, no necromancers in the making).
pub const ALLOWED_QUIRKS: [Quirk; 9] = [
    Quirk::Coward,
    Quirk::Lazy,
    Quirk::Bully,
    Quirk::Diligent,
    Quirk::Fighter,
    Quirk::LovesWomen,
    Quirk::Chaste,
    Quirk::Rumourmonger,
    Quirk::Drunkard,
];

/// How long the director waits for the server's description of a newcomer before inventing one.
pub const NEWCOMER_PATIENCE_MINUTES: f64 = 360.0;

impl Blueprint {
    /// From a description: the class's look, a recolour where the model has one, and a voice drawn
    /// from the sim's RNG.
    pub fn from_villager(ctx: &mut Ctx, v: &NewVillager) -> Self {
        let def = v.class_id.def();
        let base = def.model.to_lowercase();
        let texture =
            RECOLOURED.contains(&base.as_str()).then(|| format!("{base}_texture_alt_{}", ctx.pick(&["A", "B", "C"])));
        let pitch = ctx.range(0.7, 1.3);
        let rate = ctx.range(0.85, 1.15);
        Self {
            class_id: v.class_id,
            model: None,
            texture,
            hide: Vec::new(),
            main_hand: None,
            off_hand: None,
            home_id: None,
            jobs: None,
            hobbies: None,
            profile: Profile {
                name: v.name.clone(),
                pronouns: v.pronouns,
                age: v.age,
                occupation: v.occupation.clone(),
                backstory: v.backstory.clone(),
                traits: v.traits.clone(),
                goals: v.goals.clone(),
                speech_style: v.speech_style.clone(),
                temper: v.temper,
                bravery: v.bravery,
                sociability: v.sociability,
                spite: v.spite,
                allure: v.allure,
                shyness: v.shyness,
                looks: v.looks,
                voice: VoiceProfile { pitch, rate, timbre: v.timbre.clone() },
                secret: v.secret.clone(),
            },
            quirks: v.quirks.iter().copied().filter(|q| ALLOWED_QUIRKS.contains(q)).collect(),
            child: None,
            ambition: v.ambition.clone(),
        }
    }
}

/// Whether someone living or dead already goes by this first name (ignoring case). Prompts and
/// the brain's answers name people by first name, so two of a kind would be one to the model.
pub fn first_name_taken(state: &SimState, name: &str) -> bool {
    let Some(first) = name.split_whitespace().next() else { return true };
    let first = first.to_lowercase();
    living(state)
        .map(|c| c.profile.name.as_str())
        .chain(state.dead.values().map(|d| d.name.as_str()))
        .any(|n| n.split_whitespace().next().is_some_and(|f| f.to_lowercase() == first))
}

/// The living villager with this full name, ignoring case.
pub fn living_by_name(state: &SimState, name: &str) -> Option<Id> {
    living(state).find(|c| c.profile.name.eq_ignore_ascii_case(name)).map(|c| c.id.clone())
}

/// Puts someone in their home and in the world; nothing else happens yet.
pub fn settle(state: &mut SimState, blueprint: &Blueprint) -> Id {
    let home = blueprint.home_id.clone().unwrap_or_else(|| free_home(state));
    let c = make_character(state, blueprint, &home);
    let id = c.id.clone();
    state.characters.insert(id.clone(), c);
    state.order.push(id.clone());
    id
}

/// `a` knows `b` a little: everyone in a village this small does.
pub fn acquaint(state: &mut SimState, a: &Id, b: &Id) {
    let mut r = new_relationship();
    r.familiarity = range(&mut state.rng, 18.0, 32.0);
    r.affinity = js_round(range(&mut state.rng, 0.0, 14.0));
    r.trust = js_round(range(&mut state.rng, 0.0, 12.0));
    r.affinity += looks_welcome(&state.characters[b]);
    state.characters[a].relationships.insert(b.clone(), r);
}

/// Labels every relationship `a` has, from its numbers.
/// Labels everyone's record afresh; a spouse keeps that label whatever the numbers say.
fn relabel(state: &mut SimState, a: &Id) {
    let c = &mut state.characters[a];
    let (partner, spouse) = (c.partner_id.clone(), c.spouse_id.clone());
    for (other, r) in c.relationships.iter_mut() {
        r.label = if spouse.as_deref() == Some(other.as_str()) {
            RelationLabel::Spouse
        } else {
            label_for(r, partner.as_deref() == Some(other.as_str()))
        };
    }
}

impl BondKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sibling => "sibling",
            Self::Parent => "parent",
            Self::Child => "child",
            Self::Spouse => "spouse",
            Self::Friend => "friend",
            Self::Comrade => "comrade",
            Self::Rival => "rival",
        }
    }

    /// What the other side of the bond is: a parent's child, and so on.
    fn reverse(self) -> Self {
        match self {
            Self::Parent => Self::Child,
            Self::Child => Self::Parent,
            other => other,
        }
    }
}

/// Ties a newcomer to the living people their description names, both ways. A name nobody
/// living has is skipped.
pub fn apply_bonds(ctx: &mut Ctx, id: &Id, bonds: &[Bond]) {
    let tick = ctx.tick();
    for bond in bonds {
        let Some(other) = living_by_name(ctx.state, &bond.to) else { continue };
        if other == *id {
            continue;
        }
        let first = first_name(ctx.cr(id)).to_owned();
        let other_first = first_name(ctx.cr(&other)).to_owned();
        let mine = if bond.summary.is_empty() {
            format!("{other_first} is my {}.", bond.relation.as_str())
        } else {
            bond.summary.clone()
        };
        let theirs = format!("{first} is my {}.", bond.relation.reverse().as_str());
        for (from, to, summary) in [(id, &other, mine), (&other, id, theirs)] {
            let r = rel(ctx.c(from), to);
            match bond.relation {
                BondKind::Sibling | BondKind::Parent | BondKind::Child => {
                    r.kin = true;
                    r.affinity = 50.0;
                    r.trust = 80.0;
                    r.familiarity = 90.0;
                }
                BondKind::Spouse => {
                    r.attraction = 70.0;
                    r.affinity = 60.0;
                    r.trust = 70.0;
                    r.familiarity = 95.0;
                }
                BondKind::Friend => {
                    r.affinity = 35.0;
                    r.trust = 40.0;
                    r.familiarity = 70.0;
                }
                BondKind::Comrade => {
                    r.affinity = 30.0;
                    r.trust = 55.0;
                    r.familiarity = 75.0;
                }
                BondKind::Rival => {
                    r.affinity = -25.0;
                    r.grievance = 25.0;
                    r.familiarity = 60.0;
                    r.last_offence_tick = tick;
                }
            }
            r.summary = summary;
            r.label = label_for(r, false);
        }
        // Someone who comes as a spouse is married on arrival, if both are free to be.
        if bond.relation == BondKind::Spouse && ctx.cr(id).partner_id.is_none() && ctx.cr(&other).partner_id.is_none() {
            for (me, them) in [(id, &other), (&other, id)] {
                let c = ctx.c(me);
                c.partner_id = Some(them.clone());
                c.spouse_id = Some(them.clone());
                c.partner_since_tick = tick;
                rel(c, them).label = RelationLabel::Spouse;
            }
        }
    }
}

/// The founding is over: the dead may rise from the first evening on.
pub fn finish_founding(state: &mut SimState) {
    state.director.founding = false;
    let day = fsw_core::time::game_time(state.tick).day;
    let tonight = tick_at(day, 20.6);
    state.director.next_wave_tick = if tonight > state.tick { tonight } else { tick_at(day + 1, 20.6) };
}

/// Settles a whole cast at once (the test fixture's), in the order the TypeScript game did: everyone
/// at their door, then acquainted with everyone, then `bonds` (the authored starting feelings),
/// then every relationship labelled.
pub fn found_village(state: &mut SimState, blueprints: &[Blueprint], bonds: impl FnOnce(&mut SimState, &[Id])) {
    let ids: Vec<Id> = blueprints.iter().map(|blueprint| settle(state, blueprint)).collect();
    for a in &ids {
        for b in &ids {
            if a != b {
                acquaint(state, a, b);
            }
        }
    }
    bonds(state, &ids);
    for a in &ids {
        relabel(state, a);
    }
    let grown = ids.iter().filter(|id| !state.characters[*id].is_child()).count();
    state.director.population_target = state.director.population_target.max(grown);
    finish_founding(state);
}

/// `Command::Settle`: one more founder, described by the model. Ignored once the founding is over
/// or when someone already goes by that first name.
pub fn settle_villager(ctx: &mut Ctx, v: &NewVillager) {
    if !ctx.state.director.founding || first_name_taken(ctx.state, &v.name) {
        return;
    }
    let blueprint = Blueprint::from_villager(ctx, v);
    settle_and_introduce(ctx, &blueprint, &v.bonds);
    if ctx.state.order.len() >= ctx.state.director.population_target {
        conclude_founding(ctx);
    }
}

/// The last founder is in: the village has its children, the chronicle says so, and the dead may come.
fn conclude_founding(ctx: &mut Ctx) {
    finish_founding(ctx.state);
    let souls = ctx.state.order.len();
    ctx.chronicle(
        ChronicleKind::World,
        format!("The village is founded: {souls} souls under the valley's sky."),
        Vec::new(),
    );
    hide_a_murderer(ctx.state);
    found_children(ctx);
    ctx.emit(SimEvent::SavePoint { reason: "founded".into() });
}

/// One new world in this many starts with a murderer among its founders.
const FOUNDING_MURDERER_ONE_IN: u32 = 3;

/// Now and then one grown founder is secretly a murderer. Nobody tells the model (it described an ordinary
/// villager), and it is settled by a hash of the world's seed, not the RNG, so the founding runs as before.
fn hide_a_murderer(state: &mut SimState) {
    if !hash_string(&format!("founding-murderer:{}", state.seed)).is_multiple_of(FOUNDING_MURDERER_ONE_IN) {
        return;
    }
    let grown: Vec<Id> = state.order.iter().filter(|id| !state.characters[*id].is_child()).cloned().collect();
    if grown.is_empty() {
        return;
    }
    let pick = hash_string(&format!("founding-murderer-who:{}", state.seed)) as usize % grown.len();
    if let Some(c) = state.characters.get_mut(&grown[pick]) {
        c.quirks.push(Quirk::Murderous);
    }
}

/// Settles one person during the founding and makes them known to everyone already here.
fn settle_and_introduce(ctx: &mut Ctx, blueprint: &Blueprint, bonds: &[Bond]) -> Id {
    let id = settle(ctx.state, blueprint);
    let others: Vec<Id> = ctx.state.order.iter().filter(|other| **other != id).cloned().collect();
    for other in &others {
        acquaint(ctx.state, &id, other);
        acquaint(ctx.state, other, &id);
    }
    apply_bonds(ctx, &id, bonds);
    relabel(ctx.state, &id);
    for other in &others {
        relabel(ctx.state, other);
    }
    let c = ctx.cr(&id);
    let (name, occupation, home) = (c.profile.name.clone(), c.profile.occupation.clone(), get_place(&c.home_id).name);
    ctx.chronicle(ChronicleKind::Arrival, format!("{name}, {occupation}, has settled at {home}."), vec![id.clone()]);
    ctx.emit(SimEvent::Arrival { id: id.clone() });
    ctx.emit(SimEvent::SavePoint { reason: "settle".into() });
    id
}

/// `Command::FoundVillage`: fills the village to its target with villagers the sim invents, for a
/// server without a model (or whose model would not answer). Ignored once the founding is over.
pub fn fill_village(ctx: &mut Ctx) {
    if !ctx.state.director.founding {
        return;
    }
    while ctx.state.order.len() < ctx.state.director.population_target {
        let v = invent_villager(ctx, true);
        let blueprint = Blueprint::from_villager(ctx, &v);
        settle_and_introduce(ctx, &blueprint, &v.bonds);
    }
    conclude_founding(ctx);
}

/// A villager made up from the lists in `data::names`: a founder knows a few of the others already,
/// a newcomer has a reason for coming up the road.
pub fn invent_villager(ctx: &mut Ctx, founder: bool) -> NewVillager {
    // Farmers, knights, rangers and barbarians are the backbone of any village; the rest come rarer.
    const CLASSES: [ClassId; 18] = [
        ClassId::Knight,
        ClassId::Knight,
        ClassId::Barbarian,
        ClassId::Barbarian,
        ClassId::Mage,
        ClassId::Ranger,
        ClassId::Ranger,
        ClassId::Rogue,
        ClassId::Scout,
        ClassId::Druid,
        ClassId::Engineer,
        ClassId::Paladin,
        ClassId::Cleric,
        ClassId::Witch,
        ClassId::Tiefling,
        ClassId::Lorekeeper,
        ClassId::Farmer,
        ClassId::Farmer,
    ];
    let class_id = ctx.pick(&CLASSES);
    let pronouns = ctx.pick(&[Pronouns::SheHer, Pronouns::HeHim, Pronouns::TheyThem]);
    let mut name = String::new();
    let mut tries = 0;
    while tries < 20 && (name.is_empty() || first_name_taken(ctx.state, &name)) {
        let first = match pronouns {
            Pronouns::SheHer => ctx.pick(&FIRST_NAMES_SHE),
            Pronouns::HeHim => ctx.pick(&FIRST_NAMES_HE),
            Pronouns::TheyThem => ctx.pick(&FIRST_NAMES_THEY),
        };
        name = format!("{first} {}", ctx.pick(&LAST_NAMES));
        tries += 1;
    }
    let mut traits: Vec<String> = Vec::new();
    while traits.len() < 3 {
        let t = ctx.pick(&TRAITS);
        if !traits.iter().any(|existing| existing == t) {
            traits.push(t.to_owned());
        }
    }
    let mut goals: Vec<String> = Vec::new();
    let wanted = ctx.int_range(2, 3) as usize;
    while goals.len() < wanted {
        let g = ctx.pick(&GOALS);
        if !goals.iter().any(|existing| existing == g) {
            goals.push(g.to_owned());
        }
    }
    let class_index = ClassId::ALL.iter().position(|c| *c == class_id).unwrap_or(0);
    let occupation = ctx.pick(&OCCUPATIONS[class_index]).to_owned();
    let backstory = format!("{} {} {}", ctx.pick(&ORIGINS), ctx.pick(&TURNS), ctx.pick(&NOWS));
    let speech_style = ctx.pick(&SPEECH_STYLES).to_owned();
    let timbre = ctx.pick(&TIMBRES).to_owned();
    let age = ctx.int_range(19, 58);
    let temper = clamp(ctx.next_float(), 0.15, 0.85);
    let bravery = clamp(ctx.next_float(), 0.25, 0.9);
    let sociability = clamp(ctx.next_float(), 0.25, 0.9);
    let quirks = if ctx.chance(0.2) { vec![ctx.pick(&ALLOWED_QUIRKS)] } else { Vec::new() };
    let secret = if ctx.chance(0.15) { ctx.pick(&SECRETS).to_owned() } else { String::new() };
    let allure = if ctx.chance(0.08) { 0.5 } else { 0.0 };
    let shyness = if ctx.chance(0.08) { 0.6 } else { 0.0 };
    let looks = if ctx.chance(0.08) { 0.5 } else { 0.0 };
    let reason = if founder { String::new() } else { ctx.pick(&REASONS).to_owned() };
    let mut bonds = Vec::new();
    if founder && !ctx.state.order.is_empty() && ctx.chance(0.3) {
        let index = ctx.pick_index(ctx.state.order.len());
        let other = ctx.state.characters[&ctx.state.order[index]].profile.name.clone();
        let relation =
            ctx.pick(&[BondKind::Sibling, BondKind::Friend, BondKind::Friend, BondKind::Comrade, BondKind::Rival]);
        let summary = match relation {
            BondKind::Sibling => ctx.pick(&SIBLING_SUMMARIES),
            BondKind::Comrade => ctx.pick(&COMRADE_SUMMARIES),
            BondKind::Rival => ctx.pick(&RIVAL_SUMMARIES),
            _ => ctx.pick(&FRIEND_SUMMARIES),
        };
        bonds.push(Bond { to: other, relation, summary: summary.to_owned() });
    }
    NewVillager {
        class_id,
        name,
        pronouns,
        age,
        occupation,
        backstory,
        traits,
        goals,
        speech_style,
        timbre,
        temper,
        bravery,
        sociability,
        spite: 0.0,
        allure,
        shyness,
        looks,
        secret,
        reason,
        quirks,
        bonds,
        ambition: None,
    }
}
