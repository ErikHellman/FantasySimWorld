//! Which memories a prompt recalls (port of `src/brain/memory/retrieval.ts`).
//! No embeddings: subject ids and shared words stand in for relevance.

use std::collections::HashSet;

use fsw_core::jsmath::pow;
use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::state::{Character, Memory, MemoryKind};

const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "was", "were", "have", "has", "had", "you", "your", "are", "not",
    "but", "from", "they", "them", "his", "her", "she", "him", "about", "into", "near", "just", "what", "when",
];

fn words(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_ascii_lowercase())
        .filter(|word| word.len() > 3 && !STOP_WORDS.contains(word))
        .map(str::to_owned)
        .collect()
}

pub struct Cue<'a> {
    /// people the situation involves
    pub subjects: &'a [String],
    /// free text describing the situation (stimuli, place, activity)
    pub text: &'a str,
}

/// The memories most worth recalling: recent, important, and about the people
/// or things at hand. Returned in chronological order.
pub fn retrieve<'c>(c: &'c Character, tick: i64, cue: Cue, count: usize) -> Vec<&'c Memory> {
    let cue_words = words(cue.text);
    let mut scored: Vec<(&Memory, f64)> = c
        .memories
        .iter()
        .map(|memory| {
            let age_days = (tick - memory.tick).max(0) as f64 / TICKS_PER_DAY as f64;
            let recency = pow(0.5, age_days / 1.5);
            let mut relevance = 0.0;
            for id in &memory.subjects {
                if cue.subjects.contains(id) {
                    relevance += 1.0;
                }
            }
            if !cue_words.is_empty() {
                let shared = words(&memory.text).iter().filter(|w| cue_words.contains(*w)).count();
                relevance += (shared as f64 * 0.34).min(1.0);
            }
            let bonus = if memory.kind == MemoryKind::Reflection { 0.35 } else { 0.0 };
            let score = recency + (memory.importance as f64 / 10.0) * 1.1 + relevance.min(2.0) * 0.9 + bonus;
            (memory, score)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut chosen: Vec<&Memory> = scored.into_iter().take(count).map(|(memory, _)| memory).collect();
    chosen.sort_by_key(|memory| memory.tick);
    chosen
}

/// Memories formed on or after a tick, most important first, then in order.
pub fn since(c: &Character, tick: i64, count: usize) -> Vec<&Memory> {
    let mut found: Vec<&Memory> =
        c.memories.iter().filter(|m| m.tick >= tick && m.kind != MemoryKind::Reflection).collect();
    found.sort_by(|a, b| b.importance.cmp(&a.importance).then(a.tick.cmp(&b.tick)));
    found.truncate(count);
    found.sort_by_key(|memory| memory.tick);
    found
}
