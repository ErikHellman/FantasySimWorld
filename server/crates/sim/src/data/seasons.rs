//! What is said at the village's feasts and at the choosing of its elder when the model has written nothing.
//! `{names}` is the candidates, already joined with "and", `{name}` one person, `{votes}` a tally.

/// The one who speaks the rite opens the harvest festival.
pub const HARVEST_SPEECH: [&str; 3] = [
    "Another harvest in, and all of us still here to eat it. Fill your cups, neighbours: this one is ours.",
    "The fields gave, the stores are full and the dead did not have us. That is worth a feast. Eat!",
    "We worked for this all year. Tonight nobody works. To the harvest, and to the valley!",
];

/// The one who speaks the rite opens midwinter.
pub const MIDWINTER_SPEECH: [&str; 3] = [
    "The longest night is here. Whatever lies between you and your neighbour, set it down until the sun comes back.",
    "Light the lanterns. Midwinter is for the living, and we are still living. Forgive someone tonight.",
    "The cold is outside and we are in here together. Let old quarrels freeze where they lie.",
];

/// Someone in the crowd at a feast.
pub const TOASTS: [&str; 4] = [
    "To the valley!",
    "To full stores and short winters!",
    "To everyone we lost this year. They would want us to drink.",
    "More ale! It's a feast, isn't it?",
];

/// The one who speaks the rite opens the choosing.
pub const ELECTION_OPENING: [&str; 2] = [
    "We are here to choose who speaks for this village. {names} have stood. Hear them, then choose.",
    "Neighbours, the village needs an elder. {names} would serve. Listen well.",
];

/// A candidate speaks for themselves.
pub const CAMPAIGN: [&str; 4] = [
    "I have kept watch, I have worked, and I have buried our dead beside you. Let me speak for you.",
    "I will not promise you an easy year. I will promise to be there for every hard night of it.",
    "This village deserves someone who listens. I listen. Choose me.",
    "You know me. You know what I have done here. That is all I ask you to weigh.",
];

/// The one who speaks the rite names the elder.
pub const ELECTION_RESULT: [&str; 2] = [
    "The village has chosen: {name} is our elder, {votes}. Let nobody say it was not fairly done.",
    "It is decided, {votes}. {name}, the village is in your keeping.",
];
