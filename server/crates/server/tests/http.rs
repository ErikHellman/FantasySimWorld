//! The router end to end, without a socket.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use fsw_server::config::Config;
use fsw_server::store::Store;
use fsw_server::{AppState, brain_task, router, world_task};
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
    app_with(name, &[("AUTH_TOKEN", TOKEN)]).await
}

/// The router with these settings, plus a throwaway web build and database.
async fn app_with(name: &str, settings: &[(&str, &str)]) -> (axum::Router, CancellationToken, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let mut vars: Vec<(String, String)> = settings.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    vars.push(("STATIC_DIR".to_string(), static_dir(name).display().to_string()));
    vars.push(("DATA_DIR".to_string(), data.path().display().to_string()));
    let config = Config::from_vars(vars).unwrap();
    let shutdown = CancellationToken::new();
    let store = Store::open(&config.data_dir).await.unwrap();
    let (world, _task) = world_task::start(store.clone(), 1, shutdown.clone()).await.unwrap();
    let brain = brain_task::start(world.clone(), store.clone(), config.llm.clone(), config.llm_api_key.clone()).await;
    (router(AppState::new(config, world, store, brain, shutdown.clone()).unwrap()), shutdown, data)
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

    let providers = app.clone().oneshot(get("/auth/providers")).await.unwrap();
    assert_eq!(text(providers).await, r#"{"open":false,"token":true,"google":false,"apple":false}"#);

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
    assert!(seen.contains("event: snapshot\n"), "{seen}");
    assert!(seen.contains(r#""entities":[{"static":{"id":"c1","kind":"villager""#), "{seen}");
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

#[tokio::test]
async fn with_sign_in_off_everyone_is_an_admin() {
    let (app, _, _data) = app_with("off", &[("AUTH_MODE", "off")]).await;
    let me = app.clone().oneshot(get("/api/me")).await.unwrap();
    assert_eq!(me.status(), StatusCode::OK);
    assert_eq!(text(me).await, r#"{"email":"","displayName":"Local developer","role":"admin"}"#);
    assert_eq!(app.clone().oneshot(get("/api/admin/attempts")).await.unwrap().status(), StatusCode::OK);

    let providers = app.clone().oneshot(get("/auth/providers")).await.unwrap();
    assert_eq!(text(providers).await, r#"{"open":true,"token":false,"google":false,"apple":false}"#);
    assert_eq!(app.clone().oneshot(get("/auth/token")).await.unwrap().status(), StatusCode::NOT_FOUND);

    // The CSRF rule still holds.
    let form = Request::post("/api/watch")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("viewer=a"))
        .unwrap();
    assert_eq!(app.clone().oneshot(form).await.unwrap().status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let logout =
        Request::post("/api/logout").header(header::CONTENT_TYPE, "application/json").body(Body::from("{}")).unwrap();
    let logout = app.oneshot(logout).await.unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(logout.headers().get(header::SET_COOKIE).is_none());
}

fn json_request(method: &str, uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

/// Reads stream events until `done` says enough, or fails after 5 s.
async fn read_events(body: &mut Body, done: impl Fn(&str) -> bool) -> String {
    let mut seen = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !done(&seen) {
        let frame = tokio::time::timeout_at(deadline, body.frame()).await.expect(&seen).unwrap().unwrap();
        if let Ok(data) = frame.into_data() {
            seen.push_str(std::str::from_utf8(&data).unwrap());
        }
    }
    seen
}

#[tokio::test]
async fn the_world_answers_its_rest_calls() {
    let (app, _, _data) = app("world").await;
    let detail = app.clone().oneshot(authed("/api/characters/c1")).await.unwrap();
    assert_eq!(detail.status(), StatusCode::OK);
    assert!(text(detail).await.contains(r#""name":"Aldric Vane""#));
    assert_eq!(app.clone().oneshot(authed("/api/characters/c99")).await.unwrap().status(), StatusCode::NOT_FOUND);

    let page = text(app.clone().oneshot(authed("/api/chronicle?limit=5")).await.unwrap()).await;
    assert!(page.contains("A new day dawns over the valley."), "{page}");

    let speed = app.clone().oneshot(json_request("POST", "/api/admin/speed", r#"{"speed":3}"#)).await.unwrap();
    assert_eq!(speed.status(), StatusCode::BAD_REQUEST);
    let speed = app.clone().oneshot(json_request("POST", "/api/admin/speed", r#"{"speed":4}"#)).await.unwrap();
    assert_eq!(speed.status(), StatusCode::NO_CONTENT);

    let health = text(app.clone().oneshot(authed("/api/admin/health")).await.unwrap()).await;
    assert!(health.contains(r#""dbBytes""#), "{health}");

    // Export, then import the export: the same world comes back.
    let export = app.clone().oneshot(authed("/api/admin/world/export")).await.unwrap();
    assert_eq!(export.status(), StatusCode::OK);
    assert!(export.headers()[header::CONTENT_DISPOSITION].to_str().unwrap().contains("fantasy-sim-world-day1.json"));
    let save = text(export).await;
    assert!(save.starts_with(r#"{"version":1,"savedAt":"#));
    let imported = app.clone().oneshot(json_request("POST", "/api/admin/world/import", &save)).await.unwrap();
    assert_eq!(imported.status(), StatusCode::NO_CONTENT);
    let bad = app.clone().oneshot(json_request("POST", "/api/admin/world/import", r#"{"version":1}"#)).await.unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn watching_sends_details_and_a_new_world_resets_viewers() {
    let (app, _, _data) = app("watch").await;
    let stream = app.clone().oneshot(authed("/api/stream?viewer=tab1")).await.unwrap();
    let mut body = stream.into_body();
    read_events(&mut body, |seen| seen.contains("event: snapshot")).await;
    let watch = json_request("POST", "/api/watch", r#"{"viewer":"tab1","id":"c2"}"#);
    assert_eq!(app.clone().oneshot(watch).await.unwrap().status(), StatusCode::NO_CONTENT);
    let seen = read_events(&mut body, |seen| seen.contains("event: detail")).await;
    assert!(seen.contains(r#""name":"Brenna Ashdown""#), "{seen}");

    let reset = app.clone().oneshot(json_request("POST", "/api/admin/world/new", r#"{"seed":5}"#)).await.unwrap();
    assert_eq!(reset.status(), StatusCode::NO_CONTENT);
    let seen = read_events(&mut body, |seen| seen.contains("event: world_reset")).await;
    assert!(seen.contains(r#"{"reason":"new_world"}"#), "{seen}");
}

#[tokio::test]
async fn a_quick_reconnect_replays_the_missed_ticks() {
    let (app, _, _data) = app("replay").await;
    let stream = app.clone().oneshot(authed("/api/stream")).await.unwrap();
    let mut body = stream.into_body();
    let seen = read_events(&mut body, |seen| seen.matches("event: tick").count() >= 2).await;
    let id = seen.lines().rev().find_map(|line| line.strip_prefix("id: ")).unwrap().to_owned();
    drop(body);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let again = Request::get("/api/stream")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header("last-event-id", &id)
        .body(Body::empty())
        .unwrap();
    let mut body = app.oneshot(again).await.unwrap().into_body();
    let seen = read_events(&mut body, |seen| seen.matches("event: tick").count() >= 3).await;
    assert!(!seen.contains("event: snapshot"), "{seen}");
    let (boot, tick) = id.rsplit_once(':').unwrap();
    let next = format!("id: {boot}:{}", tick.parse::<u64>().unwrap() + 1);
    assert!(seen.contains(&next), "{seen}");
}

#[tokio::test]
async fn the_model_is_off_without_its_environment() {
    let (app, _, _data) = app("llm-off").await;
    let current = text(app.clone().oneshot(authed("/api/admin/settings/llm")).await.unwrap()).await;
    assert!(current.contains(r#""enabled":false"#) && current.contains(r#""apiKeySet":false"#), "{current}");
    let test = text(app.oneshot(json_request("POST", "/api/admin/llm/test", "")).await.unwrap()).await;
    assert!(test.contains(r#""ok":false"#) && test.contains("LLM_BASE_URL"), "{test}");
}

#[tokio::test]
async fn model_settings_come_from_the_environment_and_cannot_be_changed() {
    let env = [
        ("AUTH_TOKEN", TOKEN),
        ("LLM_BASE_URL", "http://127.0.0.1:9/v1/"),
        ("LLM_MODEL", "m"),
        ("LLM_API_KEY", "sk-secret-9876"),
        ("LLM_CONCURRENCY", "3"),
        ("LLM_PROTOCOL", "json"),
        ("LLM_HEADERS", "X-Title: valley\nX-Secret: hush"),
    ];
    let (app, _, _data) = app_with("llm", &env).await;
    let current = text(app.clone().oneshot(authed("/api/admin/settings/llm")).await.unwrap()).await;
    assert!(!current.contains("sk-secret") && !current.contains("hush"), "{current}");
    assert!(current.contains(r#""headers":"X-Title\nX-Secret""#), "{current}");
    assert!(current.contains(r#""enabled":true"#) && current.contains(r#""apiKeyHint":"…9876""#), "{current}");
    assert!(
        current.contains(r#""baseUrl":"http://127.0.0.1:9/v1""#) && current.contains(r#""model":"m""#),
        "{current}"
    );
    assert!(current.contains(r#""concurrency":3"#) && current.contains(r#""protocol":"json""#), "{current}");

    // No way to change them over HTTP.
    let put = app.clone().oneshot(json_request("PUT", "/api/admin/settings/llm", r#"{"model":"other"}"#)).await;
    assert_eq!(put.unwrap().status(), StatusCode::METHOD_NOT_ALLOWED);
    let models = app.clone().oneshot(authed("/api/admin/llm/models")).await.unwrap();
    assert_ne!(models.status(), StatusCode::OK);
    let after = text(app.clone().oneshot(authed("/api/admin/settings/llm")).await.unwrap()).await;
    assert_eq!(after, current);

    // The test asks the configured endpoint, which is not there.
    let test = text(app.clone().oneshot(json_request("POST", "/api/admin/llm/test", "")).await.unwrap()).await;
    assert!(test.contains(r#""ok":false"#) && !test.contains("sk-secret"), "{test}");
    let log = app.oneshot(authed("/api/admin/brain-log?limit=5")).await.unwrap();
    assert_eq!(log.status(), StatusCode::OK);
    assert_eq!(text(log).await, "[]");
}

#[test]
fn appstate_is_shareable() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Arc<AppState>>();
}
