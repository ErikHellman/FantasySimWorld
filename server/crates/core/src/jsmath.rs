//! JavaScript's `Math` where Rust's std differs, and transcendental functions
//! from `libm` so every machine computes the same bits.

/// `Math.round`: halves round towards +∞ (`-2.5` → `-2`), unlike `f64::round`.
pub fn js_round(x: f64) -> f64 {
    // `f64::round` rounds halves away from zero; only negative halves differ.
    // (`floor(x + 0.5)` would be wrong for 0.49999999999999994 and large odd integers.)
    let r = x.round();
    if x - r == 0.5 { r + 1.0 } else { r }
}

/// `Math.sign`: 0 (and NaN) keep their value, unlike `f64::signum`.
pub fn js_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// `Math.hypot(a, b)` as V8 and JavaScriptCore compute it: scaled by the
/// largest magnitude, Kahan-summed, then `sqrt * max`.
pub fn js_hypot(a: f64, b: f64) -> f64 {
    let max = a.abs().max(b.abs());
    if max.is_infinite() {
        return f64::INFINITY;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    let max = if max == 0.0 { 1.0 } else { max };
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for value in [a, b] {
        let scaled = value / max;
        let summand = scaled * scaled - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

pub fn pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

pub fn log(x: f64) -> f64 {
    libm::log(x)
}

/// `ToInt32`: what JavaScript's bitwise operators do to a number.
pub fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let m = x.trunc().rem_euclid(4294967296.0);
    (m as u32) as i32
}

/// Number of UTF-16 code units, which is what `String.length` counts.
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// `text.slice(0, max)` in UTF-16 code units, never splitting a surrogate pair.
pub fn utf16_truncate(text: &str, max: usize) -> &str {
    let mut units = 0;
    for (index, ch) in text.char_indices() {
        units += ch.len_utf16();
        if units > max {
            return &text[..index];
        }
    }
    text
}

/// How JavaScript prints a number with `String(x)` / template literals, for
/// the values the sim prints (integers and short decimals).
pub fn js_number_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.fract() == 0.0 && x.abs() < 1e21 {
        return format!("{}", x as i64);
    }
    // Rust's shortest round-trip formatting matches JavaScript's for ordinary magnitudes.
    format!("{x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_like_javascript() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(-0.4), 0.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
        assert_eq!(js_round(4503599627370497.0), 4503599627370497.0);
        assert_eq!(js_round(-3.7), -4.0);
    }

    #[test]
    fn hypot_and_sign() {
        assert_eq!(js_hypot(3.0, 4.0), 5.0);
        assert_eq!(js_hypot(0.0, 0.0), 0.0);
        assert_eq!(js_sign(0.0), 0.0);
        assert_eq!(js_sign(-2.0), -1.0);
    }

    #[test]
    fn utf16_lengths() {
        assert_eq!(utf16_len("a😀b"), 4);
        assert_eq!(utf16_truncate("a😀b", 2), "a");
        assert_eq!(utf16_truncate("a😀b", 3), "a😀");
        assert_eq!(to_int32(4294967297.0), 1);
        assert_eq!(to_int32(-1.0), -1);
        assert_eq!(js_number_string(3.0), "3");
        assert_eq!(js_number_string(0.5), "0.5");
    }
}
