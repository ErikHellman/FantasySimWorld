//! The model settings (read-only: they come from the server's environment), the connection test and the decision log.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use fsw_brain::llm::probe::probe;
use fsw_brain::settings::{ApiKey, LlmSettings, parse_headers};

use crate::AppState;
use crate::api::ApiFailure;
use crate::api::wire::{BrainLogRecord, LlmSettingsView, LlmTestResult};
use crate::brain_task::transport;

/// The wire enum with the same lowercase names as the brain's.
fn same<T: serde::Serialize, U: serde::de::DeserializeOwned>(value: T) -> U {
    serde_json::from_value(serde_json::to_value(value).expect("enums serialise")).expect("same names")
}

fn view(settings: &LlmSettings, key: &ApiKey) -> LlmSettingsView {
    LlmSettingsView {
        enabled: settings.enabled,
        base_url: settings.base_url.clone(),
        model: settings.model.clone(),
        api_key_set: !key.is_empty(),
        api_key_hint: (!key.is_empty()).then(|| {
            let tail: String = key.expose().chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            format!("…{tail}")
        }),
        concurrency: settings.concurrency,
        protocol: same(settings.protocol),
        conversations: same(settings.conversations),
        reasoning: same(settings.reasoning),
        temperature: settings.temperature,
        max_tokens: settings.max_tokens,
        // Header values can be credentials too, so only their names leave the server.
        headers: parse_headers(&settings.headers).into_iter().map(|(name, _)| name).collect::<Vec<_>>().join("\n"),
        requests_per_minute: settings.requests_per_minute,
        max_requests_per_day: settings.max_requests_per_day.min(u64::from(u32::MAX)) as u32,
        max_tokens_per_day: settings.max_tokens_per_day.min(u64::from(u32::MAX)) as u32,
    }
}

/// `GET /api/admin/settings/llm`: the model the server was started with (from its `LLM_*` variables).
pub async fn get_settings(State(app): State<Arc<AppState>>) -> Json<LlmSettingsView> {
    Json(view(&app.config.llm, &app.config.llm_api_key))
}

/// `POST /api/admin/llm/test`: asks the configured model for one decision. Changes nothing.
pub async fn test(State(app): State<Arc<AppState>>) -> Json<LlmTestResult> {
    let settings = &app.config.llm;
    if !settings.enabled {
        return Json(LlmTestResult { ok: false, message: "Set LLM_BASE_URL and LLM_MODEL on the server.".into() });
    }
    let result = probe(settings, app.config.llm_api_key.clone(), transport()).await;
    Json(LlmTestResult { ok: result.ok, message: result.message })
}

/// `GET /api/admin/brain-log?before=<id>&limit=<n>`, newest first.
pub async fn brain_log(
    State(app): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Vec<BrainLogRecord>>, ApiFailure> {
    let before = query.get("before").and_then(|b| b.parse().ok());
    let limit = query.get("limit").and_then(|l| l.parse().ok()).unwrap_or(150).clamp(1, 500);
    Ok(Json(app.store.brain_log(before, limit).await?))
}
