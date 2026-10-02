//! Checks an endpoint (port of `src/brain/llm/probe.ts`) and measures it (the
//! measuring half of `tools/llm-probe.ts`).
//!
//! [`probe`] asks for one small decision, for the admin's Test button.
//! [`run`] repeats it and reports latency and how often the answer was usable;
//! [`run_cases`] does the same for any requests, so a later probe binary can
//! feed it real prompts from a running world.

use std::fmt;
use std::sync::Arc;

use serde::Serialize;
use serde_json::json;

use super::client::LlmClient;
use super::transport::{LlmTransport, ReqwestTransport};
use super::types::{ChatMessage, LlmErrorKind, LlmRequest, Protocol, ToolCall, ToolDef};
use crate::settings::{ApiKey, LlmSettings};

fn probe_tools() -> Vec<ToolDef> {
    vec![
        ToolDef::new(
            "go_to",
            "Walk to a place.",
            json!({
                "type": "object",
                "properties": { "place": { "type": "string", "description": "tavern, market or chapel" } },
                "required": ["place"],
                "additionalProperties": false,
            }),
        ),
        ToolDef::new(
            "continue",
            "Carry on with what you are doing.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
    ]
}

/// The one small decision the probe asks for.
pub fn probe_request(settings: &LlmSettings) -> LlmRequest {
    LlmRequest {
        messages: vec![
            ChatMessage::system("You are a villager. Answer only by calling a tool."),
            ChatMessage::user("You are hungry and the tavern serves food. Decide what to do."),
        ],
        tools: probe_tools(),
        max_calls: 1,
        max_tokens: Some(settings.max_tokens.max(200)),
        ..LlmRequest::default()
    }
}

fn protocol_name(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Tools => "native tool calls",
        Protocol::Json => "JSON output",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub ok: bool,
    pub message: String,
    pub latency_ms: f64,
    pub protocol: Protocol,
}

/// Asks the endpoint for one small decision, to check that it is reachable and can call tools.
pub async fn probe(settings: &LlmSettings, api_key: ApiKey, transport: Arc<dyn LlmTransport>) -> ProbeResult {
    let client = LlmClient::new(LlmSettings { enabled: true, ..settings.clone() }, api_key, transport);
    let request = probe_request(settings);
    let mut last_error = None;
    // Two tries: the first may teach the client a quirk of this endpoint (or switch it to JSON output).
    for _ in 0..2 {
        match client.complete(&request).await {
            Ok(response) => {
                let call = &response.calls[0];
                let latency_ms = response.latency.as_secs_f64() * 1000.0;
                return ProbeResult {
                    ok: true,
                    latency_ms,
                    protocol: response.protocol,
                    message: format!(
                        "Works. The model chose {}({}) in {} ms using {}.",
                        call.name,
                        serde_json::Value::Object(call.args.clone()),
                        latency_ms.round(),
                        protocol_name(response.protocol)
                    ),
                };
            }
            Err(error) => {
                let retry = matches!(error.kind, LlmErrorKind::NoCall | LlmErrorKind::Truncated);
                last_error = Some(error);
                if !retry {
                    break;
                }
            }
        }
    }
    let message = last_error.map_or_else(String::new, |error| error.message);
    ProbeResult { ok: false, latency_ms: 0.0, protocol: client.protocol(), message: format!("Failed: {message}") }
}

/// Whether a reply is usable: `None` if so, otherwise why not.
pub type Usable = Box<dyn Fn(&[ToolCall]) -> Option<String> + Send + Sync>;

/// One measured request.
pub struct ProbeCase {
    pub kind: String,
    pub request: LlmRequest,
    pub usable: Usable,
}

/// The built-in case: the probe's decision, usable if it names a tool with sensible arguments.
pub fn decision_case(settings: &LlmSettings) -> ProbeCase {
    ProbeCase {
        kind: "decide".into(),
        request: probe_request(settings),
        usable: Box::new(|calls| match calls.first() {
            Some(call) if call.name == "continue" => None,
            Some(call) if call.name == "go_to" && call.args.get("place").is_some_and(|p| p.is_string()) => None,
            Some(call) => Some(format!("{} without a place", call.name)),
            None => Some("no usable call".into()),
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeRun {
    pub kind: String,
    pub latency_ms: Option<f64>,
    /// why the answer was unusable or the request failed; `None` if it was fine
    pub problem: Option<String>,
    pub failed: bool,
    pub calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub runs: Vec<ProbeRun>,
    pub protocol: Protocol,
    pub usable: usize,
    pub total: usize,
    pub median_ms: f64,
    pub p90_ms: f64,
    pub tokens: u64,
}

impl ProbeReport {
    /// Requests one at a time could serve per minute at the median latency.
    pub fn decisions_per_minute(&self) -> Option<f64> {
        (self.median_ms > 0.0).then(|| (60_000.0 / self.median_ms).round())
    }
}

impl fmt::Display for ProbeReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, run) in self.runs.iter().enumerate() {
            let n = i + 1;
            match (run.failed, run.latency_ms) {
                (false, Some(ms)) => {
                    let calls: Vec<String> = run
                        .calls
                        .iter()
                        .map(|call| {
                            let args: String =
                                serde_json::Value::Object(call.args.clone()).to_string().chars().take(110).collect();
                            format!("{}({args})", call.name)
                        })
                        .collect();
                    let verdict = run.problem.as_ref().map_or_else(|| "ok".to_string(), |p| format!("UNUSABLE ({p})"));
                    writeln!(f, "{n:>3} {:<9} {:>6} ms  {verdict}  {}", run.kind, ms.round(), calls.join(" "))?;
                }
                _ => writeln!(f, "{n:>3} {:<9}   FAILED  {}", run.kind, run.problem.as_deref().unwrap_or(""))?,
            }
        }
        writeln!(f, "\nprotocol in use: {}", protocol_name(self.protocol))?;
        let percent = if self.total > 0 { (self.usable as f64 / self.total as f64 * 100.0).round() } else { 0.0 };
        writeln!(f, "usable answers: {}/{} ({percent}%)", self.usable, self.total)?;
        writeln!(
            f,
            "latency: median {} ms, 90th percentile {} ms; {} tokens in total",
            self.median_ms.round(),
            self.p90_ms.round(),
            self.tokens
        )?;
        if let Some(per_minute) = self.decisions_per_minute() {
            writeln!(f, "at this speed one request at a time serves about {per_minute} decisions a minute")?;
        }
        Ok(())
    }
}

/// Sends each case once, in order, and reports. Stops early if the endpoint cannot be reached.
pub async fn run_cases(client: &LlmClient, cases: &[ProbeCase]) -> ProbeReport {
    let mut runs = Vec::new();
    let mut latencies = Vec::new();
    let mut tokens = 0;
    for case in cases {
        match client.complete(&case.request).await {
            Ok(response) => {
                let ms = response.latency.as_secs_f64() * 1000.0;
                latencies.push(ms);
                tokens += response.prompt_tokens + response.completion_tokens;
                let problem = (case.usable)(&response.calls);
                runs.push(ProbeRun {
                    kind: case.kind.clone(),
                    latency_ms: Some(ms),
                    problem,
                    failed: false,
                    calls: response.calls,
                });
            }
            Err(error) => {
                let network = error.kind == LlmErrorKind::Network;
                let problem = format!(
                    "{}: {}",
                    serde_json::to_value(error.kind).unwrap_or_default().as_str().unwrap_or(""),
                    error.message
                );
                runs.push(ProbeRun {
                    kind: case.kind.clone(),
                    latency_ms: None,
                    problem: Some(problem),
                    failed: true,
                    calls: vec![],
                });
                if network {
                    break;
                }
            }
        }
    }
    latencies.sort_by(f64::total_cmp);
    let at = |q: f64| -> f64 {
        if latencies.is_empty() {
            return 0.0;
        }
        latencies[((latencies.len() as f64 * q).floor() as usize).min(latencies.len() - 1)]
    };
    let usable = runs.iter().filter(|run| !run.failed && run.problem.is_none()).count();
    ProbeReport {
        protocol: client.protocol(),
        usable,
        total: runs.len(),
        median_ms: at(0.5),
        p90_ms: at(0.9),
        tokens,
        runs,
    }
}

/// Measures an endpoint with `n` probe decisions over HTTP.
pub async fn run(settings: &LlmSettings, api_key: ApiKey, n: usize) -> Result<ProbeReport, String> {
    let transport = Arc::new(ReqwestTransport::new()?);
    Ok(run_with(settings, api_key, transport, n).await)
}

/// [`run`] over any transport.
pub async fn run_with(
    settings: &LlmSettings,
    api_key: ApiKey,
    transport: Arc<dyn LlmTransport>,
    n: usize,
) -> ProbeReport {
    let client = LlmClient::new(LlmSettings { enabled: true, ..settings.clone() }, api_key, transport);
    let cases: Vec<ProbeCase> = (0..n).map(|_| decision_case(settings)).collect();
    run_cases(&client, &cases).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::llm::transport::{FakeTransport, HttpResponse};

    fn settings() -> LlmSettings {
        LlmSettings { base_url: "http://llm.test/v1".into(), model: "m".into(), ..LlmSettings::default() }
    }

    #[tokio::test]
    async fn reports_a_working_endpoint() {
        let transport = FakeTransport::always(
            200,
            json!({ "choices": [{ "message": { "tool_calls": [
                { "function": { "name": "go_to", "arguments": "{\"place\":\"tavern\"}" } },
            ] } }] }),
        );
        let result = probe(&settings(), ApiKey::default(), transport.clone()).await;
        assert!(result.ok, "{}", result.message);
        assert!(
            result.message.starts_with("Works. The model chose go_to({\"place\":\"tavern\"}) in "),
            "{}",
            result.message
        );
        assert!(result.message.ends_with("ms using native tool calls."));
        assert_eq!(transport.bodies()[0]["max_tokens"], 600);
    }

    #[tokio::test]
    async fn tries_twice_when_the_model_answers_in_prose() {
        let transport =
            FakeTransport::always(200, json!({ "choices": [{ "message": { "content": "I am hungry." } }] }));
        let result = probe(
            &LlmSettings { protocol: crate::settings::WireProtocol::Tools, ..settings() },
            ApiKey::default(),
            transport.clone(),
        )
        .await;
        assert!(!result.ok);
        assert_eq!(result.message, "Failed: The model answered without calling a tool.");
        assert_eq!(transport.requests().len(), 2);
    }

    #[tokio::test]
    async fn gives_up_at_once_when_unreachable() {
        let transport = FakeTransport::new(|_| async { Err("connection refused".to_string()) });
        let result = probe(&settings(), ApiKey::default(), transport.clone()).await;
        assert!(!result.ok);
        assert!(result.message.starts_with("Failed: Could not reach http://llm.test/v1"), "{}", result.message);
        assert_eq!(transport.requests().len(), 1);
        assert_eq!(result.protocol, Protocol::Tools);
    }

    #[tokio::test]
    async fn measures_several_runs() {
        let transport = FakeTransport::new(|request| {
            let json = request.json()["messages"].to_string().contains("Reply with JSON only");
            let body = if json {
                json!({ "choices": [{ "message": { "content": "{\"calls\":[{\"tool\":\"continue\",\"args\":{}}]}" } }], "usage": { "prompt_tokens": 50, "completion_tokens": 10 } })
            } else {
                json!({ "choices": [{ "message": { "content": "hmm" } }], "usage": { "prompt_tokens": 50, "completion_tokens": 10 } })
            };
            async move { Ok(HttpResponse::json(200, &body)) }
        });
        let report = run_with(&settings(), ApiKey::default(), transport, 3).await;
        assert_eq!(report.total, 3);
        assert_eq!(report.usable, 3);
        assert_eq!(report.protocol, Protocol::Json);
        assert_eq!(report.tokens, 180);
        let text = report.to_string();
        assert!(text.contains("usable answers: 3/3 (100%)"), "{text}");
        assert!(text.contains("protocol in use: JSON output"), "{text}");
    }

    #[tokio::test]
    async fn stops_measuring_when_unreachable() {
        let transport = FakeTransport::new(|_| async { Err("refused".to_string()) });
        let report = run_with(&settings(), ApiKey::default(), transport, 5).await;
        assert_eq!(report.total, 1);
        assert!(report.runs[0].failed);
        assert!(report.to_string().contains("FAILED  network: Could not reach"));
    }
}
