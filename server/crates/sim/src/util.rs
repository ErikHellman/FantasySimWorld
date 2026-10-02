//! The per-tick context and the helpers every system shares.

use fsw_core::Vec2;
use fsw_core::jsmath::js_round;
use fsw_core::math2d::{clamp, dist};
use fsw_core::rng;
use fsw_core::time::{GameTime, game_time, seconds_to_ticks};
use fsw_world::places::nearest_place;

use crate::data::anims::AnimId;
use crate::events::SimEvent;
use crate::state::{
    Character, ChronicleEntry, ChronicleKind, Id, IntentAction, Memory, MemoryKind, Monster, MonsterPhase, Pronouns,
    RelationLabel, Relationship, SimState, Stimulus, StimulusKind,
};

pub const MEMORY_CAP: usize = 140;
pub const CHRONICLE_CAP: usize = 600;
const STIMULUS_CAP: usize = 8;

/// Per-tick context handed to every system. Characters are reached by id
/// (`ctx.c(id)`), so a system can touch several of them in turn, and the RNG
/// through the helpers below.
pub struct Ctx<'a> {
    pub state: &'a mut SimState,
    pub events: &'a mut Vec<SimEvent>,
    pub time: GameTime,
}

impl<'a> Ctx<'a> {
    pub fn new(state: &'a mut SimState, events: &'a mut Vec<SimEvent>) -> Self {
        let time = game_time(state.tick);
        Self { state, events, time }
    }

    pub fn tick(&self) -> i64 {
        self.state.tick
    }

    /// A living character; panics if the id is not one (callers check first when it may not be).
    pub fn c(&mut self, id: &str) -> &mut Character {
        self.state.characters.get_mut(id).unwrap_or_else(|| panic!("no living character {id}"))
    }

    pub fn cr(&self, id: &str) -> &Character {
        self.state.characters.get(id).unwrap_or_else(|| panic!("no living character {id}"))
    }

    pub fn get(&self, id: Option<&str>) -> Option<&Character> {
        id.and_then(|id| self.state.characters.get(id))
    }

    pub fn alive(&self, id: &str) -> bool {
        self.state.characters.contains_key(id)
    }

    pub fn m(&mut self, id: &str) -> &mut Monster {
        self.state.monsters.get_mut(id).unwrap_or_else(|| panic!("no monster {id}"))
    }

    pub fn next_float(&mut self) -> f64 {
        rng::next_float(&mut self.state.rng)
    }

    pub fn range(&mut self, min: f64, max: f64) -> f64 {
        rng::range(&mut self.state.rng, min, max)
    }

    pub fn int_range(&mut self, min: i64, max: i64) -> i64 {
        rng::int_range(&mut self.state.rng, min, max)
    }

    pub fn chance(&mut self, probability: f64) -> bool {
        rng::chance(&mut self.state.rng, probability)
    }

    pub fn pick_index(&mut self, len: usize) -> usize {
        rng::pick_index(&mut self.state.rng, len)
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.pick_index(items.len())]
    }

    pub fn emit(&mut self, event: SimEvent) {
        self.events.push(event);
    }

    pub fn remember(
        &mut self,
        id: &str,
        text: impl Into<String>,
        importance: f64,
        kind: MemoryKind,
        subjects: Vec<Id>,
    ) {
        let tick = self.tick();
        let day = self.time.day;
        remember(self.c(id), tick, day, text.into(), importance, kind, subjects);
    }

    pub fn stimulate(
        &mut self,
        id: &str,
        kind: StimulusKind,
        text: impl Into<String>,
        urgency: i64,
        subject: Option<&str>,
    ) {
        let tick = self.tick();
        stimulate(self.c(id), tick, kind, text.into(), urgency, subject);
    }

    pub fn chronicle(&mut self, kind: ChronicleKind, text: impl Into<String>, ids: Vec<Id>) {
        let entry = ChronicleEntry { tick: self.tick(), kind, text: text.into(), ids };
        let chronicle = &mut self.state.chronicle;
        chronicle.push(entry.clone());
        if chronicle.len() > CHRONICLE_CAP {
            let excess = chronicle.len() - CHRONICLE_CAP;
            chronicle.drain(..excess);
        }
        self.events.push(SimEvent::Chronicle { entry });
    }

    /// Says a line aloud; returns how many ticks it stays up.
    pub fn say(&mut self, speaker: &str, text: impl Into<String>, to: Option<&str>, loud: bool) -> i64 {
        let text = text.into();
        let duration_ticks = speech_ticks(&text);
        self.events.push(SimEvent::Speech {
            speaker_id: speaker.to_owned(),
            to_id: to.map(str::to_owned),
            text,
            duration_ticks,
            loud,
        });
        duration_ticks
    }
}

/// Ids of the living characters, in their stable order. Systems iterate a
/// copy because characters die (and arrive) mid-tick.
pub fn living_ids(state: &SimState) -> Vec<Id> {
    state.order.iter().filter(|id| state.characters.contains_key(*id)).cloned().collect()
}

/// Living characters in a stable order.
pub fn living(state: &SimState) -> impl Iterator<Item = &Character> {
    state.order.iter().filter_map(|id| state.characters.get(id))
}

/// Ids of the monsters, including dying ones, in creation order.
pub fn monster_ids(state: &SimState) -> Vec<Id> {
    state.monsters.keys().cloned().collect()
}

/// A monster that is not dying.
pub fn get_monster<'s>(state: &'s SimState, id: Option<&str>) -> Option<&'s Monster> {
    state.monsters.get(id?).filter(|m| m.phase != MonsterPhase::Dying)
}

pub fn first_name(c: &Character) -> &str {
    c.profile.name.split(' ').next().unwrap_or_default()
}

pub fn name_of(state: &SimState, id: &str) -> String {
    if let Some(c) = state.characters.get(id) {
        return first_name(c).to_owned();
    }
    match state.dead.get(id) {
        Some(dead) => dead.name.split(' ').next().unwrap_or_default().to_owned(),
        None => "someone".into(),
    }
}

pub fn new_relationship() -> Relationship {
    Relationship {
        affinity: 0.0,
        trust: 0.0,
        attraction: 0.0,
        grievance: 0.0,
        familiarity: 0.0,
        label: RelationLabel::Stranger,
        kin: false,
        summary: String::new(),
        last_talk_tick: -1_000_000_000,
        last_offence_tick: -1_000_000_000,
    }
}

/// The relationship record `from` keeps about `to_id`, created on first use (inside the sim only).
pub fn rel<'c>(from: &'c mut Character, to_id: &str) -> &'c mut Relationship {
    from.relationships.entry(to_id.to_owned()).or_insert_with(new_relationship)
}

/// The relationship record `from` keeps about `to_id`, or a fresh one; never creates it.
pub fn rel_or_default(from: &Character, to_id: &str) -> Relationship {
    from.relationships.get(to_id).cloned().unwrap_or_else(new_relationship)
}

/// Something with a position, a facing and an animation: a character or a monster.
pub trait Body {
    fn pos(&self) -> Vec2;
    fn set_pos(&mut self, pos: Vec2);
    fn facing_mut(&mut self) -> &mut f64;
    fn set_moving(&mut self, moving: bool);
    fn anim_mut(&mut self) -> (&mut AnimId, &mut i64);
}

macro_rules! body {
    ($t:ty) => {
        impl Body for $t {
            fn pos(&self) -> Vec2 {
                self.pos
            }
            fn set_pos(&mut self, pos: Vec2) {
                self.pos = pos;
            }
            fn facing_mut(&mut self) -> &mut f64 {
                &mut self.facing
            }
            fn set_moving(&mut self, moving: bool) {
                self.moving = moving;
            }
            fn anim_mut(&mut self) -> (&mut AnimId, &mut i64) {
                (&mut self.anim, &mut self.anim_seq)
            }
        }
    };
}
body!(Character);
body!(Monster);

pub fn set_anim(entity: &mut impl Body, anim: AnimId, restart: bool) {
    let (current, seq) = entity.anim_mut();
    if *current != anim || restart {
        *current = anim;
        *seq += 1;
    }
}

/// How long a line stays up: reading time now, speaking time once there are voices.
pub fn speech_ticks(text: &str) -> i64 {
    seconds_to_ticks(clamp(1.3 + fsw_core::jsmath::utf16_len(text) as f64 * 0.055, 2.0, 9.0))
}

pub fn remember(
    c: &mut Character,
    tick: i64,
    day: i64,
    text: String,
    importance: f64,
    kind: MemoryKind,
    subjects: Vec<Id>,
) {
    let importance = clamp(js_round(importance), 1.0, 10.0) as i64;
    c.memories.push(Memory { tick, day, text, importance, kind, subjects });
    if c.memories.len() > MEMORY_CAP {
        prune_memories(c, tick);
    }
}

/// Drops the least valuable memories: old and unimportant go first, reflections last.
pub fn prune_memories(c: &mut Character, tick: i64) {
    let keep = (MEMORY_CAP as f64 * 0.8).floor() as usize;
    let mut scored: Vec<(usize, f64)> = c
        .memories
        .iter()
        .enumerate()
        .map(|(index, memory)| {
            let age_days = (tick - memory.tick) as f64 / (20.0 * 60.0 * 24.0);
            let bonus = if memory.kind == MemoryKind::Reflection { 6.0 } else { 0.0 };
            (index, memory.importance as f64 * 1.6 - age_days * 1.2 + bonus)
        })
        .collect();
    // Stable, highest score first (as Array.prototype.sort with `b.score - a.score`).
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut kept = vec![false; c.memories.len()];
    for &(index, _) in scored.iter().take(keep) {
        kept[index] = true;
    }
    let mut index = 0;
    c.memories.retain(|_| {
        let keep = kept[index];
        index += 1;
        keep
    });
}

pub fn stimulate(c: &mut Character, tick: i64, kind: StimulusKind, text: String, urgency: i64, subject: Option<&str>) {
    c.stimuli.push(Stimulus { kind, tick, text, subject_id: subject.map(str::to_owned), urgency });
    if c.stimuli.len() > STIMULUS_CAP {
        c.stimuli.remove(0);
    }
}

pub fn has_recent_stimulus(c: &Character, kind: StimulusKind, since_tick: i64, subject: Option<&str>) -> bool {
    c.stimuli
        .iter()
        .any(|s| s.kind == kind && s.tick >= since_tick && subject.is_none_or(|id| s.subject_id.as_deref() == Some(id)))
}

/// Out in the open and conscious: can be seen, spoken to or attacked.
pub fn is_present(c: &Character) -> bool {
    c.inside.is_none() && !c.asleep
}

pub fn is_fighting(c: &Character) -> bool {
    matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::Fight { .. } | IntentAction::Flee))
}

/// Free to be approached for a chat or a gift.
pub fn is_approachable(c: &Character) -> bool {
    is_present(c) && c.convo_id.is_none() && !is_fighting(c) && !c.combat.yielded
}

pub fn place_name_at(pos: Vec2) -> String {
    if let Some(place) = nearest_place(pos, 16.0) {
        return place.name.to_owned();
    }
    match nearest_place(pos, 80.0) {
        Some(landmark) => format!("the fields near {}", landmark.name),
        None => "the wilds".into(),
    }
}

/// Ids of the characters outdoors within `radius`, in the stable order.
pub fn nearby_characters(state: &SimState, pos: Vec2, radius: f64, except: Option<&str>) -> Vec<Id> {
    living(state)
        .filter(|c| Some(c.id.as_str()) != except && c.inside.is_none() && dist(c.pos, pos) <= radius)
        .map(|c| c.id.clone())
        .collect()
}

/// Invalidate any decision that was requested before now.
pub fn bump_epoch(c: &mut Character) {
    c.epoch += 1;
}

pub fn join_names<S: AsRef<str>>(names: &[S]) -> String {
    match names {
        [] => String::new(),
        [one] => one.as_ref().to_owned(),
        [rest @ .., last] => {
            format!("{} and {}", rest.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(", "), last.as_ref())
        }
    }
}

/// Possessive pronoun for narration: "her", "his" or "their".
pub fn their(c: &Character) -> &'static str {
    match c.profile.pronouns {
        Pronouns::SheHer => "her",
        Pronouns::HeHim => "his",
        Pronouns::TheyThem => "their",
    }
}

/// Whether two people could fall for each other at all: a fixed property of
/// the pair, so romance grows between some and never between others.
pub fn chemistry(a: &Character, b: &Character) -> bool {
    let key = if a.id < b.id { format!("{}|{}", a.id, b.id) } else { format!("{}|{}", b.id, a.id) };
    rng::hash_string(&key) % 100 < 38
}

pub fn hp_fraction(c: &Character, max_hp: f64) -> f64 {
    clamp(c.hp / max_hp, 0.0, 1.0)
}
