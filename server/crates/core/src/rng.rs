//! Small seeded RNG (mulberry32). The state is a single `u32` that lives in the
//! sim's JSON state; every function takes it and advances it.

pub fn next_float(rng: &mut u32) -> f64 {
    *rng = rng.wrapping_add(0x6d2b_79f5);
    let mut t = *rng;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    f64::from(t ^ (t >> 14)) / 4294967296.0
}

pub fn range(rng: &mut u32, min: f64, max: f64) -> f64 {
    min + next_float(rng) * (max - min)
}

/// Integer in `[min, max]` inclusive.
pub fn int_range(rng: &mut u32, min: i64, max: i64) -> i64 {
    range(rng, min as f64, (max + 1) as f64).floor() as i64
}

pub fn chance(rng: &mut u32, probability: f64) -> bool {
    next_float(rng) < probability
}

/// Index of a uniformly picked item (`pick()` returns the item itself).
pub fn pick_index(rng: &mut u32, len: usize) -> usize {
    assert!(len > 0, "pick() from empty list");
    (next_float(rng) * len as f64).floor() as usize
}

pub fn pick<'a, T>(rng: &mut u32, items: &'a [T]) -> &'a T {
    &items[pick_index(rng, items.len())]
}

/// Index of an item picked with probability proportional to its weight (negative weights count as 0);
/// `None` when no item has weight.
pub fn weighted_pick_index<T>(rng: &mut u32, items: &[T], weight: impl Fn(&T) -> f64) -> Option<usize> {
    let mut total = 0.0;
    for item in items {
        total += weight(item).max(0.0);
    }
    if total <= 0.0 {
        return None;
    }
    let mut roll = next_float(rng) * total;
    for (index, item) in items.iter().enumerate() {
        roll -= weight(item).max(0.0);
        if roll <= 0.0 {
            return Some(index);
        }
    }
    items.len().checked_sub(1)
}

pub fn weighted_pick<'a, T>(rng: &mut u32, items: &'a [T], weight: impl Fn(&T) -> f64) -> Option<&'a T> {
    weighted_pick_index(rng, items, weight).map(|index| &items[index])
}

/// FNV-1a over UTF-16 code units, as `hashString` does with `charCodeAt`.
pub fn hash_string(text: &str) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for unit in text.encode_utf16() {
        h ^= u32::from(unit);
        h = h.wrapping_mul(16_777_619);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_typescript_sequence() {
        // From `src/core/rng.ts` with { rng: 1 }.
        let mut rng = 1;
        let first: Vec<f64> = (0..3).map(|_| next_float(&mut rng)).collect();
        assert_eq!(first, [0.6270739405881613, 0.002735721180215478, 0.5274470399599522]);
        assert_eq!(rng, 1_199_730_144);
    }

    #[test]
    fn hashes_like_typescript() {
        assert_eq!(hash_string(""), 2_166_136_261);
        assert_eq!(hash_string("3,4"), 3_511_042_106);
        assert_eq!(hash_string("héllo😀"), 4_113_148_722);
    }

    #[test]
    fn weighted_pick_skips_zero_weights() {
        let mut rng = 7;
        let items = [0.0, 0.0, 5.0];
        for _ in 0..20 {
            assert_eq!(weighted_pick_index(&mut rng, &items, |w| *w), Some(2));
        }
        assert_eq!(weighted_pick_index(&mut rng, &[0.0], |w| *w), None);
    }
}
