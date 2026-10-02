//! End to end over real HTTP against `tools/mock-llm.mjs`. Ignored by default:
//!
//! ```text
//! node tools/mock-llm.mjs &
//! cargo test -p fsw-brain --test mock_llm -- --ignored
//! ```
//!
//! `MOCK_LLM_URL` points it elsewhere (default http://localhost:8787/v1); set `MOCK_MODE=text`
//! when the mock runs in text mode to also try the JSON protocol.

use std::sync::Arc;

use fsw_brain::llm::probe;
use fsw_brain::llm::{ChatMessage, LlmClient, LlmRequest, Protocol, ReqwestTransport, ToolDef};
use fsw_brain::settings::{ApiKey, LlmSettings, WireProtocol};
use serde_json::json;

fn settings() -> LlmSettings {
    let base_url = std::env::var("MOCK_LLM_URL").unwrap_or_else(|_| "http://localhost:8787/v1".into());
    LlmSettings { enabled: true, base_url, model: "mock-villager".into(), ..LlmSettings::default() }
}

#[tokio::test]
#[ignore = "needs node tools/mock-llm.mjs"]
async fn talks_to_the_mock_endpoint() {
    let transport = Arc::new(ReqwestTransport::new().unwrap());
    let client = LlmClient::new(settings(), ApiKey::new("not-needed"), transport.clone());

    assert_eq!(client.list_models().await.unwrap(), vec!["mock-villager"]);

    let place = json!({ "type": "object", "properties": { "place": { "type": "string" } }, "required": ["place"] });
    let activity = json!({ "type": "object", "properties": { "activity": { "type": "string" } } });
    let request = LlmRequest {
        messages: vec![
            ChatMessage::system("You are a villager."),
            ChatMessage::user("Places: tavern, market.\nActivities: rest (sit down), eat (have a meal)\n\nDecide."),
        ],
        tools: vec![
            ToolDef::new("go_to", "Walk to a place.", place),
            ToolDef::new("do_activity", "Do something here.", activity),
            ToolDef::new("continue", "Carry on.", json!({ "type": "object", "properties": {} })),
        ],
        max_calls: 2,
        ..LlmRequest::default()
    };
    for _ in 0..5 {
        let response = client.complete(&request).await.unwrap();
        assert!(!response.calls.is_empty(), "{}", response.raw);
        assert!(response.prompt_tokens > 0);
        for call in &response.calls {
            assert!(["go_to", "do_activity", "continue"].contains(&call.name.as_str()), "{call:?}");
        }
        println!("{:?} {:?} in {:?}", response.protocol, response.calls, response.latency);
    }

    // The JSON protocol sends no tools, so the mock answers `continue`. Only its text mode writes
    // that as JSON text; otherwise it sends a native call, which the JSON protocol does not read.
    if std::env::var("MOCK_MODE").is_ok_and(|mode| mode == "text") {
        let json_settings = LlmSettings { protocol: WireProtocol::Json, ..settings() };
        let json_client = LlmClient::new(json_settings, ApiKey::default(), transport.clone());
        let response = json_client.complete(&request).await.unwrap();
        assert_eq!(response.protocol, Protocol::Json);
        assert_eq!(response.calls[0].name, "continue", "{}", response.raw);
    }

    let result = probe::probe(&settings(), ApiKey::default(), transport).await;
    assert!(result.ok, "{}", result.message);
    println!("{}", result.message);

    let report = probe::run(&settings(), ApiKey::default(), 3).await.unwrap();
    println!("{report}");
    assert_eq!(report.usable, 3);
}
