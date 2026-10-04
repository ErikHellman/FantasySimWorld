//! `/api/admin/allowlist` and `/api/admin/attempts`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::Viewer;
use crate::AppState;
use crate::api::wire::{AllowlistAdd, AllowlistEntry, AuthAttempt, Role};
use crate::api::{ApiFailure, error};
use crate::config::plausible_email;

/// `GET /api/admin/allowlist`
pub async fn list(State(app): State<Arc<AppState>>) -> Result<Json<Vec<AllowlistEntry>>, ApiFailure> {
    Ok(Json(app.store.allowlist().await?))
}

/// `POST /api/admin/allowlist` (also the Allow button on an attempt): adds or updates an entry.
pub async fn add(
    State(app): State<Arc<AppState>>,
    Extension(viewer): Extension<Viewer>,
    Json(body): Json<AllowlistAdd>,
) -> Result<Response, ApiFailure> {
    let email = body.email.trim();
    if !plausible_email(email) {
        return Ok(error(StatusCode::UNPROCESSABLE_ENTITY, "not an e-mail address"));
    }
    if body.role == Role::Guest {
        return Ok(error(StatusCode::UNPROCESSABLE_ENTITY, "the allowlist has admins and viewers, not guests"));
    }
    let note: String = body.note.chars().take(200).collect();
    let added_by = if viewer.email.is_empty() { "token" } else { viewer.email.as_str() };
    let entry = app.store.allow(email, body.role, added_by, &note).await?;
    tracing::info!(event = "allowlist_add", email = entry.email, role = ?entry.role, by = added_by);
    Ok(Json(entry).into_response())
}

#[derive(Deserialize)]
pub struct RemoveQuery {
    email: String,
}

/// `DELETE /api/admin/allowlist?email=`: removes the entry and signs that person out everywhere.
pub async fn remove(
    State(app): State<Arc<AppState>>,
    Extension(viewer): Extension<Viewer>,
    Query(query): Query<RemoveQuery>,
) -> Result<Response, ApiFailure> {
    if !app.store.disallow(&query.email).await? {
        return Ok(error(StatusCode::NOT_FOUND, "not on the allowlist"));
    }
    tracing::info!(event = "allowlist_remove", email = query.email, by = viewer.email);
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub struct AttemptsQuery {
    #[serde(default)]
    limit: Option<u32>,
}

/// `GET /api/admin/attempts?limit=`: refused sign-ins, newest first (default 100).
pub async fn attempts(
    State(app): State<Arc<AppState>>,
    Query(query): Query<AttemptsQuery>,
) -> Result<Json<Vec<AuthAttempt>>, ApiFailure> {
    Ok(Json(app.store.attempts(query.limit.unwrap_or(100)).await?))
}
