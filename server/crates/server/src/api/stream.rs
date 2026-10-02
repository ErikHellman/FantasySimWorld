//! `GET /api/stream?viewer=<id>`: the world as server-sent events.
//!
//! A new connection gets `hello`, then either a replay of the ticks it missed
//! (a `Last-Event-ID` from this boot within the replay ring) or a `snapshot`,
//! then the current `status`, `roster` and the `detail` it watches; after
//! that, every `tick` and whatever changes. Event ids are `"<bootId>:<tick>"`.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, HeaderName};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio::sync::broadcast::error::RecvError;

use crate::AppState;
use crate::api::wire::{Hello, PROTOCOL_VERSION, Role, Snapshot};
use crate::auth::{Viewer, session};
use crate::digest::{chronicle_tail, is_alarm, roster_of};
use crate::encode::grave_of;
use crate::world_task::{Core, Outgoing, World};

const KEEP_ALIVE: Duration = Duration::from_secs(15);
const RETRY: Duration = Duration::from_millis(500);
/// How often an open stream checks that its session still exists (sign-out, allowlist removal).
pub const SESSION_RECHECK: Duration = Duration::from_secs(60);

/// Counts a viewer (and keeps their watch alive) for as long as their stream lives.
struct ViewerGuard(Arc<AppState>, String);

impl ViewerGuard {
    fn new(app: Arc<AppState>, viewer: String) -> Self {
        app.viewers.fetch_add(1, Ordering::Relaxed);
        *app.world.connected.lock().entry(viewer.clone()).or_default() += 1;
        Self(app, viewer)
    }
}

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        self.0.viewers.fetch_sub(1, Ordering::Relaxed);
        let mut connected = self.0.world.connected.lock();
        if let Some(count) = connected.get_mut(&self.1) {
            *count -= 1;
            if *count == 0 {
                connected.remove(&self.1);
                // The watch lives on for a while, so a quick reconnect keeps it.
                if let Some(watch) = self.0.world.watches.lock().get_mut(&self.1) {
                    watch.at = std::time::Instant::now();
                }
            }
        }
    }
}

/// Parses a `Last-Event-ID` of the form `"<bootId>:<tick>"`.
pub fn parse_event_id(value: &str) -> Option<(&str, u64)> {
    let (boot, tick) = value.rsplit_once(':')?;
    (!boot.is_empty()).then_some(())?;
    Some((boot, tick.parse().ok()?))
}

pub fn snapshot_of(core: &Core, world: &World) -> Snapshot {
    let state = &core.sim.state;
    Snapshot {
        tick: core.tick(),
        speed: core.speed,
        entities: core.tracker.entities(),
        roster: roster_of(state),
        chronicle: chronicle_tail(state),
        graves: state.graves.iter().map(grave_of).collect(),
        alarm: is_alarm(state),
        llm: world.llm.lock().clone(),
        voices: core.voices.iter().cloned().collect(),
    }
}

/// What a connection starts with, taken at one consistent tick.
struct Opening {
    boot: String,
    tick: u64,
    hello: String,
    /// the replayed ticks, or `None` when a snapshot is needed
    replay: Option<Vec<(u64, Arc<str>)>>,
    snapshot: Option<String>,
}

fn opening(app: &AppState, role: Role, last_event_id: Option<&str>) -> Opening {
    let world = &app.world;
    let core = world.core.read();
    let tick = core.tick();
    let hello = Hello {
        boot_id: core.boot_id.clone(),
        tick,
        speed: core.speed,
        protocol_version: PROTOCOL_VERSION,
        world_hash: app.world_hash.clone(),
        viewers: app.viewers.load(Ordering::Relaxed),
        role,
    };
    let replay = last_event_id
        .and_then(parse_event_id)
        .filter(|(boot, _)| *boot == core.boot_id)
        .and_then(|(_, last)| core.ring.since(last, tick));
    let snapshot = replay
        .is_none()
        .then(|| serde_json::to_string(&snapshot_of(&core, world)).expect("snapshots always serialise"));
    Opening {
        boot: core.boot_id.clone(),
        tick,
        hello: serde_json::to_string(&hello).expect("hello always serialises"),
        replay,
        snapshot,
    }
}

fn watched(world: &World, viewer: &str) -> Option<String> {
    world.watches.lock().get(viewer).and_then(|watch| watch.id.clone())
}

pub async fn stream(
    State(app): State<Arc<AppState>>,
    Extension(viewer): Extension<Viewer>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let viewer_id = query.get("viewer").cloned().unwrap_or_default();
    // Subscribe before taking the opening, so no tick falls between the two.
    let mut outgoing = app.world.outgoing.subscribe();
    let last_event_id = headers.get("last-event-id").and_then(|v| v.to_str().ok());
    let opening = opening(&app, viewer.role, last_event_id);
    let guard = ViewerGuard::new(app.clone(), viewer_id.clone());
    let admin = viewer.role == Role::Admin;

    let events = async_stream::stream! {
        let _guard = guard;
        let world = app.world.clone();
        let boot = opening.boot;
        let mut last_tick = opening.tick;
        yield Ok::<_, Infallible>(Event::default().event("hello").retry(RETRY).data(opening.hello));
        match opening.replay {
            Some(frames) => {
                for (tick, json) in frames {
                    yield Ok(Event::default().event("tick").id(format!("{boot}:{tick}")).data(&*json));
                }
            }
            None => {
                let snapshot = opening.snapshot.unwrap_or_default();
                yield Ok(Event::default().event("snapshot").id(format!("{boot}:{last_tick}")).data(snapshot));
            }
        }
        let status = world.status.lock().clone();
        if let Some(status) = status {
            yield Ok(Event::default().event("status").data(&*status));
        }
        let roster = world.roster.lock().clone();
        if let Some(roster) = roster {
            yield Ok(Event::default().event("roster").data(&*roster));
        }
        let mut last_detail: Option<Arc<str>> = None;
        if let Some(id) = watched(&world, &viewer_id) {
            let detail = world.details.lock().get(&id).cloned();
            if let Some(detail) = detail {
                yield Ok(Event::default().event("detail").data(&*detail));
                last_detail = Some(detail);
            }
        }

        let mut recheck = tokio::time::interval_at(tokio::time::Instant::now() + SESSION_RECHECK, SESSION_RECHECK);
        loop {
            let item = tokio::select! {
                () = app.shutdown.cancelled() => break,
                _ = recheck.tick() => {
                    if session::still_valid(&app, &viewer).await {
                        continue;
                    }
                    tracing::info!(event = "stream_signed_out", "session ended; closing the stream");
                    break;
                }
                item = outgoing.recv() => item,
            };
            match item {
                Ok(Outgoing::Ticks(frames)) => {
                    for (tick, json) in frames.iter() {
                        if *tick > last_tick {
                            last_tick = *tick;
                            yield Ok(Event::default().event("tick").id(format!("{boot}:{tick}")).data(&**json));
                        }
                    }
                }
                Ok(Outgoing::Status(json)) => yield Ok(Event::default().event("status").data(&*json)),
                Ok(Outgoing::Roster(json)) => yield Ok(Event::default().event("roster").data(&*json)),
                Ok(Outgoing::Detail { id, json }) => {
                    let fresh = last_detail.as_ref().is_none_or(|last| **last != *json);
                    if fresh && watched(&world, &viewer_id).as_deref() == Some(id.as_str()) {
                        yield Ok(Event::default().event("detail").data(&*json));
                        last_detail = Some(json);
                    }
                }
                Ok(Outgoing::BrainLog(json)) => {
                    if admin {
                        yield Ok(Event::default().event("brainlog").data(&*json));
                    }
                }
                Ok(Outgoing::Reset(json)) => {
                    // The client reloads; this stream belongs to a world that is gone.
                    yield Ok(Event::default().event("world_reset").data(&*json));
                    break;
                }
                Err(RecvError::Lagged(missed)) => {
                    // Too slow to keep up: end the stream; the browser reconnects and replays or starts afresh.
                    app.backlog_drops.fetch_add(1, Ordering::Relaxed);
                    tracing::info!(missed, "dropping a viewer that fell behind");
                    break;
                }
                Err(RecvError::Closed) => break,
            }
        }
    };

    (
        // Tell nginx-style proxies not to buffer; axum already sets Cache-Control: no-cache.
        [(HeaderName::from_static("x-accel-buffering"), "no")],
        Sse::new(events).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)),
    )
}

#[cfg(test)]
mod tests {
    use super::parse_event_id;

    #[test]
    fn event_ids_name_the_boot_and_the_tick() {
        assert_eq!(parse_event_id("a1b2:42"), Some(("a1b2", 42)));
        assert_eq!(parse_event_id("42"), None);
        assert_eq!(parse_event_id(":42"), None);
        assert_eq!(parse_event_id("a1b2:x"), None);
    }
}
