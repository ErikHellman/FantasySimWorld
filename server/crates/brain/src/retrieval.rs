//! Which memories a prompt recalls (port of `src/brain/memory/retrieval.ts`).
//! Relevance is the people involved plus either meaning (with embeddings, see
//! [`crate::recall`]) or shared words, memory by memory.

use std::collections::HashSet;

use fsw_core::jsmath::pow;
use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::state::{Character, Memory, MemoryKind};

use crate::recall::{MeaningScale, Recall};

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
pub fn retrieve<'c>(c: &'c Character, tick: i64, cue: Cue, count: usize, recall: Option<&Recall>) -> Vec<&'c Memory> {
    let cue_words = words(cue.text);
    let closeness: Vec<Option<f64>> =
        c.memories.iter().map(|memory| recall.and_then(|recall| recall.closeness(&memory.text))).collect();
    let scale = MeaningScale::of(&closeness.iter().flatten().copied().collect::<Vec<_>>());
    let mut scored: Vec<(&Memory, f64)> = c
        .memories
        .iter()
        .zip(&closeness)
        .map(|(memory, closeness)| {
            let age_days = (tick - memory.tick).max(0) as f64 / TICKS_PER_DAY as f64;
            let recency = pow(0.5, age_days / 1.5);
            let mut relevance = 0.0;
            for id in &memory.subjects {
                if cue.subjects.contains(id) {
                    relevance += 1.0;
                }
            }
            match scale.zip(*closeness) {
                Some((scale, closeness)) => relevance += scale.score(closeness),
                None if !cue_words.is_empty() => {
                    let shared = words(&memory.text).iter().filter(|w| cue_words.contains(*w)).count();
                    relevance += (shared as f64 * 0.34).min(1.0);
                }
                None => {}
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fsw_testing::create_test_state;

    use super::*;
    use crate::recall::{Recall, VectorCache};

    const NOW: i64 = 10 * TICKS_PER_DAY;

    fn memory(days_ago: i64, text: &str, importance: i64) -> Memory {
        let tick = NOW - days_ago * TICKS_PER_DAY;
        Memory {
            tick,
            day: tick / TICKS_PER_DAY,
            text: text.into(),
            importance,
            kind: MemoryKind::Event,
            subjects: vec![],
        }
    }

    fn villager(memories: Vec<Memory>) -> Character {
        let state = create_test_state(1);
        let mut c = state.characters.values().next().unwrap().clone();
        c.memories = memories;
        c
    }

    const FLAMES: &str = "Carried water while the flames took the mill roof.";
    const BREAD: &str = "Bought fresh bread from the baker.";

    #[test]
    fn meaning_finds_an_older_memory_that_shares_no_words_with_the_cue() {
        let c = villager(vec![memory(3, FLAMES, 3), memory(0, BREAD, 5)]);
        let cue = Cue { subjects: &[], text: "Smoke over the village" };
        assert_eq!(retrieve(&c, NOW, cue, 1, None)[0].text, BREAD);

        let cache = VectorCache::new("m");
        cache.insert(cache.key(FLAMES), Arc::from(vec![1.0f32, 0.0]));
        cache.insert(cache.key(BREAD), Arc::from(vec![0.0f32, 1.0]));
        let vector = [1.0f32, 0.0];
        let recall = Recall { lookup: &cache, cue: Some(&vector) };
        assert_eq!(
            retrieve(&c, NOW, Cue { subjects: &[], text: "Smoke over the village" }, 1, Some(&recall))[0].text,
            FLAMES
        );
    }

    /// A unit vector whose cosine with [1, 0] is `closeness`.
    fn at(closeness: f32) -> Arc<[f32]> {
        Arc::from(vec![closeness, (1.0 - closeness * closeness).sqrt()])
    }

    #[test]
    fn bunched_closeness_still_picks_the_memory_closest_in_meaning() {
        // Closeness as multilingual-e5 gives it for a smoke cue: the fire memory is only a little ahead.
        let memories = [
            (FLAMES, 3, 3, 0.82),
            (BREAD, 0, 5, 0.73),
            ("Mended the fence posts on the farm.", 0, 5, 0.74),
            ("Sigrid gave me a small gift.", 1, 4, 0.73),
            ("Halvard was killed near the ruins.", 2, 6, 0.75),
        ];
        let c = villager(memories.iter().map(|(text, days, importance, _)| memory(*days, text, *importance)).collect());
        let cache = VectorCache::new("m");
        for (text, _, _, closeness) in memories {
            cache.insert(cache.key(text), at(closeness));
        }
        let vector = [1.0f32, 0.0];
        let recall = Recall { lookup: &cache, cue: Some(&vector) };
        let cue = Cue { subjects: &[], text: "Smoke over the village" };
        assert_eq!(retrieve(&c, NOW, cue, 1, Some(&recall))[0].text, FLAMES);
    }

    #[test]
    fn without_vectors_the_result_is_the_keyword_result() {
        let c = villager(vec![memory(0, "The mill fire spread fast.", 3), memory(0, BREAD, 5), memory(2, FLAMES, 7)]);
        let text = "the mill fire";
        let keyword: Vec<&str> =
            retrieve(&c, NOW, Cue { subjects: &[], text }, 2, None).iter().map(|m| m.text.as_str()).collect();

        let empty = VectorCache::new("m");
        let vector = [1.0f32, 0.0];
        let no_vectors = Recall { lookup: &empty, cue: Some(&vector) };
        let with_empty: Vec<&str> = retrieve(&c, NOW, Cue { subjects: &[], text }, 2, Some(&no_vectors))
            .iter()
            .map(|m| m.text.as_str())
            .collect();
        assert_eq!(with_empty, keyword);

        // Vectors from another model (another size) count as none.
        let other = VectorCache::new("m");
        for m in &c.memories {
            other.insert(other.key(&m.text), Arc::from(vec![1.0f32, 0.0, 0.0]));
        }
        let mismatched = Recall { lookup: &other, cue: Some(&vector) };
        let with_other: Vec<&str> = retrieve(&c, NOW, Cue { subjects: &[], text }, 2, Some(&mismatched))
            .iter()
            .map(|m| m.text.as_str())
            .collect();
        assert_eq!(with_other, keyword);
    }
}
