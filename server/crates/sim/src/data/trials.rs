//! What is said at a trial on the green (`systems::cases`): `{victim}`, `{accused}`, `{accuser}` and `{fact}`
//! are filled in with `data::ceremonies::line`.

/// The one who speaks the rite, to open it.
pub const TRIAL_OPENING: [&str; 3] = [
    "Neighbours, {victim} was struck down and nobody saw the hand that did it. {accused} stands accused. Hear it, and judge.",
    "We are here for {victim}, who cannot speak for themselves. {accused} is accused of the killing. Listen well.",
    "A killer walks among us, or stands before us. {accused}, you are accused of the death of {victim}.",
];

/// The accuser, laying out what they know.
pub const ACCUSATIONS: [&str; 3] = [
    "{fact}. I will swear to it before anyone.",
    "I know what I know: {fact}. Let {accused} explain that.",
    "{fact}. Tell me that means nothing.",
];

/// The accused, whether they did it or not; the guilty lie as well as anyone.
pub const PLEAS: [&str; 4] = [
    "I did not kill {victim}. I swear it on everything I have.",
    "This is madness. I never laid a hand on {victim}, and you all know it.",
    "Look at me. Do I look like a killer to you? I did not do this.",
    "You want someone to blame, and I am standing closest. I am innocent.",
];

/// A voice from the crowd against the accused.
pub const AGAINST: [&str; 3] =
    ["Guilty! Send them away!", "I always said there was something wrong about {accused}.", "Justice for {victim}!"];

/// A voice from the crowd for the accused.
pub const FOR: [&str; 3] =
    ["There is no proof! Let {accused} be!", "I know {accused}. They could never do this.", "Gossip is not evidence."];

/// The verdict, guilty.
pub const GUILTY: [&str; 2] = [
    "The village has spoken. {accused}, you are found guilty of killing {victim}. Leave the valley by the west road and never come back.",
    "It is decided: {accused} is guilty. Go, and do not return, or the valley will not be so kind a second time.",
];

/// The verdict, not guilty.
pub const ACQUITTED: [&str; 2] = [
    "The village is not persuaded. {accused} goes free. Whoever killed {victim} is still out there.",
    "There is not enough to send {accused} away. Go home, all of you, and keep your doors shut.",
];

/// The truth about a scene, pointing at someone: `{suspect}`.
pub const TRACES: [&str; 4] = [
    "a scrap of cloth like {suspect}'s was caught on a branch by the body",
    "boot prints in the mud by the body looked like {suspect}'s",
    "a button like one of {suspect}'s lay beside the body",
    "{suspect}'s name was scratched into the dirt by the body",
];
