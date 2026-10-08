//! The stream encoder (port of the reference server's `tools/ref-server/encode.ts`
//! at the `ts-sim-reference` tag): what each entity looks like on the wire, how a tick's changes
//! become a `tick` frame, and which sim events viewers get.
//!
//! - Ids: villagers and monsters keep their sim ids; a corpse is
//!   `corpse:<id of the dead>` (it appears in the tick its villager leaves).
//! - Order: villagers in `state.order`, monsters in insertion order, corpses in
//!   array order; `add`, `del` and `d` follow it.
//! - Quantising: positions and facing ×100 and hit points, rounded the
//!   JavaScript way; diffs compare quantised values, so an idle entity sends nothing.
//! - "At ease": weapons are put away while sitting or lying, and a work tool
//!   replaces the main-hand weapon and empties the off hand.

use fsw_core::jsmath::js_round;
use fsw_core::time::TICK_SECONDS;
use fsw_core::time::{format_clock, game_time};
use fsw_sim::data::anims::AnimId;
use fsw_sim::events::{Emote as SimEmote, Projectile, SimEvent};
use fsw_sim::state::BoardTone;
use fsw_sim::state::{
    ActPhase, CeremonyPhase as SimCeremonyPhase, Character, ChronicleEntry as SimChronicleEntry,
    ChronicleKind as SimChronicleKind, Corpse, Grave as SimGrave, HousePhase as SimHousePhase, IntentAction,
    Look as SimLook, Monster, Resource, Rite, SimState, VoiceProfile as SimVoice, WorldObject as SimWorldObject,
};
use fsw_sim::systems::billboards::shown;
use fsw_sim::systems::combat::max_hp;
use fsw_sim::systems::fire::built_fraction;
use fsw_sim::systems::world::{object_name, progress_of};
use fsw_sim::util::{first_name, name_of};
use fsw_world::places::{billboards, find_place, get_place};
use indexmap::IndexMap;

use crate::api::wire::{
    Billboard, BillboardTone, Ceremony, CeremonyKind, CeremonyPhase, ChronicleEntry, ChronicleKind, Dyn, DynDelta,
    Emote, Entity, EntityKind, EntityStatic, Grave, House, HousePhase, Look, Placed, Ranged, Stockpile, TickFrame,
    Utterance, Vec2, VoiceProfile, WireEvent, WorldObject, WorldObjectKind,
};

pub fn corpse_id(id: &str) -> String {
    format!("corpse:{id}")
}

/// ×100, rounded the JavaScript way.
fn centi(value: f64) -> i32 {
    js_round(value * 100.0) as i32
}

fn point(v: fsw_core::Vec2) -> Vec2 {
    Vec2 { x: f64::from(centi(v.x)) / 100.0, z: f64::from(centi(v.z)) / 100.0 }
}

fn look(look: &SimLook) -> Look {
    Look {
        model: look.model.clone(),
        texture: look.texture.clone(),
        hide: look.hide.clone(),
        main_hand: look.main_hand.clone(),
        off_hand: look.off_hand.clone(),
    }
}

pub fn voice(voice: &SimVoice) -> VoiceProfile {
    VoiceProfile { pitch: voice.pitch, rate: voice.rate, timbre: voice.timbre.clone() }
}

pub fn character_static(c: &Character) -> EntityStatic {
    EntityStatic {
        id: c.id.clone(),
        kind: EntityKind::Villager,
        name: first_name(c).to_owned(),
        look: look(&c.look),
        max_hp: max_hp(c),
        hostile: false,
        voice: Some(voice(&c.profile.voice)),
    }
}

fn at_ease(anim: AnimId) -> bool {
    matches!(anim, AnimId::SitChair | AnimId::SitFloor | AnimId::Lie)
}

pub fn character_dyn(c: &Character) -> Dyn {
    let ease = at_ease(c.anim);
    let act = match &c.intent {
        Some(intent) if c.act.phase == ActPhase::Perform => match intent.action {
            IntentAction::Activity { activity, .. } => Some(activity.as_str().to_owned()),
            IntentAction::WorldWork { job, .. } => Some(job.as_str().to_owned()),
            _ => None,
        },
        _ => None,
    };
    Dyn {
        p: [centi(c.pos.x), centi(c.pos.z)],
        f: centi(c.facing),
        a: (c.anim.as_str().to_owned(), c.anim_seq as u32),
        hp: js_round(c.hp) as i32,
        v: c.inside.is_none(),
        mh: c.tool.clone().or_else(|| if ease { None } else { c.look.main_hand.clone() }),
        oh: if c.tool.as_deref().is_some_and(|t| !t.is_empty()) || ease { None } else { c.look.off_hand.clone() },
        act,
        z: c.asleep,
    }
}

pub fn monster_static(m: &Monster) -> EntityStatic {
    let def = m.kind.def();
    EntityStatic {
        id: m.id.clone(),
        kind: EntityKind::Monster,
        // A villager who turned keeps their name.
        name: m.name.clone().unwrap_or_else(|| def.label.into()),
        look: look(&m.look),
        max_hp: def.max_hp,
        hostile: true,
        voice: None,
    }
}

pub fn monster_dyn(m: &Monster) -> Dyn {
    Dyn {
        p: [centi(m.pos.x), centi(m.pos.z)],
        f: centi(m.facing),
        a: (m.anim.as_str().to_owned(), m.anim_seq as u32),
        hp: js_round(m.hp.max(0.0)) as i32,
        v: true,
        mh: m.look.main_hand.clone(),
        oh: m.look.off_hand.clone(),
        act: None,
        z: false,
    }
}

/// A body has no name (so no label), no health and never moves.
pub fn corpse_static(corpse: &Corpse) -> EntityStatic {
    EntityStatic {
        id: corpse_id(&corpse.id),
        kind: EntityKind::Corpse,
        name: String::new(),
        look: look(&corpse.look),
        max_hp: 1.0,
        hostile: false,
        voice: None,
    }
}

pub fn corpse_dyn(corpse: &Corpse) -> Dyn {
    Dyn {
        p: [centi(corpse.pos.x), centi(corpse.pos.z)],
        f: centi(corpse.facing),
        a: (corpse.anim.as_str().to_owned(), 0),
        hp: 0,
        v: true,
        mh: corpse.look.main_hand.clone(),
        oh: corpse.look.off_hand.clone(),
        act: None,
        z: false,
    }
}

/// Every entity a viewer draws, in wire order.
pub fn entities_of(state: &SimState) -> Vec<Entity> {
    let mut result = Vec::new();
    for id in &state.order {
        if let Some(c) = state.characters.get(id) {
            result.push(Entity { fixed: character_static(c), state: character_dyn(c) });
        }
    }
    for m in state.monsters.values() {
        result.push(Entity { fixed: monster_static(m), state: monster_dyn(m) });
    }
    for corpse in &state.corpses {
        result.push(Entity { fixed: corpse_static(corpse), state: corpse_dyn(corpse) });
    }
    result
}

/// The fields of `next` that differ from `prev`, or `None` when nothing did.
pub fn diff_dyn(id: &str, prev: &Dyn, next: &Dyn) -> Option<DynDelta> {
    let mut delta = DynDelta { id: id.to_owned(), ..Default::default() };
    let mut changed = false;
    macro_rules! field {
        ($name:ident) => {
            if prev.$name != next.$name {
                delta.$name = Some(next.$name.clone());
                changed = true;
            }
        };
    }
    field!(p);
    field!(f);
    field!(a);
    field!(hp);
    field!(v);
    field!(mh);
    field!(oh);
    field!(act);
    field!(z);
    changed.then_some(delta)
}

/// Applies a delta to a dyn (what the client does; used by the tests).
pub fn apply_delta(dyn_: &Dyn, delta: &DynDelta) -> Dyn {
    let mut next = dyn_.clone();
    macro_rules! field {
        ($name:ident) => {
            if let Some(value) = &delta.$name {
                next.$name = value.clone();
            }
        };
    }
    field!(p);
    field!(f);
    field!(a);
    field!(hp);
    field!(v);
    field!(mh);
    field!(oh);
    field!(act);
    field!(z);
    next
}

fn grave_key(grave: &SimGrave) -> String {
    format!("{}:{}", grave.plot, grave.character_id)
}

pub fn chronicle_kind(kind: SimChronicleKind) -> ChronicleKind {
    match kind {
        SimChronicleKind::World => ChronicleKind::World,
        SimChronicleKind::Social => ChronicleKind::Social,
        SimChronicleKind::Romance => ChronicleKind::Romance,
        SimChronicleKind::Conflict => ChronicleKind::Conflict,
        SimChronicleKind::Combat => ChronicleKind::Combat,
        SimChronicleKind::Death => ChronicleKind::Death,
        SimChronicleKind::Arrival => ChronicleKind::Arrival,
        SimChronicleKind::Fire => ChronicleKind::Fire,
    }
}

pub fn chronicle_entry(entry: &SimChronicleEntry) -> ChronicleEntry {
    ChronicleEntry {
        tick: entry.tick.max(0) as u64,
        kind: chronicle_kind(entry.kind),
        text: entry.text.clone(),
        ids: entry.ids.clone(),
    }
}

pub fn grave_of(grave: &SimGrave) -> Grave {
    Grave {
        plot: grave.plot as u32,
        character_id: grave.character_id.clone(),
        name: grave.name.clone(),
        day: grave.day as u32,
        cause: grave.cause.clone(),
    }
}

/// The homes that are burning or in ruins, as viewers are told of them.
/// The ceremony the village has arranged, as viewers are told of it.
pub fn ceremony_of(state: &SimState) -> Option<Ceremony> {
    let ceremony = state.ceremony.as_ref()?;
    let (kind, ids) = match &ceremony.rite {
        Rite::Wedding { a, b } => (CeremonyKind::Wedding, vec![a.clone(), b.clone()]),
        Rite::Funeral { dead } => (CeremonyKind::Funeral, dead.clone()),
    };
    let when = game_time(ceremony.start_tick);
    Some(Ceremony {
        id: ceremony.id.clone(),
        kind,
        phase: match ceremony.phase {
            SimCeremonyPhase::Announced => CeremonyPhase::Announced,
            SimCeremonyPhase::Gathering => CeremonyPhase::Gathering,
            SimCeremonyPhase::Rite => CeremonyPhase::Rite,
            SimCeremonyPhase::Feast => CeremonyPhase::Feast,
        },
        place_id: ceremony.place_id.clone(),
        place_name: find_place(&ceremony.place_id).map_or(ceremony.place_id.clone(), |place| place.name.into()),
        names: ids.iter().map(|id| name_of(state, id)).collect(),
        ids,
        day: when.day.max(0) as u32,
        time: format_clock(when.minute_of_day),
    })
}

pub fn houses_of(state: &SimState) -> Vec<House> {
    state
        .houses
        .iter()
        .map(|(id, house)| House {
            id: id.clone(),
            name: find_place(id).map_or(id.clone(), |place| place.name.into()),
            phase: match house.phase {
                SimHousePhase::Burning => HousePhase::Burning,
                SimHousePhase::Ruined => HousePhase::Ruin,
            },
            // Coarse steps, so a fire is a handful of events rather than one a second.
            fire: (js_round(house.fire.clamp(0.0, 100.0) / 5.0) * 5.0) as u8,
            built: (built_fraction(house) * 100.0).floor() as u8,
        })
        .collect()
}

/// Everything that differs from the map, as viewers are told of it.
pub fn objects_of(state: &SimState) -> Vec<WorldObject> {
    state.objects.iter().map(|(id, object)| wire_object(id, object)).collect()
}

fn wire_object(id: &str, object: &SimWorldObject) -> WorldObject {
    let kind = match object {
        SimWorldObject::Tree { .. } => WorldObjectKind::Tree,
        SimWorldObject::Rock { .. } => WorldObjectKind::Rock,
        SimWorldObject::Prop { .. } => WorldObjectKind::Prop,
        SimWorldObject::Building { .. } => WorldObjectKind::Building,
        SimWorldObject::Ground { .. } => WorldObjectKind::Ground,
        SimWorldObject::Built { .. } => WorldObjectKind::Built,
    };
    let placed = match object {
        SimWorldObject::Built { spec, .. } => Some(Placed {
            bundle: spec.bundle.clone(),
            model: spec.model.clone(),
            x: spec.x,
            z: spec.z,
            rotation: spec.rotation,
            scale: spec.scale,
        }),
        _ => None,
    };
    let (stage, since_tick, progress, fire) = object_facts(object);
    WorldObject {
        id: id.to_owned(),
        kind,
        name: object_name(id, object),
        stage,
        since_tick: since_tick.max(0) as u64,
        progress,
        fire,
        placed,
    }
}

/// An object's stage, the tick it began, and its work and fire in percent, as viewers are told them: the
/// percentages in coarse steps, so a shift of work is a handful of events rather than one a minute.
pub fn object_facts(object: &SimWorldObject) -> (String, i64, u8, u8) {
    (
        object.stage_name().to_owned(),
        object.since_tick(),
        (js_round(progress_of(object) * 20.0) * 5.0) as u8,
        (js_round(object.fire().clamp(0.0, 100.0) / 5.0) * 5.0) as u8,
    )
}

/// The stockpiles that hold anything.
pub fn stockpiles_of(state: &SimState) -> Vec<Stockpile> {
    state
        .stockpiles
        .iter()
        .filter(|(_, store)| !store.is_empty())
        .map(|(place, store)| Stockpile {
            place: place.clone(),
            name: find_place(place).map_or(place.clone(), |p| p.name.into()),
            wood: store.get(Resource::Wood).max(0) as u32,
            stone: store.get(Resource::Stone).max(0) as u32,
            ore: store.get(Resource::Ore).max(0) as u32,
        })
        .collect()
}

pub fn billboard_tone(tone: BoardTone) -> BillboardTone {
    match tone {
        BoardTone::Uplifting => BillboardTone::Uplifting,
        BoardTone::Absurd => BillboardTone::Absurd,
        BoardTone::Pushy => BillboardTone::Pushy,
        BoardTone::Ominous => BillboardTone::Ominous,
        BoardTone::Wholesome => BillboardTone::Wholesome,
    }
}

pub fn sim_tone(tone: BillboardTone) -> BoardTone {
    match tone {
        BillboardTone::Uplifting => BoardTone::Uplifting,
        BillboardTone::Absurd => BoardTone::Absurd,
        BillboardTone::Pushy => BoardTone::Pushy,
        BillboardTone::Ominous => BoardTone::Ominous,
        BillboardTone::Wholesome => BoardTone::Wholesome,
    }
}

/// What every billboard shows, in map order.
pub fn billboards_of(state: &SimState) -> Vec<Billboard> {
    billboards()
        .iter()
        .enumerate()
        .map(|(index, board)| {
            let shown = shown(state, index);
            Billboard {
                id: board.id.clone(),
                place: get_place(&board.place_id).name.to_owned(),
                text: shown.text,
                tone: billboard_tone(shown.tone),
                custom: shown.custom,
            }
        })
        .collect()
}

fn emote(emote: SimEmote) -> Emote {
    match emote {
        SimEmote::Heart => Emote::Heart,
        SimEmote::Anger => Emote::Anger,
        SimEmote::Alert => Emote::Alert,
        SimEmote::Thinking => Emote::Thinking,
        SimEmote::Sleep => Emote::Sleep,
        SimEmote::Grief => Emote::Grief,
        SimEmote::Laugh => Emote::Laugh,
        SimEmote::BrokenHeart => Emote::BrokenHeart,
        SimEmote::Gift => Emote::Gift,
    }
}

/// A spoken line with everything a sink needs. A line from someone who died in
/// the same tick is dropped; a listener who is gone is left out.
pub fn utterance_of(
    state: &SimState,
    speaker_id: &str,
    to_id: Option<&str>,
    text: &str,
    duration_ticks: i64,
    loud: bool,
) -> Option<Utterance> {
    let speaker = state.characters.get(speaker_id)?;
    let listener = to_id.and_then(|id| state.characters.get(id));
    Some(Utterance {
        tick: state.tick.max(0) as u64,
        speaker_id: speaker.id.clone(),
        speaker_name: first_name(speaker).to_owned(),
        to_id: listener.map(|l| l.id.clone()),
        to_name: listener.map(|l| first_name(l).to_owned()),
        text: text.to_owned(),
        loud,
        duration_ms: js_round(duration_ticks as f64 * TICK_SECONDS * 1000.0) as u32,
        voice: voice(&speaker.profile.voice),
        position: point(speaker.pos),
    })
}

/// The sim's one-shot events that viewers see; the rest (conversations, mercy, save points…) stay on the server.
pub fn wire_events(state: &SimState, events: &[SimEvent]) -> Vec<WireEvent> {
    let mut result = Vec::new();
    for event in events {
        match event {
            SimEvent::Speech { speaker_id, to_id, text, duration_ticks, loud } => {
                if let Some(u) = utterance_of(state, speaker_id, to_id.as_deref(), text, *duration_ticks, *loud) {
                    result.push(WireEvent::Speech(u));
                }
            }
            SimEvent::Emote { id, emote: e } => result.push(WireEvent::Emote { id: id.clone(), emote: emote(*e) }),
            SimEvent::Attack { attacker_id, target_id, ranged, from, to } => result.push(WireEvent::Attack {
                attacker_id: attacker_id.clone(),
                target_id: target_id.clone(),
                ranged: ranged.map(|r| match r {
                    Projectile::Arrow => Ranged::Arrow,
                    Projectile::Bolt => Ranged::Bolt,
                    Projectile::Magic => Ranged::Magic,
                    Projectile::Food => Ranged::Food,
                }),
                from: point(*from),
                to: point(*to),
            }),
            SimEvent::Hit { attacker_id, target_id, damage, pos, fatal } => result.push(WireEvent::Hit {
                attacker_id: attacker_id.clone(),
                target_id: target_id.clone(),
                damage: js_round(damage * 10.0) / 10.0,
                pos: point(*pos),
                fatal: *fatal,
            }),
            SimEvent::Death { id, name, pos, monster } => result.push(WireEvent::Death {
                id: id.clone(),
                name: name.clone(),
                pos: point(*pos),
                monster: *monster,
            }),
            SimEvent::MonsterSpawn { id, pos } => {
                result.push(WireEvent::MonsterSpawn { id: id.clone(), pos: point(*pos) })
            }
            SimEvent::Chronicle { entry } => result.push(WireEvent::Chronicle { entry: chronicle_entry(entry) }),
            _ => {}
        }
    }
    result
}

/// Remembers what every viewer has been told and turns the next tick into a
/// frame of differences. One tracker serves all viewers: a new viewer gets
/// [`Tracker::entities`] as its snapshot, exactly what the next frame differs from.
#[derive(Default)]
pub struct Tracker {
    known: IndexMap<String, Entity>,
    graves: Vec<String>,
    billboards: Vec<Billboard>,
    houses: Vec<House>,
    ceremony: Option<Ceremony>,
    objects: Vec<WorldObject>,
    stockpiles: Vec<Stockpile>,
}

impl Tracker {
    pub fn new(state: &SimState) -> Self {
        let mut tracker = Self::default();
        tracker.reset(state);
        tracker
    }

    pub fn reset(&mut self, state: &SimState) {
        self.known = entities_of(state).into_iter().map(|e| (e.fixed.id.clone(), e)).collect();
        self.graves = state.graves.iter().map(grave_key).collect();
        self.billboards = billboards_of(state);
        self.houses = houses_of(state);
        self.ceremony = ceremony_of(state);
        self.objects = objects_of(state);
        self.stockpiles = stockpiles_of(state);
    }

    /// What viewers currently know, in wire order (for a snapshot).
    pub fn entities(&self) -> Vec<Entity> {
        self.known.values().cloned().collect()
    }

    /// What viewers have been told every billboard shows (for a snapshot).
    pub fn billboards(&self) -> Vec<Billboard> {
        self.billboards.clone()
    }

    /// What viewers have been told of the homes that do not stand whole (for a snapshot).
    pub fn houses(&self) -> Vec<House> {
        self.houses.clone()
    }

    /// What viewers have been told of the ceremony the village has arranged (for a snapshot).
    pub fn ceremony(&self) -> Option<Ceremony> {
        self.ceremony.clone()
    }

    /// What viewers have been told differs from the map (for a snapshot).
    pub fn world_objects(&self) -> Vec<WorldObject> {
        self.objects.clone()
    }

    /// What viewers have been told the stockpiles hold (for a snapshot).
    pub fn stockpiles(&self) -> Vec<Stockpile> {
        self.stockpiles.clone()
    }

    /// The frame for the tick the sim has just run; `events` are what it returned.
    pub fn frame(&mut self, state: &SimState, events: &[SimEvent]) -> TickFrame {
        let mut frame = TickFrame { t: state.tick.max(0) as u64, ..Default::default() };
        let mut next: IndexMap<String, Entity> = IndexMap::with_capacity(self.known.len());
        for entity in entities_of(state) {
            let id = entity.fixed.id.clone();
            match self.known.get(&id) {
                None => {
                    frame.add.push(entity.clone());
                    next.insert(id, entity);
                }
                // The static part changes only when a child comes of age (a new look and more hit points). A
                // viewer learns of it as a removal now and an addition next frame, never both in one frame: the
                // client applies additions before removals.
                Some(before) if before.fixed != entity.fixed => {}
                Some(before) => {
                    if let Some(delta) = diff_dyn(&id, &before.state, &entity.state) {
                        frame.d.push(delta);
                    }
                    next.insert(id, Entity { fixed: before.fixed.clone(), state: entity.state });
                }
            }
        }
        frame.del = self.known.keys().filter(|id| !next.contains_key(*id)).cloned().collect();
        self.known = next;

        frame.ev = wire_events(state, events);
        // New graves are not a sim event; they show up as a change in state.graves.
        for grave in &state.graves {
            let key = grave_key(grave);
            if !self.graves.contains(&key) {
                self.graves.push(key);
                frame.ev.push(WireEvent::Grave { grave: grave_of(grave) });
            }
        }
        self.graves.retain(|key| state.graves.iter().any(|grave| &grave_key(grave) == key));
        // Nor is a billboard changing: the day's slogan turns over at midnight, and admins change notices.
        for (now, before) in billboards_of(state).into_iter().zip(self.billboards.iter_mut()) {
            if now != *before {
                *before = now.clone();
                frame.ev.push(WireEvent::Billboard { billboard: now });
            }
        }
        // Nor is a house catching fire, burning on, falling in ruins or being rebuilt.
        if !(state.houses.is_empty() && self.houses.is_empty()) {
            let now = houses_of(state);
            for house in &now {
                if !self.houses.contains(house) {
                    frame.ev.push(WireEvent::House { house: house.clone() });
                }
            }
            for before in &self.houses {
                if !now.iter().any(|house| house.id == before.id) {
                    let whole = House { phase: HousePhase::Intact, fire: 0, built: 100, ..before.clone() };
                    frame.ev.push(WireEvent::House { house: whole });
                }
            }
            self.houses = now;
        }
        // Nor a wedding or funeral being arranged, gathering, spoken, feasted and done with.
        if !(state.ceremony.is_none() && self.ceremony.is_none()) {
            let now = ceremony_of(state);
            if now != self.ceremony {
                let told = match (&now, &self.ceremony) {
                    (Some(now), _) => now.clone(),
                    (None, Some(before)) => Ceremony { phase: CeremonyPhase::Over, ..before.clone() },
                    (None, None) => unreachable!("one of them is Some"),
                };
                frame.ev.push(WireEvent::Ceremony { ceremony: told });
                self.ceremony = now;
            }
        }
        // Nor is a tree coming down, a rock mined out, or anything else done to the map.
        if !(state.objects.is_empty() && self.objects.is_empty()) {
            let now = objects_of(state);
            for object in &now {
                if !self.objects.contains(object) {
                    frame.ev.push(WireEvent::WorldObject { object: object.clone() });
                }
            }
            for before in &self.objects {
                if !now.iter().any(|object| object.id == before.id) {
                    let whole = WorldObject { stage: "intact".into(), progress: 0, fire: 0, ..before.clone() };
                    frame.ev.push(WireEvent::WorldObject { object: whole });
                }
            }
            self.objects = now;
        }
        // Nor is a stockpile filling or emptying.
        if !(state.stockpiles.is_empty() && self.stockpiles.is_empty()) {
            let now = stockpiles_of(state);
            for pile in &now {
                if !self.stockpiles.contains(pile) {
                    frame.ev.push(WireEvent::Stockpile { stockpile: pile.clone() });
                }
            }
            for before in &self.stockpiles {
                if !now.iter().any(|pile| pile.place == before.place) {
                    let empty = Stockpile { wood: 0, stone: 0, ore: 0, ..before.clone() };
                    frame.ev.push(WireEvent::Stockpile { stockpile: empty });
                }
            }
            self.stockpiles = now;
        }
        frame
    }
}

/// Seed and length of the frames in `tests/fixtures/server-frames.json`.
pub const FRAMES_FIXTURE: (u32, usize) = (99, 600);

/// What the web client's tests replay through its view of the world: the
/// snapshot of a fresh, empty world, the frames of its first ticks (the first of which founds the
/// village from the sim's own lists), and the entities at the end (JSON; `cargo run --bin
/// export-frames` writes it).
pub fn frames_fixture() -> String {
    let (seed, ticks) = FRAMES_FIXTURE;
    let mut sim = fsw_sim::Sim::new(fsw_sim::create_state(seed));
    sim.enqueue(fsw_sim::Command::FoundVillage);
    let mut tracker = Tracker::new(&sim.state);
    let start = sim.state.tick;
    let entities = tracker.entities();
    let frames: Vec<TickFrame> = (0..ticks)
        .map(|_| {
            let events = sim.step();
            tracker.frame(&sim.state, &events)
        })
        .collect();
    let fixture = serde_json::json!({
        "seed": seed,
        "tick": start,
        "entities": entities,
        "frames": frames,
        "expected": entities_of(&sim.state),
    });
    let mut out = serde_json::to_string(&fixture).expect("frames serialise");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsw_sim::Sim;
    use fsw_testing::create_test_state;

    #[test]
    fn deltas_rebuild_the_state_a_snapshot_would_show() {
        let mut sim = Sim::new(create_test_state(3));
        let mut tracker = Tracker::new(&sim.state);
        let mut client: IndexMap<String, Entity> =
            tracker.entities().into_iter().map(|e| (e.fixed.id.clone(), e)).collect();
        let mut sizes = 0usize;
        for _ in 0..3000 {
            let events = sim.step();
            let frame = tracker.frame(&sim.state, &events);
            sizes += serde_json::to_string(&frame).unwrap().len();
            for id in &frame.del {
                client.shift_remove(id);
            }
            for entity in &frame.add {
                client.insert(entity.fixed.id.clone(), entity.clone());
            }
            for delta in &frame.d {
                let entity = client.get_mut(&delta.id).expect("a delta for a known entity");
                entity.state = apply_delta(&entity.state, delta);
            }
        }
        let fresh: Vec<Entity> = entities_of(&sim.state);
        let rebuilt: Vec<Entity> = client.into_values().collect();
        assert_eq!(rebuilt.len(), fresh.len());
        for (a, b) in rebuilt.iter().zip(&fresh) {
            assert_eq!(a.fixed.id, b.fixed.id);
            assert_eq!(a.state, b.state);
        }
        // By day, about 4 KB/s at 20 ticks a second.
        assert!(sizes / 150 < 8000, "{} bytes per second", sizes / 150);
    }

    #[test]
    fn a_changed_look_is_sent_as_a_removal_then_an_addition() {
        let mut sim = Sim::new(create_test_state(1));
        let mut tracker = Tracker::new(&sim.state);
        let pip = sim.state.characters.values().find(|c| c.profile.name == "Pip Vane").unwrap().id.clone();
        sim.step();
        tracker.frame(&sim.state, &[]);
        // Growing up changes the look and the hit points: the static part viewers were sent is stale.
        let c = sim.state.characters.get_mut(&pip).unwrap();
        c.child = None;
        c.look.model = "Knight".into();
        c.hp = 120.0;
        let first = tracker.frame(&sim.state, &[]);
        assert!(first.del.contains(&pip), "{:?}", first.del);
        assert!(!first.add.iter().any(|e| e.fixed.id == pip), "del and add never share a frame");
        let second = tracker.frame(&sim.state, &[]);
        let added = second.add.iter().find(|e| e.fixed.id == pip).expect("re-added with the new look");
        assert_eq!(added.fixed.look.model, "Knight");
        assert_eq!(added.fixed.max_hp, 120.0);
        assert!(!second.del.contains(&pip));
        let third = tracker.frame(&sim.state, &[]);
        assert!(third.add.is_empty() && third.del.is_empty());
    }

    #[test]
    fn idle_ticks_send_nothing() {
        let sim = Sim::new(create_test_state(3));
        let mut tracker = Tracker::new(&sim.state);
        let frame = tracker.frame(&sim.state, &[]);
        assert!(frame.add.is_empty() && frame.d.is_empty() && frame.del.is_empty());
    }

    #[test]
    fn billboards_are_announced_when_they_change() {
        let mut sim = Sim::new(create_test_state(3));
        let mut tracker = Tracker::new(&sim.state);
        let count = |frame: &TickFrame| frame.ev.iter().filter(|e| matches!(e, WireEvent::Billboard { .. })).count();
        // An ordinary tick, then the next midnight, when every board turns to a new slogan.
        assert_eq!(count(&tracker.frame(&sim.state, &[])), 0);
        sim.state.tick += fsw_core::time::TICKS_PER_DAY;
        assert_eq!(count(&tracker.frame(&sim.state, &[])), billboards().len());
        assert_eq!(count(&tracker.frame(&sim.state, &[])), 0);

        sim.enqueue(fsw_sim::Command::SetBillboard {
            board_id: "market_board".into(),
            notice: Some(fsw_sim::state::BoardNotice { text: "Cheese!".into(), tone: BoardTone::Absurd }),
        });
        let events = sim.step();
        let frame = tracker.frame(&sim.state, &events);
        assert_eq!(count(&frame), 1);
        assert!(frame.ev.iter().any(
            |e| matches!(e, WireEvent::Billboard { billboard } if billboard.text == "Cheese!" && billboard.custom)
        ));
        assert_eq!(tracker.billboards(), billboards_of(&sim.state));
    }

    #[test]
    fn houses_are_announced_as_they_burn_fall_and_rise_again() {
        use fsw_sim::state::{House as SimHouse, HousePhase as SimPhase};
        let mut sim = Sim::new(create_test_state(3));
        let mut tracker = Tracker::new(&sim.state);
        let houses = |frame: &TickFrame| -> Vec<House> {
            frame
                .ev
                .iter()
                .filter_map(|e| if let WireEvent::House { house } = e { Some(house.clone()) } else { None })
                .collect()
        };
        assert!(houses(&tracker.frame(&sim.state, &[])).is_empty());

        let burning = SimHouse { phase: SimPhase::Burning, fire: 21.0, built: 0.0, since_tick: 0, helpers: Vec::new() };
        sim.state.houses.insert("home_1".into(), burning);
        let told = houses(&tracker.frame(&sim.state, &[]));
        assert_eq!(told.len(), 1);
        assert_eq!((told[0].id.as_str(), told[0].phase, told[0].fire), ("home_1", HousePhase::Burning, 20));
        assert_eq!(told[0].name, "the red cottage");
        // A little more fire is not news; a step of five is.
        sim.state.houses["home_1"].fire = 22.0;
        assert!(houses(&tracker.frame(&sim.state, &[])).is_empty());
        sim.state.houses["home_1"].fire = 23.0;
        assert_eq!(houses(&tracker.frame(&sim.state, &[]))[0].fire, 25);

        let house = &mut sim.state.houses["home_1"];
        house.phase = SimPhase::Ruined;
        house.fire = 0.0;
        house.built = fsw_sim::systems::fire::REBUILD_WORK / 2.0;
        let told = houses(&tracker.frame(&sim.state, &[]));
        assert_eq!((told[0].phase, told[0].built), (HousePhase::Ruin, 50));
        assert_eq!(tracker.houses(), houses_of(&sim.state));

        sim.state.houses.clear();
        let told = houses(&tracker.frame(&sim.state, &[]));
        assert_eq!((told[0].id.as_str(), told[0].phase), ("home_1", HousePhase::Intact));
        assert!(tracker.houses().is_empty());
        assert!(houses(&tracker.frame(&sim.state, &[])).is_empty());
    }

    #[test]
    fn a_ceremony_is_announced_as_it_is_arranged_moves_on_and_ends() {
        use fsw_sim::state::{Ceremony as SimCeremony, CeremonyPhase as SimPhase, Rite};
        let mut sim = Sim::new(fsw_testing::create_test_state(3));
        let mut tracker = Tracker::new(&sim.state);
        let ceremonies = |frame: &TickFrame| -> Vec<Ceremony> {
            frame
                .ev
                .iter()
                .filter_map(|e| if let WireEvent::Ceremony { ceremony } = e { Some(ceremony.clone()) } else { None })
                .collect()
        };
        assert!(ceremonies(&tracker.frame(&sim.state, &[])).is_empty());
        assert!(tracker.ceremony().is_none());

        let (a, b) = (sim.state.order[0].clone(), sim.state.order[1].clone());
        sim.state.ceremony = Some(SimCeremony {
            id: "y1".into(),
            rite: Rite::Wedding { a: a.clone(), b: b.clone() },
            place_id: "green".into(),
            start_tick: fsw_sim::systems::director::tick_at(2, 14.0),
            phase: SimPhase::Announced,
            step: 0,
            until_tick: 0,
            officiant: sim.state.order[2].clone(),
            speeches: Vec::new(),
            attendees: Vec::new(),
            slips: 0,
        });
        let told = ceremonies(&tracker.frame(&sim.state, &[]));
        assert_eq!(told.len(), 1);
        assert_eq!((told[0].kind, told[0].phase, told[0].day), (CeremonyKind::Wedding, CeremonyPhase::Announced, 2));
        assert_eq!(told[0].time, "14:00");
        assert_eq!(told[0].place_name, "the village green");
        assert_eq!(told[0].ids, vec![a, b]);
        assert_eq!(told[0].names.len(), 2);
        assert_eq!(tracker.ceremony(), Some(told[0].clone()));
        // Nothing changed: nothing said.
        assert!(ceremonies(&tracker.frame(&sim.state, &[])).is_empty());

        sim.state.ceremony.as_mut().unwrap().phase = SimPhase::Gathering;
        let told = ceremonies(&tracker.frame(&sim.state, &[]));
        assert_eq!(told[0].phase, CeremonyPhase::Gathering);

        sim.state.ceremony = None;
        let told = ceremonies(&tracker.frame(&sim.state, &[]));
        assert_eq!((told[0].id.as_str(), told[0].phase), ("y1", CeremonyPhase::Over));
        assert!(tracker.ceremony().is_none());
        assert!(ceremonies(&tracker.frame(&sim.state, &[])).is_empty());
    }

    #[test]
    fn world_objects_and_stockpiles_are_announced_as_they_change() {
        use fsw_sim::state::{Resource, RockStage, TreeStage, WorldObject as SimObject};
        let mut sim = Sim::new(create_test_state(3));
        let mut tracker = Tracker::new(&sim.state);
        let objects = |frame: &TickFrame| -> Vec<WorldObject> {
            frame
                .ev
                .iter()
                .filter_map(|e| if let WireEvent::WorldObject { object } = e { Some(object.clone()) } else { None })
                .collect()
        };
        let piles = |frame: &TickFrame| -> Vec<Stockpile> {
            frame
                .ev
                .iter()
                .filter_map(|e| if let WireEvent::Stockpile { stockpile } = e { Some(stockpile.clone()) } else { None })
                .collect()
        };
        let frame = tracker.frame(&sim.state, &[]);
        assert!(objects(&frame).is_empty() && piles(&frame).is_empty());

        // The first swings at a tree: progress in steps of five, so a minute's work is not always news.
        sim.state
            .objects
            .insert("s:7".into(), SimObject::Tree { stage: TreeStage::Standing, since_tick: 40, work: 1.0 });
        let told = objects(&tracker.frame(&sim.state, &[]));
        assert_eq!(told.len(), 1);
        assert_eq!(
            (told[0].id.as_str(), told[0].kind, told[0].stage.as_str()),
            ("s:7", WorldObjectKind::Tree, "standing")
        );
        assert_eq!((told[0].since_tick, told[0].progress, told[0].fire, told[0].placed.is_none()), (40, 0, 0, true));
        sim.state.objects["s:7"] = SimObject::Tree { stage: TreeStage::Standing, since_tick: 40, work: 2.0 };
        assert!(objects(&tracker.frame(&sim.state, &[])).is_empty());
        sim.state.objects["s:7"] = SimObject::Tree { stage: TreeStage::Standing, since_tick: 40, work: 45.0 };
        assert_eq!(objects(&tracker.frame(&sim.state, &[]))[0].progress, 50);

        sim.state.objects["s:7"] = SimObject::Tree { stage: TreeStage::Falling, since_tick: 900, work: 0.0 };
        let told = objects(&tracker.frame(&sim.state, &[]));
        assert_eq!((told[0].stage.as_str(), told[0].since_tick, told[0].progress), ("falling", 900, 0));
        sim.state.objects["s:7"] = SimObject::Tree { stage: TreeStage::Stump, since_tick: 930, work: 0.0 };
        assert_eq!(objects(&tracker.frame(&sim.state, &[]))[0].stage, "stump");
        assert_eq!(tracker.world_objects(), objects_of(&sim.state));

        sim.state
            .objects
            .insert("s:9".into(), SimObject::Rock { stage: RockStage::MinedOut, since_tick: 950, work: 0.0 });
        let told = objects(&tracker.frame(&sim.state, &[]));
        assert_eq!(
            (told[0].id.as_str(), told[0].kind, told[0].stage.as_str()),
            ("s:9", WorldObjectKind::Rock, "mined_out")
        );

        sim.state.objects.shift_remove("s:7");
        let told = objects(&tracker.frame(&sim.state, &[]));
        assert_eq!((told[0].id.as_str(), told[0].stage.as_str()), ("s:7", "intact"));
        assert_eq!(tracker.world_objects().len(), 1);

        // Stockpiles: told when they change, and told empty when the last of it goes.
        sim.state.stockpiles.entry("lumber_camp".into()).or_default().add(Resource::Wood, 4, 60);
        let told = piles(&tracker.frame(&sim.state, &[]));
        assert_eq!(
            (told[0].place.as_str(), told[0].name.as_str(), told[0].wood, told[0].stone),
            ("lumber_camp", "the lumber camp", 4, 0)
        );
        assert!(piles(&tracker.frame(&sim.state, &[])).is_empty());
        sim.state.stockpiles.clear();
        let told = piles(&tracker.frame(&sim.state, &[]));
        assert_eq!((told[0].place.as_str(), told[0].wood), ("lumber_camp", 0));
        assert!(tracker.stockpiles().is_empty());
    }

    #[test]
    fn weapons_are_put_away_at_ease() {
        let mut state = create_test_state(3);
        let id = state.order[0].clone();
        let c = state.characters.get_mut(&id).unwrap();
        c.anim = AnimId::SitChair;
        let d = character_dyn(c);
        assert!(d.mh.is_none() && d.oh.is_none());
        c.anim = AnimId::Idle;
        c.tool = Some("tools/hammer".into());
        let d = character_dyn(c);
        assert_eq!(d.mh.as_deref(), Some("tools/hammer"));
        assert!(d.oh.is_none());
    }

    #[test]
    fn the_clients_frame_fixture_is_current() {
        const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/fixtures/server-frames.json");
        let committed = std::fs::read_to_string(PATH).unwrap_or_default();
        assert!(
            committed == frames_fixture(),
            "tests/fixtures/server-frames.json is stale; run `cargo run --bin export-frames` in server/ and commit the result"
        );
    }
}
