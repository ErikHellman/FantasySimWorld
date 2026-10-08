//! The world's REST surface: which character a tab watches, the inspector, the
//! chronicle's history, and the admin controls (speed, billboards, new world,
//! import, export, health).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use fsw_core::jsmath::utf16_len;
use fsw_core::time::game_time;
use fsw_sim::state::{BoardNotice, SAVE_VERSION, SaveFile};
use fsw_sim::systems::billboards::{TEXT_CAP, clean_text};
use fsw_sim::{Command, create_state};
use fsw_world::places::find_billboard;
use tokio::sync::oneshot;

use crate::AppState;
use crate::api::wire::{
    AnimalDetail, BillboardRequest, BillboardTone, CharacterDetail, ChroniclePage, HealthReport, MonsterDetail,
    NewWorldRequest, SiteDetail, SpeedRequest, WatchRequest,
};
use crate::api::{ApiFailure, error};
use crate::digest::{animal_detail_of, character_detail_of, detail_json, monster_detail_of, site_of};
use crate::encode::sim_tone;
use crate::store::{SnapshotReason, now_ms};
use crate::world_task::{Outgoing, SPEEDS, Watch, WorldInput};

/// `POST /api/watch`: the character (or one of the dead) whose `detail`, or the site whose `site`, this tab receives.
pub async fn watch(State(app): State<Arc<AppState>>, Json(body): Json<WatchRequest>) -> Response {
    if body.viewer.is_empty() || body.viewer.len() > 64 {
        return error(StatusCode::BAD_REQUEST, "viewer is required");
    }
    let world = &app.world;
    world.watches.lock().insert(body.viewer.clone(), Watch { id: body.id.clone(), at: std::time::Instant::now() });
    // Sent at once rather than at the next digest, so the inspector opens without a wait.
    if let Some(id) = body.id {
        let json = {
            let core = world.core.read();
            detail_json(&core.sim.state, &id)
        };
        if let Some(json) = json {
            let json: Arc<str> = Arc::from(json);
            world.details.lock().insert(id.clone(), json.clone());
            let _ = world.outgoing.send(Outgoing::Detail { id, json });
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `GET /api/characters/{id}`
pub async fn character(
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<CharacterDetail>, ApiFailure> {
    let core = app.world.core.read();
    character_detail_of(&core.sim.state, &id)
        .map(Json)
        .ok_or_else(|| ApiFailure(StatusCode::NOT_FOUND, "no such villager".into()))
}

/// `GET /api/sites/{id}`: the page for a building (`b:<place>`) or something the village built (`n:<n>`).
pub async fn site(State(app): State<Arc<AppState>>, Path(id): Path<String>) -> Result<Json<SiteDetail>, ApiFailure> {
    let core = app.world.core.read();
    site_of(&core.sim.state, &id).map(Json).ok_or_else(|| ApiFailure(StatusCode::NOT_FOUND, "no such site".into()))
}

/// `GET /api/monsters/{id}`: the card of one of the dead.
pub async fn monster(
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<MonsterDetail>, ApiFailure> {
    let core = app.world.core.read();
    monster_detail_of(&core.sim.state, &id)
        .map(Json)
        .ok_or_else(|| ApiFailure(StatusCode::NOT_FOUND, "no such monster".into()))
}

/// `GET /api/animals/{id}`: the card of one of the valley's animals.
pub async fn animal(
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<AnimalDetail>, ApiFailure> {
    let core = app.world.core.read();
    animal_detail_of(&core.sim.state, &id)
        .map(Json)
        .ok_or_else(|| ApiFailure(StatusCode::NOT_FOUND, "no such animal".into()))
}

/// `GET /api/chronicle?before=<tick>&limit=<n>`: history from the database, oldest first.
pub async fn chronicle(
    State(app): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<ChroniclePage>, ApiFailure> {
    let before = query.get("before").and_then(|b| b.parse::<u64>().ok());
    let limit = query.get("limit").and_then(|l| l.parse::<u32>().ok()).unwrap_or(100).clamp(1, 500);
    let world_id = app.world.core.read().world_id;
    // Entries of the last second are still buffered; a page must not miss them.
    app.store.flush().await?;
    Ok(Json(app.store.chronicle_page(world_id, before, limit).await?))
}

/// `POST /api/admin/speed`
pub async fn speed(State(app): State<Arc<AppState>>, Json(body): Json<SpeedRequest>) -> Response {
    if !SPEEDS.contains(&body.speed) {
        return error(StatusCode::BAD_REQUEST, "speed must be 0, 1, 2 or 4");
    }
    match app.world.inputs.send(WorldInput::Speed(body.speed)).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "the world is stopping"),
    }
}

/// `POST /api/admin/billboards {id, text?, tone?}`: puts text up on a billboard, or without text gives it
/// back to the daily slogans. It reaches the sim as a command, like everything else from outside.
pub async fn billboard(State(app): State<Arc<AppState>>, Json(body): Json<BillboardRequest>) -> Response {
    if find_billboard(&body.id).is_none() {
        return error(StatusCode::NOT_FOUND, "no such billboard");
    }
    let notice = match body.text {
        None => None,
        Some(text) => {
            let Some(text) = clean_text(&text) else {
                return error(StatusCode::BAD_REQUEST, "the text is empty or has characters a board cannot show");
            };
            if utf16_len(&text) > TEXT_CAP {
                return error(StatusCode::BAD_REQUEST, &format!("a billboard holds at most {TEXT_CAP} characters"));
            }
            Some(BoardNotice { text, tone: sim_tone(body.tone.unwrap_or(BillboardTone::Absurd)) })
        }
    };
    let command = Command::SetBillboard { board_id: body.id, notice };
    match app.world.inputs.send(WorldInput::Command { command, source: "admin" }).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "the world is stopping"),
    }
}

async fn replace(app: &AppState, state: fsw_sim::SimState, reason: SnapshotReason) -> Response {
    let (reply, answer) = oneshot::channel();
    if app.world.inputs.send(WorldInput::Replace { state: Box::new(state), reason, reply }).await.is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "the world is stopping");
    }
    match answer.await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(message)) => {
            tracing::error!(message, "replacing the world failed");
            error(StatusCode::SERVICE_UNAVAILABLE, "storage unavailable")
        }
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "the world is stopping"),
    }
}

/// `POST /api/admin/world/new {seed?}`
pub async fn new_world(State(app): State<Arc<AppState>>, Json(body): Json<NewWorldRequest>) -> Response {
    let seed = body.seed.unwrap_or_else(|| {
        let mut bytes = [0u8; 4];
        getrandom::fill(&mut bytes).expect("the OS random source works");
        u32::from_le_bytes(bytes) & 0x7fff_ffff
    });
    replace(&app, create_state(seed), SnapshotReason::NewWorld).await
}

/// `POST /api/admin/world/import`: a browser (or server) export, `{version, savedAt, state}`.
pub async fn import(State(app): State<Arc<AppState>>, body: Bytes) -> Response {
    let save: SaveFile = match serde_json::from_slice(&body) {
        Ok(save) => save,
        Err(e) => {
            return error(StatusCode::BAD_REQUEST, &format!("that file is not a save this version can read ({e})"));
        }
    };
    if save.version != SAVE_VERSION || save.state.version != SAVE_VERSION {
        return error(StatusCode::BAD_REQUEST, "that save is from a different version");
    }
    replace(&app, save.state, SnapshotReason::Import).await
}

/// `GET /api/admin/world/export`: the same envelope the browser's Export wrote.
pub async fn export(State(app): State<Arc<AppState>>) -> Response {
    let state = app.world.core.read().sim.state.clone();
    let day = game_time(state.tick).day;
    let save = SaveFile { version: SAVE_VERSION, saved_at: now_ms().max(0) as u64, state };
    let body = match tokio::task::spawn_blocking(move || serde_json::to_vec(&save)).await {
        Ok(Ok(body)) => body,
        _ => return error(StatusCode::INTERNAL_SERVER_ERROR, "cannot write the save"),
    };
    (
        [
            (header::CONTENT_TYPE, "application/json".to_owned()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"fantasy-sim-world-day{day}.json\"")),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        body,
    )
        .into_response()
}

fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((p * sorted.len() as f64).floor() as usize).min(sorted.len() - 1);
    (sorted[index] * 100.0).round() / 100.0
}

/// `GET /api/admin/health`
pub async fn health(State(app): State<Arc<AppState>>) -> Result<Json<HealthReport>, ApiFailure> {
    let world = &app.world;
    let jitter: Vec<f64> = world.jitter.lock().iter().copied().collect();
    let tick = world.core.read().tick();
    let snapshot_age_seconds =
        app.store.latest_snapshot_at().await?.map(|at| ((now_ms() - at).max(0) as f64 / 1000.0).round());
    Ok(Json(HealthReport {
        tick,
        tick_jitter_p50_ms: percentile(&jitter, 0.5),
        tick_jitter_p99_ms: percentile(&jitter, 0.99),
        clock_skips: world.clock_skips.load(Ordering::Relaxed),
        backlog_drops: app.backlog_drops.load(Ordering::Relaxed),
        viewers: app.viewers.load(Ordering::Relaxed),
        db_bytes: app.store.db_bytes().await?,
        snapshot_age_seconds,
        llm: app.store.usage_today().await?,
    }))
}
