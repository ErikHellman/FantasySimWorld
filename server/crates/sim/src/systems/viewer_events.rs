//! What viewers pay to make happen: gifts, blessings and the dead. To the villagers a viewer
//! is a named power that watches the valley; they remember what it did and thank it, plead
//! with it and pray to it by name.

use fsw_core::jsmath::utf16_len;
use fsw_core::time::minutes_to_ticks;
use fsw_core::{Vec2, vec2};
use fsw_world::navgrid::nearest_walkable;
use fsw_world::places::get_place;

use crate::commands::ViewerEvent;
use crate::data::activities::ITEM_CAP;
use crate::data::prayers::{self, DEVOTIONS, ENTREATIES, PLEAS, PRAISE, THANKS};
use crate::events::{Emote, SimEvent};
use crate::state::{ChronicleKind, Id, ItemId, MemoryKind, MonsterKind, MonsterPhase, Power, SimState, StimulusKind};
use crate::systems::combat::{max_hp, witnesses};
use crate::systems::monsters::{MAX_MONSTERS, random_monster_kind, spawn_monster};
use crate::util::{Ctx, is_present, living, living_ids, name_of, place_name_at};

/// How long one of the dead raised by a viewer walks before it crumbles, in game minutes.
pub const BOUGHT_MINUTES: f64 = 180.0;
/// The dead in a raid a viewer calls, room allowing.
pub const RAID_SIZE: usize = 4;
/// The most powers the village keeps in mind; the one longest quiet is forgotten first.
pub const POWERS_CAP: usize = 24;
/// How far from the asked-for spot a monster may rise, in nav cells.
const RISE_REACH: i64 = 6;
/// Hunger at which nobody would call themselves hungry.
const WELL_FED: f64 = 90.0;
const NAME_MIN: usize = 2;
const NAME_MAX: usize = 24;

/// Why an event cannot happen as the world stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NoSuchVillager,
    PackFull,
    NowhereToRise,
    TooManyDead,
    NecromancerAbroad,
    NobodyHurt,
    NobodyHungry,
    /// not something a viewer may ask for
    NotOffered,
}

impl Refusal {
    /// A sentence for the viewer who asked.
    pub fn message(self) -> &'static str {
        match self {
            Self::NoSuchVillager => "That villager is no longer here.",
            Self::PackFull => "Their pack has no room for another.",
            Self::NowhereToRise => "Nothing can rise there.",
            Self::TooManyDead => "Too many of the dead are abroad already.",
            Self::NecromancerAbroad => "A necromancer already walks the valley.",
            Self::NobodyHurt => "Nobody in the village is hurt.",
            Self::NobodyHungry => "Nobody in the village is hungry.",
            Self::NotOffered => "That is not on offer.",
        }
    }
}

/// A viewer's name as the villagers may say it, or `None` when it is not one they could:
/// the same rule as chat names (letters, digits, spaces and `- _ ' .`, 2 to 24 UTF-16 units).
/// The narrow alphabet is also what keeps a name from reading as instructions to the model.
pub fn power_name(raw: &str) -> Option<String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let fits = (NAME_MIN..=NAME_MAX).contains(&utf16_len(&name));
    (fits && name.chars().all(|c| c.is_alphanumeric() || " -_'.".contains(c))).then_some(name)
}

fn room_for_the_dead(state: &SimState) -> usize {
    MAX_MONSTERS.saturating_sub(state.monsters.len())
}

/// Where a monster asked for at `pos` would rise.
fn rise_spot(pos: Vec2) -> Option<Vec2> {
    (pos.x.is_finite() && pos.z.is_finite()).then(|| nearest_walkable(pos, RISE_REACH)).flatten()
}

/// Whether `event` can happen in this world right now. The server asks before it takes a
/// viewer's credits, and the sim asks again when the command is applied.
pub fn check(state: &SimState, event: &ViewerEvent) -> Result<(), Refusal> {
    match event {
        ViewerEvent::Gift { character_id, item } => {
            let c = state.characters.get(character_id).ok_or(Refusal::NoSuchVillager)?;
            if c.inventory.get(*item) >= ITEM_CAP {
                return Err(Refusal::PackFull);
            }
        }
        ViewerEvent::Monster { monster, pos } => {
            if *monster == MonsterKind::Necromancer {
                return Err(Refusal::NotOffered);
            }
            if rise_spot(*pos).is_none() {
                return Err(Refusal::NowhereToRise);
            }
            if room_for_the_dead(state) == 0 {
                return Err(Refusal::TooManyDead);
            }
        }
        ViewerEvent::Raid => {
            if room_for_the_dead(state) == 0 {
                return Err(Refusal::TooManyDead);
            }
        }
        ViewerEvent::Necromancer => {
            let abroad = |m: &crate::state::Monster| {
                m.phase != MonsterPhase::Dying && (m.lasting || m.kind == MonsterKind::Necromancer)
            };
            if state.monsters.values().any(abroad) {
                return Err(Refusal::NecromancerAbroad);
            }
            if room_for_the_dead(state) == 0 {
                return Err(Refusal::TooManyDead);
            }
        }
        ViewerEvent::Heal => {
            if !living(state).any(|c| c.hp < max_hp(c)) {
                return Err(Refusal::NobodyHurt);
            }
        }
        ViewerEvent::Feast => {
            if !living(state).any(|c| c.needs.hunger < WELL_FED) {
                return Err(Refusal::NobodyHungry);
            }
        }
    }
    Ok(())
}

/// What `event` does, as the rest of a sentence about whoever did it ("sent Rowan some food"),
/// for the chronicle and the viewers' chat.
pub fn describe(state: &SimState, event: &ViewerEvent) -> String {
    match event {
        ViewerEvent::Gift { character_id, item } => {
            format!("sent {} {}", name_of(state, character_id), describe_item(*item))
        }
        ViewerEvent::Monster { monster, pos } => {
            format!("raised a {} at {}", monster.def().label, place_name_at(rise_spot(*pos).unwrap_or(*pos)))
        }
        ViewerEvent::Raid => "called the dead up from the old graveyard".into(),
        ViewerEvent::Necromancer => "set a necromancer walking in the old graveyard".into(),
        ViewerEvent::Heal => "closed every wound in the village".into(),
        ViewerEvent::Feast => "filled every belly in the village".into(),
    }
}

/// The power as villagers speak of it and to it.
struct Patron {
    /// starts a sentence about them: their name, or "A nameless power"
    who: String,
    /// what they are called to their face: their name, or "lord"
    address: String,
}

impl Patron {
    /// "Erik, one of the powers that watch the valley," for a memory.
    fn introduced(&self) -> String {
        format!("{}, one of the powers that watch the valley,", self.who)
    }
}

/// Counts a kindness or a cruelty for a named power, forgetting the one longest quiet when
/// the village knows too many.
fn note(ctx: &mut Ctx, name: Option<&str>, kind: StimulusKind) {
    let Some(name) = name else { return };
    let tick = ctx.tick();
    let power: &mut Power = ctx.state.powers.entry(name.to_owned()).or_default();
    if kind == StimulusKind::Wrath {
        power.wraths += 1;
    } else {
        power.favours += 1;
    }
    power.last_tick = tick;
    while ctx.state.powers.len() > POWERS_CAP {
        let oldest = ctx.state.powers.iter().min_by_key(|(_, power)| power.last_tick).map(|(name, _)| name.clone());
        let Some(oldest) = oldest else { break };
        ctx.state.powers.shift_remove(&oldest);
    }
}

/// Up to two of `ids`, picked by the sim's dice, say a line from `lines`.
fn cry_out(ctx: &mut Ctx, mut ids: Vec<Id>, lines: &[&str], address: &str, loud: bool) {
    for _ in 0..2 {
        if ids.is_empty() {
            return;
        }
        let speaker = ids.remove(ctx.pick_index(ids.len()));
        let line = prayers::line(ctx.pick(lines), address);
        ctx.say(&speaker, line, None, loud);
    }
}

/// Everyone alive remembers `memory`; those awake are also given it to think about now.
fn tell_everyone(ctx: &mut Ctx, kind: StimulusKind, memory: &str, urgency: i64) {
    for id in living_ids(ctx.state) {
        ctx.remember(&id, memory, 6.0, MemoryKind::Event, Vec::new());
        if !ctx.cr(&id).asleep {
            ctx.stimulate(&id, kind, memory, urgency, None);
        }
    }
}

fn present_ids(state: &SimState) -> Vec<Id> {
    living(state).filter(|c| is_present(c)).map(|c| c.id.clone()).collect()
}

fn describe_item(item: ItemId) -> &'static str {
    match item {
        ItemId::Food => "some food",
        ItemId::Potion => "a healing potion",
        ItemId::Gift => "a small gift",
    }
}

/// Raises `kind` at `pos`, to crumble when a bought monster's time is up.
fn raise(ctx: &mut Ctx, kind: MonsterKind, pos: Vec2, facing: f64) {
    let until = ctx.tick() + minutes_to_ticks(BOUGHT_MINUTES);
    let id = spawn_monster(ctx, kind, pos, facing);
    ctx.m(&id).until_tick = Some(until);
}

/// Carries out what a viewer called `by` paid for, if it can still happen.
pub fn apply(ctx: &mut Ctx, by: &str, event: ViewerEvent) {
    if check(ctx.state, &event).is_err() {
        return;
    }
    let name = power_name(by);
    let patron = Patron {
        who: name.clone().unwrap_or_else(|| "A nameless power".into()),
        address: name.clone().unwrap_or_else(|| "lord".into()),
    };
    let name = name.as_deref();
    let who = &patron.who;
    let done = describe(ctx.state, &event);

    match event {
        ViewerEvent::Gift { character_id, item } => {
            note(ctx, name, StimulusKind::Favour);
            let what = describe_item(item);
            *ctx.c(&character_id).inventory.get_mut(item) += 1;
            ctx.remember(
                &character_id,
                format!("{} put {what} in my pack.", patron.introduced()),
                5.0,
                MemoryKind::Event,
                Vec::new(),
            );
            ctx.emit(SimEvent::Emote { id: character_id.clone(), emote: Emote::Gift });
            if !ctx.cr(&character_id).asleep {
                let seen = format!("{} just put {what} in your pack.", patron.introduced());
                ctx.stimulate(&character_id, StimulusKind::Favour, seen, 1, None);
                let line = prayers::line(ctx.pick(&THANKS), &patron.address);
                ctx.say(&character_id, line, None, false);
            }
            ctx.chronicle(ChronicleKind::World, format!("{who} {done}."), vec![character_id]);
        }
        ViewerEvent::Monster { monster, pos } => {
            let Some(spot) = rise_spot(pos) else { return };
            note(ctx, name, StimulusKind::Wrath);
            raise(ctx, monster, spot, 0.0);
            let (label, place) = (monster.def().label, place_name_at(spot));
            let saw = witnesses(ctx, spot, &[]);
            let memory = format!("{} raised a {label} at {place}.", patron.introduced());
            for id in &saw {
                ctx.remember(id, memory.as_str(), 6.0, MemoryKind::Event, Vec::new());
                ctx.stimulate(id, StimulusKind::Wrath, memory.as_str(), 2, None);
            }
            cry_out(ctx, saw, &PLEAS, &patron.address, true);
            ctx.chronicle(ChronicleKind::World, format!("{who} {done}."), Vec::new());
        }
        ViewerEvent::Raid => {
            note(ctx, name, StimulusKind::Wrath);
            let graveyard = get_place("graveyard");
            let mut points = graveyard.spawn_points.clone();
            for _ in 0..RAID_SIZE.min(room_for_the_dead(ctx.state)) {
                let point =
                    if points.is_empty() { graveyard.origin } else { points.remove(ctx.pick_index(points.len())) };
                let x = point.x + ctx.range(-0.6, 0.6);
                let z = point.z + ctx.range(-0.6, 0.6);
                let Some(spot) = nearest_walkable(vec2(x, z), 5) else { continue };
                let kind = random_monster_kind(ctx);
                raise(ctx, kind, spot, graveyard.facing);
            }
            let memory = format!("{} called the dead up from the old graveyard.", patron.introduced());
            tell_everyone(ctx, StimulusKind::Wrath, &memory, 2);
            let present = present_ids(ctx.state);
            cry_out(ctx, present, &PLEAS, &patron.address, true);
            ctx.chronicle(ChronicleKind::World, format!("{who} {done}."), Vec::new());
            ctx.emit(SimEvent::SavePoint { reason: "a viewer's raid".into() });
        }
        ViewerEvent::Necromancer => {
            let graveyard = get_place("graveyard");
            let Some(spot) = nearest_walkable(graveyard.origin, 5) else { return };
            note(ctx, name, StimulusKind::Wrath);
            raise(ctx, MonsterKind::Necromancer, spot, graveyard.facing);
            let memory = format!("{} set a necromancer walking in the old graveyard.", patron.introduced());
            tell_everyone(ctx, StimulusKind::Wrath, &memory, 2);
            let present = present_ids(ctx.state);
            cry_out(ctx, present, &PLEAS, &patron.address, true);
            ctx.chronicle(ChronicleKind::World, format!("{who} {done}."), Vec::new());
            ctx.emit(SimEvent::SavePoint { reason: "a viewer's necromancer".into() });
        }
        ViewerEvent::Heal => {
            note(ctx, name, StimulusKind::Favour);
            for id in living_ids(ctx.state) {
                let c = ctx.c(&id);
                c.hp = max_hp(c);
            }
            let memory = format!("{} closed every wound in the village.", patron.introduced());
            tell_everyone(ctx, StimulusKind::Favour, &memory, 1);
            let present = present_ids(ctx.state);
            cry_out(ctx, present, &PRAISE, &patron.address, false);
            ctx.chronicle(ChronicleKind::World, format!("{who} {done}."), Vec::new());
            ctx.emit(SimEvent::SavePoint { reason: "a viewer's healing".into() });
        }
        ViewerEvent::Feast => {
            note(ctx, name, StimulusKind::Favour);
            for id in living_ids(ctx.state) {
                ctx.c(&id).needs.hunger = 100.0;
            }
            let memory = format!("{} filled every belly in the village.", patron.introduced());
            tell_everyone(ctx, StimulusKind::Favour, &memory, 1);
            let present = present_ids(ctx.state);
            cry_out(ctx, present, &PRAISE, &patron.address, false);
            ctx.chronicle(ChronicleKind::World, format!("{who} {done}."), Vec::new());
            ctx.emit(SimEvent::SavePoint { reason: "a viewer's feast".into() });
        }
    }
}

/// Someone settling down to pray now and then says a prayer to one of the powers the village
/// knows: thanks to a kind one, pleading to a cruel one. Draws no dice while none is known,
/// so a world no viewer has touched runs exactly as before.
pub fn pray(ctx: &mut Ctx, id: &str) {
    if ctx.state.powers.is_empty() || !ctx.chance(0.6) {
        return;
    }
    let index = ctx.pick_index(ctx.state.powers.len());
    let Some((name, power)) = ctx.state.powers.get_index(index) else { return };
    let (name, cruel) = (name.clone(), power.wraths > power.favours);
    let line = prayers::line(ctx.pick(if cruel { &ENTREATIES[..] } else { &DEVOTIONS[..] }), &name);
    ctx.say(id, line, None, false);
}
