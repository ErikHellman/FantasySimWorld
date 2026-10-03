//! What villagers say to the powers that watch the valley. `{name}` is the power's name,
//! or "lord" when it has none they can say.

/// On finding something in one's pack.
pub const THANKS: [&str; 5] = [
    "Thank you, {name}!",
    "Bless you, {name}. I will not forget this.",
    "{name} has remembered me. Thank you!",
    "A gift from {name}! Thank you, thank you.",
    "I am grateful, {name}.",
];

/// On seeing the dead rise where no grave is.
pub const PLEAS: [&str; 5] = [
    "Spare us, {name}!",
    "{name}, have mercy!",
    "What have we done to anger you, {name}?",
    "Call them back, {name}, I beg you!",
    "Mercy, {name}! Mercy!",
];

/// After a blessing on the whole village.
pub const PRAISE: [&str; 4] = [
    "Praise {name}! The valley is blessed.",
    "Thank you, {name}, for watching over us.",
    "{name} is good to us. Give thanks!",
    "Bless you, {name}!",
];

/// At prayer, to a power that has been kind.
pub const DEVOTIONS: [&str; 4] = [
    "{name}, watch over us another day.",
    "Thank you for your kindness, {name}.",
    "{name}, keep the valley in your care.",
    "I light this candle for you, {name}.",
];

/// At prayer, to a power that has been cruel.
pub const ENTREATIES: [&str; 4] = [
    "{name}, stay your hand tonight.",
    "Spare us, {name}. We are only small folk.",
    "{name}, let the dead lie still.",
    "Whatever we did, {name}, forgive it.",
];

/// A line from `lines` with the name put in.
pub fn line(template: &str, name: &str) -> String {
    template.replace("{name}", name)
}
