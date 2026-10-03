//! Locally written conversations: used when no LLM is configured, when it is
//! too slow, or when the budget says a chat is not worth a request. Lines use
//! {a} for the initiator's first name and {b} for the other's.

use fsw_core::rng::{chance, next_float, pick_index};
use fsw_core::time::{TICKS_PER_DAY, part_of_day};

use crate::state::{Character, ConversationLine, Feeling, Quirk, Relationship, Rumour, SimState, TalkTone};
use crate::util::{first_name, living, name_of, rel_or_default};

#[derive(Clone, Copy, PartialEq, Eq)]
enum When {
    Always,
    NightWasLoud,
    RecentDeath,
    Newcomer,
}

struct Exchange {
    topic: &'static str,
    opener: &'static str,
    /// remaining lines, alternating starting with the other speaker
    lines: &'static [&'static str],
    when: When,
}

const fn ex(topic: &'static str, opener: &'static str, lines: &'static [&'static str]) -> Exchange {
    Exchange { topic, opener, lines, when: When::Always }
}

const fn ex_when(topic: &'static str, opener: &'static str, lines: &'static [&'static str], when: When) -> Exchange {
    Exchange { topic, opener, lines, when }
}

fn recent_death(state: &SimState) -> Option<String> {
    let mut latest = None;
    let mut latest_day = -1;
    let today = state.tick.div_euclid(TICKS_PER_DAY) + 1;
    for record in state.dead.values() {
        if today - record.day <= 2 && record.day > latest_day {
            latest = Some(record.name.split(' ').next().unwrap_or_default().to_owned());
            latest_day = record.day;
        }
    }
    latest
}

fn night_was_loud(state: &SimState) -> bool {
    ((state.tick - state.director.alarm_until_tick) as f64) < TICKS_PER_DAY as f64 * 0.6
        && state.director.alarm_until_tick > 0
}

fn has_newcomer(state: &SimState) -> bool {
    living(state).any(|c| state.tick - c.arrived_tick < TICKS_PER_DAY * 2 && c.arrived_tick > 0)
}

fn applies(when: When, state: &SimState) -> bool {
    match when {
        When::Always => true,
        When::NightWasLoud => night_was_loud(state),
        When::RecentDeath => recent_death(state).is_some(),
        When::Newcomer => has_newcomer(state),
    }
}

const FRIENDLY: &[Exchange] = &[
    ex("the day", "Fine {part} for it, {b}.", &["It is. Let us hope the night is as quiet.", "We can hope."]),
    ex("work", "How goes the work, {b}?", &["Slow, but honest. And yours?", "Same as ever. I should not complain."]),
    ex(
        "the tavern",
        "Will you be at the Tankard tonight, {b}?",
        &["I would not miss it. First round is yours.", "It always is, somehow."],
    ),
    ex(
        "the harvest",
        "The fields look good this year.",
        &["They do. A full store makes for an easier winter.", "If the dead leave us in peace to bring it in."],
    ),
    ex(
        "the lake",
        "They say the pike in the lake are biting.",
        &["Kael says that every week.", "One of these weeks he will be right."],
    ),
    ex(
        "an aching back",
        "{b}, you look worn out.",
        &["A long day. Nothing a seat and a bowl of stew will not mend.", "Then go and sit. The work will keep."],
    ),
    ex(
        "the road",
        "No traders on the west road in days.",
        &[
            "Word has got round about our nights, I expect.",
            "Then we shall have to make do with our own company.",
            "There are worse things.",
        ],
    ),
    ex(
        "the market",
        "Anything good at the market today?",
        &["Fresh bread, and cheese that has seen better weeks.", "I will take the bread."],
    ),
    ex(
        "old stories",
        "My grandmother swore the old ruins were a lord's hall once.",
        &["Mine said worse things were buried there than lords.", "Cheerful folk, our grandmothers."],
    ),
    ex_when(
        "last night's attack",
        "Did you hear them last night, out by the graveyard?",
        &["I did. I barely slept after.", "The watch held. That is what matters.", "This time."],
        When::NightWasLoud,
    ),
    ex_when(
        "a loss",
        "I still cannot believe {dead} is gone.",
        &["Nor I. The village feels smaller.", "We should visit the grave. Together, perhaps.", "I would like that."],
        When::RecentDeath,
    ),
    ex_when(
        "a newcomer",
        "Have you spoken to {new} yet?",
        &["Only a word. Seems decent enough.", "We could use decent. And another pair of hands."],
        When::Newcomer,
    ),
];

const WARM: &[Exchange] = &[
    ex(
        "friendship",
        "{b}! I was hoping to run into you.",
        &[
            "And here I am. What is on your mind?",
            "Nothing at all. It is just good to see a friendly face.",
            "Then sit a while.",
        ],
    ),
    ex(
        "a favour",
        "I owe you for the other day, {b}.",
        &["You owe me nothing. That is what friends are for.", "All the same. Thank you."],
    ),
    ex(
        "plans",
        "When all this is quieter, what will you do?",
        &["Sleep for a week. Then, who knows? You?", "The same, I think. But it is good to wonder."],
    ),
];

const COOL: &[Exchange] = &[
    ex("nothing much", "{b}.", &["{a}.", "Busy day.", "Aren't they all."]),
    ex("an awkward silence", "I did not expect to see you here.", &["It is a small village.", "So it is."]),
];

const HOSTILE: &[Exchange] = &[
    ex(
        "an old grudge",
        "We need to have this out, {b}.",
        &["Do we? I have nothing to say to you.", "You never do. That is the trouble.", "Then stop asking."],
    ),
    ex(
        "blame",
        "You know what you did, {b}. Do not pretend otherwise.",
        &["I know what you think I did.", "Everyone knows it.", "Then everyone is as wrong as you are."],
    ),
    ex(
        "a warning",
        "Stay out of my way, {b}. I will not say it twice.",
        &["Or what? You will glare at me some more?", "Keep pushing and you will find out."],
    ),
    ex(
        "an insult",
        "I have had enough of your sneering, {b}.",
        &["Then stop giving me cause.", "One day that mouth will cost you."],
    ),
];

const HOSTILE_RESOLVED: &[Exchange] = &[ex(
    "clearing the air",
    "We need to have this out, {b}.",
    &["...Aye. Perhaps we do.", "I am tired of carrying it.", "So am I. Say your piece, and I will say mine."],
)];

const ROMANTIC_YES: &[Exchange] = &[
    ex(
        "feelings",
        "{b}, there is something I have wanted to tell you for a long time.",
        &["I think I know. I have been hoping you would.", "Then you do not mind?", "Mind? I have been waiting."],
    ),
    ex(
        "a walk together",
        "Walk with me a while, {b}? Just the two of us.",
        &["I would like nothing better.", "I hoped you would say that."],
    ),
];

const ROMANTIC_NO: &[Exchange] = &[ex(
    "feelings unreturned",
    "{b}, there is something I have wanted to tell you for a long time.",
    &["Oh. {a}, I... I am fond of you. But not like that.", "I see. Forget I spoke.", "I am sorry."],
)];

const APOLOGY_YES: &[Exchange] = &[ex(
    "making amends",
    "{b}, I was wrong, and I am sorry for it.",
    &["That cannot have been easy to say.", "It was not.", "Then let us put it behind us."],
)];

const APOLOGY_NO: &[Exchange] = &[ex(
    "an apology refused",
    "{b}, I was wrong, and I am sorry for it.",
    &["Sorry does not mend it.", "I know. I wanted you to hear it anyway.", "I have heard it."],
)];

/// Someone sworn off love, being courted and not understanding a word of it.
const ROMANTIC_FLUSTERED: &[Exchange] = &[
    ex(
        "a misunderstanding",
        "{b}, there is something I have wanted to tell you for a long time.",
        &[
            "Is it about the watch rota? I did mean to raise it with the captain.",
            "No, {b}. It is about you. And me.",
            "Ah. Yes. Well. I... I believe I am needed at the chapel. Good day.",
        ],
    ),
    ex(
        "an evening walk",
        "Walk with me a while, {b}? Just the two of us.",
        &[
            "Of course. Two blades are better than one on the roads at dusk.",
            "That is not quite what I meant.",
            "Then I confess I do not know what you meant.",
        ],
    ),
];

/// Ordinary warmth, taken for something else.
const CONFUSED: &[Exchange] = &[
    ex(
        "good intentions",
        "Good to see you, {b}. You look well.",
        &["Do I? Is something the matter with how I usually look?", "No, I only meant...", "I see. I think. Good day."],
    ),
    ex(
        "a kind word",
        "You are always welcome at our table, {b}.",
        &[
            "Welcome? Why? What is it you want of me?",
            "Nothing at all. It was only kindly meant.",
            "...I shall pray on it.",
        ],
    ),
];

/// A bully picking on someone who has done nothing to deserve it.
const BULLYING: &[Exchange] = &[
    ex(
        "a sneer",
        "Out of my way, {b}. Or are you too slow for that as well?",
        &["I was only passing.", "Pass faster.", "...Fine."],
    ),
    ex(
        "contempt",
        "Is that the best you can manage, {b}? Pathetic.",
        &["Leave me be, {a}.", "Or what? You will cry at me?"],
    ),
    ex(
        "a shove",
        "Look at you, {b}. The dead would not even bother with you.",
        &["What have I ever done to you?", "Nothing. That is the point."],
    ),
];

/// A killer, accused, and lying with a smile.
const DENIAL: &[Exchange] = &[
    ex(
        "an accusation",
        "I know what you are, {b}. I know what you did.",
        &[
            "What I did? I sold a love charm and a cure for warts. Hang me for it.",
            "Do not play the fool with me.",
            "Look closer to home before you go pointing fingers, {a}.",
        ],
    ),
    ex(
        "suspicion",
        "Where were you when it happened, {b}?",
        &[
            "Picking herbs, alone, as I do every day. Do you suspect the nettles too?",
            "Somebody did it.",
            "Then go and find them, and leave me be.",
        ],
    ),
];

/// A gossip passing on a story about someone who is not there; `{t}` is who it is about.
const RUMOUR: &[Exchange] = &[
    ex(
        "a story about {t}",
        "Heard about {t}? You did not hear it from me. {story}",
        &["{t}? Never. Are you sure?", "Sure as sunrise.", "Well. I shall be keeping an eye on {t}, then."],
    ),
    ex(
        "a story about {t}",
        "Come here, {b}. About {t}. {story}",
        &["You are making that up.", "Ask anyone.", "Hm. I always did wonder about {t}."],
    ),
];

/// Made-up stories a gossip tells about the neighbours; `{t}` is who it is about, `{b}` the listener.
pub const FALSE_STORIES: [&str; 8] = [
    "{t} has been pinching coins from the market stalls when nobody is looking.",
    "{t} goes out to the graveyard after dark and talks to the graves.",
    "{t} called you a fool behind your back, {b}.",
    "{t} waters down the ale at the tavern and pockets the difference.",
    "{t} ran from the dead the other night and left the rest to it.",
    "{t} is sweet on someone who is already spoken for.",
    "{t} keeps a bag of stolen trinkets under the floorboards.",
    "{t} was seen whispering with strangers on the west road.",
];

fn fill(text: &str, state: &SimState, a: &Character, b: &Character) -> String {
    let hour = ((6.0 * 60.0 + state.tick as f64 / 20.0) % 1440.0) / 60.0;
    let newcomer = living(state).find(|c| {
        state.tick - c.arrived_tick < TICKS_PER_DAY * 2 && c.arrived_tick > 0 && c.id != a.id && c.id != b.id
    });
    let mut out = text.replace("{a}", first_name(a)).replace("{b}", first_name(b)).replace("{part}", part_of_day(hour));
    if out.contains("{dead}") {
        out = out.replace("{dead}", recent_death(state).as_deref().unwrap_or("the old days"));
    }
    out.replace("{new}", newcomer.map_or("the newcomer", first_name))
}

pub struct LocalScript {
    pub topic: String,
    pub opener: String,
    pub lines: Vec<ConversationLine>,
    pub feelings: [Feeling; 2],
}

fn told(text: &str, rumour: &Rumour, state: &SimState) -> String {
    text.replace("{t}", &name_of(state, &rumour.about)).replace("{story}", &rumour.text)
}

fn roll_feeling(rng: &mut u32, warmth: f64) -> Feeling {
    let roll = next_float(rng) + warmth;
    if roll > 1.2 {
        Feeling::MuchBetter
    } else if roll > 0.6 {
        Feeling::Better
    } else if roll > 0.22 {
        Feeling::Same
    } else if roll > 0.04 {
        Feeling::Worse
    } else {
        Feeling::MuchWorse
    }
}

fn warmth(c: &Character, r: &Relationship) -> f64 {
    r.affinity / 170.0 + 0.1 - c.profile.temper * 0.12 - r.grievance / 220.0
}

/// Picks an exchange that fits how the two feel about each other, and decides
/// how each comes away feeling. Reads relationships without creating them (the
/// TypeScript sim's `rel()` creates a default record here; the two always know
/// each other by the time they talk, so nothing changes).
pub fn local_script(
    state: &mut SimState,
    a_id: &str,
    b_id: &str,
    tone: TalkTone,
    rumour: Option<&Rumour>,
) -> LocalScript {
    let (ab, ba, b_temper, b_partner) = {
        let a = &state.characters[a_id];
        let b = &state.characters[b_id];
        (rel_or_default(a, b_id), rel_or_default(b, a_id), b.profile.temper, b.partner_id.is_some())
    };
    let (b_chaste, b_murderous, a_bully) = {
        let b = &state.characters[b_id];
        (b.has(Quirk::Chaste), b.has(Quirk::Murderous), state.characters[a_id].has(Quirk::Bully))
    };
    let rng = &mut state.rng;
    let pool: &[Exchange];
    let feelings: [Feeling; 2];

    match tone {
        TalkTone::Friendly if rumour.is_some() => {
            pool = RUMOUR;
            // A juicy story is a pleasure to tell and, mostly, to hear.
            let heard = if chance(rng, 0.6) { Feeling::Better } else { Feeling::Same };
            feelings = [Feeling::Better, heard];
        }
        TalkTone::Hostile if a_bully && ab.grievance < 40.0 => {
            pool = BULLYING;
            // The bully enjoys it; the one picked on does not.
            let hurt = if chance(rng, 0.5) { Feeling::Worse } else { Feeling::MuchWorse };
            feelings = [Feeling::Better, hurt];
        }
        TalkTone::Hostile if b_murderous => {
            pool = DENIAL;
            feelings = [Feeling::Worse, Feeling::Same];
        }
        TalkTone::Romantic if b_chaste => {
            pool = ROMANTIC_FLUSTERED;
            feelings = [Feeling::Same, Feeling::Same];
        }
        TalkTone::Friendly if b_chaste && chance(rng, 0.35) => {
            pool = CONFUSED;
            let a = &state.characters[a_id];
            let wa = warmth(a, &ab);
            let rng = &mut state.rng;
            feelings = [roll_feeling(rng, wa), Feeling::Same];
        }
        TalkTone::Hostile => {
            let cools = chance(rng, 0.22 + (1.0 - b_temper) * 0.15);
            pool = if cools { HOSTILE_RESOLVED } else { HOSTILE };
            feelings = if cools {
                [Feeling::Better, Feeling::Better]
            } else {
                let first = if chance(rng, 0.5) { Feeling::Worse } else { Feeling::MuchWorse };
                let second = if chance(rng, 0.4) { Feeling::MuchWorse } else { Feeling::Worse };
                [first, second]
            };
        }
        TalkTone::Romantic => {
            let returned = ba.attraction >= 42.0 && ba.affinity >= 25.0 && !b_partner && !ba.kin;
            pool = if returned { ROMANTIC_YES } else { ROMANTIC_NO };
            feelings =
                if returned { [Feeling::MuchBetter, Feeling::MuchBetter] } else { [Feeling::Worse, Feeling::Same] };
        }
        TalkTone::Apology => {
            let forgiven = chance(rng, 0.7 - b_temper * 0.35);
            pool = if forgiven { APOLOGY_YES } else { APOLOGY_NO };
            feelings = if forgiven { [Feeling::Better, Feeling::MuchBetter] } else { [Feeling::Same, Feeling::Same] };
        }
        TalkTone::Friendly => {
            let mutual = (ab.affinity + ba.affinity) / 2.0;
            pool = if mutual >= 35.0 && chance(rng, 0.5) {
                WARM
            } else if mutual <= -15.0 {
                COOL
            } else {
                FRIENDLY
            };
            // Hot tempers and old grudges make an ordinary chat more likely to go wrong.
            let a = &state.characters[a_id];
            let b = &state.characters[b_id];
            let (wa, wb) = (warmth(a, &ab), warmth(b, &ba));
            let rng = &mut state.rng;
            feelings = [roll_feeling(rng, wa), roll_feeling(rng, wb)];
        }
    }

    let eligible: Vec<&Exchange> = pool.iter().filter(|exchange| applies(exchange.when, state)).collect();
    let choices: Vec<&Exchange> = if eligible.is_empty() { pool.iter().collect() } else { eligible };
    let exchange = choices[pick_index(&mut state.rng, choices.len())];
    let a = &state.characters[a_id];
    let b = &state.characters[b_id];
    let write = |text: &str| match rumour {
        Some(rumour) => fill(&told(text, rumour, state), state, a, b),
        None => fill(text, state, a, b),
    };
    let lines = exchange
        .lines
        .iter()
        .enumerate()
        .map(|(index, text)| ConversationLine {
            speaker_id: if index % 2 == 0 { b.id.clone() } else { a.id.clone() },
            text: write(text),
        })
        .collect();
    LocalScript { topic: write(exchange.topic), opener: write(exchange.opener), lines, feelings }
}

fn generic(tone: TalkTone) -> &'static [&'static [&'static str]] {
    match tone {
        TalkTone::Friendly => &[
            &["Hm. I had not thought of it like that.", "Well, it is something to chew on."],
            &["You may be right.", "I usually am. Now and then."],
            &["Ha. Fair enough.", "We should talk again soon."],
        ],
        TalkTone::Romantic => &[
            &["You have a way of catching me off guard.", "I hope that is no bad thing."],
            &["I... need a moment to think about that.", "Take all the time you need."],
        ],
        TalkTone::Hostile => &[
            &["I have heard enough.", "You have not heard the half of it."],
            &["Say what you like. It changes nothing.", "We shall see."],
        ],
        TalkTone::Apology => &[
            &["I hear you. Give me time.", "That is all I ask."],
            &["It will take more than words.", "Then I will find more than words."],
        ],
    }
}

/// A neutral way to wind up a conversation whose opening line was written elsewhere.
pub fn generic_continuation(
    state: &mut SimState,
    a_id: &str,
    b_id: &str,
    tone: TalkTone,
    rumour: Option<&Rumour>,
) -> LocalScript {
    let script = local_script(state, a_id, b_id, tone, rumour);
    let options = generic(tone);
    let replies = options[pick_index(&mut state.rng, options.len())];
    LocalScript {
        topic: if tone == TalkTone::Friendly { "this and that".into() } else { script.topic },
        opener: String::new(),
        lines: replies
            .iter()
            .enumerate()
            .map(|(index, text)| ConversationLine {
                speaker_id: if index % 2 == 0 { b_id.to_owned() } else { a_id.to_owned() },
                text: (*text).to_owned(),
            })
            .collect(),
        feelings: script.feelings,
    }
}
