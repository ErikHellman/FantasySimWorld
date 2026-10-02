//! All routes: the API, sign-in, the health check and the web client's files.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::AppState;
use crate::api::stream;
use crate::api::wire::ApiError;
use crate::auth;

/// `/healthz` fails when the world has not ticked for this long (unless paused).
const STALE_AFTER: Duration = Duration::from_secs(5);

pub fn router(app: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/stream", get(stream::stream))
        .route("/me", get(auth::me))
        .fallback(api_not_found)
        .layer(middleware::from_fn_with_state(app.clone(), auth::require_viewer));

    Router::new()
        .route("/healthz", get(healthz))
        .route("/auth/token", get(auth::token_form).post(auth::token_login))
        .nest("/api", api)
        .merge(static_files(&app.config.static_dir))
        .layer(TraceLayer::new_for_http())
        .with_state(app)
}

async fn healthz(State(app): State<Arc<AppState>>) -> Response {
    let clock = app.clock.borrow().clone();
    if clock.speed > 0 && clock.last_tick_at.elapsed() > STALE_AFTER {
        return (StatusCode::SERVICE_UNAVAILABLE, format!("stalled at tick {}", clock.tick)).into_response();
    }
    format!("ok tick {}", clock.tick).into_response()
}

async fn api_not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(ApiError { error: "no such endpoint".into() })).into_response()
}

/// The Vite build. `/assets` holds hashed bundles and the model packs: cached
/// for good, and a missing file is a 404. Everything else falls back to
/// `index.html` (client-side routes) and is revalidated on every load.
/// Compression applies here only, never to the event stream.
fn static_files(dir: &Path) -> Router<Arc<AppState>> {
    let assets =
        Router::new().fallback_service(ServeDir::new(dir.join("assets"))).layer(middleware::from_fn(
            |request: Request, next: Next| async move {
                cache(next.run(request).await, "public, max-age=31536000, immutable")
            },
        ));
    let pages = Router::new()
        .fallback_service(ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html"))))
        .layer(middleware::from_fn(|request: Request, next: Next| async move {
            cache(next.run(request).await, "no-cache")
        }));
    Router::new().nest("/assets", assets).merge(pages).layer(CompressionLayer::new())
}

fn cache(mut response: Response, policy: &'static str) -> Response {
    if response.status().is_success() {
        response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(policy));
    }
    response
}
