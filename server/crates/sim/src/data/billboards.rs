//! What the billboards say when nobody has put anything else up, and what people say back.

use fsw_world::places::ActivityId::{self, *};

use self::Advice::*;
use crate::state::BoardReaction::{self, *};
use crate::state::BoardTone::{self, *};

/// What a board urges on whoever reads it, which some of them go and do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advice {
    /// An activity (resting is done at home)
    Do(ActivityId),
    /// Whatever the reader's own work is
    Work,
    /// A friendly word with someone nearby
    Mingle,
}

/// The slogans the boards cycle through, one a day each, with what each one urges on its readers (if anything).
/// Every one fits in 70 UTF-16 units.
pub static DECK: [(&str, BoardTone, Option<Advice>); 40] = [
    ("Believe in yourself. The skeletons don't.", Uplifting, Some(Do(Train))),
    ("Monday: survived by 97% of villagers.", Absurd, None),
    ("Buy one loaf, get one loaf. Same price!", Pushy, Some(Do(Shop))),
    ("Have you prayed today? The dead have.", Ominous, Some(Do(Pray))),
    ("Wave to a neighbour. They might wave back.", Wholesome, Some(Mingle)),
    ("Hard work pays. Eventually. Maybe.", Uplifting, Some(Work)),
    ("Lost: one sheep. Answers to 'Dinner'.", Absurd, None),
    ("Swords! Slightly used! Mostly unbloodied!", Pushy, Some(Do(Train))),
    ("Lock your door tonight. No reason.", Ominous, Some(Do(Rest))),
    ("You are someone's favourite villager.", Wholesome, Some(Mingle)),
    ("Today is a gift. Tomorrow is a rumour.", Uplifting, None),
    ("This billboard is load-bearing. Do not lean.", Absurd, None),
    ("The Gilded Tankard: our ale is legally drinkable.", Pushy, Some(Do(Drink))),
    ("The graveyard is hiring. Positions are permanent.", Ominous, None),
    ("Drink water. Ale does not count.", Wholesome, None),
    ("Dream big. Dig deep. Mind the mine shaft.", Uplifting, Some(Do(Mine))),
    ("Ask your healer if leeches are right for you.", Absurd, None),
    ("Fresh fish! Freshness not guaranteed after noon.", Pushy, Some(Do(Shop))),
    ("Count your neighbours. Then count them again.", Ominous, Some(Mingle)),
    ("Hug a farmer. They grew your lunch.", Wholesome, Some(Mingle)),
    ("Every hero started as a villager with a stick.", Uplifting, Some(Do(Train))),
    ("Warning: the river is wet.", Absurd, None),
    ("New boots? Your feet deserve it. Your purse disagrees.", Pushy, Some(Do(Shop))),
    ("Something in the mine misses you.", Ominous, None),
    ("Be kind. Everyone is fighting a skeleton.", Wholesome, Some(Mingle)),
    ("You miss every sword swing you never take.", Uplifting, Some(Do(Train))),
    ("Free hugs. Ogres excluded.", Absurd, Some(Mingle)),
    ("Limited offer: a free nap with every pillow.", Pushy, Some(Do(Rest))),
    ("The nights grow longer. So do the teeth.", Ominous, Some(Do(Rest))),
    ("The sun came up again. Well done, everyone.", Wholesome, None),
    ("Rise and shine! The chickens already did.", Uplifting, Some(Work)),
    ("Village idiot wanted. Apply within.", Absurd, None),
    ("Insure your cottage against dragons today!", Pushy, None),
    ("Do not feed the skeletons. They have no stomachs.", Ominous, None),
    ("Somebody out there thinks your soup is great.", Wholesome, Some(Do(Eat))),
    ("Be the hammer, not the anvil. Unless you're the smith.", Uplifting, Some(Work)),
    ("Reading this sign counts as exercise.", Absurd, None),
    ("Tired of walking? Try sitting down. Ask us how.", Pushy, Some(Do(Rest))),
    ("Sleep well. We'll be watching.", Ominous, Some(Do(Pray))),
    ("Rest when you need to. The fields will wait.", Wholesome, Some(Do(Rest))),
];

/// Slogans for the days a board hints that someone beyond the world is looking in, when the model has not
/// written one. Each keeps one of the five tones, so it still reads like a billboard.
pub static WATCHER_DECK: [(&str, BoardTone, Option<Advice>); 10] = [
    ("Smile! You are being watched from beyond the sky.", Uplifting, None),
    ("Wave to the faces in the clouds. They wave back.", Wholesome, None),
    ("Somebody up there is rooting for you. Several, in fact.", Wholesome, Some(Mingle)),
    ("This village is being watched. Live accordingly.", Ominous, Some(Do(Pray))),
    ("They see the whole valley at once. Lock your door anyway.", Ominous, Some(Do(Rest))),
    ("Notice to our audience: the village is not a play.", Absurd, None),
    ("If you can read this, so can they.", Absurd, None),
    ("Do something interesting. You have an audience.", Pushy, Some(Work)),
    ("Our watchers love a busy market. Buy something!", Pushy, Some(Do(Shop))),
    ("You are never alone. Statistically, never.", Uplifting, None),
];

/// Words in a notice put up from outside that urge something. The first word of the notice that is one of these,
/// or one of these with a plain ending ("praying", "meals"), decides.
pub static ADVICE_WORDS: [(&str, Advice); 24] = [
    ("pray", Do(Pray)),
    ("temple", Do(Pray)),
    ("shrine", Do(Pray)),
    ("grave", Do(Mourn)),
    ("eat", Do(Eat)),
    ("meal", Do(Eat)),
    ("supper", Do(Eat)),
    ("soup", Do(Eat)),
    ("ale", Do(Drink)),
    ("drink", Do(Drink)),
    ("tavern", Do(Drink)),
    ("nap", Do(Rest)),
    ("rest", Do(Rest)),
    ("fish", Do(Fish)),
    ("buy", Do(Shop)),
    ("market", Do(Shop)),
    ("shop", Do(Shop)),
    ("train", Do(Train)),
    ("sword", Do(Train)),
    ("herb", Do(Forage)),
    ("work", Work),
    ("neighbour", Mingle),
    ("friend", Mingle),
    ("hug", Mingle),
];

/// What a notice put up from outside urges, read from its words.
pub fn advice_in(text: &str) -> Option<Advice> {
    const ENDINGS: [&str; 7] = ["", "s", "es", "ed", "ing", "er", "ers"];
    text.split(|ch: char| !ch.is_alphanumeric()).map(str::to_lowercase).find_map(|word| {
        ADVICE_WORDS.iter().find_map(|&(stem, advice)| {
            word.strip_prefix(stem).is_some_and(|ending| ENDINGS.contains(&ending)).then_some(advice)
        })
    })
}

/// What people say when they have read a board, by how they took it.
pub fn lines(reaction: BoardReaction) -> &'static [&'static str] {
    match reaction {
        Laugh => &[
            "Ha! Who writes these?",
            "That's the best thing I've read all week.",
            "Hah! I needed that.",
            "Oh, that's good. That's very good.",
        ],
        Love => &[
            "You know, that's rather lovely.",
            "I'll remember that one.",
            "That warms the heart, it does.",
            "Right. I needed to hear that today.",
        ],
        Scoff => &[
            "Hmph. Who pays for these things?",
            "Somebody had too much paint and time.",
            "Well, that's nonsense.",
            "I'll pretend I didn't read that.",
        ],
        Annoyed => &[
            "Who keeps putting these up?",
            "As if I haven't enough to worry about.",
            "Bah! Tear it down, I say.",
            "Don't tell me what to do, board.",
        ],
        Uneasy => &[
            "I don't like the sound of that.",
            "That's... not funny.",
            "Who wrote that? Who's watching?",
            "I'll be locking my door tonight.",
        ],
        Shocked => &[
            "Watched? By whom? From where?",
            "Gods above... or whoever's up there.",
            "I felt eyes on me all week. I knew it!",
            "Don't look up. Don't look up.",
        ],
        Intrigued => &[
            "Hello? Is somebody out there?",
            "Watchers beyond the sky... I'd like a word with them.",
            "Well now. I wonder what they make of us.",
            "If they're watching, I'd best give them something to see.",
        ],
        Angry => &[
            "Whoever's spying on us, show yourselves!",
            "Watching us? Get your own village!",
            "I've a mind to shake my fist at the sky.",
            "No one asked to be stared at. No one!",
        ],
    }
}

#[cfg(test)]
mod tests {
    use fsw_core::jsmath::utf16_len;

    use super::*;

    #[test]
    fn slogans_are_short_and_unique() {
        for (text, _, _) in &DECK {
            assert!(utf16_len(text) <= 70, "{text}");
        }
        let unique: std::collections::HashSet<_> = DECK.iter().chain(&WATCHER_DECK).map(|(text, _, _)| text).collect();
        assert_eq!(unique.len(), DECK.len() + WATCHER_DECK.len());
        for (text, _, _) in &WATCHER_DECK {
            assert!(utf16_len(text) <= 70, "{text}");
        }
    }

    #[test]
    fn a_notice_urges_what_its_words_say() {
        assert_eq!(advice_in("Pray at the shrine tonight!"), Some(Do(Pray)));
        assert_eq!(advice_in("Hungry? Hot MEALS at the Gilded Tankard"), Some(Do(Eat)));
        assert_eq!(advice_in("Be a good neighbour"), Some(Mingle));
        assert_eq!(advice_in("Workers wanted"), Some(Work));
        // Only whole words with a plain ending count.
        assert_eq!(advice_in("Stay alert, restless souls"), None);
        assert_eq!(advice_in("The river is wet."), None);
    }
}
