//! Port of `src/brain/brain.test.ts`: the orchestrator against a scripted model.

use std::sync::Arc;
use std::time::Duration;

use fsw_brain::brain::{Brain, BrainStatus, LogRecord, LogStatus, RequestKind, WorldLink};
use fsw_brain::llm::embed::Embedder;
use fsw_brain::llm::transport::{FakeTransport, HttpResponse};
use fsw_brain::recall::MemoryIndex;
use fsw_brain::settings::{ApiKey, ConversationMode, EmbedSettings, LlmSettings, WireProtocol};
use fsw_core::Vec2;
use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::state::{
    Intent, IntentAction, IntentSource, Memory, MemoryKind, SimState, Stimulus, StimulusKind, TalkTone,
};
use fsw_sim::util::{first_name, living};
use fsw_sim::{Command, Sim, SimEvent};
use fsw_testing::create_test_state;
use parking_lot::Mutex;
use serde_json::{Value, json};

struct Link {
    sim: Mutex<Sim>,
    logs: Mutex<Vec<LogRecord>>,
}

impl WorldLink for Link {
    fn with_state(&self, f: &mut dyn FnMut(&SimState)) {
        f(&self.sim.lock().state);
    }
    fn send(&self, command: Command) {
        self.sim.lock().enqueue(command);
    }
    fn speed(&self) -> u8 {
        1
    }
    fn log(&self, record: LogRecord) {
        self.logs.lock().push(record);
    }
    fn status(&self, _: BrainStatus) {}
}

type Responder = Arc<dyn Fn(&[String], &Value) -> Value + Send + Sync>;

fn settings(conversations: ConversationMode) -> LlmSettings {
    LlmSettings {
        enabled: true,
        base_url: "http://llm.test/v1".into(),
        model: "mock".into(),
        concurrency: 4,
        conversations,
        protocol: WireProtocol::Tools,
        ..LlmSettings::default()
    }
}

struct Harness {
    link: Arc<Link>,
    brain: Brain,
    events: Vec<SimEvent>,
    transport: Arc<FakeTransport>,
}

/// A tool call reply, or plain text when `calls` is a string.
fn reply(calls: Value) -> Value {
    let message = match calls {
        Value::String(text) => json!({ "content": text }),
        Value::Array(calls) => json!({
            "content": null,
            "tool_calls": calls.iter().map(|c| json!({
                "type": "function",
                "function": { "name": c["name"], "arguments": c["args"].to_string() },
            })).collect::<Vec<_>>(),
        }),
        _ => json!({ "content": "" }),
    };
    json!({ "choices": [{ "message": message }], "usage": { "prompt_tokens": 100, "completion_tokens": 20 } })
}

fn harness(responder: Responder, conversations: ConversationMode) -> Harness {
    let mut state = create_test_state(30);
    // Nobody falls into talk on their own: the tests start the conversations they need, and who
    // happens to be chatting at dawn otherwise depends on the dice.
    for c in state.characters.values_mut() {
        c.next_social_tick = TICKS_PER_DAY;
    }
    let link = Arc::new(Link { sim: Mutex::new(Sim::new(state)), logs: Mutex::new(Vec::new()) });
    let transport = FakeTransport::new(move |request| {
        let body = request.json();
        let tools: Vec<String> = body["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["function"]["name"].as_str().map(str::to_owned))
            .collect();
        // Morning plans are requested for everyone; answer them the same way in every test.
        let calls = if tools.iter().any(|t| t == "set_plan") {
            json!([{ "name": "set_plan", "args": { "blocks": [{"hour": 7, "activity": "eat"}, {"hour": 8, "activity": "stroll"}, {"hour": 22, "activity": "sleep"}] } }])
        } else {
            responder(&tools, &body)
        };
        let response = HttpResponse::json(200, &reply(calls));
        async move { Ok(response) }
    });
    let brain = Brain::new(link.clone(), transport.clone());
    let harness = Harness { link, brain, events: Vec::new(), transport };
    harness.brain.configure(settings(conversations), ApiKey::default(), None, None);
    harness.brain.set_initial_budget_for_tests(1000.0);
    harness
}

impl Harness {
    fn step(&mut self, ticks: usize) {
        for _ in 0..ticks {
            let events = self.link.sim.lock().step();
            self.brain.handle(&events);
            self.events.extend(events);
        }
    }

    async fn wait(&self) {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
        self.brain.update(false);
    }

    /// Lets the morning's plan requests go out and come back.
    async fn settle(&mut self) {
        self.step(2);
        self.brain.update(true);
        self.wait().await;
        self.step(1);
    }

    fn who(&self, name: &str) -> String {
        let sim = self.link.sim.lock();
        living(&sim.state).find(|c| first_name(c) == name).unwrap().id.clone()
    }

    fn stimulate(&self, id: &str, text: &str) {
        let mut sim = self.link.sim.lock();
        let tick = sim.state.tick;
        sim.state.characters.get_mut(id).unwrap().stimuli.push(Stimulus {
            kind: StimulusKind::Gift,
            tick,
            text: text.into(),
            subject_id: None,
            urgency: 1,
        });
    }

    fn talk(&self, a: &str, b: &str, opener: &str, source: IntentSource) {
        let mut sim = self.link.sim.lock();
        let epoch = sim.state.characters[a].epoch;
        let intent = Intent::new(
            IntentAction::Talk { target_id: b.into(), opener: opener.into(), tone: TalkTone::Friendly },
            source,
        );
        sim.enqueue(Command::SetIntents { id: a.into(), epoch, intents: vec![intent] });
    }

    /// Steps until the conversation between two characters has ended; returns what they said to each other.
    async fn conversation(&mut self, a: &str, b: &str) -> Vec<(String, String)> {
        let ended = |events: &[SimEvent]| {
            events.iter().any(|e| matches!(e, SimEvent::ConversationEnded { a: x, b: y, .. } if x == a && y == b))
        };
        // Only this conversation: the two may have chatted on their own earlier.
        let start = self.events.len();
        for i in 0..6000 {
            if ended(&self.events[start..]) {
                break;
            }
            self.step(1);
            if i % 10 == 0 {
                self.wait().await;
            }
        }
        assert!(ended(&self.events[start..]), "the conversation ended");
        self.events[start..]
            .iter()
            .filter_map(|e| match e {
                SimEvent::Speech { speaker_id, to_id: Some(to), text, .. }
                    if [a, b].contains(&speaker_id.as_str()) && [a, b].contains(&to.as_str()) =>
                {
                    Some((speaker_id.clone(), text.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Steps until none of these people is in, or on the way to, a conversation of the village's own making,
    /// so a test's own talk is not turned away because the dice already sent them to chat.
    fn until_free(&mut self, ids: &[&str]) {
        let busy = |sim: &Sim| {
            ids.iter().any(|id| {
                let c = &sim.state.characters[*id];
                c.convo_id.is_some() || c.intent.as_ref().is_some_and(|i| matches!(i.action, IntentAction::Talk { .. }))
            })
        };
        for _ in 0..4000 {
            if !busy(&self.link.sim.lock()) {
                return;
            }
            self.step(1);
        }
        panic!("{ids:?} never had a free moment");
    }

    fn log(&self, name: &str, kind: RequestKind) -> Option<LogRecord> {
        self.link.logs.lock().iter().find(|r| r.character == name && r.kind == kind).cloned()
    }
}

fn continue_otherwise(f: impl Fn(&[String], &Value) -> Option<Value> + Send + Sync + 'static) -> Responder {
    Arc::new(move |tools, body| f(tools, body).unwrap_or_else(|| json!([{ "name": "continue", "args": {} }])))
}

#[tokio::test]
async fn turns_a_decide_reply_into_intents_for_the_character() {
    let mut h = harness(
        continue_otherwise(|tools, _| {
            tools.iter().any(|t| t == "go_to").then(|| {
                json!([
                    { "name": "go_to", "args": { "place": "the chapel", "thought": "I need quiet." } },
                    { "name": "do_activity", "args": { "activity": "pray" } },
                ])
            })
        }),
        ConversationMode::Batched,
    );
    h.settle().await;
    let id = h.who("Aldric");
    h.stimulate(&id, "Someone left flowers at your door.");
    h.brain.update(true);
    h.wait().await;
    h.step(1);
    let sim = h.link.sim.lock();
    let c = &sim.state.characters[&id];
    assert!(matches!(&c.intent.as_ref().unwrap().action, IntentAction::GoTo { place_id } if place_id == "chapel"));
    assert_eq!(c.intent.as_ref().unwrap().source, IntentSource::Llm);
    assert!(matches!(c.queue[0].action, IntentAction::Activity { .. }));
    assert_eq!(c.thought, "I need quiet.");
    assert!(!c.stimuli.iter().any(|s| s.text.contains("flowers")));
    assert_eq!(c.plan_source, fsw_sim::state::PlanSource::Llm);
    drop(sim);
    let log = h.log("Aldric", RequestKind::Decide).unwrap();
    assert_eq!(log.status, LogStatus::Ok);
    assert_eq!(log.commands, vec!["go to chapel → pray".to_owned()]);
    assert!(h.transport.bodies().iter().any(|b| b.to_string().contains("Someone left flowers at your door.")));
}

#[tokio::test]
async fn drops_an_answer_when_the_situation_changed_while_the_model_was_thinking() {
    let world: Arc<std::sync::OnceLock<(Arc<Link>, String)>> = Arc::new(std::sync::OnceLock::new());
    let seen = world.clone();
    let mut h = harness(
        Arc::new(move |tools, _| {
            if tools.iter().any(|t| t == "go_to")
                && let Some((link, id)) = seen.get()
            {
                // Something happens to them while the model is thinking.
                link.sim.lock().state.characters.get_mut(id).unwrap().epoch += 1;
            }
            json!([{ "name": "go_to", "args": { "place": "market" } }])
        }),
        ConversationMode::Batched,
    );
    h.settle().await;
    let id = h.who("Aldric");
    world.set((h.link.clone(), id.clone())).ok().unwrap();
    h.stimulate(&id, "x");
    h.brain.update(true);
    h.wait().await;
    h.step(1);
    let source = h.link.sim.lock().state.characters[&id].intent.as_ref().map(|i| i.source);
    assert_ne!(source, Some(IntentSource::Llm));
    assert_eq!(h.log("Aldric", RequestKind::Decide).unwrap().status, LogStatus::Stale);
}

#[tokio::test]
async fn repairs_sloppy_arguments_and_reports_what_it_had_to_drop() {
    let mut h = harness(
        Arc::new(|_, _| {
            json!([
                { "name": "talk_to", "args": { "person": "garrik", "message": "\"Fancy a drink?\"", "tone": "cheerful" } },
                { "name": "go_to", "args": { "place": "the moon" } },
            ])
        }),
        ConversationMode::Batched,
    );
    h.settle().await;
    let garrick = h.who("Garrick");
    // Anyone but Garrick who is free and whose mind has not failed a request already (and so is not waiting
    // out a retry); Dorn in the TypeScript test.
    let mut asker = None;
    for _ in 0..40 {
        let failed: Vec<String> =
            h.link.logs.lock().iter().filter(|r| r.status != LogStatus::Ok).map(|r| r.character.clone()).collect();
        let sim = h.link.sim.lock();
        asker = living(&sim.state)
            .find(|c| {
                c.id != garrick
                    && c.convo_id.is_none()
                    && !c.intent.as_ref().is_some_and(|i| matches!(i.action, IntentAction::Talk { .. }))
                    && !failed.iter().any(|name| name == first_name(c))
            })
            .map(|c| (c.id.clone(), first_name(c).to_owned()));
        drop(sim);
        if asker.is_some() {
            break;
        }
        h.step(20);
    }
    let (id, name) = asker.expect("someone free to ask");
    h.stimulate(&id, "You feel lonely.");
    h.brain.update(true);
    h.wait().await;
    h.step(1);
    let sim = h.link.sim.lock();
    let intent = sim.state.characters[&id].intent.clone().unwrap();
    assert!(matches!(&intent.action, IntentAction::Talk { target_id, opener, tone: TalkTone::Friendly }
        if *target_id == garrick && opener == "Fancy a drink?"));
    drop(sim);
    assert!(h.log(&name, RequestKind::Decide).unwrap().summary.contains("unknown place"));
}

#[tokio::test]
async fn writes_a_whole_conversation_in_one_call_and_applies_the_feelings() {
    let mut h = harness(
        continue_otherwise(|tools, _| {
            tools.iter().any(|t| t == "conversation").then(|| {
                json!([{ "name": "conversation", "args": {
                    "lines": [
                        { "speaker": "Dorn", "text": "You look like a man with a plan, and I do not like it." },
                        { "speaker": "Garrick", "text": "Only a plan for ale." },
                    ],
                    "topic": "ale and bad ideas", "initiator_feels": "much_better", "other_feels": "better",
                } }])
            })
        }),
        ConversationMode::Batched,
    );
    h.step(2);
    let (garrick, dorn) = (h.who("Garrick"), h.who("Dorn"));
    h.until_free(&[&garrick, &dorn]);
    let before = h.link.sim.lock().state.characters[&garrick].relationships[&dorn].affinity;
    h.talk(&garrick, &dorn, "Dorn! A word.", IntentSource::Llm);
    let lines: Vec<String> = h.conversation(&garrick, &dorn).await.into_iter().map(|(_, text)| text).collect();
    assert_eq!(
        lines,
        ["Dorn! A word.", "You look like a man with a plan, and I do not like it.", "Only a plan for ale."]
    );
    let sim = h.link.sim.lock();
    let g = &sim.state.characters[&garrick];
    assert!(g.relationships[&dorn].affinity > before);
    assert!(g.memories.iter().any(|m| m.text.contains("ale and bad ideas")));
    drop(sim);
    assert!(h.link.logs.lock().iter().any(|r| r.kind == RequestKind::Converse && r.status == LogStatus::Ok));
}

#[tokio::test]
async fn writes_the_opening_line_too_when_a_villager_starts_a_chat_on_their_own() {
    let mut h = harness(
        continue_otherwise(|tools, body| {
            tools.iter().any(|t| t == "conversation").then(|| {
                assert!(body["messages"][1]["content"].as_str().unwrap().contains("Write the whole conversation"));
                json!([{ "name": "conversation", "args": {
                    "lines": [
                        { "speaker": "Rowan", "text": "The comfrey is early this year." },
                        { "speaker": "Isolde", "text": "Is that a good sign?" },
                        { "speaker": "Rowan", "text": "It is a sign. Good is up to us." },
                    ],
                    "topic": "early comfrey", "initiator_feels": "better", "other_feels": "better",
                } }])
            })
        }),
        ConversationMode::Batched,
    );
    h.settle().await;
    let (rowan, isolde) = (h.who("Rowan"), h.who("Isolde"));
    // An empty opener is what the routine brain produces.
    h.talk(&rowan, &isolde, "", IntentSource::Routine);
    let lines = h.conversation(&rowan, &isolde).await;
    let texts: Vec<&str> = lines.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(texts, ["The comfrey is early this year.", "Is that a good sign?", "It is a sign. Good is up to us."]);
    let speakers: Vec<&str> = lines.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(speakers, [rowan.as_str(), isolde.as_str(), rowan.as_str()]);
}

#[tokio::test]
async fn falls_back_to_local_lines_when_the_model_answers_with_nonsense() {
    let mut h = harness(Arc::new(|_, _| json!("I would rather write a poem.")), ConversationMode::Batched);
    h.step(2);
    let (a, b) = {
        let sim = h.link.sim.lock();
        (sim.state.order[0].clone(), sim.state.order[1].clone())
    };
    h.talk(&a, &b, "Morning.", IntentSource::Llm);
    let lines = h.conversation(&a, &b).await;
    assert!(lines.len() >= 2);
    assert_eq!(lines[0].1, "Morning.");
    assert!(h.link.logs.lock().iter().any(|r| r.status == LogStatus::Error));
}

#[tokio::test]
async fn takes_turns_when_asked_to() {
    // Each speaker ends the conversation once three lines have been said after the opener.
    let mut h = harness(
        continue_otherwise(|tools, body| {
            tools.iter().any(|t| t == "reply").then(|| {
                let content = body["messages"][1]["content"].as_str().unwrap();
                let so_far = content.split("The conversation so far:").nth(1).unwrap_or("");
                let said = so_far
                    .lines()
                    .filter(|line| line.split_once(": ").is_some_and(|(who, _)| !who.is_empty() && !who.contains(' ')))
                    .count();
                json!([{ "name": "reply", "args": { "text": format!("Line {said}."), "feeling": "better", "end": said >= 3 } }])
            })
        }),
        ConversationMode::Turns,
    );
    h.step(2);
    let (a, b) = {
        let sim = h.link.sim.lock();
        (sim.state.order[0].clone(), sim.state.order[1].clone())
    };
    h.talk(&a, &b, "Hello.", IntentSource::Llm);
    let lines = h.conversation(&a, &b).await;
    let speakers: Vec<&str> = lines.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(speakers, [a.as_str(), b.as_str(), a.as_str(), b.as_str()]);
    assert_eq!(lines[0].1, "Hello.");
    assert!(lines[1..].iter().all(|(_, t)| t.starts_with("Line ") && t.ends_with('.')));
}

#[tokio::test]
async fn does_nothing_and_keeps_the_world_local_when_disabled() {
    let mut h = harness(Arc::new(|_, _| json!([])), ConversationMode::Batched);
    h.brain.configure(
        LlmSettings { enabled: false, ..settings(ConversationMode::Batched) },
        ApiKey::default(),
        None,
        None,
    );
    h.step(3);
    assert!(!h.brain.enabled());
    assert_eq!(h.link.sim.lock().state.tuning.script_wait_ticks, 0);
}

#[tokio::test]
async fn a_plan_written_before_a_restart_is_not_asked_for_again() {
    let mut h = harness(continue_otherwise(|_, _| None), ConversationMode::Batched);
    h.settle().await;
    let all_planned = |h: &Harness| {
        let sim = h.link.sim.lock();
        living(&sim.state).all(|c| c.plan_source == fsw_sim::state::PlanSource::Llm)
    };
    // Those busy talking are planned once their conversation is over.
    for _ in 0..300 {
        if all_planned(&h) {
            break;
        }
        h.brain.update(true);
        h.wait().await;
        h.step(20);
    }
    assert!(all_planned(&h));
    let planned = h.transport.bodies().iter().filter(|b| b.to_string().contains("set_plan")).count();
    // A new brain (a restart) over the same world asks for no plans: they were written today.
    let again = Brain::new(h.link.clone(), h.transport.clone());
    again.configure(settings(ConversationMode::Batched), ApiKey::default(), None, None);
    again.set_initial_budget_for_tests(1000.0);
    again.update(true);
    h.wait().await;
    let after = h.transport.bodies().iter().filter(|b| b.to_string().contains("set_plan")).count();
    assert_eq!(after, planned);
}

impl Harness {
    /// Sends someone to the green's billboard and steps until they have said something there.
    async fn read_the_green_board(&mut self, name: &str) -> (String, Vec<String>) {
        let id = self.who(name);
        {
            let mut sim = self.link.sim.lock();
            let epoch = sim.state.characters[&id].epoch;
            let read = Intent::new(
                IntentAction::Activity {
                    activity: fsw_sim::state::ActivityId::ReadBoard,
                    place_id: Some("green".into()),
                    minutes: None,
                },
                IntentSource::Llm,
            );
            sim.enqueue(Command::SetIntents { id: id.clone(), epoch, intents: vec![read] });
        }
        let said = |events: &[SimEvent]| -> Vec<String> {
            events
                .iter()
                .filter_map(|e| match e {
                    SimEvent::Speech { speaker_id, to_id: None, text, .. } if *speaker_id == id => Some(text.clone()),
                    _ => None,
                })
                .collect()
        };
        for i in 0..6000 {
            if !said(&self.events).is_empty() {
                break;
            }
            self.step(1);
            if i % 10 == 0 {
                self.wait().await;
            }
        }
        assert!(self.events.iter().any(|e| matches!(e, SimEvent::BillboardRead { id: who, .. } if *who == id)));
        let lines = said(&self.events);
        (id, lines)
    }
}

#[tokio::test]
async fn asks_the_model_how_someone_takes_a_billboard() {
    let mut h = harness(
        continue_otherwise(|tools, _| {
            tools.iter().any(|t| t == "react_to_billboard").then(|| {
                json!([{ "name": "react_to_billboard", "args": { "reaction": "annoyed", "say": "\"Who pays for this rubbish?\"" } }])
            })
        }),
        ConversationMode::Batched,
    );
    h.settle().await;
    let (id, lines) = h.read_the_green_board("Rowan").await;
    assert_eq!(lines, vec!["Who pays for this rubbish?".to_owned()]);
    let log = h.log("Rowan", RequestKind::Billboard).expect("a billboard request");
    assert_eq!(log.status, LogStatus::Ok);
    let sim = h.link.sim.lock();
    let green = fsw_sim::systems::billboards::board_index("green_board").unwrap();
    let slogan = fsw_sim::systems::billboards::shown(&sim.state, green).text;
    assert!(sim.state.characters[&id].memories.iter().any(|m| m.text.ends_with("It annoyed me.")));
    drop(sim);
    let asked = h.transport.bodies().iter().any(|b| b.to_string().contains(&slogan));
    assert!(asked, "the prompt quotes the slogan");
}

#[tokio::test]
async fn a_billboard_reaction_that_makes_no_sense_leaves_the_stock_one() {
    let mut h = harness(
        continue_otherwise(|tools, _| {
            tools
                .iter()
                .any(|t| t == "react_to_billboard")
                .then(|| json!([{ "name": "react_to_billboard", "args": { "reaction": "confused" } }]))
        }),
        ConversationMode::Batched,
    );
    h.settle().await;
    let (_, lines) = h.read_the_green_board("Rowan").await;
    assert_eq!(lines.len(), 1);
    assert_eq!(h.log("Rowan", RequestKind::Billboard).unwrap().status, LogStatus::Fallback);
}

const FLAMES: &str = "Carried buckets until the flames at the red cottage died down.";

/// Gives Aldric twenty ordinary memories from yesterday and one old one about a fire.
fn remember_a_fire(h: &Harness) -> (String, Vec<String>) {
    let id = h.who("Aldric");
    let mut sim = h.link.sim.lock();
    let tick = sim.state.tick;
    let memory = |days: i64, text: String, importance: i64| Memory {
        tick: tick - days * TICKS_PER_DAY,
        day: (tick - days * TICKS_PER_DAY) / TICKS_PER_DAY,
        text,
        importance,
        kind: MemoryKind::Event,
        subjects: vec![],
    };
    let ordinary: Vec<String> = (0..20).map(|i| format!("Mended fence post number {i} on the farm.")).collect();
    let c = sim.state.characters.get_mut(&id).unwrap();
    c.memories = vec![memory(3, FLAMES.into(), 2)];
    c.memories.extend(ordinary.iter().map(|text| memory(1, text.clone(), 6)));
    (id, ordinary)
}

fn memory_index(endpoint: Arc<FakeTransport>) -> Arc<MemoryIndex> {
    let settings = EmbedSettings { base_url: "http://embed.test/v1".into(), model: "e".into() };
    Arc::new(MemoryIndex::new(Embedder::new(settings, ApiKey::default(), endpoint)))
}

#[tokio::test]
async fn recalls_a_memory_by_meaning_when_embeddings_are_on() {
    let mut h = harness(continue_otherwise(|_, _| None), ConversationMode::Batched);
    h.settle().await;
    let (id, ordinary) = remember_a_fire(&h);
    // Every cue means "fire"; the fire memory does too, the fences do not.
    let endpoint = FakeTransport::always(200, json!({ "data": [{ "index": 0, "embedding": [1.0, 0.0] }] }));
    let index = memory_index(endpoint.clone());
    index.cache().insert(index.cache().key(FLAMES), Arc::from(vec![1.0f32, 0.0]));
    for text in &ordinary {
        index.cache().insert(index.cache().key(text), Arc::from(vec![0.0f32, 1.0]));
    }
    h.brain.set_memory(Some(index));
    h.stimulate(&id, "Smoke is rising from a house nearby.");
    h.brain.update(true);
    h.wait().await;
    h.step(1);
    let log = h.log("Aldric", RequestKind::Decide).unwrap();
    assert!(log.prompt.contains(FLAMES), "{}", log.prompt);
    assert!(endpoint.requests().iter().any(|r| r.url.ends_with("/embeddings")));
}

#[tokio::test]
async fn recalls_by_words_when_embeddings_are_off() {
    let mut h = harness(continue_otherwise(|_, _| None), ConversationMode::Batched);
    h.settle().await;
    let (id, _) = remember_a_fire(&h);
    h.stimulate(&id, "Smoke is rising from a house nearby.");
    h.brain.update(true);
    h.wait().await;
    h.step(1);
    let log = h.log("Aldric", RequestKind::Decide).unwrap();
    assert!(!log.prompt.contains(FLAMES));
}

#[tokio::test(start_paused = true)]
async fn a_hung_embedding_endpoint_does_not_hold_up_a_decision() {
    let mut h = harness(continue_otherwise(|_, _| None), ConversationMode::Batched);
    h.settle().await;
    let (id, _) = remember_a_fire(&h);
    let hung = FakeTransport::new(|_| std::future::pending::<Result<HttpResponse, String>>());
    h.brain.set_memory(Some(memory_index(hung)));
    h.stimulate(&id, "Smoke is rising from a house nearby.");
    h.brain.update(true);
    tokio::time::sleep(Duration::from_secs(3)).await;
    h.wait().await;
    h.step(1);
    let log = h.log("Aldric", RequestKind::Decide).unwrap();
    assert_eq!(log.status, LogStatus::Ok);
    assert!(!log.prompt.contains(FLAMES));
}

#[tokio::test(start_paused = true)]
async fn a_request_cancelled_while_its_cue_waits_is_never_built_or_sent() {
    const SMOKE: &str = "Smoke is rising from a house nearby.";
    let mut h = harness(continue_otherwise(|_, _| None), ConversationMode::Batched);
    h.settle().await;
    let id = h.who("Aldric");
    let hung = FakeTransport::new(|_| std::future::pending::<Result<HttpResponse, String>>());
    h.brain.set_memory(Some(memory_index(hung.clone())));
    h.stimulate(&id, SMOKE);
    h.brain.update(true);
    h.wait().await;
    assert_eq!(hung.requests().len(), 1, "the decision waits for its cue");

    // A death cancels the villager's requests; this one is still waiting for its cue.
    h.brain.handle(&[SimEvent::Death { id: id.clone(), name: "Aldric".into(), pos: Vec2::default(), monster: false }]);
    h.wait().await;
    h.step(1);
    let stimuli = h.link.sim.lock().state.characters[&id].stimuli.clone();
    assert!(stimuli.iter().any(|s| s.text == SMOKE), "the build's commands were not sent");
    assert!(!h.transport.bodies().iter().any(|body| body.to_string().contains(SMOKE)), "no prompt was sent");
}
