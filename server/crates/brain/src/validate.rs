//! Checks a model's tool calls against the world as it is and turns them into
//! commands, repairing what can be repaired (port of `src/brain/tools/validate.ts`).

use fsw_core::jsmath::{js_hypot, utf16_len, utf16_truncate};
use fsw_sim::data::economy::LOOT_REACH;
use fsw_sim::state::{
    Character, ConversationLine, Feeling, FightMode, Id, Intent, IntentAction, IntentSource, ItemId, Map, MonsterPhase,
    PlanBlock, SimState, TalkTone, WorldJob,
};
use fsw_sim::systems::animals;
use fsw_sim::systems::ceremonies;
use fsw_sim::systems::economy;
use fsw_sim::systems::fire;
use fsw_sim::systems::relationships::brawl_unlocked;
use fsw_sim::systems::world::{self, gather_target};
use fsw_sim::util::{first_name, living};
use fsw_world::places::{ActivityId, Place, places};
use serde_json::{Map as JsonMap, Value};

use crate::llm::types::ToolCall;
use crate::prompts::animals_near;

const MAX_SPOKEN: usize = 220;

const PLACE_ALIASES: &[(&str, &str)] = &[
    ("tavern", "tavern"),
    ("tankard", "tavern"),
    ("inn", "tavern"),
    ("pub", "tavern"),
    ("green", "green"),
    ("well", "green"),
    ("square", "green"),
    ("crossroads", "green"),
    ("market", "market"),
    ("stalls", "market"),
    ("smithy", "smithy"),
    ("forge", "smithy"),
    ("blacksmith", "smithy"),
    ("chapel", "chapel"),
    ("church", "chapel"),
    ("temple", "chapel"),
    ("yard", "training_yard"),
    ("barracks", "training_yard"),
    ("training", "training_yard"),
    ("farm", "farm"),
    ("windmill", "farm"),
    ("fields", "farm"),
    ("lumber", "lumber_camp"),
    ("sawmill", "lumber_camp"),
    ("glade", "glade"),
    ("shrine", "glade"),
    ("forest", "glade"),
    ("campfire", "glade"),
    ("woods", "glade"),
    ("mine", "mine"),
    ("lake", "docks"),
    ("shore", "docks"),
    ("docks", "docks"),
    ("beach", "docks"),
    ("watchtower", "watchtower"),
    ("tower", "watchtower"),
    ("bridge", "watchtower"),
    ("graveyard", "graveyard"),
    ("cemetery", "graveyard"),
    ("graves", "graveyard"),
    ("ruins", "ruins"),
    ("road", "west_road"),
];

const ACTIVITY_ALIASES: &[(&str, ActivityId)] = {
    use ActivityId::*;
    &[
        ("work", Smith),
        ("forge", Smith),
        ("smithing", Smith),
        ("hammer", Smith),
        ("wood", ChopWood),
        ("chop", ChopWood),
        ("lumber", ChopWood),
        ("fishing", Fish),
        ("farming", Farm),
        ("mining", Mine),
        ("dig", Mine),
        ("meal", Eat),
        ("breakfast", Eat),
        ("lunch", Eat),
        ("dinner", Eat),
        ("supper", Eat),
        ("food", Eat),
        ("ale", Drink),
        ("beer", Drink),
        ("drinking", Drink),
        ("nap", Sleep),
        ("bed", Sleep),
        ("sleeping", Sleep),
        ("sit", Rest),
        ("bench", Rest),
        ("market", Trade),
        ("stall", Trade),
        ("sell", Trade),
        ("trading", Trade),
        ("buy", Shop),
        ("browse", Shop),
        ("shopping", Shop),
        ("meditate", Pray),
        ("prayer", Pray),
        ("praying", Pray),
        ("training", Train),
        ("practise", Train),
        ("practice", Train),
        ("spar", Train),
        ("herbs", Forage),
        ("gather", Forage),
        ("foraging", Forage),
        ("sing", Perform),
        ("singing", Perform),
        ("song", Perform),
        ("music", Perform),
        ("busk", Perform),
        ("lute", Perform),
        ("fiddle", Perform),
        ("performing", Perform),
        ("guard", Watch),
        ("patrol", Watch),
        ("campfire", Relax),
        ("fire", Relax),
        ("potions", Brew),
        ("brewing", Brew),
        ("potion", Brew),
        ("grave", Mourn),
        ("graves", Mourn),
        ("walk", Stroll),
        ("wander", Stroll),
    ]
};

/// Lower-cased, trimmed, without a leading "the " and without unusual characters.
fn norm(value: Option<&Value>) -> String {
    let Some(Value::String(text)) = value else { return String::new() };
    norm_str(text)
}

fn norm_str(text: &str) -> String {
    let lower = text.trim().to_lowercase();
    let lower = lower.strip_prefix("the ").map(str::trim_start).unwrap_or(&lower).to_owned();
    lower
        .chars()
        .filter(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | ' ' | '\'' | '-'))
        .collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut previous = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let current = row[j];
            row[j] = (row[j] + 1).min(row[j - 1] + 1).min(previous + usize::from(a[i - 1] != b[j - 1]));
            previous = current;
        }
    }
    row[b.len()]
}

/// Finds a living person by the name a model gave: exact, first name, prefix, or a near miss.
pub fn resolve_person<'s>(state: &'s SimState, name: Option<&Value>, except: Option<&str>) -> Option<&'s Character> {
    let wanted = norm(name);
    if wanted.is_empty() {
        return None;
    }
    let people: Vec<&Character> = living(state).filter(|c| Some(c.id.as_str()) != except).collect();
    let first = |c: &Character| first_name(c).to_lowercase();
    let wanted_first = wanted.split(' ').next().unwrap_or_default();
    people
        .iter()
        .find(|c| c.profile.name.to_lowercase() == wanted || first(c) == wanted)
        .or_else(|| {
            people.iter().find(|c| {
                wanted_first == first(c)
                    || first(c).starts_with(&wanted)
                    || c.profile.name.to_lowercase().starts_with(&wanted)
            })
        })
        .or_else(|| people.iter().find(|c| edit_distance(&first(c), wanted_first) <= 1))
        .copied()
}

pub fn resolve_place(c: &Character, name: Option<&Value>) -> Option<&'static Place> {
    let wanted = norm(name);
    if wanted.is_empty() {
        return None;
    }
    let all = places();
    if matches!(wanted.as_str(), "home" | "my home" | "my house") {
        return all.iter().find(|p| p.id == c.home_id);
    }
    let underscored = wanted.replace(' ', "_");
    if let Some(exact) = all.iter().find(|p| p.id == wanted || p.id == underscored || norm_str(p.name) == wanted) {
        return Some(exact);
    }
    for word in wanted.split([' ', '_']).filter(|w| !w.is_empty()) {
        if let Some((_, id)) = PLACE_ALIASES.iter().find(|(alias, _)| *alias == word) {
            return all.iter().find(|p| p.id == *id);
        }
    }
    all.iter().find(|p| {
        let name = norm_str(p.name);
        name.contains(&wanted) || wanted.contains(&name)
    })
}

pub fn resolve_activity(value: Option<&Value>) -> Option<ActivityId> {
    let wanted = norm(value).replace(' ', "_");
    if wanted.is_empty() {
        return None;
    }
    let offered = |text: &str| ActivityId::parse(text).filter(|id| ActivityId::OFFERED.contains(id));
    if let Some(id) = offered(&wanted) {
        return Some(id);
    }
    // "play music" is a song, not a child's game: settled before the word by word pass sees "play".
    if wanted.split('_').any(|word| matches!(word, "music" | "sing" | "song" | "tune")) {
        return Some(ActivityId::Perform);
    }
    for word in wanted.split('_') {
        if let Some(id) = offered(word) {
            return Some(id);
        }
        if let Some((_, id)) = ACTIVITY_ALIASES.iter().find(|(alias, _)| *alias == word) {
            return Some(*id);
        }
    }
    let spaced = wanted.replace('_', " ");
    ActivityId::OFFERED
        .into_iter()
        .find(|id| fsw_sim::data::activities::activity(*id).label.to_lowercase().contains(&spaced))
}

/// As [`resolve_activity`], and also rebuilding while there is a ruin to rebuild.
pub fn resolve_doable(state: &SimState, value: Option<&Value>) -> Option<ActivityId> {
    if fire::ruin(state).is_some() && norm(value).contains("rebuild") {
        return Some(ActivityId::Rebuild);
    }
    resolve_activity(value)
}

/// Removes `open … close` spans (non-nested), like `/\*[^*]*\*/g` or `/\([^)]*\)/g`.
fn strip_spans(text: &str, open: char, close: char) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(open) {
        let after = &rest[start + open.len_utf8()..];
        match after.find(close) {
            Some(end) => {
                out.push_str(&rest[..start]);
                out.push(' ');
                rest = &after[end + close.len_utf8()..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// Tidies a spoken line: no quotes, no stage directions, not too long.
pub fn spoken(value: Option<&Value>) -> Option<String> {
    tidy(value, MAX_SPOKEN)
}

/// The text of a ceremony speech from the `speech` call: tidied like any spoken line, but allowed to run
/// longer. `None` when nothing usable was said.
pub fn interpret_speech(calls: &[ToolCall]) -> Option<String> {
    let call = calls.iter().find(|c| c.name == "speech")?;
    tidy(call.args.get("text"), ceremonies::SPEECH_CAP)
}

/// The words the model painted on a billboard with `write_billboard`, tidied like a spoken line and kept to what a
/// board holds. `None` when nothing usable was written.
pub fn interpret_billboard(calls: &[ToolCall]) -> Option<String> {
    let call = calls.iter().find(|c| c.name == "write_billboard")?;
    tidy(call.args.get("text"), fsw_sim::systems::billboards::TEXT_CAP)
}

fn tidy(value: Option<&Value>, max: usize) -> Option<String> {
    let Some(Value::String(text)) = value else { return None };
    let text = strip_spans(&strip_spans(text, '*', '*'), '(', ')');
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = text.trim_start_matches(['"', '“', '\'', '‘']).trim_end_matches(['"', '”', '\'', '’']).trim().to_owned();
    if text.is_empty() {
        return None;
    }
    if utf16_len(&text) <= max {
        return Some(text);
    }
    let cut = utf16_truncate(&text, max);
    let stop = [". ", "! ", "? "].iter().filter_map(|mark| cut.rfind(mark)).max();
    Some(match stop {
        Some(stop) if utf16_len(&cut[..stop]) > 60 => cut[..=stop].to_owned(),
        _ => format!("{}…", cut.trim_end()),
    })
}

fn thought(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) if !text.trim().is_empty() => Some(utf16_truncate(text.trim(), 240).to_owned()),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub struct Decision {
    pub intents: Vec<Intent>,
    /// the model chose to carry on
    pub keep: bool,
    pub thought: Option<String>,
    /// what was wrong with calls that were dropped, for the decision log
    pub problems: Vec<String>,
}

fn shown(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "undefined".into(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn tone(value: Option<&Value>) -> TalkTone {
    match value.and_then(Value::as_str) {
        Some("romantic") => TalkTone::Romantic,
        Some("hostile") => TalkTone::Hostile,
        Some("apology") => TalkTone::Apology,
        _ => TalkTone::Friendly,
    }
}

fn item(value: Option<&Value>) -> Option<ItemId> {
    match value.and_then(Value::as_str) {
        Some("food") => Some(ItemId::Food),
        Some("potion") => Some(ItemId::Potion),
        Some("gift") => Some(ItemId::Gift),
        Some("trinket") => Some(ItemId::Trinket),
        Some("relic") => Some(ItemId::Relic),
        _ => None,
    }
}

/// The commission the model named, by the words of its label ("room", "wagon", "strongbox"), else
/// the first affordable; `None` when there is nothing to pay for.
pub fn resolve_commission(state: &SimState, c: &Character, value: Option<&Value>) -> Option<economy::Commission> {
    let options = economy::affordable(state, c);
    let text = norm(value);
    let words: Vec<&str> = text.split(' ').filter(|w| w.len() > 2).collect();
    let mut best: Option<(usize, &economy::Commission)> = None;
    for option in &options {
        let label = norm_str(&option.label());
        let hits = words.iter().filter(|w| label.contains(*w)).count();
        if hits > 0 && best.is_none_or(|(b, _)| hits > b) {
            best = Some((hits, option));
        }
    }
    best.map(|(_, o)| o.clone()).or_else(|| options.first().cloned())
}

/// The loot within reach the model named, by the words of its label ("coins", "trinket", the place),
/// else the nearest; `None` when there is none within reach.
pub fn resolve_loot(state: &SimState, c: &Character, value: Option<&Value>) -> Option<String> {
    let near = economy::loot_near(state, c.pos, LOOT_REACH);
    let text = norm(value);
    let words: Vec<&str> = text.split(' ').filter(|w| w.len() > 2).collect();
    let mut best: Option<(usize, &String)> = None;
    for (_, id) in &near {
        let label = norm_str(&economy::loot_label(&state.loot[id]));
        let hits = words.iter().filter(|w| label.contains(*w)).count();
        if hits > 0 && best.is_none_or(|(b, _)| hits > b) {
            best = Some((hits, id));
        }
    }
    best.map(|(_, id)| id.clone()).or_else(|| near.first().map(|(_, id)| id.clone()))
}

/// The animal near `c` the model means: by name, or by kind ("the cow", "sheep"), the nearest that fits.
pub fn resolve_animal(state: &SimState, c: &Character, value: Option<&Value>) -> Option<String> {
    let text = norm(value);
    if text.is_empty() {
        return None;
    }
    let near = animals_near(state, c);
    let by_name = near.iter().find(|a| a.name.as_deref().is_some_and(|n| text.contains(&norm_str(n))));
    let by_kind = || {
        near.iter().find(|a| {
            let def = a.species.def();
            text.split(' ').any(|w| w == def.noun || w == def.plural)
        })
    };
    by_name.or_else(by_kind).map(|a| a.id.clone())
}

/// The name a villager gave their new pet, cleaned up the way the sim takes it.
pub fn interpret_pet_name(calls: &[ToolCall]) -> Option<String> {
    let call = calls.iter().find(|c| c.name == "name_pet")?;
    let Some(Value::String(name)) = call.args.get("name") else { return None };
    animals::clean_name(name)
}

/// Turns the tool calls of a "decide" or "react" reply into intents.
pub fn interpret_decision(state: &SimState, c: &Character, calls: &[ToolCall]) -> Decision {
    let mut decision = Decision::default();
    for call in calls {
        let args = &call.args;
        let say = spoken(args.get("say"));
        let thought = thought(args.get("thought"));
        if decision.thought.is_none() {
            decision.thought = thought.clone();
        }
        let intent = |action: IntentAction| Intent {
            action,
            source: IntentSource::Llm,
            say: say.clone(),
            thought: thought.clone(),
        };
        match call.name.as_str() {
            "continue" => decision.keep = true,
            "go_to" => match resolve_place(c, args.get("place")) {
                Some(place) => decision.intents.push(intent(IntentAction::GoTo { place_id: place.id.clone() })),
                None => decision.problems.push(format!("unknown place \"{}\"", shown(args.get("place")))),
            },
            "do_activity" => match resolve_doable(state, args.get("activity")) {
                Some(activity) if c.is_child() && !ActivityId::CHILD_OFFERED.contains(&activity) => {
                    decision.problems.push(format!("a child does not {}", activity.as_str()));
                }
                // Chopping wood and mining mean a real tree or rock when there is one within reach.
                Some(activity) => match real_work(state, c, activity) {
                    Some(action) => decision.intents.push(intent(action)),
                    None => decision.intents.push(intent(IntentAction::Activity {
                        activity,
                        place_id: None,
                        minutes: None,
                    })),
                },
                None => decision.problems.push(format!("unknown activity \"{}\"", shown(args.get("activity")))),
            },
            "talk_to" => {
                let person = resolve_person(state, args.get("person"), Some(&c.id));
                let opener = spoken(args.get("message"));
                match (person, opener) {
                    (Some(person), Some(opener)) => decision.intents.push(Intent {
                        action: IntentAction::Talk {
                            target_id: person.id.clone(),
                            opener,
                            tone: tone(args.get("tone")),
                        },
                        source: IntentSource::Llm,
                        say: None,
                        thought: thought.clone(),
                    }),
                    (Some(_), None) => decision.problems.push("talk_to without a message".into()),
                    (None, _) => decision.problems.push(format!("unknown person \"{}\"", shown(args.get("person")))),
                }
            }
            // A child has nothing to give, never raises a hand and does no work on the world; the sim would
            // refuse it too.
            "give" | "brawl" | "attack" | "work_on" | "trade" | "commission" if c.is_child() => {
                decision.problems.push(format!("a child does not {}", call.name));
            }
            "work_on" => {
                let job = norm(args.get("job"));
                let job =
                    WorldJob::parse(&job).or_else(|| JOB_ALIASES.iter().find(|(a, _)| *a == job).map(|(_, j)| *j));
                match job {
                    None => decision.problems.push(format!("unknown job \"{}\"", shown(args.get("job")))),
                    Some(WorldJob::Deliver) => decision.problems.push("carrying is not chosen; it follows".into()),
                    Some(job) => match resolve_target(state, c, job, args.get("target")) {
                        Some(target) => {
                            decision.intents.push(intent(IntentAction::WorldWork { job, target, minutes: None }))
                        }
                        None => decision.problems.push(format!(
                            "nothing to {} at \"{}\"",
                            job.as_str(),
                            shown(args.get("target"))
                        )),
                    },
                }
            }
            "trade" => {
                let person = resolve_person(state, args.get("person"), Some(&c.id));
                let item = item(args.get("item"));
                let buy = match norm(args.get("deal")).as_str() {
                    "buy" => Some(true),
                    "sell" => Some(false),
                    _ => None,
                };
                match (person, item, buy) {
                    (None, _, _) => decision.problems.push(format!("unknown person \"{}\"", shown(args.get("person")))),
                    (_, None, _) => decision.problems.push(format!("unknown item \"{}\"", shown(args.get("item")))),
                    (_, _, None) => decision.problems.push(format!("unknown deal \"{}\"", shown(args.get("deal")))),
                    (Some(person), Some(item), Some(buy)) => {
                        let (buyer, seller) = if buy { (c, person) } else { (person, c) };
                        match economy::trade_offer(state, buyer, seller, item) {
                            Ok(_) => decision.intents.push(intent(IntentAction::Trade {
                                target_id: person.id.clone(),
                                item,
                                buy,
                            })),
                            Err(why) => decision.problems.push(format!(
                                "cannot {} {} {} {}: {why}",
                                if buy { "buy" } else { "sell" },
                                item.as_str(),
                                if buy { "from" } else { "to" },
                                first_name(person)
                            )),
                        }
                    }
                }
            }
            "commission" => match resolve_commission(state, c, args.get("what")) {
                Some(choice) => decision.intents.push(intent(IntentAction::Commission { what: choice.key() })),
                None => decision.problems.push(format!("nothing to pay for at \"{}\"", shown(args.get("what")))),
            },
            "pet" => match resolve_animal(state, c, args.get("animal")) {
                Some(animal_id) => decision.intents.push(intent(IntentAction::Pet { animal_id })),
                None => decision.problems.push(format!("no animal near you called \"{}\"", shown(args.get("animal")))),
            },
            "pick_up" => match resolve_loot(state, c, args.get("what")) {
                Some(loot_id) => decision.intents.push(intent(IntentAction::PickUp { loot_id })),
                None => decision.problems.push(format!("nothing to pick up at \"{}\"", shown(args.get("what")))),
            },
            "give" => {
                let person = resolve_person(state, args.get("person"), Some(&c.id));
                let item = item(args.get("item"));
                match (person, item) {
                    (None, _) => decision.problems.push(format!("unknown person \"{}\"", shown(args.get("person")))),
                    (Some(person), Some(item)) if c.inventory.get(item) > 0 => {
                        decision.intents.push(intent(IntentAction::Give { target_id: person.id.clone(), item }))
                    }
                    _ => decision.problems.push(format!("nothing to give ({})", shown(args.get("item")))),
                }
            }
            "brawl" | "attack" => {
                let Some(person) = resolve_person(state, args.get("person"), Some(&c.id)) else {
                    decision.problems.push(format!("unknown person \"{}\"", shown(args.get("person"))));
                    continue;
                };
                let lethal = call.name == "attack";
                if !lethal && !brawl_unlocked(c, &person.id) {
                    decision.problems.push(format!("not enough of a feud with {} to brawl", first_name(person)));
                    continue;
                }
                let mode = if lethal { FightMode::Lethal } else { FightMode::Brawl };
                decision.intents.push(intent(IntentAction::Fight { target_id: person.id.clone(), mode }));
            }
            "fight" => {
                // The nearest danger: a monster if there is one, otherwise whoever is attacking.
                let mut target: Option<&Id> = None;
                let mut best = f64::INFINITY;
                for monster in state.monsters.values() {
                    if monster.phase == MonsterPhase::Dying {
                        continue;
                    }
                    let d = js_hypot(monster.pos.x - c.pos.x, monster.pos.z - c.pos.z);
                    if d < best {
                        best = d;
                        target = Some(&monster.id);
                    }
                }
                if let Some(target) = target.filter(|_| best < 160.0) {
                    decision
                        .intents
                        .push(intent(IntentAction::Fight { target_id: target.clone(), mode: FightMode::Monster }));
                } else {
                    let attacker = living(state).find(|o| o.intent.as_ref().is_some_and(|i| i.fights(&c.id)));
                    match attacker.and_then(|a| a.intent.as_ref().map(|i| (a, i))) {
                        Some((a, Intent { action: IntentAction::Fight { mode, .. }, .. })) => {
                            decision.intents.push(intent(IntentAction::Fight { target_id: a.id.clone(), mode: *mode }))
                        }
                        _ => decision.problems.push("nothing to fight".into()),
                    }
                }
            }
            "flee" => decision.intents.push(intent(IntentAction::Flee)),
            other => decision.problems.push(format!("unexpected tool {other}")),
        }
    }
    decision
}

pub struct Plan {
    pub blocks: Vec<PlanBlock>,
    pub thought: Option<String>,
}

/// A plan hour: a number, a numeric string, or "13:30".
const JOB_ALIASES: &[(&str, WorldJob)] = &[
    ("chop", WorldJob::Fell),
    ("cut down", WorldJob::Fell),
    ("cut", WorldJob::Fell),
    ("quarry", WorldJob::Mine),
    ("break", WorldJob::Mine),
    ("shovel", WorldJob::Dig),
    ("scythe", WorldJob::Mow),
    ("cut grass", WorldJob::Mow),
    ("take apart", WorldJob::Dismantle),
    ("disassemble", WorldJob::Dismantle),
    ("demolish", WorldJob::Dismantle),
    ("tear down", WorldJob::Dismantle),
    ("torch", WorldJob::Burn),
    ("set fire", WorldJob::Burn),
    ("light", WorldJob::Burn),
    ("arson", WorldJob::Burn),
];

/// What a `work_on` target names, among the things within reach: a tree or rock (the nearest), the
/// ground (underfoot, or the nearest that will take a spade), a building by its place's name, or a prop by
/// its name and place. `None` when nothing within reach fits.
pub fn resolve_target(state: &SimState, c: &Character, job: WorldJob, value: Option<&Value>) -> Option<String> {
    let text = norm(value);
    match job {
        WorldJob::Fell | WorldJob::Mine => world::nearest_target(state, c.pos, job),
        WorldJob::Dig | WorldJob::Mow => world::ground_near(state, c.pos, job),
        WorldJob::Dismantle | WorldJob::Burn => {
            let targets = world::nearby_targets(state, c.pos, 24);
            let fits = |t: &world::WorkTarget| t.jobs.contains(&job);
            // The exact label first, then a prop word plus a place, then a place name alone (its building).
            if let Some(t) = targets.iter().find(|t| fits(t) && norm_str(&t.label) == text) {
                return Some(t.id.clone());
            }
            let words: Vec<&str> = text.split(' ').filter(|w| w.len() > 2).collect();
            let mut best: Option<(usize, &world::WorkTarget)> = None;
            for t in targets.iter().filter(|t| fits(t)) {
                let label = norm_str(&t.label);
                let hits = words.iter().filter(|w| label.contains(*w)).count();
                if hits > 0 && best.is_none_or(|(b, _)| hits > b) {
                    best = Some((hits, t));
                }
            }
            if let Some((_, t)) = best {
                return Some(t.id.clone());
            }
            let place = resolve_place(c, value)?;
            let id = format!("b:{}", place.id);
            targets.iter().any(|t| fits(t) && t.id == id).then_some(id)
        }
        WorldJob::Build => {
            let targets = world::nearby_targets(state, c.pos, 24);
            let builds: Vec<&world::WorkTarget> =
                targets.iter().filter(|t| t.jobs.contains(&WorldJob::Build)).collect();
            let words: Vec<&str> = text.split(' ').filter(|w| w.len() > 2).collect();
            builds
                .iter()
                .find(|t| words.iter().any(|w| norm_str(&t.label).contains(w)))
                .or(builds.first())
                .map(|t| t.id.clone())
        }
        WorldJob::Deliver => None,
    }
}

/// The world work a chosen activity stands for, when the world offers it: felling the nearest standing tree
/// for `chop_wood`, mining the nearest whole rock for `mine`. `None` leaves the slot activity as it is.
fn real_work(state: &SimState, c: &Character, activity: ActivityId) -> Option<IntentAction> {
    let job = match activity {
        ActivityId::ChopWood => WorldJob::Fell,
        ActivityId::Mine => WorldJob::Mine,
        _ => return None,
    };
    let target = gather_target(state, c.pos, job)?;
    Some(IntentAction::WorldWork { job, target, minutes: None })
}

fn plan_hour(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(text) if text.contains(':') => {
            let mut parts = text.split(':').map(|p| p.trim().parse::<f64>().ok());
            let h = parts.next().flatten().unwrap_or(0.0);
            let m = parts.next().flatten().unwrap_or(0.0);
            Some(h + m / 60.0)
        }
        Value::String(text) => {
            // parseFloat: the longest numeric prefix.
            let end = text
                .trim_start()
                .char_indices()
                .find(|&(i, ch)| !(ch.is_ascii_digit() || ch == '.' || (i == 0 && (ch == '-' || ch == '+'))))
                .map_or(text.trim_start().len(), |(i, _)| i);
            text.trim_start()[..end].parse().ok()
        }
        _ => None,
    }
}

pub fn interpret_plan(calls: &[ToolCall]) -> Option<Plan> {
    let call = calls.iter().find(|call| call.name == "set_plan")?;
    let Some(Value::Array(raw)) = call.args.get("blocks") else { return None };
    let mut blocks = Vec::new();
    for entry in raw {
        let Value::Object(item) = entry else { continue };
        let Some(activity) = resolve_activity(item.get("activity")) else { continue };
        let Some(hour) = plan_hour(item.get("hour")).filter(|h| h.is_finite() && *h >= 0.0 && *h < 24.0) else {
            continue;
        };
        let note = match item.get("note") {
            Some(Value::String(note)) => Some(utf16_truncate(note.trim(), 100).to_owned()).filter(|n| !n.is_empty()),
            _ => None,
        };
        blocks.push(PlanBlock { hour, activity, place_id: None, note });
    }
    (blocks.len() >= 2).then(|| Plan { blocks, thought: thought(call.args.get("thought")) })
}

pub fn feeling(value: Option<&Value>) -> Option<Feeling> {
    match norm(value).replace(' ', "_").as_str() {
        "much_worse" => Some(Feeling::MuchWorse),
        "worse" => Some(Feeling::Worse),
        "same" => Some(Feeling::Same),
        "better" => Some(Feeling::Better),
        "much_better" => Some(Feeling::MuchBetter),
        _ => None,
    }
}

pub struct Script {
    pub lines: Vec<ConversationLine>,
    pub topic: String,
    pub feelings: Map<Feeling>,
}

/// Lines of a whole scripted conversation; speakers are matched by name and must be the two participants.
pub fn interpret_conversation(
    calls: &[ToolCall],
    a: &Character,
    b: &Character,
    starts_with_initiator: bool,
) -> Option<Script> {
    let call = calls.iter().find(|call| call.name == "conversation")?;
    let Some(Value::Array(raw)) = call.args.get("lines") else { return None };
    let (name_a, name_b) = (first_name(a).to_lowercase(), first_name(b).to_lowercase());
    // Normally the opening line is already spoken, so the other person answers first.
    let mut expected = if starts_with_initiator { &a.id } else { &b.id };
    let mut lines = Vec::new();
    for entry in raw {
        let Value::Object(item) = entry else { continue };
        let Some(text) = spoken(item.get("text")) else { continue };
        let speaker = norm(item.get("speaker"));
        let speaker = speaker.split(' ').next().unwrap_or_default();
        let speaker_id = if speaker == name_a {
            &a.id
        } else if speaker == name_b {
            &b.id
        } else {
            expected
        };
        lines.push(ConversationLine { speaker_id: speaker_id.clone(), text });
        expected = if speaker_id == &a.id { &b.id } else { &a.id };
    }
    if lines.is_empty() {
        return None;
    }
    let mut feelings = Map::new();
    if let Some(f) = feeling(call.args.get("initiator_feels")) {
        feelings.insert(a.id.clone(), f);
    }
    if let Some(f) = feeling(call.args.get("other_feels")) {
        feelings.insert(b.id.clone(), f);
    }
    let topic = match call.args.get("topic") {
        Some(Value::String(topic)) => utf16_truncate(topic.trim(), 80).to_owned(),
        _ => String::new(),
    };
    lines.truncate(6);
    Some(Script { lines, topic, feelings })
}

/// The `relationships` of a reflection: (person id, opinion) for the people the model named.
pub fn reflection_relationships(state: &SimState, args: &JsonMap<String, Value>, self_id: &str) -> Vec<(Id, String)> {
    let Some(Value::Array(entries)) = args.get("relationships") else { return Vec::new() };
    entries
        .iter()
        .filter_map(|entry| {
            let item = entry.as_object()?;
            let person = resolve_person(state, item.get("person"), Some(self_id))?;
            let opinion = item.get("opinion")?.as_str()?;
            Some((person.id.clone(), opinion.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsw_testing::create_test_state;
    use serde_json::json;

    fn call(name: &str, args: Value) -> ToolCall {
        ToolCall::new(name, args)
    }

    #[test]
    fn resolves_people_places_and_activities_loosely() {
        let state = create_test_state(1);
        let me = &state.characters["c1"];
        assert_eq!(resolve_person(&state, Some(&json!("Brenna")), None).unwrap().profile.name, "Brenna Ashdown");
        assert_eq!(resolve_person(&state, Some(&json!("brena")), None).unwrap().profile.name, "Brenna Ashdown");
        assert!(resolve_person(&state, Some(&json!("Zed")), None).is_none());
        assert_eq!(resolve_place(me, Some(&json!("the Tankard"))).unwrap().id, "tavern");
        assert_eq!(resolve_place(me, Some(&json!("home"))).unwrap().id, me.home_id);
        assert_eq!(resolve_place(me, Some(&json!("Training Yard"))).unwrap().id, "training_yard");
        assert_eq!(resolve_activity(Some(&json!("breakfast"))), Some(ActivityId::Eat));
        assert_eq!(resolve_activity(Some(&json!("chop wood"))), Some(ActivityId::ChopWood));
        assert_eq!(resolve_activity(Some(&json!("dance"))), None);
        // Only the routine sends people to the billboards.
        assert_eq!(resolve_activity(Some(&json!("read_board"))), None);
        assert_eq!(resolve_activity(Some(&json!("read the billboard"))), None);
        assert_eq!(resolve_activity(Some(&json!("rebuild"))), None);
        // Rebuilding is only there to choose while a house lies in ruins.
        let mut state = create_test_state(1);
        assert_eq!(resolve_doable(&state, Some(&json!("rebuild"))), None);
        assert_eq!(resolve_doable(&state, Some(&json!("farm"))), Some(ActivityId::Farm));
        let ruin = fsw_sim::state::House {
            phase: fsw_sim::state::HousePhase::Ruined,
            fire: 0.0,
            built: 0.0,
            since_tick: 0,
            helpers: Vec::new(),
        };
        state.houses.insert("home_1".into(), ruin);
        assert_eq!(resolve_doable(&state, Some(&json!("rebuild"))), Some(ActivityId::Rebuild));
    }

    #[test]
    fn chopping_wood_means_a_real_tree_when_one_stands_within_reach() {
        let state = create_test_state(1);
        let me = &state.characters["c3"];
        let decision = interpret_decision(&state, me, &[call("do_activity", json!({"activity": "chop wood"}))]);
        match &decision.intents[0].action {
            IntentAction::WorldWork { job: WorldJob::Fell, target, .. } => {
                assert!(target.starts_with("s:"), "{target}");
                assert!(fsw_sim::systems::world::job_accepts(&state, WorldJob::Fell, target));
            }
            other => panic!("expected felling, got {other:?}"),
        }
        let decision = interpret_decision(&state, me, &[call("do_activity", json!({"activity": "farm"}))]);
        assert!(matches!(decision.intents[0].action, IntentAction::Activity { activity: ActivityId::Farm, .. }));
    }

    #[test]
    fn work_on_names_the_things_within_reach() {
        let state = create_test_state(1);
        let me = &state.characters["c1"];
        // c1's home is the red cottage; its bench and barrel are within reach, as is the house itself.
        let home = me.home_id.clone();
        let fell = interpret_decision(&state, me, &[call("work_on", json!({"job": "chop", "target": "that tree"}))]);
        assert!(
            matches!(&fell.intents[0].action, IntentAction::WorldWork { job: WorldJob::Fell, target, .. } if target.starts_with("s:"))
        );
        let burn = interpret_decision(&state, me, &[call("work_on", json!({"job": "burn", "target": "home"}))]);
        match &burn.intents[0].action {
            IntentAction::WorldWork { job: WorldJob::Burn, target, .. } => assert_eq!(*target, format!("b:{home}")),
            other => panic!("{other:?}"),
        }
        let apart =
            interpret_decision(&state, me, &[call("work_on", json!({"job": "dismantle", "target": "the barrel"}))]);
        assert!(
            matches!(&apart.intents[0].action, IntentAction::WorldWork { job: WorldJob::Dismantle, target, .. } if target.starts_with(&format!("p:{home}:")))
        );
        let nothing = interpret_decision(&state, me, &[call("work_on", json!({"job": "burn", "target": "the moon"}))]);
        assert!(nothing.intents.is_empty() && !nothing.problems.is_empty());
        let carry =
            interpret_decision(&state, me, &[call("work_on", json!({"job": "deliver", "target": "lumber camp"}))]);
        assert!(carry.intents.is_empty());
    }

    #[test]
    fn tidies_spoken_lines() {
        assert_eq!(spoken(Some(&json!("  \"Hello *waves* there (smiling)\" "))).as_deref(), Some("Hello there"));
        assert_eq!(spoken(Some(&json!(""))), None);
        let long = "Word. ".repeat(60);
        let tidy = spoken(Some(&json!(long))).unwrap();
        assert!(tidy.len() <= 221 && tidy.ends_with('.'));
    }

    #[test]
    fn interprets_decisions_plans_and_conversations() {
        let state = create_test_state(1);
        let me = &state.characters["c1"];
        let decision = interpret_decision(
            &state,
            me,
            &[
                call("go_to", json!({"place": "market", "thought": "I need cloth."})),
                call("talk_to", json!({"person": "Brenna", "message": "Morning.", "tone": "friendly"})),
                call("brawl", json!({"person": "Brenna"})),
                call("dance", json!({})),
            ],
        );
        assert_eq!(decision.intents.len(), 2);
        assert_eq!(decision.thought.as_deref(), Some("I need cloth."));
        assert_eq!(decision.problems.len(), 2);

        let plan = interpret_plan(&[call(
            "set_plan",
            json!({"blocks": [{"hour": "7:30", "activity": "eat"}, {"hour": 9, "activity": "train"}, {"hour": 30, "activity": "sleep"}]}),
        )])
        .unwrap();
        assert_eq!(plan.blocks.len(), 2);
        assert_eq!(plan.blocks[0].hour, 7.5);

        let (a, b) = (&state.characters["c1"], &state.characters["c2"]);
        let script = interpret_conversation(
            &[call(
                "conversation",
                json!({"lines": [{"speaker": "Brenna", "text": "Captain."}, {"speaker": "?", "text": "Brenna."}], "topic": "duty", "initiator_feels": "better", "other_feels": "same"}),
            )],
            a,
            b,
            false,
        )
        .unwrap();
        assert_eq!(script.lines[0].speaker_id, "c2");
        assert_eq!(script.lines[1].speaker_id, "c1");
        assert_eq!(script.feelings["c1"], Feeling::Better);
    }
}
