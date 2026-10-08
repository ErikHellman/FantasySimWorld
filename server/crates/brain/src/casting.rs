//! Casting: asking the language model to invent a villager. The founders of a new world come one by
//! one this way (`Command::Settle`), and later newcomers on demand (`Command::NewcomerDraft`); the
//! server's casting task drives it with its own client, outside the brain's queue and governor.
//!
//! The model is told who lives here, who died lately and who misses them, which work is short, and,
//! for the flavour of the valley, the people of its first cast; it answers by calling `create_villager`.
//! Every answer is cleaned, cut to size and checked here before it becomes a `NewVillager`.

use std::fmt;
use std::time::Duration;

use fsw_core::jsmath::utf16_truncate;
use fsw_core::time::{TICKS_PER_DAY, game_time};
use fsw_sim::data::classes::ClassId;
use fsw_sim::state::{Bond, BondKind, NewVillager, Pronouns, Quirk, SimState};
use fsw_sim::systems::arrivals::free_home;
use fsw_sim::systems::billboards::clean_text;
use fsw_sim::systems::founding::ALLOWED_QUIRKS;
use fsw_sim::util::living;
use fsw_world::places::{ActivityId, get_place};
use serde_json::{Map, Value, json};

use crate::llm::client::LlmClient;
use crate::llm::types::{ChatMessage, LlmError, LlmErrorKind, LlmRequest, ToolCall, ToolDef};
use crate::prompts::Prompt;

/// The people of the valley's first cast (name, class, one line), for the flavour of the place. The
/// model is asked for new people in their spirit and refused any of their names.
pub const INSPIRATION: &[(&str, &str, &str)] = &[
    ("Aldric Vane", "knight", "widowed veteran who captains the watch and still carries a lost patrol"),
    ("Brenna Ashdown", "knight", "miller's daughter turned eager guard, sweet on a fisherman"),
    ("Garrick Stonefist", "barbarian", "bitter woodcutter who blames the captain for his brother's death"),
    ("Elowen Mistral", "mage", "scholar who left the academy to study the restless dead"),
    ("Theron Mistral", "mage", "her lazy younger brother, a charmer at the market"),
    ("Sylvi Fenwick", "ranger", "quiet forest girl who has loved the same man in silence for two years"),
    ("Kael Thornwood", "ranger", "drifting fisherman and storyteller who flirts with everyone"),
    ("Mira Quickfingers", "rogue", "ex-cutpurse market trader who enjoys men fighting over her"),
    ("Vex", "scout", "former thief-hunter who makes up gossip about the neighbours"),
    ("Rowan Oakheart", "druid", "herbalist of fifty-eight and keeper of the glade shrine"),
    ("Isolde Briar", "druid", "orphan apprentice herbalist, afraid of the nights"),
    ("Dorn Copperkettle", "engineer", "generous dwarf smith who drinks too much and brawls"),
    ("Torvald Greymane", "barbarian", "gentle giant, painfully shy and strikingly handsome"),
    ("Bjarke Greymane", "barbarian", "his younger brother, come down from the hills to find him"),
    ("Halvard Dawnmere", "paladin", "wandering knight sworn to cleanse the graveyard, and sworn off love"),
    ("Hesper Nightbloom", "witch", "travelling seller of remedies and curses with a dark secret"),
    ("Tobin Haybarrow", "farmer", "lost his farm to a flood; cowardly and idle"),
    ("Maren Holloway", "cleric", "sent to tend the empty chapel, quietly doubting"),
    ("Zariel Emberhorn", "swordsman", "horned sellsword, hard-working and a bully"),
    ("Quill Farrow", "lorekeeper", "chasing a half-burnt chronicle about what lies under the graveyard"),
    ("Ulfa Ironbraid", "barbarian", "hill-clan shieldmaiden looking for kin"),
    ("Nell Haybarrow", "farmer", "restless, nosy farmhand"),
    ("Corwin Hale", "knight", "deserter from a lost war who wants a quiet post"),
    ("Lys Marrow", "ranger", "wary trapper from the high woods"),
];

/// The most tokens an answer may take: a whole person, with room for a model that pads.
const MAX_TOKENS: u32 = 1200;
/// How long one request may take; a founding is not a decision in a fight.
const TIMEOUT: Duration = Duration::from_secs(90);
/// The dead of this many days are told of, so a newcomer may be kin or friend to one.
const MOURNING_DAYS: i64 = 10;

/// What the villager is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// one of the founders: `settled` of `total` are here already
    Founder { settled: usize, total: usize },
    /// someone who comes up the west road into a village that is already living
    Newcomer,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub name: String,
    pub occupation: String,
    pub class: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dead {
    pub name: String,
    pub day: i64,
    pub cause: String,
    /// first names of the living who were close to them
    pub mourners: Vec<String>,
}

/// What the model is told about the village, and what its answer is checked against.
#[derive(Clone, Debug, PartialEq)]
pub struct CastingContext {
    pub purpose: Purpose,
    pub day: i64,
    pub living: Vec<Person>,
    pub dead: Vec<Dead>,
    /// first names nobody may take, lowercased: the living, the dead, and names reserved by answers in flight
    pub taken: Vec<String>,
    /// work fewer than two people do
    pub short_work: Vec<&'static str>,
    pub free_home: String,
}

/// The work a village is short of when fewer than two do it.
const TRADES: [(ActivityId, &str); 9] = [
    (ActivityId::Watch, "the night watch"),
    (ActivityId::Farm, "farming"),
    (ActivityId::Smith, "smithing"),
    (ActivityId::Brew, "brewing"),
    (ActivityId::Forage, "foraging"),
    (ActivityId::Fish, "fishing"),
    (ActivityId::ChopWood, "woodcutting"),
    (ActivityId::Mine, "mining"),
    (ActivityId::Trade, "trading"),
];

fn first_of(name: &str) -> String {
    name.split_whitespace().next().unwrap_or("").to_lowercase()
}

impl CastingContext {
    /// The village as it is now. `reserved` are full names of people whose descriptions have been
    /// accepted but not yet settled.
    pub fn from_state(state: &SimState, purpose: Purpose, reserved: &[String]) -> Self {
        let day = game_time(state.tick).day;
        let living_people: Vec<Person> = living(state)
            .map(|c| Person {
                name: c.profile.name.clone(),
                occupation: c.profile.occupation.clone(),
                class: c.class_id.def().label,
            })
            .collect();
        let dead = state
            .dead
            .iter()
            .filter(|(_, d)| day - d.day <= MOURNING_DAYS)
            .map(|(id, d)| Dead {
                name: d.name.clone(),
                day: d.day,
                cause: d.cause.clone(),
                mourners: living(state)
                    .filter(|c| c.relationships.get(id).is_some_and(|r| r.kin || r.affinity >= 30.0))
                    .map(|c| first_of(&c.profile.name))
                    .map(|f| capitalise(&f))
                    .collect(),
            })
            .collect();
        let mut taken: Vec<String> = living(state)
            .map(|c| first_of(&c.profile.name))
            .chain(state.dead.values().map(|d| first_of(&d.name)))
            .chain(reserved.iter().map(|n| first_of(n)))
            .filter(|f| !f.is_empty())
            .collect();
        taken.sort();
        taken.dedup();
        let short_work = TRADES
            .iter()
            .filter(|(job, _)| living(state).filter(|c| c.jobs.contains(job)).count() < 2)
            .map(|(_, word)| *word)
            .collect();
        Self {
            purpose,
            day,
            living: living_people,
            dead,
            taken,
            short_work,
            free_home: get_place(&free_home(state)).name.to_owned(),
        }
    }

    fn first_name_taken(&self, first: &str) -> bool {
        let first = first.to_lowercase();
        self.taken.contains(&first) || INSPIRATION.iter().any(|(name, _, _)| first_of(name) == first)
    }
}

fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

const WORLD: &str = "You invent the people of a small fantasy village in a valley: a tavern, a market, a smithy, a chapel, a village green with a well, a training yard and cottages, with a farm, a lumber camp, an old mine, a lake, a forest glade with a shrine and a watchtower around it. East of the river lie an old graveyard and ruins. On some nights the dead rise from the graveyard and attack; the watch raises the alarm and those who can fight defend the village. Death is permanent, and the village fills its empty beds with newcomers from the west road.

Each person you invent is a whole human being with a trade, a past, flaws, wants and a way of talking: the sort of person a good novelist would be glad to have written. Vary ages, pronouns, classes and temperaments across the village. Plain, concrete words; no clichés about destiny or chosen ones. Answer only by calling the tool create_villager, once.";

/// The messages and the one tool for a casting request.
pub fn casting_prompt(ctx: &CastingContext) -> Prompt {
    let mut lines = Vec::new();
    match ctx.purpose {
        Purpose::Founder { settled, total } => lines.push(format!(
            "A village is being founded in the valley. {settled} of its {total} founders have settled; invent the next one. The founders are the people who will live here from the first day: they need not all be fighters, but someone must keep watch at night, grow food and mend what breaks."
        )),
        Purpose::Newcomer => lines.push(
            "Someone is coming up the west road to settle in a village that has been living for a while. Invent them. They may be a stranger, or kin or an old friend of someone here, living or dead, come to join them or to mourn them.".into(),
        ),
    }
    lines.push(format!("It is day {}.", ctx.day));
    if ctx.living.is_empty() {
        lines.push("Nobody lives here yet.".into());
    } else {
        lines.push("The people here:".into());
        for p in &ctx.living {
            lines.push(format!("- {}, {} ({})", p.name, p.occupation, p.class));
        }
    }
    if !ctx.dead.is_empty() {
        lines.push("Dead of late:".into());
        for d in &ctx.dead {
            let missed =
                if d.mourners.is_empty() { String::new() } else { format!("; missed by {}", d.mourners.join(", ")) };
            lines.push(format!("- {}, day {}, {}{missed}", d.name, d.day, d.cause));
        }
    }
    if !ctx.short_work.is_empty() {
        lines.push(format!("Work too few people do: {}.", ctx.short_work.join(", ")));
    }
    lines.push(format!("Their home would be {}.", ctx.free_home));
    if !ctx.taken.is_empty() {
        let taken: Vec<String> = ctx.taken.iter().map(|t| capitalise(t)).collect();
        lines.push(format!("First names already taken, which the new person must not have: {}.", taken.join(", ")));
    }
    lines.push("For the flavour of the valley, here are people who once lived in such villages. Do not reuse their names or copy them; make someone new in the same spirit:".into());
    for (name, class, line) in INSPIRATION {
        lines.push(format!("- {name} ({class}): {line}"));
    }
    lines.push("Now invent one new person for this village and call create_villager with them.".into());
    Prompt {
        messages: vec![ChatMessage::system(WORLD), ChatMessage::user(lines.join("\n"))],
        tools: vec![villager_tool()],
        max_calls: 1,
    }
}

fn class_ids() -> Vec<&'static str> {
    ClassId::ALL.iter().map(|c| c.def().label).collect()
}

fn quirk_names() -> Vec<String> {
    ALLOWED_QUIRKS.iter().map(|q| serde_json::to_value(q).unwrap().as_str().unwrap().to_owned()).collect()
}

/// The tool a casting answer calls.
pub fn villager_tool() -> ToolDef {
    let text =
        |description: &str, max: usize| json!({ "type": "string", "maxLength": max, "description": description });
    let number =
        |description: &str| json!({ "type": "number", "minimum": 0, "maximum": 1, "description": description });
    ToolDef::new(
        "create_villager",
        "One new person for the village.",
        json!({
            "type": "object",
            "properties": {
                "firstName": text("A first name nobody here has.", 24),
                "lastName": text("A family name.", 24),
                "pronouns": { "type": "string", "enum": ["she/her", "he/him", "they/them"] },
                "age": { "type": "integer", "minimum": 18, "maximum": 70 },
                "class": { "type": "string", "enum": class_ids(), "description": "What they are, which decides how they look, what work suits them and how they fight." },
                "occupation": text("Their trade, in a few words, e.g. 'hunter and trapper'.", 40),
                "backstory": text("Where they come from, what happened to them and why they are here: two to five sentences, in the third person.", 600),
                "speechStyle": text("How they talk, in one sentence.", 120),
                "timbre": text("How their voice sounds, in a few words.", 40),
                "traits": { "type": "array", "minItems": 2, "maxItems": 4, "items": text("One trait, a word or two.", 24) },
                "goals": { "type": "array", "minItems": 2, "maxItems": 3, "items": text("Something they want, in the first person.", 80) },
                "secret": text("Optional. Something they hide from everyone, in the first person.", 200),
                "temper": number("0 calm to 1 hot-headed."),
                "bravery": number("0 timid to 1 fearless."),
                "sociability": number("0 solitary to 1 gregarious."),
                "allure": number("Optional. 0 none to 1 a shameless flirt."),
                "shyness": number("Optional. 0 at ease to 1 tongue-tied with strangers."),
                "looks": number("Optional. 0 unremarkable to 1 strikingly good-looking."),
                "quirk": { "type": "string", "enum": quirk_names(), "description": "Optional. A leaning the village will feel." },
                "reason": text("Optional, for a newcomer: why they came, as a phrase that completes 'arrived by the west road, …'.", 200),
                "bonds": {
                    "type": "array", "maxItems": 2,
                    "description": "Optional. People here (by full name, from the message) they already know.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "to": text("The full name of someone living here.", 48),
                            "relation": { "type": "string", "enum": ["sibling", "parent", "child", "spouse", "friend", "comrade", "rival"] },
                            "summary": text("Optional. How the new person puts it, in the first person, one sentence.", 120)
                        },
                        "required": ["to", "relation"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["firstName", "lastName", "pronouns", "age", "class", "occupation", "backstory", "speechStyle", "timbre", "traits", "goals", "temper", "bravery", "sociability"],
            "additionalProperties": false
        }),
    )
}

fn text(args: &Map<String, Value>, key: &str, max: usize) -> Option<String> {
    args.get(key).and_then(Value::as_str).and_then(clean_text).map(|s| utf16_truncate(&s, max).to_owned())
}

fn texts(args: &Map<String, Value>, key: &str, max: usize, most: usize) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .filter_map(clean_text)
                .map(|s| utf16_truncate(&s, max).to_owned())
                .take(most)
                .collect()
        })
        .unwrap_or_default()
}

fn unit(args: &Map<String, Value>, key: &str) -> Option<f64> {
    args.get(key).and_then(Value::as_f64).filter(|n| n.is_finite()).map(|n| n.clamp(0.0, 1.0))
}

/// Turns the model's call into a villager, or says what was wrong with it. Bonds to people not
/// living here are dropped, not refused.
pub fn interpret_villager(call: &ToolCall, ctx: &CastingContext) -> Result<NewVillager, String> {
    if call.name != "create_villager" {
        return Err(format!("called {} instead of create_villager", call.name));
    }
    let args = &call.args;
    let first = text(args, "firstName", 24).ok_or("no first name")?;
    let last = text(args, "lastName", 24).ok_or("no last name")?;
    if first.contains(' ') || !first.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-') {
        return Err(format!("first name {first:?} is not one word"));
    }
    if ctx.first_name_taken(&first) {
        return Err(format!("the first name {first} is taken"));
    }
    let name = format!("{first} {last}");
    if INSPIRATION.iter().any(|(old, _, _)| old.eq_ignore_ascii_case(&name)) {
        return Err(format!("{name} is one of the old cast"));
    }
    let pronouns = match args.get("pronouns").and_then(Value::as_str) {
        Some("she/her") => Pronouns::SheHer,
        Some("he/him") => Pronouns::HeHim,
        Some("they/them") => Pronouns::TheyThem,
        other => return Err(format!("pronouns {other:?}")),
    };
    let age = args.get("age").and_then(Value::as_i64).ok_or("no age")?.clamp(18, 70);
    let class_id = args
        .get("class")
        .and_then(Value::as_str)
        .and_then(|label| ClassId::ALL.into_iter().find(|c| c.def().label.eq_ignore_ascii_case(label)))
        .ok_or_else(|| format!("class {:?}", args.get("class")))?;
    let occupation = text(args, "occupation", 40).ok_or("no occupation")?;
    let backstory = text(args, "backstory", 600).ok_or("no backstory")?;
    let speech_style = text(args, "speechStyle", 120).ok_or("no speech style")?;
    let timbre = text(args, "timbre", 40).ok_or("no timbre")?;
    let traits = texts(args, "traits", 24, 4);
    if traits.len() < 2 {
        return Err("fewer than two traits".into());
    }
    let goals = texts(args, "goals", 80, 3);
    if goals.len() < 2 {
        return Err("fewer than two goals".into());
    }
    let temper = unit(args, "temper").ok_or("no temper")?;
    let bravery = unit(args, "bravery").ok_or("no bravery")?;
    let sociability = unit(args, "sociability").ok_or("no sociability")?;
    let quirks = match args.get("quirk").and_then(Value::as_str).filter(|q| !q.is_empty()) {
        None => Vec::new(),
        Some(q) => match serde_json::from_value::<Quirk>(Value::String(q.to_owned())) {
            Ok(quirk) if ALLOWED_QUIRKS.contains(&quirk) => vec![quirk],
            _ => return Err(format!("quirk {q:?}")),
        },
    };
    let bonds = args
        .get("bonds")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let to = item.get("to").and_then(Value::as_str)?;
                    let person = ctx.living.iter().find(|p| p.name.eq_ignore_ascii_case(to))?;
                    let relation = serde_json::from_value::<BondKind>(item.get("relation")?.clone()).ok()?;
                    let summary = item.as_object().and_then(|o| text(o, "summary", 120)).unwrap_or_default();
                    Some(Bond { to: person.name.clone(), relation, summary })
                })
                .take(2)
                .collect()
        })
        .unwrap_or_default();
    Ok(NewVillager {
        class_id,
        name,
        pronouns,
        age,
        occupation,
        backstory,
        traits,
        goals,
        speech_style,
        timbre,
        temper,
        bravery,
        sociability,
        spite: 0.0,
        allure: unit(args, "allure").unwrap_or(0.0),
        shyness: unit(args, "shyness").unwrap_or(0.0),
        looks: unit(args, "looks").unwrap_or(0.0),
        secret: text(args, "secret", 200).unwrap_or_default(),
        reason: text(args, "reason", 200).unwrap_or_default(),
        quirks,
        bonds,
    })
}

/// Why a casting request gave no villager.
#[derive(Debug, Clone, PartialEq)]
pub enum CastingError {
    /// the model could not be asked, or did not answer
    Model(LlmError),
    /// it answered, but not with a person the village can take
    Refused(String),
}

impl fmt::Display for CastingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(error) => write!(f, "the model did not answer: {error}"),
            Self::Refused(why) => write!(f, "the answer was refused: {why}"),
        }
    }
}

/// Asks the model for one villager. An answer without a usable call, or one the village cannot
/// take, is asked for once more; the second refusal is final.
pub async fn generate_villager(client: &LlmClient, ctx: &CastingContext) -> Result<NewVillager, CastingError> {
    let prompt = casting_prompt(ctx);
    let request = LlmRequest {
        messages: prompt.messages,
        tools: prompt.tools,
        max_calls: 1,
        max_tokens: Some(MAX_TOKENS),
        timeout: Some(TIMEOUT),
        ..LlmRequest::default()
    };
    let mut last = None;
    for _ in 0..2 {
        match client.complete(&request).await {
            Ok(response) => match response.calls.first() {
                Some(call) => match interpret_villager(call, ctx) {
                    Ok(villager) => return Ok(villager),
                    Err(why) => last = Some(CastingError::Refused(why)),
                },
                None => last = Some(CastingError::Refused("no tool call".into())),
            },
            Err(error) => {
                let again = matches!(error.kind, LlmErrorKind::NoCall | LlmErrorKind::Truncated);
                last = Some(CastingError::Model(error));
                if !again {
                    break;
                }
            }
        }
    }
    Err(last.expect("two tries leave a reason"))
}

/// Whether a description is still usable the moment it arrives: the village may have changed.
pub fn days_since(state: &SimState, tick: i64) -> i64 {
    (state.tick - tick) / TICKS_PER_DAY
}

#[cfg(test)]
mod tests {
    use fsw_sim::{Command, Sim, create_state};
    use fsw_testing::create_test_state;

    use super::*;
    use crate::prompts::prompt_text;

    fn founding_context() -> CastingContext {
        let mut sim = Sim::new(create_state(3));
        sim.enqueue(Command::FoundVillage);
        sim.step();
        let mut state = sim.state;
        // Keep three, so there is room and work to be short of.
        while state.order.len() > 3 {
            let id = state.order.pop().unwrap();
            state.characters.shift_remove(&id);
        }
        state.director.founding = true;
        CastingContext::from_state(&state, Purpose::Founder { settled: 3, total: 13 }, &["Reserved Name".into()])
    }

    fn good_call() -> ToolCall {
        ToolCall::new(
            "create_villager",
            json!({
                "firstName": "Oswin", "lastName": "Thistlewood", "pronouns": "he/him", "age": 44, "class": "farmer",
                "occupation": "farmer and beekeeper", "backstory": "Came over the pass with   two hives and a bad knee. Wants peace.",
                "speechStyle": "Slow and kindly.", "timbre": "warm and low", "traits": ["patient", "stubborn", "sweet-toothed"],
                "goals": ["see the hives through winter", "find someone to leave them to"], "temper": 0.2, "bravery": 0.4, "sociability": 0.7,
                "looks": 0.3, "quirk": "diligent", "secret": "I left a wife who was glad to see me go."
            }),
        )
    }

    #[test]
    fn the_prompt_tells_of_the_village_the_taken_names_and_the_old_cast() {
        let ctx = founding_context();
        let text = prompt_text(&casting_prompt(&ctx));
        assert!(text.contains("3 of its 13 founders have settled"), "{text}");
        assert!(text.contains("The people here:"), "{text}");
        assert!(text.contains("First names already taken, which the new person must not have: "), "{text}");
        assert!(text.contains("Reserved"), "{text}");
        assert!(text.contains("- Aldric Vane (knight): widowed veteran"), "{text}");
        assert!(text.contains("Work too few people do:"), "{text}");
        assert!(text.contains("Their home would be "), "{text}");
        let prompt = casting_prompt(&ctx);
        let tools: Vec<&str> = prompt.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(tools, ["create_villager"]);
    }

    #[test]
    fn a_newcomers_prompt_names_the_dead_and_who_misses_them() {
        let mut state = create_test_state(1);
        let id = state.order[0].clone();
        let name = state.characters[&id].profile.name.clone();
        state.characters.shift_remove(&id);
        state.order.retain(|other| *other != id);
        state.dead.insert(
            id.clone(),
            fsw_sim::state::DeadRecord {
                name: name.clone(),
                day: 1,
                cause: "slain by a skeleton".into(),
                killer_id: None,
                hidden_killer_id: None,
                spouse_id: None,
            },
        );
        for other in state.order.clone() {
            state.characters[&other].relationships[&id].affinity = 40.0;
        }
        let ctx = CastingContext::from_state(&state, Purpose::Newcomer, &[]);
        assert_eq!(ctx.dead.len(), 1);
        assert!(ctx.dead[0].mourners.len() >= 3);
        assert!(ctx.taken.contains(&"aldric".to_string()), "the dead keep their names: {:?}", ctx.taken);
        let text = prompt_text(&casting_prompt(&ctx));
        assert!(text.contains("coming up the west road"), "{text}");
        assert!(text.contains(&format!("- {name}, day 1, slain by a skeleton; missed by ")), "{text}");
    }

    #[test]
    fn a_good_answer_becomes_a_villager_cleaned_and_cut_to_size() {
        let ctx = founding_context();
        let v = interpret_villager(&good_call(), &ctx).unwrap();
        assert_eq!(v.name, "Oswin Thistlewood");
        assert_eq!(v.class_id, ClassId::Farmer);
        assert_eq!(v.backstory, "Came over the pass with two hives and a bad knee. Wants peace.");
        assert_eq!(v.quirks, vec![Quirk::Diligent]);
        assert_eq!((v.temper, v.bravery, v.sociability, v.looks), (0.2, 0.4, 0.7, 0.3));
        assert_eq!(v.secret, "I left a wife who was glad to see me go.");
        assert!(v.bonds.is_empty() && v.reason.is_empty());

        let mut long = good_call();
        long.args.insert("backstory".into(), Value::String("x".repeat(700)));
        long.args.insert("traits".into(), json!(["a", "b", "c", "d", "e"]));
        long.args.insert("age".into(), json!(99));
        long.args.insert("temper".into(), json!(7));
        let v = interpret_villager(&long, &ctx).unwrap();
        assert_eq!(v.backstory.len(), 600);
        assert_eq!(v.traits.len(), 4);
        assert_eq!((v.age, v.temper), (70, 1.0));
    }

    #[test]
    fn bad_answers_are_refused_with_a_reason() {
        let ctx = founding_context();
        let with = |key: &str, value: Value| {
            let mut call = good_call();
            call.args.insert(key.into(), value);
            interpret_villager(&call, &ctx).unwrap_err()
        };
        let taken = capitalise(&ctx.taken[0]);
        assert!(with("firstName", json!(taken)).contains("is taken"));
        assert!(with("firstName", json!("reserved")).contains("is taken"));
        assert!(with("firstName", json!("Aldric")).contains("is taken"));
        assert!(with("firstName", json!("Two Words")).contains("not one word"));
        assert!(with("class", json!("dragon")).contains("class"));
        assert!(with("quirk", json!("murderous")).contains("quirk"));
        assert!(with("quirk", json!("occultist")).contains("quirk"));
        assert!(with("pronouns", json!("xe/xem")).contains("pronouns"));
        assert!(with("traits", json!(["only one"])).contains("fewer than two traits"));
        assert!(with("goals", json!([])).contains("fewer than two goals"));
        let mut missing = good_call();
        missing.args.remove("backstory");
        assert_eq!(interpret_villager(&missing, &ctx).unwrap_err(), "no backstory");
        let wrong = ToolCall::new("go_to", json!({}));
        assert!(interpret_villager(&wrong, &ctx).unwrap_err().contains("instead of create_villager"));
    }

    #[test]
    fn bonds_to_the_living_are_kept_and_the_rest_dropped() {
        let ctx = founding_context();
        let here = ctx.living[0].name.clone();
        let mut call = good_call();
        call.args.insert(
            "bonds".into(),
            json!([
                { "to": here.to_uppercase(), "relation": "sibling", "summary": "My  sister." },
                { "to": "Nobody Here", "relation": "friend" },
                { "to": here, "relation": "dragon" }
            ]),
        );
        let v = interpret_villager(&call, &ctx).unwrap();
        assert_eq!(v.bonds.len(), 1);
        assert_eq!(v.bonds[0].relation, BondKind::Sibling);
        assert_eq!(v.bonds[0].summary, "My sister.");
        assert_eq!(v.bonds[0].to, ctx.living[0].name);
    }

    #[tokio::test]
    async fn generate_asks_again_once_after_a_refusal_and_gives_up_the_second_time() {
        use std::sync::Arc;

        use crate::llm::transport::{FakeTransport, HttpResponse};
        use crate::settings::{ApiKey, LlmSettings};

        let ctx = founding_context();
        let taken = capitalise(&ctx.taken[0]);
        let mut bad = good_call();
        bad.args.insert("firstName".into(), json!(taken));
        let answers = Arc::new(std::sync::Mutex::new(vec![good_call(), bad.clone(), bad]));
        let transport = FakeTransport::new({
            let answers = answers.clone();
            move |_| {
                let call = answers.lock().unwrap().pop().unwrap();
                let body = json!({ "choices": [{ "message": { "tool_calls": [
                    { "function": { "name": call.name, "arguments": Value::Object(call.args).to_string() } },
                ] } }] });
                async move { Ok(HttpResponse::json(200, &body)) }
            }
        });
        let settings = LlmSettings {
            enabled: true,
            base_url: "http://llm.test/v1".into(),
            model: "m".into(),
            ..LlmSettings::default()
        };
        let client = LlmClient::new(settings, ApiKey::default(), transport.clone());
        let error = generate_villager(&client, &ctx).await.unwrap_err();
        assert!(matches!(error, CastingError::Refused(ref why) if why.contains("is taken")), "{error}");
        assert_eq!(transport.requests().len(), 2);
        let villager = generate_villager(&client, &ctx).await.unwrap();
        assert_eq!(villager.name, "Oswin Thistlewood");
        let body = &transport.bodies()[2];
        assert_eq!(body["max_tokens"], MAX_TOKENS);
        assert_eq!(body["tools"][0]["function"]["name"], "create_villager");
    }
}
