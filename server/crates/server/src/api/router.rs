//! All routes: the API, sign-in, the health check and the web client's files.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::AppState;
use crate::api::{llm, stream, world};
use crate::auth;

/// A save is a few hundred KB; a long-lived world stays well under this.
const IMPORT_LIMIT: usize = 32 * 1024 * 1024;
/// `/healthz` fails when the world has not ticked for this long (unless paused).
const STALE_AFTER: Duration = Duration::from_secs(5);

pub fn router(app: Arc<AppState>) -> Router {
    let admin = Router::new()
        .route("/allowlist", get(auth::allowlist::list).post(auth::allowlist::add).delete(auth::allowlist::remove))
        .route("/attempts", get(auth::allowlist::attempts))
        .route("/speed", post(world::speed))
        .route("/world/new", post(world::new_world))
        .route("/world/import", post(world::import).layer(DefaultBodyLimit::max(IMPORT_LIMIT)))
        .route("/world/export", get(world::export))
        .route("/health", get(world::health))
        .route("/settings/llm", get(llm::get_settings).put(llm::put_settings))
        .route("/llm/test", post(llm::test))
        .route("/llm/models", get(llm::models))
        .route("/brain-log", get(llm::brain_log))
        .route_layer(middleware::from_fn(auth::middleware::require_admin));

    // Layers run outside in: CSRF first, then the session, then the handler.
    let api = Router::new()
        .route("/stream", get(stream::stream))
        .route("/me", get(auth::me))
        .route("/watch", post(world::watch))
        .route("/characters/{id}", get(world::character))
        .route("/chronicle", get(world::chronicle))
        .route("/logout", post(auth::logout))
        .nest("/admin", admin)
        .fallback(api_not_found)
        .layer(middleware::from_fn_with_state(app.clone(), auth::middleware::require_viewer))
        .layer(middleware::from_fn_with_state(app.clone(), auth::middleware::csrf));

    let sign_in = Router::new()
        .route("/providers", get(auth::providers))
        .route("/token", get(auth::token::form).post(auth::token::login))
        .route("/{provider}/start", get(auth::flow::start))
        .route("/{provider}/callback", get(auth::flow::callback_get).post(auth::flow::callback_post))
        .layer(middleware::from_fn_with_state(app.clone(), auth::middleware::rate_limit));

    Router::new()
        .route("/healthz", get(healthz))
        .nest("/auth", sign_in)
        .nest("/api", api)
        .merge(static_files(&app.config.static_dir))
        .layer(TraceLayer::new_for_http())
        .with_state(app)
}

async fn healthz(State(app): State<Arc<AppState>>) -> Response {
    let clock = app.world.clock.borrow().clone();
    if clock.speed > 0 && clock.last_tick_at.elapsed() > STALE_AFTER {
        return (StatusCode::SERVICE_UNAVAILABLE, format!("stalled at tick {}", clock.tick)).into_response();
    }
    format!("ok tick {}", clock.tick).into_response()
}

async fn api_not_found() -> Response {
    super::error(StatusCode::NOT_FOUND, "no such endpoint")
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
