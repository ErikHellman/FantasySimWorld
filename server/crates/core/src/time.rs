//! Simulation timing. One game minute is one real second at 1x speed.

use crate::jsmath::js_round;

pub const TICKS_PER_SECOND: i64 = 20;
pub const TICK_SECONDS: f64 = 1.0 / TICKS_PER_SECOND as f64;
pub const TICKS_PER_GAME_MINUTE: i64 = 20;
pub const MINUTES_PER_DAY: i64 = 24 * 60;
pub const TICKS_PER_DAY: i64 = MINUTES_PER_DAY * TICKS_PER_GAME_MINUTE;
pub const GAME_MINUTES_PER_TICK: f64 = 1.0 / TICKS_PER_GAME_MINUTE as f64;

/// The world starts at 06:00 on day 1.
pub const START_MINUTE: i64 = 6 * 60;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GameTime {
    pub day: i64,
    /// minutes since midnight, fractional
    pub minute_of_day: f64,
    pub hour: f64,
    /// total game minutes since the start of day 1
    pub total_minutes: f64,
}

pub fn game_time(tick: i64) -> GameTime {
    let total_minutes = START_MINUTE as f64 + tick as f64 * GAME_MINUTES_PER_TICK;
    let day = (total_minutes / MINUTES_PER_DAY as f64).floor() as i64 + 1;
    let minute_of_day = total_minutes % MINUTES_PER_DAY as f64;
    GameTime { day, minute_of_day, hour: minute_of_day / 60.0, total_minutes }
}

pub fn minutes_to_ticks(minutes: f64) -> i64 {
    js_round(minutes * TICKS_PER_GAME_MINUTE as f64) as i64
}

pub fn seconds_to_ticks(seconds: f64) -> i64 {
    (js_round(seconds * TICKS_PER_SECOND as f64) as i64).max(1)
}

pub fn format_clock(minute_of_day: f64) -> String {
    let h = (minute_of_day / 60.0).floor() as i64 % 24;
    let m = (minute_of_day % 60.0).floor() as i64;
    format!("{h:02}:{m:02}")
}

pub fn is_night(hour: f64) -> bool {
    !(5.5..21.0).contains(&hour)
}

pub fn part_of_day(hour: f64) -> &'static str {
    if hour < 5.5 {
        "night"
    } else if hour < 8.0 {
        "early morning"
    } else if hour < 12.0 {
        "morning"
    } else if hour < 14.0 {
        "midday"
    } else if hour < 18.0 {
        "afternoon"
    } else if hour < 21.0 {
        "evening"
    } else {
        "night"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_start_at_six() {
        let t = game_time(0);
        assert_eq!((t.day, t.minute_of_day, t.hour), (1, 360.0, 6.0));
        let t = game_time(TICKS_PER_DAY * 3 / 4);
        assert_eq!((t.day, t.minute_of_day), (2, 0.0));
        assert_eq!(format_clock(game_time(1234).minute_of_day), "07:01");
        assert_eq!(minutes_to_ticks(1.5), 30);
        assert_eq!(seconds_to_ticks(0.01), 1);
        assert_eq!(part_of_day(22.0), "night");
    }
}
