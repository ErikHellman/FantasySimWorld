//! What villagers say to someone standing in their way, and what the hot-tempered say back. `{b}` is
//! the first name of the one in the way and `{a}` the walker's.

/// A word to someone in the way, from the gentle to the short-tempered.
pub static POLITE: [&str; 5] = [
    "Pardon me, {b}.",
    "Excuse me, {b}, may I get by?",
    "Mind if I squeeze past, {b}?",
    "Just getting by, {b}.",
    "Sorry, {b}, coming through.",
];

pub static CURT: [&str; 5] = [
    "{b}, you're in my way.",
    "Do you mind, {b}?",
    "Some of us have places to be, {b}.",
    "Shift over, {b}.",
    "You're blocking the path, {b}.",
];

/// Said loudly, with the anger showing.
pub static ANGRY: [&str; 5] = [
    "Move, {b}!",
    "Out of my way, {b}!",
    "Are you rooted there, {b}? Move!",
    "Get out of the road, {b}!",
    "Every time, {b}! Shift!",
];

/// The one in the way answering a sharp word back.
pub static RETORT: [&str; 4] =
    ["Walk round, then!", "There's room enough, {a}.", "Mind your tongue, {a}.", "Ask nicely next time, {a}."];
