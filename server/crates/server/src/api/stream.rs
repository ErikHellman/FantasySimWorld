//! `GET /api/stream?viewer=<id>&name=<chat name>`: the world as server-sent events. `name` is
//! the chat name this browser would like, if it is free (optional).
//!
//! A new connection gets `hello`, then either a replay of the ticks it missed
//! (a `Last-Event-ID` from this boot within the replay ring) or a `snapshot`,
//! then the current `status`, `roster`, the `detail` it watches and the chat
//! (`chat_welcome`); after that, every `tick` and whatever changes. Event ids
//! are `"<bootId>:<tick>"`. A guest's stream ends with `guest_end` when they leave or another
//! tab of theirs takes the stream over.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use tokio::sync::broadcast::error::RecvError;

use crate::AppState;
use crate::api::error;
use crate::api::wire::{GuestEnd, GuestEndReason, Hello, PROTOCOL_VERSION, Role, Snapshot};
use crate::auth::Viewer;
use crate::auth::guest::still_valid;
use crate::chat::{ChatKey, ChatOut};
use crate::digest::{chronicle_tail, is_alarm, roster_of};
use crate::encode::grave_of;
use crate::world_task::{Core, Outgoing, World};

const KEEP_ALIVE: Duration = Duration::from_secs(15);
const RETRY: Duration = Duration::from_millis(500);
/// How often an open stream checks that its guest seat is still held.
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
        billboards: core.tracker.billboards(),
        houses: core.tracker.houses(),
        ceremony: core.tracker.ceremony(),
        world_objects: core.tracker.world_objects(),
        stockpiles: core.tracker.stockpiles(),
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

/// Whichever channel spoke first.
enum Next {
    World(Result<Outgoing, RecvError>),
    Chat(Result<ChatOut, RecvError>),
    GuestEnd(GuestEndReason),
}

fn to_json(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).expect("wire types always serialise")
}

fn watched(world: &World, viewer: &str) -> Option<String> {
    world.watches.lock().get(viewer).and_then(|watch| watch.id.clone())
}

pub async fn stream(
    State(app): State<Arc<AppState>>,
    Extension(viewer): Extension<Viewer>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let viewer_id = query.get("viewer").cloned().unwrap_or_default();
    // A guest's pass may have run out since the request was let in.
    let guest = match viewer.guest {
        Some(pass) => match app.guests.open_stream(pass.id, Instant::now()) {
            Some(stream) => Some(stream),
            None => return error(StatusCode::UNAUTHORIZED, "not signed in"),
        },
        None => None,
    };
    // Subscribe before taking the opening, so no tick falls between the two.
    let mut outgoing = app.world.outgoing.subscribe();
    let mut chat = app.chat.out.subscribe();
    let last_event_id = headers.get("last-event-id").and_then(|v| v.to_str().ok());
    let opening = opening(&app, viewer.role, last_event_id);
    let guard = ViewerGuard::new(app.clone(), viewer_id.clone());
    let admin = viewer.role == Role::Admin;
    let wanted_name = query.get("name").map(String::as_str);
    let (chat_guard, welcome) = app.chat.join(ChatKey::of(&viewer, &viewer_id), wanted_name, Instant::now());

    let events = async_stream::stream! {
        let _guard = guard;
        let is_guest = guest.is_some();
        let superseded = guest.as_ref().map(|g| g.superseded.clone()).unwrap_or_default();
        let guest = guest;
        let chat_guard = chat_guard;
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
        // Messages sent between subscribing and joining are in both; skip them on the channel.
        let mut last_chat = welcome.history.last().map_or(0, |m| m.id);
        yield Ok(Event::default().event("chat_welcome").data(to_json(&welcome)));

        let mut recheck = tokio::time::interval_at(tokio::time::Instant::now() + SESSION_RECHECK, SESSION_RECHECK);
        loop {
            let item = tokio::select! {
                () = app.shutdown.cancelled() => break,
                _ = recheck.tick() => {
                    if still_valid(&app, &viewer) {
                        continue;
                    }
                    tracing::info!(event = "stream_signed_out", "the seat is gone; closing the stream");
                    break;
                }
                () = superseded.cancelled(), if is_guest => {
                    let held = guest.as_ref().is_some_and(|g| app.guests.holds(g.id, Instant::now()));
                    Next::GuestEnd(if held { GuestEndReason::Elsewhere } else { GuestEndReason::Left })
                }
                item = outgoing.recv() => Next::World(item),
                said = chat.recv() => Next::Chat(said),
            };
            let item = match item {
                Next::World(item) => item,
                Next::GuestEnd(reason) => {
                    tracing::info!(event = "guest_stream_ended", ?reason);
                    yield Ok(Event::default().event("guest_end").data(to_json(&GuestEnd { reason })));
                    break;
                }
                Next::Chat(said) => {
                    match said {
                        Ok(ChatOut::Message { id, json }) => {
                            if id > last_chat {
                                last_chat = id;
                                yield Ok(Event::default().event("chat").data(&*json));
                            }
                        }
                        Ok(ChatOut::Renamed { key, json }) => {
                            if chat_guard.key() == Some(&key) {
                                yield Ok(Event::default().event("chat_you").data(&*json));
                            }
                        }
                        Err(RecvError::Lagged(_)) => {
                            // Chat is small: rather than drop the viewer, send the room as it is now.
                            let welcome = app.chat.welcome(chat_guard.key());
                            last_chat = welcome.history.last().map_or(last_chat, |m| m.id);
                            yield Ok(Event::default().event("chat_welcome").data(to_json(&welcome)));
                        }
                        Err(RecvError::Closed) => break,
                    }
                    continue;
                }
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
        .into_response()
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
