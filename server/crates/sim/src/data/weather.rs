//! What villagers say when the weather turns, and the words in a villager's description that tell how
//! they take to it (`systems::weather::taste`).

/// Said by someone who loves the rain as it comes on.
pub static LOVE_RAIN: [&str; 5] = [
    "Ah, rain at last!",
    "Smell that? Rain on dry earth.",
    "Good for the fields, this.",
    "I could stand out in this all day.",
    "Listen to it on the roofs.",
];

/// Said by someone who cannot abide it.
pub static HATE_RAIN: [&str; 5] = [
    "Rain again. Wonderful.",
    "I'll be soaked through.",
    "Not this again.",
    "Somebody find me a roof.",
    "Miserable weather.",
];

pub static LOVE_WIND: [&str; 4] = ["Now there's a wind!", "Feel that breeze!", "Good drying weather.", "Blow, then!"];

pub static HATE_WIND: [&str; 4] =
    ["This wind goes right through me.", "Hold on to your hat.", "Wretched wind.", "Will it never stop blowing?"];

/// Said by those who like a fair day as the clouds part.
pub static LOVE_SUN: [&str; 4] = ["Sun's out!", "What a fine day.", "Finally, some sun.", "That's better."];

/// Shouted as the hail starts.
pub static HAIL: [&str; 4] = ["Hail! Get inside!", "Ow! Run for it!", "Cover your heads!", "Indoors, everyone!"];

/// Words in a villager's trade, traits, goals or story that mean they love the rain.
pub static RAIN_LOVERS: [&str; 11] = [
    "farmer",
    "farming",
    "gardener",
    "garden",
    "fisher",
    "fisherman",
    "poet",
    "melancholy",
    "brooding",
    "dreamer",
    "herbalist",
];

/// Words that mean they hate it.
pub static RAIN_HATERS: [&str; 8] = ["merchant", "vain", "tailor", "frail", "fussy", "proud", "elegant", "sickly"];

pub static WIND_LOVERS: [&str; 6] = ["sailor", "miller", "wild", "restless", "adventurous", "carefree"];

pub static WIND_HATERS: [&str; 5] = ["timid", "nervous", "anxious", "scholar", "frail"];
