//! The Valley Herald: each morning, a front page about the day before, for those watching the valley. The model
//! writes it from the chronicle at dawn; if it has not by eight, the sim sets one from the chronicle itself. The
//! last few dozen issues are kept as an archive. No villager reads it: it is for viewers, and nothing in it
//! reaches the village.
//!
//! Nothing here draws from the RNG.

use fsw_core::jsmath::utf16_truncate;
use fsw_core::time::{TICKS_PER_GAME_MINUTE, game_time};

use crate::events::SimEvent;
use crate::state::{ChronicleEntry, ChronicleKind, HeraldIssue, SimState};
use crate::systems::billboards::clean_text;
use crate::util::Ctx;

/// The most a headline may hold, in UTF-16 code units.
pub const HEADLINE_CAP: usize = 100;
/// The most a story or the gossip may hold, in UTF-16 code units.
pub const STORY_CAP: usize = 280;
/// The most stories beside the headline.
pub const STORIES_MAX: usize = 3;
/// How many issues are kept.
pub const ISSUES_KEPT: usize = 30;
/// The paper is wanted from dawn (06:00) ...
const DAWN_MINUTE: f64 = 360.0;
/// ... and if the model has not written it by eight, the sim sets it itself.
const LATEST_MINUTE: f64 = 480.0;
/// How the chronicle announces an issue; the Herald leaves these out of its own news.
pub const HERALD_LINE: &str = "The Valley Herald is out: ";

/// The day the Herald is waiting for an issue for: today, from dawn on the second day, unless it is out already
/// or the village is still being founded.
pub fn wants_issue(state: &SimState) -> Option<i64> {
    let time = game_time(state.tick);
    if time.day < 2 || time.minute_of_day < DAWN_MINUTE || state.director.founding {
        return None;
    }
    if state.herald.last().is_some_and(|issue| issue.day >= time.day) {
        return None;
    }
    Some(time.day)
}

/// What the issue for `day` may tell of: the chronicle of the day before, leaving out the dawn and the Herald's
/// own announcements.
pub fn news(state: &SimState, day: i64) -> Vec<&ChronicleEntry> {
    state
        .chronicle
        .iter()
        .filter(|entry| game_time(entry.tick).day == day - 1)
        .filter(|entry| {
            entry.kind != ChronicleKind::World
                || !(entry.text.starts_with(HERALD_LINE) || entry.text.ends_with("dawns over the valley."))
        })
        .collect()
}

/// How much a kind of news counts for when the sim sets the page itself.
pub fn weight(kind: ChronicleKind) -> u8 {
    match kind {
        ChronicleKind::Death => 6,
        ChronicleKind::Fire => 5,
        ChronicleKind::Combat | ChronicleKind::Conflict => 4,
        ChronicleKind::Romance | ChronicleKind::Arrival => 3,
        ChronicleKind::Social => 2,
        ChronicleKind::Trade | ChronicleKind::World => 1,
    }
}

/// The page the sim sets when the model has not written one: the weightiest of yesterday's news as the headline
/// (the earliest, among equals), the next few as stories, and a bit of romance or talk as gossip.
pub fn stock_issue(state: &SimState, day: i64) -> HeraldIssue {
    let mut ranked = news(state, day);
    // A stable sort keeps the day's order among news of the same weight.
    ranked.sort_by_key(|entry| std::cmp::Reverse(weight(entry.kind)));
    let quiet = || HeraldIssue {
        day,
        headline: "All quiet in the valley".into(),
        stories: vec!["Nothing worth printing happened yesterday: the village went about its work.".into()],
        gossip: None,
        by_model: false,
    };
    let Some((top, rest)) = ranked.split_first() else { return quiet() };
    let gossip_at = rest.iter().position(|entry| matches!(entry.kind, ChronicleKind::Romance | ChronicleKind::Social));
    let gossip = gossip_at.map(|at| rest[at].text.clone());
    let stories = rest
        .iter()
        .enumerate()
        .filter(|&(at, _)| Some(at) != gossip_at)
        .take(STORIES_MAX - 1)
        .map(|(_, entry)| entry.text.clone())
        .collect();
    tidy(HeraldIssue { day, headline: top.text.clone(), stories, gossip, by_model: false }).unwrap_or_else(quiet)
}

/// An issue fit to print: every line cleaned and capped, empty stories dropped. `None` without a headline.
pub fn tidy(issue: HeraldIssue) -> Option<HeraldIssue> {
    let line = |text: &str, cap: usize| clean_text(text).map(|text| utf16_truncate(&text, cap).to_owned());
    let headline = line(&issue.headline, HEADLINE_CAP)?;
    let stories = issue.stories.iter().filter_map(|story| line(story, STORY_CAP)).take(STORIES_MAX).collect();
    let gossip = issue.gossip.as_deref().and_then(|gossip| line(gossip, STORY_CAP));
    Some(HeraldIssue { headline, stories, gossip, ..issue })
}

/// Puts an issue out, if it is the one wanted: into the archive, onto the chronicle, and to viewers.
pub fn publish(ctx: &mut Ctx, issue: HeraldIssue) {
    if wants_issue(ctx.state) != Some(issue.day) {
        return;
    }
    let Some(issue) = tidy(issue) else { return };
    let (day, headline) = (issue.day, issue.headline.clone());
    let herald = &mut ctx.state.herald;
    herald.push(issue);
    if herald.len() > ISSUES_KEPT {
        herald.drain(..herald.len() - ISSUES_KEPT);
    }
    ctx.chronicle(ChronicleKind::World, format!("{HERALD_LINE}{headline}"), Vec::new());
    ctx.emit(SimEvent::Herald { day });
}

/// Once a game minute: past eight with no issue out, the sim sets one itself.
pub fn update_herald(ctx: &mut Ctx) {
    if ctx.tick() % TICKS_PER_GAME_MINUTE != 0 || ctx.time.minute_of_day < LATEST_MINUTE {
        return;
    }
    if let Some(day) = wants_issue(ctx.state) {
        let issue = stock_issue(ctx.state, day);
        publish(ctx, issue);
    }
}
