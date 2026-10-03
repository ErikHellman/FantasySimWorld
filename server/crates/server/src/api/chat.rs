//! `POST /api/chat` and `POST /api/chat/name`: talking in the viewers' chat (see [`crate::chat`]).

use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::api::error;
use crate::api::wire::{ChatPost, ChatRename, ChatYou};
use crate::auth::Viewer;
use crate::chat::{ChatKey, ChatRefusal};

/// Sends a message to everyone watching. 204 when it went out.
pub async fn post(
    State(app): State<Arc<AppState>>,
    Extension(viewer): Extension<Viewer>,
    Json(body): Json<ChatPost>,
) -> Response {
    let Some(key) = ChatKey::of(&viewer, &body.viewer) else {
        return error(StatusCode::BAD_REQUEST, "viewer is required");
    };
    match app.chat.post(&key, &body.text, Instant::now()) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(refusal) => refused(&refusal),
    }
}

/// Changes this viewer's chat name; answers with the name as stored.
pub async fn rename(
    State(app): State<Arc<AppState>>,
    Extension(viewer): Extension<Viewer>,
    Json(body): Json<ChatRename>,
) -> Response {
    let Some(key) = ChatKey::of(&viewer, &body.viewer) else {
        return error(StatusCode::BAD_REQUEST, "viewer is required");
    };
    match app.chat.rename(&key, &body.name, Instant::now()) {
        Ok(name) => Json(ChatYou { name }).into_response(),
        Err(refusal) => refused(&refusal),
    }
}

fn refused(refusal: &ChatRefusal) -> Response {
    match refusal {
        ChatRefusal::NotConnected => error(StatusCode::CONFLICT, "open the stream first"),
        ChatRefusal::NameTaken => error(StatusCode::CONFLICT, "that name is taken"),
        ChatRefusal::TooFast => error(StatusCode::TOO_MANY_REQUESTS, "slow down"),
        ChatRefusal::Empty => error(StatusCode::UNPROCESSABLE_ENTITY, "the message is empty"),
        ChatRefusal::TooLong => error(StatusCode::UNPROCESSABLE_ENTITY, "the message is too long"),
        ChatRefusal::BadName(reason) => error(StatusCode::UNPROCESSABLE_ENTITY, reason),
        ChatRefusal::Filtered(reason) => error(StatusCode::UNPROCESSABLE_ENTITY, reason),
    }
}
