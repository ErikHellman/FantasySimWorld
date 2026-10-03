//! Allure: a character who flirts for sport. She charms admirers into wanting
//! her, sets them against each other and against whoever she dislikes, takes
//! their gifts and enjoys watching them quarrel over her. Everything here is
//! keyed on `Profile::allure` and draws nothing from the RNG, so a village
//! without a charmer lives exactly as it did before.

use fsw_core::math2d::dist;

use crate::data::activities::ITEM_CAP;
use crate::events::{Emote, SimEvent};
use crate::state::{ChronicleKind, Id, MemoryKind, SimState, StimulusKind};
use crate::systems::relationships::{RUNG_BLOOD, RUNG_GRUDGE, RelationDelta, adjust};
use crate::util::{Ctx, first_name, is_present, join_names, living, living_ids, nearby_characters, rel_or_default};

/// Allure at which a character flirts for the fun of it rather than from the heart.
pub const CHARMER: f64 = 0.5;
/// How drawn to her someone must already be before she bothers to flirt with them.
pub const MARK: f64 = 20.0;
/// Attraction that makes someone an admirer: the same bar as "you are drawn to them" in the prompts.
pub const ADMIRER: f64 = 35.0;
/// Attraction a romantic talk with a charmer adds at full allure, with or without chemistry.
pub const CHARM: f64 = 6.0;
/// Jealousy alone can carry rivals to a feud and a fist fight, never to a blood feud.
pub const JEALOUSY_CAP: f64 = RUNG_BLOOD - 10.0;
const EARSHOT: f64 = 14.0;

pub fn allure(state: &SimState, id: &str) -> f64 {
    state.characters.get(id).map_or(0.0, |c| c.profile.allure)
}

fn attraction(state: &SimState, from: &str, to_id: &str) -> f64 {
    state.characters.get(from).and_then(|c| c.relationships.get(to_id)).map_or(0.0, |r| r.attraction)
}

/// The living characters drawn to `of` at `min` or more, in the stable order.
pub fn admirers(state: &SimState, of: &str, min: f64) -> Vec<Id> {
    living(state).filter(|c| c.id != of && attraction(state, &c.id, of) >= min).map(|c| c.id.clone()).collect()
}

/// After a romantic talk between `a` and `b`: admirers of a charmer who watch
/// her flirt resent the one she flirted with.
pub fn stir_jealousy(ctx: &mut Ctx, a: &str, b: &str) {
    for (charmer, mark) in [(a, b), (b, a)] {
        if allure(ctx.state, charmer) <= 0.0 || !ctx.alive(mark) {
            continue;
        }
        let c = ctx.cr(charmer);
        let (pos, partner) = (c.pos, c.partner_id.clone());
        let (charmer_name, mark_name) = (first_name(c).to_owned(), first_name(ctx.cr(mark)).to_owned());
        let mut jealous = Vec::new();
        for witness in nearby_characters(ctx.state, pos, EARSHOT, None) {
            // Her partner already takes it badly in `finish`.
            if witness == a || witness == b || partner.as_deref() == Some(witness.as_str()) || ctx.cr(&witness).asleep {
                continue;
            }
            let wanting = attraction(ctx.state, &witness, charmer);
            let towards_mark = rel_or_default(ctx.cr(&witness), mark);
            if wanting < ADMIRER || towards_mark.kin {
                continue;
            }
            let rise = (8.0 + (wanting - ADMIRER) * 0.25).min(JEALOUSY_CAP - towards_mark.grievance).max(0.0);
            adjust(ctx, &witness, mark, RelationDelta { grievance: rise, affinity: -5.0, ..Default::default() });
            ctx.remember(
                &witness,
                format!("Saw {charmer_name} flirting with {mark_name}. I will not stand for it."),
                6.0,
                MemoryKind::Event,
                vec![charmer.to_owned(), mark.to_owned()],
            );
            ctx.stimulate(
                &witness,
                StimulusKind::Romance,
                format!("You just watched {charmer_name}, whom you want, flirting with {mark_name}."),
                1,
                Some(mark),
            );
            ctx.emit(SimEvent::Emote { id: witness.clone(), emote: Emote::Anger });
            jealous.push(witness);
        }
        if !jealous.is_empty() {
            let names: Vec<String> = jealous.iter().map(|id| first_name(ctx.cr(id)).to_owned()).collect();
            let mut ids = vec![charmer.to_owned(), mark.to_owned()];
            ids.extend(jealous);
            ctx.chronicle(
                ChronicleKind::Romance,
                format!("{} watched {charmer_name} flirt with {mark_name}, and did not like it.", join_names(&names)),
                ids,
            );
        }
    }
}

/// The one `charmer` most dislikes, other than `except`: her worst grievance net of affection.
fn worst_enemy(state: &SimState, charmer: &str, except: &str) -> Option<Id> {
    let c = state.characters.get(charmer)?;
    let mut worst: Option<(Id, f64)> = None;
    for (id, r) in &c.relationships {
        if id.as_str() == except || !state.characters.contains_key(id) || (r.grievance < 10.0 && r.affinity > -10.0) {
            continue;
        }
        let score = r.grievance - r.affinity;
        if worst.as_ref().is_none_or(|(_, best)| score > *best) {
            worst = Some((id.clone(), score));
        }
    }
    worst.map(|(id, _)| id)
}

/// After a flirt that went down well: a charmer with an admirer in her hand
/// pours poison in his ear about her enemy and walks off with his gift.
pub fn work_charm(ctx: &mut Ctx, charmer: &str, mark: &str) {
    if allure(ctx.state, charmer) < CHARMER || attraction(ctx.state, mark, charmer) < ADMIRER {
        return;
    }
    let (charmer_name, mark_name) = (first_name(ctx.cr(charmer)).to_owned(), first_name(ctx.cr(mark)).to_owned());
    if let Some(enemy) = worst_enemy(ctx.state, charmer, mark) {
        let r = rel_or_default(ctx.cr(mark), &enemy);
        if !r.kin && r.affinity < 60.0 && r.grievance < RUNG_GRUDGE {
            let enemy_name = first_name(ctx.cr(&enemy)).to_owned();
            adjust(
                ctx,
                mark,
                &enemy,
                RelationDelta { grievance: 6.0, affinity: -4.0, trust: -4.0, ..Default::default() },
            );
            ctx.remember(
                mark,
                format!("{charmer_name} confided in me about {enemy_name}. I do not like what I heard."),
                5.0,
                MemoryKind::Talk,
                vec![charmer.to_owned(), enemy.clone()],
            );
            ctx.remember(
                charmer,
                format!("Dropped a word about {enemy_name} in {mark_name}'s ear."),
                3.0,
                MemoryKind::Thought,
                vec![mark.to_owned(), enemy],
            );
        }
    }
    if ctx.cr(mark).inventory.gift > 0 && ctx.cr(charmer).inventory.gift < ITEM_CAP {
        ctx.c(mark).inventory.gift -= 1;
        ctx.c(charmer).inventory.gift += 1;
        ctx.remember(
            mark,
            format!("Gave {charmer_name} a gift, and got a smile for it."),
            5.0,
            MemoryKind::Event,
            vec![charmer.to_owned()],
        );
        ctx.remember(
            charmer,
            format!("{mark_name} handed me a gift without my even asking."),
            4.0,
            MemoryKind::Event,
            vec![mark.to_owned()],
        );
        ctx.emit(SimEvent::Emote { id: charmer.to_owned(), emote: Emote::Gift });
        ctx.chronicle(
            ChronicleKind::Social,
            format!("{charmer_name} smiled, and {mark_name} handed over a gift."),
            vec![charmer.to_owned(), mark.to_owned()],
        );
    }
}

/// When two admirers of a charmer quarrel within her earshot, or come to blows
/// anywhere (word gets round), she enjoys it. In a brawl `x` is the winner.
pub fn revel(ctx: &mut Ctx, x: &str, y: &str, brawl: bool) {
    if !ctx.alive(x) || !ctx.alive(y) {
        return;
    }
    let pos = ctx.cr(x).pos;
    let (x_name, y_name) = (first_name(ctx.cr(x)).to_owned(), first_name(ctx.cr(y)).to_owned());
    for z in living_ids(ctx.state) {
        let zc = ctx.cr(&z);
        if z == x || z == y || zc.profile.allure <= 0.0 {
            continue;
        }
        if attraction(ctx.state, x, &z) < ADMIRER || attraction(ctx.state, y, &z) < ADMIRER {
            continue;
        }
        if !brawl && (!is_present(zc) || dist(zc.pos, pos) > EARSHOT) {
            continue;
        }
        let z_name = first_name(zc).to_owned();
        let c = ctx.c(&z);
        c.needs.social = (c.needs.social + if brawl { 20.0 } else { 10.0 }).min(100.0);
        let what = if brawl { "came to blows" } else { "quarrelled" };
        ctx.remember(
            &z,
            format!("{x_name} and {y_name} {what} over me. Delicious."),
            6.0,
            MemoryKind::Event,
            vec![x.to_owned(), y.to_owned()],
        );
        ctx.stimulate(
            &z,
            StimulusKind::Romance,
            format!("{x_name} and {y_name}, who both want you, just {what}."),
            1,
            Some(x),
        );
        ctx.emit(SimEvent::Emote { id: z.clone(), emote: Emote::Laugh });
        if brawl {
            adjust(ctx, &z, x, RelationDelta { affinity: 4.0, ..Default::default() });
            ctx.chronicle(
                ChronicleKind::Romance,
                format!("Everyone knows what {x_name} and {y_name} were really fighting over: {z_name}."),
                vec![z.clone(), x.to_owned(), y.to_owned()],
            );
        }
    }
}
