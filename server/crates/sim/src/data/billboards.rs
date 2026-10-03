//! What the billboards say when nobody has put anything else up, and what people say back.

use crate::state::BoardReaction::{self, *};
use crate::state::BoardTone::{self, *};

/// The slogans the boards cycle through, one a day each. Every one fits in 70 UTF-16 units.
pub static DECK: [(&str, BoardTone); 40] = [
    ("Believe in yourself. The skeletons don't.", Uplifting),
    ("Monday: survived by 97% of villagers.", Absurd),
    ("Buy one loaf, get one loaf. Same price!", Pushy),
    ("Have you prayed today? The dead have.", Ominous),
    ("Wave to a neighbour. They might wave back.", Wholesome),
    ("Hard work pays. Eventually. Maybe.", Uplifting),
    ("Lost: one sheep. Answers to 'Dinner'.", Absurd),
    ("Swords! Slightly used! Mostly unbloodied!", Pushy),
    ("Lock your door tonight. No reason.", Ominous),
    ("You are someone's favourite villager.", Wholesome),
    ("Today is a gift. Tomorrow is a rumour.", Uplifting),
    ("This billboard is load-bearing. Do not lean.", Absurd),
    ("The Gilded Tankard: our ale is legally drinkable.", Pushy),
    ("The graveyard is hiring. Positions are permanent.", Ominous),
    ("Drink water. Ale does not count.", Wholesome),
    ("Dream big. Dig deep. Mind the mine shaft.", Uplifting),
    ("Ask your healer if leeches are right for you.", Absurd),
    ("Fresh fish! Freshness not guaranteed after noon.", Pushy),
    ("Count your neighbours. Then count them again.", Ominous),
    ("Hug a farmer. They grew your lunch.", Wholesome),
    ("Every hero started as a villager with a stick.", Uplifting),
    ("Warning: the river is wet.", Absurd),
    ("New boots? Your feet deserve it. Your purse disagrees.", Pushy),
    ("Something in the mine misses you.", Ominous),
    ("Be kind. Everyone is fighting a skeleton.", Wholesome),
    ("You miss every sword swing you never take.", Uplifting),
    ("Free hugs. Ogres excluded.", Absurd),
    ("Limited offer: a free nap with every pillow.", Pushy),
    ("The nights grow longer. So do the teeth.", Ominous),
    ("The sun came up again. Well done, everyone.", Wholesome),
    ("Rise and shine! The chickens already did.", Uplifting),
    ("Village idiot wanted. Apply within.", Absurd),
    ("Insure your cottage against dragons today!", Pushy),
    ("Do not feed the skeletons. They have no stomachs.", Ominous),
    ("Somebody out there thinks your soup is great.", Wholesome),
    ("Be the hammer, not the anvil. Unless you're the smith.", Uplifting),
    ("Reading this sign counts as exercise.", Absurd),
    ("Tired of walking? Try sitting down. Ask us how.", Pushy),
    ("Sleep well. We'll be watching.", Ominous),
    ("Rest when you need to. The fields will wait.", Wholesome),
];

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
    }
}

#[cfg(test)]
mod tests {
    use fsw_core::jsmath::utf16_len;

    use super::*;

    #[test]
    fn slogans_are_short_and_unique() {
        for (text, _) in &DECK {
            assert!(utf16_len(text) <= 70, "{text}");
        }
        let unique: std::collections::HashSet<_> = DECK.iter().map(|(text, _)| text).collect();
        assert_eq!(unique.len(), DECK.len());
    }
}
