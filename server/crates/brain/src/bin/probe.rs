//! Measures how well a model plays a villager: latency and how often its tool
//! calls are usable, over real prompts taken from a running simulation (port of
//! `tools/llm-probe.ts`).
//!
//!   cargo run --release -p fsw-brain --bin probe -- <base-url> <model> [runs] [api-key]
//!   cargo run --release -p fsw-brain --bin probe -- <base-url> <model> [runs] [api-key] --embed <embed-model>
//!   cargo run --release -p fsw-brain --bin probe -- http://localhost:11434/v1 qwen3:8b 20
//!   LLM_API_KEY=sk-... cargo run --release -p fsw-brain --bin probe -- https://api.openai.com/v1 <model> 20
//!
//! Options (environment): LLM_PROTOCOL=auto|tools|json, LLM_HEADERS="Name: value\nOther: value"
//! Options (environment): EMBED_BASE_URL (defaults to <base-url>), EMBED_API_KEY (defaults to the model's key)

use std::process::ExitCode;
use std::sync::Arc;

use fsw_brain::llm::client::LlmClient;
use fsw_brain::llm::embed::Embedder;
use fsw_brain::llm::probe::{ProbeCase, run_cases};
use fsw_brain::llm::transport::{LlmTransport, ReqwestTransport};
use fsw_brain::llm::types::LlmRequest;
use fsw_brain::prompts::{Prompt, conversation_prompt, decide_prompt, plan_prompt, react_prompt, reflect_prompt};
use fsw_brain::recall::cosine;
use fsw_brain::settings::{ApiKey, EmbedSettings, LlmSettings, WireProtocol};
use fsw_brain::validate::{interpret_conversation, interpret_decision, interpret_plan};
use fsw_sim::state::{
    Conversation, ConversationLine, ConvoAuthor, ConvoPhase, Fallback, Stimulus, StimulusKind, TalkTone,
};
use fsw_sim::util::living_ids;
use fsw_sim::{Sim, SimState, create_state};

/// A world a few days in, mid-afternoon, so prompts carry memories and relationships.
const WARM_UP_TICKS: usize = 28_800 * 2 + 9_600;
const MAX_TOKENS: usize = 1200;

fn request(prompt: Prompt) -> LlmRequest {
    LlmRequest { messages: prompt.messages, tools: prompt.tools, max_calls: prompt.max_calls, ..LlmRequest::default() }
}

fn build_case(state: &mut SimState, people: &[String], index: usize) -> ProbeCase {
    let id = people[index % people.len()].clone();
    let other = people[(index + 3) % people.len()].clone();
    match index % 5 {
        0 | 1 => {
            let tick = state.tick;
            let c = state.characters.get_mut(&id).expect("living");
            c.stimuli = vec![Stimulus {
                kind: StimulusKind::BlockEnded,
                tick,
                text: "You finished what you were doing.".into(),
                subject_id: None,
                urgency: 0,
            }];
            let snapshot = Arc::new(state.clone());
            let prompt = decide_prompt(state, &state.characters[&id], MAX_TOKENS, None);
            ProbeCase {
                kind: "decide".into(),
                request: request(prompt),
                usable: Box::new(move |calls| {
                    let decision = interpret_decision(&snapshot, &snapshot.characters[&id], calls);
                    if !decision.intents.is_empty() || decision.keep {
                        None
                    } else if decision.problems.is_empty() {
                        Some("no usable call".into())
                    } else {
                        Some(decision.problems.join("; "))
                    }
                }),
            }
        }
        2 => ProbeCase {
            kind: "plan_day".into(),
            request: request(plan_prompt(state, &state.characters[&id], MAX_TOKENS, None)),
            usable: Box::new(|calls| interpret_plan(calls).is_none().then(|| "plan could not be read".into())),
        },
        3 => {
            let convo = Conversation {
                id: "v0".into(),
                a: id.clone(),
                b: other.clone(),
                tone: TalkTone::Friendly,
                phase: ConvoPhase::Play,
                lines: vec![ConversationLine { speaker_id: id.clone(), text: "You look troubled. What is it?".into() }],
                index: 0,
                line_end_tick: 0,
                started_tick: state.tick,
                await_until_tick: 0,
                closed: false,
                author: ConvoAuthor::Local,
                topic: String::new(),
                feelings: Default::default(),
                fallback: Fallback { lines: Vec::new(), topic: String::new(), feelings: Default::default() },
                rumour: None,
            };
            let (a, b) = (state.characters[&id].clone(), state.characters[&other].clone());
            let prompt = conversation_prompt(state, &convo, &a, &b, MAX_TOKENS);
            ProbeCase {
                kind: "converse".into(),
                request: request(prompt),
                usable: Box::new(move |calls| {
                    interpret_conversation(calls, &a, &b, false).is_none().then(|| "lines could not be read".into())
                }),
            }
        }
        _ if index.is_multiple_of(2) => ProbeCase {
            kind: "reflect".into(),
            request: request(reflect_prompt(state, &state.characters[&id], MAX_TOKENS)),
            usable: Box::new(|calls| {
                let ok = calls
                    .iter()
                    .any(|call| call.name == "reflection" && call.args.get("summary").is_some_and(|s| s.is_string()));
                (!ok).then(|| "no reflection".into())
            }),
        },
        _ => ProbeCase {
            kind: "react".into(),
            request: request(react_prompt(state, &state.characters[&id])),
            usable: Box::new(|calls| {
                let ok = calls.iter().any(|call| call.name == "fight" || call.name == "flee");
                (!ok).then(|| "neither fight nor flee".into())
            }),
        },
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let embed_model = args.iter().position(|arg| arg == "--embed").and_then(|at| {
        let model = args.get(at + 1).cloned();
        args.drain(at..(at + 2).min(args.len()));
        model
    });
    let (Some(base_url), Some(model)) = (args.first(), args.get(1)) else {
        eprintln!("usage: probe <base-url> <model> [runs] [api-key]");
        return ExitCode::FAILURE;
    };
    let runs: usize = args.get(2).and_then(|r| r.parse().ok()).unwrap_or(12);
    let key = args.get(3).cloned().or_else(|| std::env::var("LLM_API_KEY").ok()).unwrap_or_default();
    let protocol = match std::env::var("LLM_PROTOCOL").as_deref() {
        Ok("tools") => WireProtocol::Tools,
        Ok("json") => WireProtocol::Json,
        _ => WireProtocol::Auto,
    };
    let settings = LlmSettings {
        enabled: true,
        base_url: base_url.clone(),
        model: model.clone(),
        protocol,
        headers: std::env::var("LLM_HEADERS").unwrap_or_default(),
        requests_per_minute: 0,
        ..LlmSettings::default()
    };
    let transport = match ReqwestTransport::new() {
        Ok(transport) => Arc::new(transport),
        Err(error) => {
            eprintln!("no HTTP client: {error}");
            return ExitCode::FAILURE;
        }
    };

    let embeddings_ok = match &embed_model {
        Some(model) => {
            let base = std::env::var("EMBED_BASE_URL").unwrap_or_else(|_| base_url.clone());
            let embed_key = std::env::var("EMBED_API_KEY").unwrap_or_else(|_| key.clone());
            check_embeddings(&base, model, &embed_key, transport.clone()).await
        }
        None => true,
    };

    let mut sim = Sim::new(create_state(2024));
    for _ in 0..WARM_UP_TICKS {
        sim.step();
    }
    let mut state = sim.state;
    let people = living_ids(&state);
    let cases: Vec<ProbeCase> = (0..runs).map(|i| build_case(&mut state, &people, i)).collect();

    let client = LlmClient::new(settings, ApiKey::new(key), transport);
    let report = run_cases(&client, &cases).await;
    print!("{report}");
    if embeddings_ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Embeds three sentences and checks that the two about a fire are closer to each other than to bread.
async fn check_embeddings(base_url: &str, model: &str, key: &str, transport: Arc<dyn LlmTransport>) -> bool {
    let settings = EmbedSettings { base_url: base_url.trim_end_matches('/').to_owned(), model: model.to_owned() };
    let embedder = Embedder::new(settings, ApiKey::new(key), transport);
    let texts = [
        "The red cottage burned in the night.",
        "Flames rose from the roof while we carried water.",
        "I bought bread at the market.",
    ]
    .map(str::to_owned);
    let started = std::time::Instant::now();
    match embedder.embed(&texts).await {
        Ok(embedded) => {
            let near = cosine(&embedded.vectors[0], &embedded.vectors[1]);
            let far = cosine(&embedded.vectors[0], &embedded.vectors[2]);
            let verdict = if near > far { "" } else { " (this model does not tell them apart)" };
            println!(
                "embeddings: {} numbers, {} ms; fire~flames {near:.2}, fire~bread {far:.2}{verdict}",
                embedded.vectors[0].len(),
                started.elapsed().as_millis()
            );
            near > far
        }
        Err(error) => {
            println!("embeddings failed: {error}");
            false
        }
    }
}
