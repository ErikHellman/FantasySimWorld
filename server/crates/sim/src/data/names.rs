//! Words the sim invents villagers from when no model writes them: names in the style of the valley's
//! first cast (Vane, Ashdown, Stonefist, Greymane…), traits, trades, turns of a life and ways of speaking.

pub const FIRST_NAMES_SHE: [&str; 20] = [
    "Edda", "Wren", "Tamsin", "Ysolt", "Linnea", "Marta", "Senna", "Hild", "Maud", "Ottilie", "Rosalind", "Thea",
    "Agnes", "Bryony", "Cerys", "Dagny", "Elspeth", "Freya", "Gwen", "Ida",
];
pub const FIRST_NAMES_HE: [&str; 20] = [
    "Bram", "Osric", "Hale", "Garnet", "Perrin", "Cade", "Jory", "Alder", "Anselm", "Berrick", "Cuthbert", "Dunstan",
    "Edric", "Wilmot", "Godric", "Hollis", "Ivo", "Jasper", "Leofric", "Merrick",
];
pub const FIRST_NAMES_THEY: [&str; 10] =
    ["Ash", "Rook", "Sorrel", "Fen", "Tansy", "Marsh", "Haze", "Linden", "Reed", "Wyn"];

pub const LAST_NAMES: [&str; 36] = [
    "Vane",
    "Ashdown",
    "Stonefist",
    "Mistral",
    "Fenwick",
    "Thornwood",
    "Quickfingers",
    "Oakheart",
    "Briar",
    "Copperkettle",
    "Greymane",
    "Dawnmere",
    "Nightbloom",
    "Haybarrow",
    "Holloway",
    "Emberhorn",
    "Farrow",
    "Ironbraid",
    "Hale",
    "Marrow",
    "Underhill",
    "Blackwater",
    "Fairweather",
    "Stonebridge",
    "Larkspur",
    "Holt",
    "Greenmantle",
    "Ashby",
    "Wolfe",
    "Tanner",
    "Thistlewood",
    "Mossglen",
    "Rookwood",
    "Saltmarsh",
    "Brightwater",
    "Coldhollow",
];

pub const TRAITS: [&str; 40] = [
    "cheerful",
    "wary",
    "stubborn",
    "generous",
    "sharp-tongued",
    "patient",
    "restless",
    "superstitious",
    "ambitious",
    "soft-hearted",
    "boastful",
    "thoughtful",
    "dutiful",
    "stern",
    "protective",
    "bitter",
    "curious",
    "lazy about chores",
    "quick to laugh",
    "slow to trust",
    "proud",
    "gentle",
    "nosy",
    "pious",
    "practical",
    "dreamy",
    "blunt",
    "secretive",
    "loyal",
    "vain",
    "melancholy",
    "hot-blooded",
    "forgiving",
    "tidy",
    "greedy",
    "brave to a fault",
    "homesick",
    "witty",
    "grave",
    "warm",
];

/// Trades by class, three each, in `ClassId` order.
pub const OCCUPATIONS: [[&str; 3]; 14] = [
    ["guard of the village watch", "sword for hire turned watchman", "drill master at the yard"],
    ["woodcutter", "hauler and miner", "quarryman"],
    ["hedge mage", "scholar of small magics", "charm-seller at the market"],
    ["hunter", "forester", "fisherman and tracker"],
    ["market trader", "reformed cutpurse", "pedlar of odds and ends"],
    ["scout of the watch", "messenger", "poacher gone straight"],
    ["herbalist", "keeper of the glade shrine", "healer"],
    ["blacksmith", "tinkerer", "millwright"],
    ["wandering paladin", "keeper of the chapel", "oath-sworn guard"],
    ["village cleric", "healer at the chapel", "scribe and almoner"],
    ["seller of remedies and curses", "midwife and hedge-witch", "reader of fortunes"],
    ["sellsword", "caravan guard between hires", "duellist"],
    ["lorekeeper", "scribe", "teller of old tales"],
    ["farmer", "farmhand", "shepherd"],
];

pub const ORIGINS: [&str; 10] = [
    "Born in the hills to the north,",
    "Raised on a farm two valleys over,",
    "Child of a river trader,",
    "Brought up in the market town downriver,",
    "Born in the valley and never left,",
    "The youngest of six from a mill up the coast,",
    "Raised by an aunt after the fever took the rest,",
    "Child of a wandering tinker,",
    "Grown up in a garrison town,",
    "Born on the road, nobody is quite sure where,",
];
pub const TURNS: [&str; 10] = [
    "they learned the trade from a hard master and left the day the apprenticeship ended.",
    "they lost a sweetheart to the dead one winter and have not spoken of it since.",
    "they followed an army for a season and came back wanting nothing more to do with armies.",
    "they ran a shop that failed and still keep the ledgers.",
    "they were the one everyone came to with a broken thing or a broken heart.",
    "they picked a quarrel with the wrong family and thought it best to move on.",
    "a flood took the farm and left them with a cart, a dog and a debt.",
    "they spent three years in a monastery and left with the reading but not the faith.",
    "they nursed a parent through a long illness and learned patience the hard way.",
    "they won a wager that bought their freedom from a bad contract.",
];
pub const NOWS: [&str; 8] = [
    "The valley is where they mean to start again.",
    "They want a roof, a trade and a few people who will miss them.",
    "They are tired of the road and ready to put down roots.",
    "They came for the quiet, and found the nights anything but.",
    "They tell themselves they will leave in the spring, and never do.",
    "They have promised themselves to do right by this place.",
    "What they want most is for the nights to be safe.",
    "They keep a list of debts to repay, most of them kindnesses.",
];

pub const GOALS: [&str; 24] = [
    "stay alive through the nights",
    "make a friend or two",
    "find a place in the village",
    "earn the watch's respect",
    "be the best at my trade in the valley",
    "fall in love, properly this time",
    "keep my temper for a whole season",
    "pay off what I owe",
    "see the graveyard quiet for good",
    "learn to read",
    "find someone who will listen",
    "keep my secret",
    "be forgiven",
    "keep the young ones out of trouble",
    "save enough for a horse",
    "brew the finest ale the tavern has served",
    "put flowers on every grave",
    "never run from a fight again",
    "find out what lies under the ruins",
    "be useful to someone",
    "sleep a whole night through",
    "settle an old score, quietly",
    "teach what I know before I forget it",
    "laugh more than I did last year",
];

pub const SPEECH_STYLES: [&str; 12] = [
    "Plain and direct.",
    "Slow, with long pauses, as if weighing every word.",
    "Quick and full of questions.",
    "Dry, with a joke buried in most sentences.",
    "Warm and talkative; uses people's names often.",
    "Formal and a little old-fashioned.",
    "Short sentences, few of them.",
    "Proverbs and sayings from home, half of them made up.",
    "Soft-spoken; trails off rather than finishes.",
    "Loud, cheerful and apt to repeat the good bits.",
    "Precise, like someone used to being misunderstood.",
    "Gruff, but kinder than it sounds.",
];

pub const TIMBRES: [&str; 10] = [
    "low and even",
    "bright and quick",
    "husky",
    "warm, a little hoarse",
    "high and clear",
    "deep, with a rasp",
    "soft, almost a murmur",
    "nasal and sharp",
    "rich and slow",
    "thin and reedy",
];

pub const SECRETS: [&str; 10] = [
    "I left a debt unpaid in the last town and gave a false name here.",
    "I cannot read, and I have let everyone believe I can.",
    "I was once a thief, and good at it.",
    "I am in love with someone who does not know I exist.",
    "I ran when the dead came to my old village, and people died.",
    "I keep a letter I should have delivered years ago.",
    "I am not as brave as I let people think.",
    "I buried my savings under a tree I can no longer find.",
    "I was sent away from home, not sent for.",
    "I write poems, badly, every night.",
];

/// Why a newcomer came up the west road.
pub const REASONS: [&str; 10] = [
    "looking for work",
    "with everything they own on their back",
    "hoping for a quieter life",
    "having heard there were beds going",
    "following a rumour of good land",
    "a day ahead of the people they owe",
    "to see the graveyard everyone talks about",
    "because the road ran out",
    "sent by a cousin who spoke well of the valley",
    "for no reason they will give",
];

/// What a founder says of an old acquaintance among the others, by bond kind.
pub const SIBLING_SUMMARIES: [&str; 3] = [
    "We grew up in the same house and came here together.",
    "My sibling; we quarrel, but we came here as one.",
    "Blood. That has always been enough.",
];
pub const FRIEND_SUMMARIES: [&str; 3] =
    ["We came up the road together.", "An old friend from the road.", "We have known each other for years."];
pub const COMRADE_SUMMARIES: [&str; 2] = ["We stood a wall together once.", "We served in the same company."];
pub const RIVAL_SUMMARIES: [&str; 2] =
    ["We have never got on, and we both know why.", "An old quarrel neither of us will drop."];
