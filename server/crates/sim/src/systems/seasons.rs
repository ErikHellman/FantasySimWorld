//! The village's year and its elder. A year is four seasons of seven days, from spring on day 1. At the end of
//! autumn the village holds its harvest festival on the green, and in the depth of winter its midwinter feast; both
//! are ceremonies (`systems::ceremonies`), with a speech, a toast and a feast at the tavern. The harvest leaves
//! everyone in good spirits; midwinter cools old quarrels.
//!
//! Now and then the village chooses an elder: as soon as it is settled and has nobody, again when its elder dies
//! or is driven out, and every spring, when the sitting elder stands again. The two most respected grown-ups stand
//! (the sitting elder always among them), speak for themselves on the green, and everyone there votes for whoever
//! they think best of. The elder speaks the rite at every ceremony after.
//!
//! Nothing here draws from the RNG: the calendar is the day, and votes are what people feel.

use fsw_core::time::{TICKS_PER_DAY, game_time};

use crate::events::{Emote, SimEvent};
use crate::state::{Character, ChronicleKind, FestivalKind, Id, MemoryKind, Season, SimState};
use crate::systems::relationships::{RelationDelta, adjust};
use crate::util::{Ctx, first_name, living, rel_or_default};

/// Days in a season, and seasons in a year.
pub const DAYS_PER_SEASON: i64 = 7;
pub const DAYS_PER_YEAR: i64 = DAYS_PER_SEASON * 4;
/// The harvest is held from the sixth day of autumn, midwinter from the fourth of winter; a feast put off by a
/// wedding, a funeral or a trial is held on a later day of the same season, or not that year.
const HARVEST_FROM: i64 = 6;
const MIDWINTER_FROM: i64 = 4;
/// Grown-ups the village needs before it chooses an elder, and the first day it may.
const ELECTION_ADULTS: usize = 4;
const FIRST_ELECTION_DAY: i64 = 3;
/// How many stand.
const CANDIDATES: usize = 2;
/// How long the good spirits of the harvest last.
const HARVEST_HEART_TICKS: i64 = TICKS_PER_DAY / 4;
/// How much midwinter takes off every grudge among those who keep it.
const MIDWINTER_PEACE: f64 = 8.0;

/// The year a day falls in, from 1.
pub fn year_of(day: i64) -> i64 {
    (day - 1).max(0) / DAYS_PER_YEAR + 1
}

/// The season a day falls in.
pub fn season_of(day: i64) -> Season {
    Season::ALL[((day - 1).max(0) % DAYS_PER_YEAR / DAYS_PER_SEASON) as usize]
}

/// Which day of its season a day is, from 1.
pub fn day_of_season(day: i64) -> i64 {
    (day - 1).max(0) % DAYS_PER_SEASON + 1
}

/// The season now.
pub fn season_now(state: &SimState) -> Season {
    season_of(game_time(state.tick).day)
}

/// The feast the village is due to hold today, if any and it has not been held this year.
pub fn festival_due(state: &SimState, day: i64) -> Option<FestivalKind> {
    let (season, at, year) = (season_of(day), day_of_season(day), year_of(day));
    match season {
        Season::Autumn if at >= HARVEST_FROM && state.village.harvest_year < year => Some(FestivalKind::Harvest),
        Season::Winter if at >= MIDWINTER_FROM && state.village.midwinter_year < year => Some(FestivalKind::Midwinter),
        _ => None,
    }
}

/// The elder, while they still live in the village.
pub fn elder(state: &SimState) -> Option<&Character> {
    let id = state.village.elder.as_deref()?;
    state.characters.get(id).filter(|c| c.exiled_since.is_none())
}

fn grown(c: &Character) -> bool {
    !c.is_child() && c.exiled_since.is_none()
}

/// Whether the village should choose an elder today: it has none, or it is the first day of a new year.
pub fn election_due(state: &SimState, day: i64) -> bool {
    if state.director.founding
        || day < FIRST_ELECTION_DAY
        || living(state).filter(|c| grown(c)).count() < ELECTION_ADULTS
    {
        return false;
    }
    let year = year_of(day);
    let new_year = season_of(day) == Season::Spring && day_of_season(day) == 1 && state.village.election_year < year;
    elder(state).is_none() || new_year
}

/// What `c` thinks of `of` as someone to lead: liking and trust.
fn regard(c: &Character, of: &str) -> f64 {
    let r = rel_or_default(c, of);
    r.affinity + r.trust * 0.5
}

/// How well the village as a whole thinks of someone.
fn standing(state: &SimState, id: &str) -> f64 {
    living(state).filter(|o| o.id != id && grown(o)).map(|o| regard(o, id)).sum()
}

/// Who stands: the sitting elder, if there is one, and the most respected grown-ups after them.
pub fn candidates(state: &SimState) -> Vec<Id> {
    let mut ranked: Vec<(&Character, f64)> =
        living(state).filter(|c| grown(c)).map(|c| (c, standing(state, &c.id))).collect();
    // A stable sort keeps the village's own order among equals.
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut chosen: Vec<Id> = elder(state).map(|c| c.id.clone()).into_iter().collect();
    for (c, _) in ranked {
        if chosen.len() >= CANDIDATES {
            break;
        }
        if !chosen.contains(&c.id) {
            chosen.push(c.id.clone());
        }
    }
    chosen
}

/// Each voter's choice: themselves if they stand, otherwise whoever they think best of (the first named, among
/// equals). Returns the votes for each candidate, in the candidates' order.
pub fn tally(state: &SimState, candidates: &[Id], voters: &[Id]) -> Vec<usize> {
    let mut votes = vec![0; candidates.len()];
    for voter in voters {
        let Some(c) = state.characters.get(voter).filter(|c| grown(c)) else { continue };
        let pick = candidates.iter().position(|id| id == voter).or_else(|| {
            let mut best: Option<(usize, f64)> = None;
            for (at, id) in candidates.iter().enumerate() {
                let score = regard(c, id);
                if best.is_none_or(|(_, b)| score > b) {
                    best = Some((at, score));
                }
            }
            best.map(|(at, _)| at)
        });
        if let Some(at) = pick {
            votes[at] += 1;
        }
    }
    votes
}

/// The winner of a tally: the most votes, the first named among equals (so a sitting elder keeps a tied seat).
pub fn winner(votes: &[usize]) -> usize {
    let mut best = 0;
    for (at, &count) in votes.iter().enumerate() {
        if count > votes[best] {
            best = at;
        }
    }
    best
}

/// "5 votes to 3", or "5 votes to 3 to 1".
pub fn votes_words(votes: &[usize]) -> String {
    let mut sorted = votes.to_vec();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    let counts: Vec<String> = sorted.iter().map(usize::to_string).collect();
    let first = sorted.first().copied().unwrap_or(0);
    format!("{} vote{} to {}", counts[0], if first == 1 { "" } else { "s" }, counts[1..].join(" to "))
}

/// The village has chosen: the elder is seated, and everyone who stood or voted remembers it.
pub fn seat_elder(ctx: &mut Ctx, candidates: &[Id], votes: &[usize], voters: &[Id]) -> Id {
    let at = winner(votes);
    let chosen = candidates[at].clone();
    let day = ctx.time.day;
    let before = ctx.state.village.elder.clone().filter(|id| ctx.state.characters.contains_key(id));
    ctx.state.village.elder = Some(chosen.clone());
    ctx.state.village.elder_day = day;
    ctx.state.village.election_year = year_of(day);
    let name = first_name(ctx.cr(&chosen)).to_owned();
    let tally = votes_words(votes);
    ctx.remember(
        &chosen,
        format!("The village chose me as its elder today, {tally}. I must not let them down."),
        9.0,
        MemoryKind::Event,
        Vec::new(),
    );
    ctx.emit(SimEvent::Emote { id: chosen.clone(), emote: Emote::Heart });
    for loser in candidates.iter().filter(|id| **id != chosen) {
        // Losing stings, and more for a sitting elder turned out.
        let sting = if before.as_deref() == Some(loser.as_str()) { 12.0 } else { 5.0 };
        adjust(ctx, loser, &chosen, RelationDelta { grievance: sting, ..Default::default() });
        let text = if before.as_deref() == Some(loser.as_str()) {
            format!("The village turned me out as its elder and chose {name} instead, {tally}.")
        } else {
            format!("I stood to be the village's elder and lost to {name}, {tally}.")
        };
        ctx.remember(loser, text, 7.0, MemoryKind::Event, vec![chosen.clone()]);
    }
    for voter in voters.iter().filter(|id| !candidates.contains(id)) {
        ctx.remember(
            voter,
            format!("The village chose {name} as its elder on the green, {tally}."),
            5.0,
            MemoryKind::Event,
            vec![chosen.clone()],
        );
    }
    let text = match before.as_deref() {
        Some(old) if old == chosen => format!("The village has kept {name} as its elder, {tally}."),
        Some(old) => {
            format!("The village has chosen {name} as its elder in place of {}, {tally}.", first_name(ctx.cr(old)))
        }
        None => format!("The village has chosen {name} as its elder, {tally}."),
    };
    let mut ids = vec![chosen.clone()];
    ids.extend(candidates.iter().filter(|id| **id != chosen).cloned());
    ctx.chronicle(ChronicleKind::Social, text, ids);
    chosen
}

/// The feast is held: it is marked for the year, and those who kept it are left in good spirits (the harvest) or a
/// little more at peace with one another (midwinter).
pub fn keep_festival(ctx: &mut Ctx, kind: FestivalKind, present: &[Id]) {
    let tick = ctx.tick();
    let year = year_of(ctx.time.day);
    match kind {
        FestivalKind::Harvest => {
            ctx.state.village.harvest_year = year;
            for id in present {
                ctx.c(id).heartened_until_tick = tick + HARVEST_HEART_TICKS;
                ctx.remember(
                    id,
                    "Kept the harvest festival on the green with the whole village.",
                    5.0,
                    MemoryKind::Event,
                    Vec::new(),
                );
            }
            ctx.chronicle(ChronicleKind::Social, "The village kept its harvest festival on the green.", Vec::new());
        }
        FestivalKind::Midwinter => {
            ctx.state.village.midwinter_year = year;
            for from in present {
                for to in present {
                    if from != to && rel_or_default(ctx.cr(from), to).grievance > 0.0 {
                        adjust(ctx, from, to, RelationDelta { grievance: -MIDWINTER_PEACE, ..Default::default() });
                    }
                }
                ctx.remember(
                    from,
                    "Kept midwinter on the green. For one night, old quarrels were set down.",
                    5.0,
                    MemoryKind::Event,
                    Vec::new(),
                );
            }
            ctx.chronicle(
                ChronicleKind::Social,
                "The village kept midwinter on the green, and set old quarrels down for the night.",
                Vec::new(),
            );
        }
    }
}

/// At the midnight roll-over: a new season is announced.
pub fn daily_seasons(ctx: &mut Ctx) {
    let day = ctx.time.day;
    if day > 1 && day_of_season(day) == 1 {
        let season = season_of(day);
        let text = match season {
            Season::Spring => format!("Spring comes to the valley, and with it year {}.", year_of(day)),
            _ => format!("{} comes to the valley.", capitalised(season.as_str())),
        };
        ctx.chronicle(ChronicleKind::World, text, Vec::new());
    }
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().collect::<String>() + chars.as_str())
}

/// The name of a feast, for the chronicle and the banner.
pub fn festival_name(kind: FestivalKind) -> &'static str {
    match kind {
        FestivalKind::Harvest => "the harvest festival",
        FestivalKind::Midwinter => "midwinter",
    }
}
