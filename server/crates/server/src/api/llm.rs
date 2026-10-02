//! The admin's model settings, the connection test, the model list and the decision log.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use fsw_brain::llm::client::LlmClient;
use fsw_brain::llm::probe::probe;
use fsw_brain::settings::{ApiKey, LlmSettings};
use serde_json::{Map, Value};

use crate::AppState;
use crate::api::wire::{BrainLogRecord, LlmSettingsUpdate, LlmSettingsView, LlmTestResult, ModelList};
use crate::api::{ApiFailure, error};
use crate::brain_task::{load_key, load_settings, reconfigure, transport};
use crate::store::settings::LLM;

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
        headers: settings.headers.clone(),
        requests_per_minute: settings.requests_per_minute,
        max_requests_per_day: settings.max_requests_per_day.min(u64::from(u32::MAX)) as u32,
        max_tokens_per_day: settings.max_tokens_per_day.min(u64::from(u32::MAX)) as u32,
    }
}

/// Settings from a (possibly partial) draft over the current ones, with numbers kept in sane ranges.
fn merged(current: &LlmSettings, draft: &Map<String, Value>) -> Result<LlmSettings, String> {
    let mut value = serde_json::to_value(current).expect("settings serialise");
    let object = value.as_object_mut().expect("settings are an object");
    for (key, field) in draft {
        if key != "apiKey" && object.contains_key(key) {
            object.insert(key.clone(), field.clone());
        }
    }
    let mut settings: LlmSettings = serde_json::from_value(value).map_err(|e| e.to_string())?;
    settings.base_url = settings.base_url.trim().to_owned();
    settings.model = settings.model.trim().to_owned();
    settings.concurrency = settings.concurrency.clamp(1, 16);
    settings.temperature = if settings.temperature.is_finite() { settings.temperature.clamp(0.0, 2.0) } else { 0.8 };
    settings.max_tokens = settings.max_tokens.max(100);
    Ok(settings)
}

/// `GET /api/admin/settings/llm`
pub async fn get_settings(State(app): State<Arc<AppState>>) -> Json<LlmSettingsView> {
    Json(view(&load_settings(&app.store).await, &load_key(&app.store).await))
}

/// `PUT /api/admin/settings/llm`: saves, then the brain picks the new settings up at once.
pub async fn put_settings(
    State(app): State<Arc<AppState>>,
    Json(update): Json<LlmSettingsUpdate>,
) -> Result<Json<LlmSettingsView>, ApiFailure> {
    let draft = serde_json::to_value(&update).expect("updates serialise");
    let settings = merged(&load_settings(&app.store).await, draft.as_object().expect("an object"))
        .map_err(|e| ApiFailure(StatusCode::BAD_REQUEST, e))?;
    app.store.put_setting(LLM, &serde_json::to_value(&settings).expect("settings serialise")).await?;
    if let Some(key) = &update.api_key {
        let key = key.trim();
        app.store.set_api_key((!key.is_empty()).then_some(key)).await?;
    }
    reconfigure(&app.brain, &app.store).await;
    Ok(Json(view(&settings, &load_key(&app.store).await)))
}

/// `POST /api/admin/llm/test`: the draft in the body (over the saved settings and key), or the saved ones.
pub async fn test(State(app): State<Arc<AppState>>, Json(draft): Json<Value>) -> Response {
    let draft = draft.as_object().cloned().unwrap_or_default();
    let settings = match merged(&load_settings(&app.store).await, &draft) {
        Ok(settings) => settings,
        Err(e) => return error(StatusCode::BAD_REQUEST, &e),
    };
    let key = match draft.get("apiKey").and_then(Value::as_str) {
        Some(key) => ApiKey::new(key.trim()),
        None => load_key(&app.store).await,
    };
    if settings.base_url.is_empty() || settings.model.is_empty() {
        return Json(LlmTestResult { ok: false, message: "Set a base URL and a model first.".into() }).into_response();
    }
    let result = probe(&settings, key, transport()).await;
    Json(LlmTestResult { ok: result.ok, message: result.message }).into_response()
}

/// `GET /api/admin/llm/models`, with the saved settings (or `?baseUrl=` for a draft).
pub async fn models(State(app): State<Arc<AppState>>, Query(query): Query<HashMap<String, String>>) -> Response {
    let mut settings = load_settings(&app.store).await;
    if let Some(base_url) = query.get("baseUrl").filter(|u| !u.trim().is_empty()) {
        settings.base_url = base_url.trim().to_owned();
    }
    if settings.base_url.is_empty() {
        return error(StatusCode::BAD_REQUEST, "Save a base URL first.");
    }
    let client = LlmClient::new(settings, load_key(&app.store).await, transport());
    match client.list_models().await {
        Ok(models) => Json(ModelList { models }).into_response(),
        Err(e) => error(StatusCode::BAD_GATEWAY, &e.message),
    }
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
