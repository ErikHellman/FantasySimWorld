//! The router end to end, without a socket.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use fsw_server::config::Config;
use fsw_server::store::Store;
use fsw_server::{AppState, router, world_task};
use http_body_util::BodyExt;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const TOKEN: &str = "test-token-0123456789";

/// A throwaway web build: index.html plus one asset.
fn static_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fsw-http-{name}-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html><title>valley</title>").unwrap();
    std::fs::write(dir.join("assets/app-1234.js"), "console.log('hello')".repeat(100)).unwrap();
    dir
}

/// The router in token mode, with its database in a temporary directory (kept while the guard lives).
async fn app(name: &str) -> (axum::Router, CancellationToken, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let config = Config::from_vars([
        ("AUTH_TOKEN".to_string(), TOKEN.to_string()),
        ("STATIC_DIR".to_string(), static_dir(name).display().to_string()),
        ("DATA_DIR".to_string(), data.path().display().to_string()),
    ])
    .unwrap();
    let shutdown = CancellationToken::new();
    let store = Store::open(&config.data_dir).await.unwrap();
    let (world, _task) = world_task::spawn(1, shutdown.clone());
    (router(AppState::new(config, world, store, shutdown.clone()).unwrap()), shutdown, data)
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

fn authed(uri: &str) -> Request<Body> {
    Request::get(uri).header(header::AUTHORIZATION, format!("Bearer {TOKEN}")).body(Body::empty()).unwrap()
}

async fn text(response: axum::response::Response) -> String {
    String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
}

#[tokio::test]
async fn health_check_reports_the_tick() {
    let (app, _, _data) = app("health").await;
    let response = app.oneshot(get("/healthz")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(text(response).await.starts_with("ok tick "));
}

#[tokio::test]
async fn api_needs_the_token() {
    let (app, _, _data) = app("auth").await;
    let response = app.clone().oneshot(get("/api/stream")).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(text(response).await, r#"{"error":"not signed in"}"#);

    let wrong = Request::get("/api/me").header(header::AUTHORIZATION, "Bearer nope").body(Body::empty()).unwrap();
    assert_eq!(app.clone().oneshot(wrong).await.unwrap().status(), StatusCode::UNAUTHORIZED);

    let response = app.clone().oneshot(authed("/api/me")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(text(response).await.contains(r#""role":"admin""#));

    let response = app.oneshot(authed("/api/nothing-here")).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn token_sign_in_sets_a_cookie_that_works() {
    let (app, _, _data) = app("signin").await;
    let form = Request::post("/auth/token")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("token={TOKEN}&return_to=%2F%2Fevil.example")))
        .unwrap();
    let response = app.clone().oneshot(form).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/");
    let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap().to_owned();
    assert!(cookie.starts_with(&format!("fsw_token={TOKEN};")));
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"));

    let pair = cookie.split(';').next().unwrap();
    let request =
        Request::get("/api/me").header(header::COOKIE, format!("other=1; {pair}")).body(Body::empty()).unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::OK);

    let wrong = Request::post("/auth/token")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("token=guess"))
        .unwrap();
    let response = app.oneshot(wrong).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get(header::SET_COOKIE).is_none());
}

#[tokio::test]
async fn stream_says_hello_then_ticks() {
    let (app, shutdown, _data) = app("stream").await;
    let response = app.oneshot(authed("/api/stream")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/event-stream");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
    assert_eq!(response.headers()["x-accel-buffering"], "no");
    assert!(response.headers().get(header::CONTENT_ENCODING).is_none());

    let mut body = response.into_body();
    let mut seen = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while seen.matches("event: tick").count() < 3 {
        let frame = tokio::time::timeout_at(deadline, body.frame()).await.expect("ticks arrive").unwrap().unwrap();
        if let Ok(data) = frame.into_data() {
            seen.push_str(std::str::from_utf8(&data).unwrap());
        }
    }
    assert!(seen.starts_with("event: hello\n"), "{seen}");
    assert!(seen.contains("retry: 500"), "{seen}");
    assert!(seen.contains(r#""protocolVersion":1"#), "{seen}");
    assert!(seen.contains(r#"data: {"t":"#), "{seen}");

    // On shutdown the stream ends, so graceful shutdown does not hang on open viewers.
    shutdown.cancel();
    let end = tokio::time::timeout(Duration::from_secs(2), body.collect()).await;
    assert!(end.is_ok(), "stream ended on shutdown");
}

#[tokio::test]
async fn static_files_are_cached_by_kind() {
    let (app, _, _data) = app("static").await;
    let page = app.clone().oneshot(get("/")).await.unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()[header::CACHE_CONTROL], "no-cache");

    // Client-side routes get the app.
    let route = app.clone().oneshot(get("/login")).await.unwrap();
    assert_eq!(route.status(), StatusCode::OK);
    assert!(text(route).await.contains("<title>valley</title>"));

    let asset =
        Request::get("/assets/app-1234.js").header(header::ACCEPT_ENCODING, "gzip").body(Body::empty()).unwrap();
    let asset = app.clone().oneshot(asset).await.unwrap();
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(asset.headers()[header::CACHE_CONTROL], "public, max-age=31536000, immutable");
    assert_eq!(asset.headers()[header::CONTENT_ENCODING], "gzip");

    // A missing asset is a 404, never index.html cached for a year.
    let missing = app.oneshot(get("/assets/gone.glb")).await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert!(missing.headers().get(header::CACHE_CONTROL).is_none());
}

#[tokio::test]
async fn token_holder_is_an_admin_and_mutations_need_json() {
    let (app, _, _data) = app("admin").await;
    let post = |content_type: &'static str, body: &'static str| {
        Request::post("/api/admin/allowlist")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .unwrap()
    };
    let form = app.clone().oneshot(post("application/x-www-form-urlencoded", "email=a%40b.c&role=viewer")).await;
    assert_eq!(form.unwrap().status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let added = app.clone().oneshot(post("application/json", r#"{"email":"Friend@Example.com","role":"viewer"}"#));
    let added = added.await.unwrap();
    assert_eq!(added.status(), StatusCode::OK);
    assert_eq!(
        text(added).await,
        text(app.clone().oneshot(authed("/api/admin/allowlist")).await.unwrap()).await.trim_matches(['[', ']'])
    );
    let bad = app.clone().oneshot(post("application/json", r#"{"email":"nobody","role":"viewer"}"#)).await;
    assert_eq!(bad.unwrap().status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.clone().oneshot(authed("/api/admin/attempts")).await.unwrap().status(), StatusCode::OK);

    // Sign-in providers are off in token mode.
    assert_eq!(app.clone().oneshot(get("/auth/google/start")).await.unwrap().status(), StatusCode::NOT_FOUND);

    let logout = Request::post("/api/logout")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let logout = app.oneshot(logout).await.unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(logout.headers()[header::SET_COOKIE].to_str().unwrap().starts_with("fsw_token=; "));
}

#[test]
fn appstate_is_shareable() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Arc<AppState>>();
}
