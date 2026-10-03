//! `GET /api/credits`, `POST /api/presence` and `POST /api/credits/buy`: what a viewer has,
//! earning it by watching, and spending it on the world (see [`crate::credits`]).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use fsw_sim::Command;
use fsw_sim::systems::viewer_events::{check, describe};

use crate::AppState;
use crate::api::wire::{BuyRequest, CreditsBalance, CreditsState, PresenceRequest};
use crate::api::{ApiFailure, error};
use crate::auth::Viewer;
use crate::chat::ChatKey;
use crate::credits::{self, BEAT_SECONDS};
use crate::store::Purchase;
use crate::world_task::WorldInput;

fn shown(balance: i64) -> u32 {
    balance.clamp(0, i64::from(u32::MAX)) as u32
}

/// `GET /api/credits?viewer=<tab>`
pub async fn state(
    State(app): State<Arc<AppState>>,
    Extension(viewer): Extension<Viewer>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<CreditsState>, ApiFailure> {
    let hub = &app.credits;
    let key = ChatKey::of(&viewer, query.get("viewer").map_or("", String::as_str));
    let balance = match &key {
        Some(key) if hub.enabled() => hub.balance(&app.store, key, Instant::now()).await?,
        _ => 0,
    };
    Ok(Json(CreditsState {
        balance: shown(balance),
        cap: hub.cap(),
        per_minute: hub.per_minute(),
        beat_seconds: BEAT_SECONDS,
        offers: if hub.enabled() { credits::offers() } else { Vec::new() },
    }))
}

/// `POST /api/presence`: this tab is in front and being watched.
pub async fn presence(
    State(app): State<Arc<AppState>>,
    Extension(viewer): Extension<Viewer>,
    Json(body): Json<PresenceRequest>,
) -> Result<Response, ApiFailure> {
    let hub = &app.credits;
    if !hub.enabled() {
        return Ok(error(StatusCode::NOT_FOUND, "credits are off"));
    }
    let Some(key) = ChatKey::of(&viewer, &body.viewer) else {
        return Ok(error(StatusCode::BAD_REQUEST, "viewer is required"));
    };
    // Watching means a stream is open: a report from a tab without one is not from a viewer.
    if !app.world.connected.lock().contains_key(&body.viewer) {
        return Ok(error(StatusCode::CONFLICT, "open the stream first"));
    }
    let now = Instant::now();
    let balance = match hub.beat(&key, now) {
        0 => hub.balance(&app.store, &key, now).await?,
        earned => hub.earn(&app.store, &key, earned, now).await?,
    };
    Ok(Json(CreditsBalance { balance: shown(balance) }).into_response())
}

/// `POST /api/credits/buy`: makes something happen in the world, for credits. Nothing is
/// taken for what cannot happen as the world stands.
pub async fn buy(
    State(app): State<Arc<AppState>>,
    Extension(viewer): Extension<Viewer>,
    Json(body): Json<BuyRequest>,
) -> Result<Response, ApiFailure> {
    let hub = &app.credits;
    if !hub.enabled() {
        return Ok(error(StatusCode::NOT_FOUND, "credits are off"));
    }
    let Some(key) = ChatKey::of(&viewer, &body.viewer) else {
        return Ok(error(StatusCode::BAD_REQUEST, "viewer is required"));
    };
    // The chat name is the name the villagers will know them by.
    let Some(name) = app.chat.name_of(&key) else {
        return Ok(error(StatusCode::CONFLICT, "open the stream first"));
    };
    let Some(offer) = credits::offer(&body.offer) else {
        return Ok(error(StatusCode::NOT_FOUND, "no such offer"));
    };
    let event = match credits::event_for(offer, &body) {
        Ok(event) => event,
        Err(missing) => return Ok(error(StatusCode::BAD_REQUEST, missing)),
    };
    let (world_id, tick, done) = {
        let core = app.world.core.read();
        if core.speed == 0 {
            return Ok(error(StatusCode::CONFLICT, "The world is paused."));
        }
        if let Err(refusal) = check(&core.sim.state, &event) {
            return Ok(error(StatusCode::CONFLICT, refusal.message()));
        }
        (core.world_id, core.tick(), describe(&core.sim.state, &event))
    };
    let now = Instant::now();
    let short = || error(StatusCode::PAYMENT_REQUIRED, "You do not have enough credits.");
    if hub.balance(&app.store, &key, now).await? < i64::from(offer.cost) {
        return Ok(short());
    }
    // Only a purchase that would go through takes one of the viewer's turns.
    if !hub.may_buy(&key, now) {
        return Ok(error(StatusCode::TOO_MANY_REQUESTS, "slow down"));
    }
    let Some(balance) = hub.spend(&app.store, &key, offer.cost, &name, now).await? else {
        return Ok(short());
    };
    let command = Command::ViewerEvent { by: name.clone(), event };
    if app.world.inputs.send(WorldInput::Command { command, source: "viewer" }).await.is_err() {
        hub.refund(&app.store, &key, offer.cost, now).await?;
        return Ok(error(StatusCode::SERVICE_UNAVAILABLE, "the world is stopping"));
    }
    let user_id = match key {
        ChatKey::User(id) => Some(id),
        ChatKey::Tab(_) => None,
    };
    let purchase =
        Purchase { user_id, name: name.clone(), world_id, offer: offer.id.into(), cost: i64::from(offer.cost), tick };
    if let Err(error) = app.store.record_purchase(purchase).await {
        tracing::error!(%error, "cannot record a purchase");
    }
    app.chat.announce(name, done);
    Ok(Json(CreditsBalance { balance: shown(balance) }).into_response())
}
