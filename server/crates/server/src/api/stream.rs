//! `GET /api/stream`: the world as server-sent events.

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::extract::{Extension, State};
use axum::http::HeaderName;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio::sync::broadcast::error::RecvError;

use crate::AppState;
use crate::api::wire::{Hello, PROTOCOL_VERSION};
use crate::auth::{Viewer, session};

const KEEP_ALIVE: Duration = Duration::from_secs(15);
const RETRY: Duration = Duration::from_millis(500);
/// How often an open stream checks that its session still exists (sign-out, allowlist removal).
pub const SESSION_RECHECK: Duration = Duration::from_secs(60);

/// Counts a viewer for as long as their stream lives.
struct ViewerGuard(Arc<AppState>);

impl ViewerGuard {
    fn new(app: Arc<AppState>) -> Self {
        app.viewers.fetch_add(1, Ordering::Relaxed);
        Self(app)
    }
}

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        self.0.viewers.fetch_sub(1, Ordering::Relaxed);
    }
}

pub async fn stream(State(app): State<Arc<AppState>>, Extension(viewer): Extension<Viewer>) -> impl IntoResponse {
    // Subscribe before reading the clock so no tick falls between the two.
    let mut frames = app.frames.subscribe();
    let clock = app.clock.borrow().clone();
    let guard = ViewerGuard::new(app.clone());
    let hello = Hello {
        boot_id: app.boot_id.clone(),
        tick: clock.tick,
        speed: clock.speed,
        protocol_version: PROTOCOL_VERSION,
        world_hash: app.world_hash.clone(),
        viewers: app.viewers.load(Ordering::Relaxed),
        role: viewer.role,
    };

    let events = async_stream::stream! {
        let _guard = guard;
        let boot = &app.boot_id;
        let hello = serde_json::to_string(&hello).expect("hello always serialises");
        yield Ok::<_, Infallible>(Event::default().event("hello").id(format!("{boot}:{}", clock.tick)).retry(RETRY).data(hello));
        let mut recheck = tokio::time::interval_at(tokio::time::Instant::now() + SESSION_RECHECK, SESSION_RECHECK);
        loop {
            let batch = tokio::select! {
                () = app.shutdown.cancelled() => break,
                _ = recheck.tick() => {
                    if session::still_valid(&app, &viewer).await {
                        continue;
                    }
                    tracing::info!(event = "stream_signed_out", "session ended; closing the stream");
                    break;
                }
                batch = frames.recv() => batch,
            };
            match batch {
                Ok(batch) => {
                    for (tick, json) in &batch.frames {
                        if *tick > clock.tick {
                            yield Ok(Event::default().event("tick").id(format!("{boot}:{tick}")).data(&**json));
                        }
                    }
                }
                Err(RecvError::Lagged(missed)) => {
                    // Too slow to keep up: end the stream; the browser reconnects and starts afresh.
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
