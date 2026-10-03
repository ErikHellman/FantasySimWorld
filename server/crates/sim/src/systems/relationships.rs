//! How characters feel about each other: labels, the hostility ladder,
//! conversations' effects, love and its end.

use fsw_core::math2d::clamp;
use fsw_core::time::TICKS_PER_DAY;

use crate::events::{Emote, SimEvent};
use crate::state::{
    Character, ChronicleKind, Feeling, Id, MemoryKind, Quirk, RelationLabel, Relationship, StimulusKind, TalkTone,
};
use crate::systems::charm;
use crate::util::{Ctx, can_fall_for, first_name, living_ids, open_to, rel, rel_or_default};

/// The warmest a bully ever feels about anyone: short of friendship.
const BULLY_AFFINITY_CAP: f64 = 20.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct RelationDelta {
    pub affinity: f64,
    pub trust: f64,
    pub attraction: f64,
    pub grievance: f64,
    pub familiarity: f64,
}

/// Hostility ladder rungs, by grievance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rung {
    Calm,
    Tension,
    Grudge,
    Feud,
    BloodFeud,
}

impl Rung {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Calm => "calm",
            Self::Tension => "tension",
            Self::Grudge => "grudge",
            Self::Feud => "feud",
            Self::BloodFeud => "blood_feud",
        }
    }
}

pub const RUNG_TENSION: f64 = 20.0;
pub const RUNG_GRUDGE: f64 = 40.0;
pub const RUNG_FEUD: f64 = 65.0;
pub const RUNG_BLOOD: f64 = 85.0;

/// The grievance at which this character will come to blows: a feud, or a lesser grudge for the spiteful.
pub fn brawl_threshold(c: &Character) -> f64 {
    RUNG_FEUD - 20.0 * c.profile.spite
}

/// Whether `from` may start a fist fight with `to`. It only reads, so the brain can ask too.
pub fn brawl_unlocked(from: &Character, to_id: &str) -> bool {
    rel_or_default(from, to_id).grievance >= brawl_threshold(from)
}

pub fn rung_of(r: &Relationship) -> Rung {
    if r.grievance >= RUNG_BLOOD {
        Rung::BloodFeud
    } else if r.grievance >= RUNG_FEUD {
        Rung::Feud
    } else if r.grievance >= RUNG_GRUDGE {
        Rung::Grudge
    } else if r.grievance >= RUNG_TENSION {
        Rung::Tension
    } else {
        Rung::Calm
    }
}

/// Label with hysteresis: a label is harder to lose than to gain, so it does not flicker.
pub fn label_for(r: &Relationship, is_partner: bool) -> RelationLabel {
    use RelationLabel::*;
    if is_partner {
        return Lover;
    }
    let was = r.label;
    if r.grievance >= 60.0 || r.affinity <= -55.0 || (was == Enemy && (r.grievance >= 40.0 || r.affinity <= -35.0)) {
        return Enemy;
    }
    if r.grievance >= 35.0 || r.affinity <= -25.0 || (was == Rival && (r.grievance >= 25.0 || r.affinity <= -10.0)) {
        return Rival;
    }
    if r.kin {
        return Family;
    }
    if (r.affinity >= 60.0 && r.familiarity >= 40.0) || (was == CloseFriend && r.affinity >= 45.0) {
        return CloseFriend;
    }
    if r.affinity >= 30.0 || ((was == Friend || was == CloseFriend) && r.affinity >= 18.0) {
        return Friend;
    }
    if r.familiarity >= 8.0 {
        return Acquaintance;
    }
    Stranger
}

fn label_note(label: RelationLabel, name: &str) -> Option<String> {
    Some(match label {
        RelationLabel::Friend => format!("I have come to think of {name} as a friend."),
        RelationLabel::CloseFriend => format!("{name} has become one of my closest friends."),
        RelationLabel::Rival => format!("There is bad blood between me and {name} now."),
        RelationLabel::Enemy => format!("{name} is my enemy."),
        RelationLabel::Acquaintance => format!("{name} and I are on speaking terms, nothing more."),
        _ => return None,
    })
}

/// Affinity everyone starts with toward a striking-looking person, before they have said a word.
pub fn looks_welcome(c: &Character) -> f64 {
    c.profile.looks * 20.0
}

/// How quirks bend a change in feeling: nobody warms to a bully, and attraction
/// only grows where it can (never for the chaste, only for women in some).
fn shaped(ctx: &Ctx, from: &str, to_id: &str, mut delta: RelationDelta) -> RelationDelta {
    let Some(to) = ctx.state.characters.get(to_id) else { return delta };
    if to.has(Quirk::Bully) {
        delta.affinity = if delta.affinity > 0.0 { 0.0 } else { delta.affinity * 1.3 };
    }
    if delta.attraction > 0.0 && !open_to(ctx.cr(from), to) {
        delta.attraction = 0.0;
    }
    delta
}

/// Applies a change to how `from` feels about `to_id` and updates the label.
/// Warmth toward a striking-looking person grows faster.
pub fn adjust(ctx: &mut Ctx, from: &str, to_id: &str, mut delta: RelationDelta) {
    let tick = ctx.tick();
    let looks = ctx.state.characters.get(to_id).map_or(0.0, |c| c.profile.looks);
    if looks > 0.0 {
        let boost = 1.0 + looks * 0.5;
        delta.affinity = if delta.affinity > 0.0 { delta.affinity * boost } else { delta.affinity };
        delta.attraction = if delta.attraction > 0.0 { delta.attraction * boost } else { delta.attraction };
    }
    let delta = shaped(ctx, from, to_id, delta);
    let bully = ctx.cr(from).has(Quirk::Bully);
    let r = rel(ctx.c(from), to_id);
    if delta.affinity != 0.0 {
        r.affinity = clamp(r.affinity + delta.affinity, -100.0, 100.0);
        if bully {
            r.affinity = r.affinity.min(BULLY_AFFINITY_CAP);
        }
    }
    if delta.trust != 0.0 {
        r.trust = clamp(r.trust + delta.trust, -100.0, 100.0);
    }
    if delta.attraction != 0.0 {
        r.attraction = clamp(r.attraction + delta.attraction, 0.0, 100.0);
    }
    if delta.familiarity != 0.0 {
        r.familiarity = clamp(r.familiarity + delta.familiarity, 0.0, 100.0);
    }
    if delta.grievance != 0.0 {
        r.grievance = clamp(r.grievance + delta.grievance, 0.0, 100.0);
        if delta.grievance > 0.0 {
            r.last_offence_tick = tick;
        }
    }
    relabel(ctx, from, to_id);
}

pub fn relabel(ctx: &mut Ctx, from: &str, to_id: &str) {
    let c = ctx.c(from);
    let is_partner = c.partner_id.as_deref() == Some(to_id);
    let r = rel(c, to_id);
    let next = label_for(r, is_partner);
    if next == r.label {
        return;
    }
    let previous = r.label;
    r.label = next;
    let Some(other) = ctx.state.characters.get(to_id) else { return };
    let other_name = first_name(other).to_owned();
    let from_name = first_name(ctx.cr(from)).to_owned();
    let worsened = matches!(next, RelationLabel::Rival | RelationLabel::Enemy);
    let improved =
        (next == RelationLabel::Friend && previous != RelationLabel::CloseFriend) || next == RelationLabel::CloseFriend;
    let Some(note) = label_note(next, &other_name) else { return };
    if worsened || improved {
        ctx.remember(from, note, if worsened { 6.0 } else { 5.0 }, MemoryKind::Thought, vec![to_id.to_owned()]);
        let text = match next {
            RelationLabel::Enemy => format!("{from_name} now counts {other_name} as an enemy."),
            RelationLabel::Rival => format!("{from_name} has fallen out with {other_name}."),
            RelationLabel::CloseFriend => format!("{from_name} and {other_name} have grown close."),
            _ => format!("{from_name} has warmed to {other_name}."),
        };
        let kind = if worsened { ChronicleKind::Conflict } else { ChronicleKind::Social };
        ctx.chronicle(kind, text, vec![from.to_owned(), to_id.to_owned()]);
    }
}

fn feeling_affinity(feeling: Feeling) -> f64 {
    match feeling {
        Feeling::MuchWorse => -11.0,
        Feeling::Worse => -5.0,
        Feeling::Same => 0.0,
        Feeling::Better => 3.0,
        Feeling::MuchBetter => 6.0,
    }
}

/// How each side's reported feeling after a conversation changes the relationship.
pub fn apply_conversation(
    ctx: &mut Ctx,
    self_id: &str,
    other_id: &str,
    feeling: Feeling,
    tone: TalkTone,
    initiator: bool,
) {
    let base = feeling_affinity(feeling);
    let mut delta = RelationDelta {
        affinity: base,
        trust: fsw_core::jsmath::js_round(base * 0.5),
        familiarity: 3.0,
        ..Default::default()
    };
    match tone {
        TalkTone::Hostile => {
            // Being confronted stings whatever is said; a good outcome clears the air.
            delta.grievance = if base < 0.0 { -base * 1.5 } else { -(8f64.max(base * 3.0)) };
            if !initiator && base <= 0.0 {
                delta.grievance += 6.0;
            }
        }
        TalkTone::Apology => delta.grievance = if base > 0.0 { -(12.0 + base * 3.0) } else { 4.0 },
        _ if base < 0.0 => delta.grievance = -base * 1.1,
        _ if base > 0.0 => delta.grievance = -base * 0.6,
        _ => {}
    }
    if tone == TalkTone::Romantic {
        delta.attraction = if base > 0.0 { base * 3.0 } else { base };
    } else if base >= 3.0 {
        let me = ctx.cr(self_id);
        let r = rel_or_default(me, other_id);
        if r.affinity >= 30.0 && me.partner_id.is_none() && !r.kin && can_fall_for(me, ctx.cr(other_id)) {
            delta.attraction = 2.0;
        }
    }
    // A charmer rarely falls for anyone herself, but her flirting works on
    // anyone who does not take it badly, chemistry or none.
    let (mine, theirs) = (ctx.cr(self_id).profile.allure, ctx.cr(other_id).profile.allure);
    if mine > 0.0 && delta.attraction > 0.0 {
        delta.attraction *= 1.0 - mine * 0.75;
    }
    if theirs > 0.0 && tone == TalkTone::Romantic && base >= 0.0 && !rel_or_default(ctx.cr(self_id), other_id).kin {
        delta.attraction += theirs * charm::CHARM;
    }
    adjust(ctx, self_id, other_id, delta);
    let tick = ctx.tick();
    rel(ctx.c(self_id), other_id).last_talk_tick = tick;
}

pub fn become_lovers(ctx: &mut Ctx, a: &str, b: &str) {
    ctx.c(a).partner_id = Some(b.to_owned());
    ctx.c(b).partner_id = Some(a.to_owned());
    let warm = RelationDelta { affinity: 15.0, trust: 10.0, attraction: 10.0, ..Default::default() };
    adjust(ctx, a, b, warm);
    adjust(ctx, b, a, warm);
    let (a_name, b_name) = (first_name(ctx.cr(a)).to_owned(), first_name(ctx.cr(b)).to_owned());
    ctx.remember(
        a,
        format!("{b_name} and I are together now. My heart is full."),
        9.0,
        MemoryKind::Event,
        vec![b.into()],
    );
    ctx.remember(
        b,
        format!("{a_name} and I are together now. My heart is full."),
        9.0,
        MemoryKind::Event,
        vec![a.into()],
    );
    ctx.chronicle(
        ChronicleKind::Romance,
        format!("{a_name} and {b_name} have fallen in love."),
        vec![a.into(), b.into()],
    );
    ctx.emit(SimEvent::Emote { id: a.into(), emote: Emote::Heart });
    ctx.emit(SimEvent::Emote { id: b.into(), emote: Emote::Heart });
    // Anyone who was sweet on either of them takes it hard.
    for other in living_ids(ctx.state) {
        if other == a || other == b {
            continue;
        }
        for (loved, rival) in [(a, b), (b, a)] {
            if rel(ctx.c(&other), loved).attraction >= 40.0 {
                adjust(ctx, &other, rival, RelationDelta { grievance: 18.0, affinity: -12.0, ..Default::default() });
                let (loved_name, rival_name) =
                    (first_name(ctx.cr(loved)).to_owned(), first_name(ctx.cr(rival)).to_owned());
                ctx.remember(
                    &other,
                    format!("{loved_name} chose {rival_name}. It hurts more than I expected."),
                    8.0,
                    MemoryKind::Event,
                    vec![loved.into(), rival.into()],
                );
                ctx.stimulate(
                    &other,
                    StimulusKind::Romance,
                    format!("You learned that {loved_name}, whom you care for, is now with {rival_name}."),
                    1,
                    Some(loved),
                );
                ctx.emit(SimEvent::Emote { id: other.clone(), emote: Emote::BrokenHeart });
            }
        }
    }
}

pub fn break_up(ctx: &mut Ctx, a: &str, b: &str, reason: &str) {
    ctx.c(a).partner_id = None;
    ctx.c(b).partner_id = None;
    let sour = RelationDelta { affinity: -15.0, attraction: -30.0, grievance: 12.0, ..Default::default() };
    adjust(ctx, a, b, sour);
    adjust(ctx, b, a, sour);
    let (a_name, b_name) = (first_name(ctx.cr(a)).to_owned(), first_name(ctx.cr(b)).to_owned());
    ctx.remember(a, format!("{b_name} and I have parted ({reason})."), 8.0, MemoryKind::Event, vec![b.into()]);
    ctx.remember(b, format!("{a_name} and I have parted ({reason})."), 8.0, MemoryKind::Event, vec![a.into()]);
    ctx.chronicle(ChronicleKind::Romance, format!("{a_name} and {b_name} have parted ways."), vec![a.into(), b.into()]);
    ctx.emit(SimEvent::Emote { id: a.into(), emote: Emote::BrokenHeart });
    ctx.emit(SimEvent::Emote { id: b.into(), emote: Emote::BrokenHeart });
}

/// Once a day: tempers cool, and couples who have turned on each other split.
pub fn daily_relationships(ctx: &mut Ctx) {
    let tick = ctx.tick();
    for id in living_ids(ctx.state) {
        if !ctx.alive(&id) {
            continue;
        }
        let others: Vec<Id> = ctx.cr(&id).relationships.keys().cloned().collect();
        for other in others {
            let c = ctx.c(&id);
            let (temper, spite) = (c.profile.temper, c.profile.spite);
            let r = &mut c.relationships[&other];
            if r.grievance > 0.0 {
                // The spiteful nurse their grudges and let them cool only half as fast.
                let recent = tick - r.last_offence_tick < TICKS_PER_DAY;
                let cooling = (if recent { 0.5 } else { 1.8 }) * (1.0 - temper * 0.6) * (1.0 - spite * 0.5);
                r.grievance = (r.grievance - cooling).max(0.0);
            }
            if ctx.alive(&other) {
                relabel(ctx, &id, &other);
            }
        }
        let Some(partner) = ctx.cr(&id).partner_id.clone().filter(|p| ctx.alive(p)) else { continue };
        if id < partner {
            let mine = rel(ctx.c(&id), &partner).clone();
            let theirs = rel(ctx.c(&partner), &id).clone();
            if mine.affinity < 12.0 || theirs.affinity < 12.0 || mine.grievance >= 50.0 || theirs.grievance >= 50.0 {
                break_up(ctx, &id, &partner, "too many quarrels");
            }
        }
    }
}

/// Whether `from` may take up arms against `to`: the top of the hostility ladder, with extra gates.
pub fn lethal_unlocked(ctx: &mut Ctx, from: &str, to_id: &str) -> bool {
    let tick = ctx.tick();
    let r = rel(ctx.c(from), to_id);
    if r.grievance < RUNG_BLOOD {
        return false;
    }
    if tick - r.last_offence_tick > TICKS_PER_DAY * 2 {
        return false;
    }
    if tick - ctx.state.director.last_lethal_tick < TICKS_PER_DAY * 3 {
        return false;
    }
    ctx.state.order.len() >= 8
}

/// The same check for a character the caller only reads (prompts, validation); never creates a record.
pub fn lethal_unlocked_read(state: &crate::state::SimState, from: &Character, to_id: &str) -> bool {
    let r = rel_or_default(from, to_id);
    r.grievance >= RUNG_BLOOD
        && state.tick - r.last_offence_tick <= TICKS_PER_DAY * 2
        && state.tick - state.director.last_lethal_tick >= TICKS_PER_DAY * 3
        && state.order.len() >= 8
}
