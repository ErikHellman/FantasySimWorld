//! The router end to end, without a socket.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use fsw_server::config::Config;
use fsw_server::guests::GuestLimits;
use fsw_server::store::SnapshotReason;
use fsw_server::store::Store;
use fsw_server::{AppState, brain_task, digest, router, world_task};
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
    let (state, shutdown, data) = state_with(name, settings).await;
    (router(state), shutdown, data)
}

/// The shared state behind [`app_with`], for tests that reach past the routes.
async fn state_with(name: &str, settings: &[(&str, &str)]) -> (Arc<AppState>, CancellationToken, tempfile::TempDir) {
    state_with_guests(name, settings, GuestLimits::default()).await
}

/// Like [`state_with`], with a seat cap of the test's choosing.
async fn state_with_guests(
    name: &str,
    settings: &[(&str, &str)],
    guests: GuestLimits,
) -> (Arc<AppState>, CancellationToken, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let mut vars: Vec<(String, String)> = settings.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    vars.push(("STATIC_DIR".to_string(), static_dir(name).display().to_string()));
    vars.push(("DATA_DIR".to_string(), data.path().display().to_string()));
    let mut config = Config::from_vars(vars).unwrap();
    config.guests = guests;
    let shutdown = CancellationToken::new();
    let store = Store::open(&config.data_dir).await.unwrap();
    let (world, _task) = world_task::start(store.clone(), 1, shutdown.clone()).await.unwrap();
    // A new world is empty until it is founded; these tests know the authored cast by name.
    let (reply, answer) = tokio::sync::oneshot::channel();
    let founded = Box::new(fsw_testing::create_test_state(1));
    let replace = world_task::WorldInput::Replace { state: founded, reason: SnapshotReason::NewWorld, reply };
    world.inputs.send(replace).await.unwrap();
    answer.await.unwrap().unwrap();
    let brain = brain_task::start(world.clone(), store.clone(), config.llm.clone(), config.llm_api_key.clone()).await;
    (AppState::new(config, world, store, brain, shutdown.clone()), shutdown, data)
}

#[tokio::test]
async fn a_fresh_world_reports_its_founding_in_the_status() {
    let data = tempfile::tempdir().unwrap();
    let vars = vec![
        ("AUTH_TOKEN".to_string(), TOKEN.to_string()),
        ("STATIC_DIR".to_string(), static_dir("founding").display().to_string()),
        ("DATA_DIR".to_string(), data.path().display().to_string()),
    ];
    let config = Config::from_vars(vars).unwrap();
    let shutdown = CancellationToken::new();
    let store = Store::open(&config.data_dir).await.unwrap();
    let (world, _task) = world_task::start(store.clone(), 1, shutdown.clone()).await.unwrap();
    let brain = brain_task::start(world.clone(), store.clone(), config.llm.clone(), config.llm_api_key.clone()).await;
    let state = AppState::new(config, world, store, brain, shutdown.clone());
    let _digest = digest::task::spawn(state.clone(), shutdown.clone());
    let app = router(state);
    let mut body = app.clone().oneshot(authed("/api/stream?viewer=tabF")).await.unwrap().into_body();
    let seen = read_events(&mut body, |seen| seen.contains("event: status")).await;
    assert!(seen.contains(r#""entities":[]"#), "{seen}");
    assert_eq!(last_event(&seen, "status")["founding"], serde_json::json!({ "settled": 0, "total": 13 }));
    shutdown.cancel();
}

#[tokio::test]
async fn one_of_the_dead_has_a_card_of_its_own() {
    use fsw_sim::state::{MonsterKind, MonsterOrigin};
    use fsw_sim::systems::monsters::spawn_monster;
    use fsw_sim::util::Ctx;

    let (state, shutdown, _data) = state_with("monster", &[("AUTH_TOKEN", TOKEN)]).await;
    let id = {
        let mut core = state.world.core.write();
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut core.sim.state, &mut events);
        let at = fsw_world::places::get_place("ruins").door;
        let id = spawn_monster(&mut ctx, MonsterKind::Necromancer, at, 0.0, MonsterOrigin::Ruins);
        // It is daytime in a fresh world; one that lies low by day stays for the test.
        ctx.m(&id).lasting = true;
        id
    };
    let app = router(state);
    let card = app.clone().oneshot(authed(&format!("/api/monsters/{id}"))).await.unwrap();
    assert_eq!(card.status(), StatusCode::OK);
    let card = text(card).await;
    assert!(card.contains(r#""species":"necromancer""#) && card.contains(r#""lasting":true"#), "{card}");
    assert!(card.contains("Crept out of the old ruins on day 1"), "{card}");
    // The stream's `detail` carries it, tagged, once a tab watches it.
    let watch = json_request("POST", "/api/watch", &format!(r#"{{"viewer":"tabM","id":"{id}"}}"#));
    assert_eq!(app.clone().oneshot(watch).await.unwrap().status(), StatusCode::NO_CONTENT);
    let mut body = app.clone().oneshot(authed("/api/stream?viewer=tabM")).await.unwrap().into_body();
    let seen = read_events(&mut body, |seen| seen.contains("event: detail")).await;
    assert_eq!(last_event(&seen, "detail")["kind"], serde_json::json!("monster"), "{seen}");
    shutdown.cancel();
}

#[tokio::test]
async fn a_ghost_has_a_card_of_its_own() {
    use fsw_sim::systems::death::kill_character;
    use fsw_sim::systems::ghosts::{Haunt, bind};
    use fsw_sim::util::Ctx;

    let (state, shutdown, _data) = state_with("ghost", &[("AUTH_TOKEN", TOKEN)]).await;
    let id = {
        let mut core = state.world.core.write();
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut core.sim.state, &mut events);
        let haunt = Haunt::of(&ctx, "c1");
        kill_character(&mut ctx, "c1", "slain by a Skeleton Warrior at the green", None);
        bind(&mut ctx, haunt)
    };
    let app = router(state);
    let card = app.clone().oneshot(authed(&format!("/api/ghosts/{id}"))).await.unwrap();
    assert_eq!(card.status(), StatusCode::OK);
    let card = text(card).await;
    assert!(card.contains(r#""name":"Aldric Vane""#) && card.contains(r#""wasId":"c1""#), "{card}");
    assert!(card.contains("slain by a Skeleton Warrior") && card.contains("not at rest"), "{card}");
    assert_eq!(app.clone().oneshot(authed("/api/ghosts/g99")).await.unwrap().status(), StatusCode::NOT_FOUND);
    assert_eq!(app.clone().oneshot(authed("/api/ghosts/c2")).await.unwrap().status(), StatusCode::NOT_FOUND);
    // The stream's `detail` carries it, tagged, once a tab watches it.
    let watch = json_request("POST", "/api/watch", &format!(r#"{{"viewer":"tabH","id":"{id}"}}"#));
    assert_eq!(app.clone().oneshot(watch).await.unwrap().status(), StatusCode::NO_CONTENT);
    let mut body = app.clone().oneshot(authed("/api/stream?viewer=tabH")).await.unwrap().into_body();
    let seen = read_events(&mut body, |seen| seen.contains("event: detail")).await;
    assert_eq!(last_event(&seen, "detail")["kind"], serde_json::json!("ghost"), "{seen}");
    shutdown.cancel();
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
    assert!(seen.contains("event: snapshot\n"), "{seen}");
    assert!(seen.contains(r#""entities":[{"static":{"id":"c1","kind":"villager""#), "{seen}");
    assert!(seen.contains("retry: 500"), "{seen}");
    assert!(seen.contains(r#""protocolVersion":13"#), "{seen}");
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
        Request::post("/api/admin/billboards")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .unwrap()
    };
    let form = app.clone().oneshot(post("application/x-www-form-urlencoded", "id=green_board&text=Hi")).await;
    assert_eq!(form.unwrap().status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let posted = app.clone().oneshot(post("application/json", r#"{"id":"green_board","text":"Hi"}"#)).await.unwrap();
    assert_eq!(posted.status(), StatusCode::NO_CONTENT);
    assert_eq!(app.clone().oneshot(authed("/api/admin/health")).await.unwrap().status(), StatusCode::OK);

    // A form on another site posting with the admin's cookie, and a foreign origin even with JSON, are refused.
    let cross_site = |content_type: &'static str, body: &'static str| {
        Request::post("/api/admin/speed")
            .header(header::COOKIE, format!("fsw_token={TOKEN}"))
            .header(header::CONTENT_TYPE, content_type)
            .header(header::ORIGIN, "https://evil.example")
            .body(Body::from(body))
            .unwrap()
    };
    let form = cross_site("application/x-www-form-urlencoded", "speed=0");
    assert_eq!(app.clone().oneshot(form).await.unwrap().status(), StatusCode::FORBIDDEN);
    let json = cross_site("application/json", r#"{"speed":0}"#);
    assert_eq!(app.clone().oneshot(json).await.unwrap().status(), StatusCode::FORBIDDEN);

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
async fn the_token_form_is_rate_limited_per_address() {
    let (app, _, _data) = app("rate").await;
    let mut statuses = Vec::new();
    for _ in 0..40 {
        statuses.push(app.clone().oneshot(get("/auth/token")).await.unwrap().status());
    }
    assert!(statuses[..30].iter().all(|s| *s == StatusCode::OK), "{statuses:?}");
    assert_eq!(statuses[39], StatusCode::TOO_MANY_REQUESTS);
    // The API is not affected.
    assert_eq!(app.oneshot(authed("/api/me")).await.unwrap().status(), StatusCode::OK);
}

#[tokio::test]
async fn with_sign_in_off_everyone_is_an_admin() {
    let (app, _, _data) = app_with("off", &[("AUTH_MODE", "off")]).await;
    let me = app.clone().oneshot(get("/api/me")).await.unwrap();
    assert_eq!(me.status(), StatusCode::OK);
    assert_eq!(text(me).await, r#"{"role":"admin"}"#);
    assert_eq!(app.clone().oneshot(get("/api/admin/health")).await.unwrap().status(), StatusCode::OK);
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
    assert_eq!(app.clone().oneshot(authed("/api/monsters/m99")).await.unwrap().status(), StatusCode::NOT_FOUND);
    assert_eq!(app.clone().oneshot(authed("/api/monsters/c1")).await.unwrap().status(), StatusCode::NOT_FOUND);

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
async fn a_site_has_a_page_and_its_watcher_gets_it_on_the_stream() {
    let (app, _, _data) = app("sites").await;
    let page = text(app.clone().oneshot(authed("/api/sites/b:home_4")).await.unwrap()).await;
    assert!(page.contains(r#""kind":"building""#) && page.contains(r#""beds":2"#), "{page}");
    for missing in ["/api/sites/c1", "/api/sites/b:green", "/api/sites/n:404"] {
        assert_eq!(app.clone().oneshot(authed(missing)).await.unwrap().status(), StatusCode::NOT_FOUND, "{missing}");
    }
    let stream = app.clone().oneshot(authed("/api/stream?viewer=tab9")).await.unwrap();
    let mut body = stream.into_body();
    read_events(&mut body, |seen| seen.contains("event: snapshot")).await;
    let watch = json_request("POST", "/api/watch", r#"{"viewer":"tab9","id":"b:tavern"}"#);
    assert_eq!(app.clone().oneshot(watch).await.unwrap().status(), StatusCode::NO_CONTENT);
    let seen = read_events(&mut body, |seen| seen.contains("event: site")).await;
    assert_eq!(last_event(&seen, "site")["name"], "The Gilded Tankard");
    assert!(!seen.contains("event: detail"), "{seen}");
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
    assert!(current.contains(r#""embedModel":null"#) && current.contains(r#""embedApiKeySet":false"#), "{current}");
    let test = text(app.oneshot(json_request("POST", "/api/admin/llm/test", "")).await.unwrap()).await;
    assert!(test.contains(r#""ok":false"#) && test.contains("LLM_BASE_URL"), "{test}");
}

#[tokio::test]
async fn embedding_settings_are_shown_without_their_key_and_tested() {
    let env = [
        ("AUTH_TOKEN", TOKEN),
        ("EMBED_BASE_URL", "http://127.0.0.1:9/v1"),
        ("EMBED_MODEL", "nomic-embed-text"),
        ("EMBED_API_KEY", "sk-embed-secret"),
    ];
    let (app, _, _data) = app_with("embed", &env).await;
    let current = text(app.clone().oneshot(authed("/api/admin/settings/llm")).await.unwrap()).await;
    assert!(current.contains(r#""embedModel":"nomic-embed-text""#), "{current}");
    assert!(current.contains(r#""embedBaseUrl":"http://127.0.0.1:9/v1""#), "{current}");
    assert!(current.contains(r#""embedApiKeySet":true"#) && !current.contains("sk-embed-secret"), "{current}");

    // Nothing listens there: the test says so, without the key.
    let test = text(app.oneshot(json_request("POST", "/api/admin/llm/test", "")).await.unwrap()).await;
    assert!(test.contains(r#""ok":false"#) && test.contains("Embeddings failed"), "{test}");
    assert!(!test.contains("sk-embed-secret"), "{test}");
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

/// The data line of the last `name` event in `seen`, as JSON.
fn last_event(seen: &str, name: &str) -> serde_json::Value {
    let marker = format!("event: {name}\ndata: ");
    let start = seen.rfind(&marker).unwrap_or_else(|| panic!("no {name} in {seen}")) + marker.len();
    let line = seen[start..].lines().next().unwrap();
    serde_json::from_str(line).unwrap()
}

#[tokio::test]
async fn chat_messages_reach_every_viewer_and_later_ones_get_the_history() {
    let (app, _, _data) = app("chat").await;
    let mut a = app.clone().oneshot(authed("/api/stream?viewer=tabA")).await.unwrap().into_body();
    let mut b = app.clone().oneshot(authed("/api/stream?viewer=tabB")).await.unwrap().into_body();
    let seen_a = read_events(&mut a, |seen| seen.contains("event: chat_welcome")).await;
    let seen_b = read_events(&mut b, |seen| seen.contains("event: chat_welcome")).await;
    assert!(seen_a.starts_with("event: hello\n"), "{seen_a}");
    let welcome = last_event(&seen_a, "chat_welcome");
    assert_eq!(welcome["maxLength"], 500);
    let name_a = welcome["you"].as_str().unwrap().to_owned();
    let name_b = last_event(&seen_b, "chat_welcome")["you"].as_str().unwrap().to_owned();
    assert_ne!(name_a, name_b, "everyone connected gets their own name");
    let history = &welcome["history"];
    assert_eq!(history.as_array().unwrap().len(), 1, "{history}");
    assert_eq!(history[0]["kind"], "join");
    assert_eq!(history[0]["author"], name_a.as_str());

    let post = json_request("POST", "/api/chat", r#"{"viewer":"tabA","text":"**hello** valley"}"#);
    assert_eq!(app.clone().oneshot(post).await.unwrap().status(), StatusCode::NO_CONTENT);
    let seen = read_events(&mut b, |seen| seen.contains("event: chat\n")).await;
    let message = last_event(&seen, "chat");
    assert_eq!(message["kind"], "say");
    assert_eq!(message["author"], name_a.as_str());
    assert_eq!(message["text"], "**hello** valley");

    let mut c = app.clone().oneshot(authed("/api/stream?viewer=tabC")).await.unwrap().into_body();
    let seen = read_events(&mut c, |seen| seen.contains("event: chat_welcome")).await;
    let history = last_event(&seen, "chat_welcome")["history"].clone();
    let lines: Vec<_> = history.as_array().unwrap().iter().map(|m| m["kind"].as_str().unwrap()).collect();
    assert_eq!(lines, ["join", "join", "say", "join"]);
    assert_eq!(history[2]["text"], "**hello** valley");
}

#[tokio::test]
async fn a_new_viewer_joins_under_the_name_their_browser_asks_for() {
    let (app, _, _data) = app("chat-join").await;
    let mut a = app.clone().oneshot(authed("/api/stream?viewer=tabA")).await.unwrap().into_body();
    read_events(&mut a, |seen| seen.contains("event: chat_welcome")).await;

    let mut b = app.clone().oneshot(authed("/api/stream?viewer=tabB&name=Old%20Tom")).await.unwrap().into_body();
    let seen_b = read_events(&mut b, |seen| seen.contains("event: chat_welcome")).await;
    assert_eq!(last_event(&seen_b, "chat_welcome")["you"], "Old Tom");
    let seen_a = read_events(&mut a, |seen| seen.contains("event: chat\n")).await;
    let joined = last_event(&seen_a, "chat");
    assert_eq!(joined["kind"], "join");
    assert_eq!(joined["author"], "Old Tom");
    assert_eq!(joined["text"], "");

    // A second tab asking for a name someone else has gets a fresh one.
    let mut c = app.clone().oneshot(authed("/api/stream?viewer=tabC&name=old%20tom")).await.unwrap().into_body();
    let seen_c = read_events(&mut c, |seen| seen.contains("event: chat_welcome")).await;
    assert_ne!(last_event(&seen_c, "chat_welcome")["you"].as_str().unwrap().to_lowercase(), "old tom");
}

#[tokio::test]
async fn chat_refuses_what_it_cannot_send() {
    let (app, _, _data) = app("chat-refusals").await;
    let early = json_request("POST", "/api/chat", r#"{"viewer":"tabA","text":"hi"}"#);
    let early = app.clone().oneshot(early).await.unwrap();
    assert_eq!(early.status(), StatusCode::CONFLICT);
    assert_eq!(text(early).await, r#"{"error":"open the stream first"}"#);

    let mut a = app.clone().oneshot(authed("/api/stream?viewer=tabA")).await.unwrap().into_body();
    read_events(&mut a, |seen| seen.contains("event: chat_welcome")).await;
    let long = format!(r#"{{"viewer":"tabA","text":"{}"}}"#, "x".repeat(501));
    let long = app.clone().oneshot(json_request("POST", "/api/chat", &long)).await.unwrap();
    assert_eq!(long.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let empty = json_request("POST", "/api/chat", r#"{"viewer":"tabA","text":" \n "}"#);
    assert_eq!(app.clone().oneshot(empty).await.unwrap().status(), StatusCode::UNPROCESSABLE_ENTITY);
    let anonymous = json_request("POST", "/api/chat", r#"{"viewer":"","text":"hi"}"#);
    assert_eq!(app.clone().oneshot(anonymous).await.unwrap().status(), StatusCode::BAD_REQUEST);

    let form = Request::post("/api/chat")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(r#"{"viewer":"tabA","text":"hi"}"#))
        .unwrap();
    assert_eq!(app.clone().oneshot(form).await.unwrap().status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn chat_names_are_unique_among_connected_viewers() {
    let (app, _, _data) = app("chat-names").await;
    let mut a = app.clone().oneshot(authed("/api/stream?viewer=tabA")).await.unwrap().into_body();
    let mut b = app.clone().oneshot(authed("/api/stream?viewer=tabB")).await.unwrap().into_body();
    read_events(&mut a, |seen| seen.contains("event: chat_welcome")).await;
    let seen_b = read_events(&mut b, |seen| seen.contains("event: chat_welcome")).await;
    let name_b = last_event(&seen_b, "chat_welcome")["you"].as_str().unwrap().to_owned();

    let taken = format!(r#"{{"viewer":"tabA","name":"{}"}}"#, name_b.to_uppercase());
    let taken = app.clone().oneshot(json_request("POST", "/api/chat/name", &taken)).await.unwrap();
    assert_eq!(taken.status(), StatusCode::CONFLICT);
    assert_eq!(text(taken).await, r#"{"error":"that name is taken"}"#);
    let bad = json_request("POST", "/api/chat/name", r#"{"viewer":"tabA","name":"<b>"}"#);
    assert_eq!(app.clone().oneshot(bad).await.unwrap().status(), StatusCode::UNPROCESSABLE_ENTITY);

    let rename = json_request("POST", "/api/chat/name", r#"{"viewer":"tabA","name":"  Old  Tom "}"#);
    let rename = app.clone().oneshot(rename).await.unwrap();
    assert_eq!(rename.status(), StatusCode::OK);
    assert_eq!(text(rename).await, r#"{"name":"Old Tom"}"#);
    let seen = read_events(&mut a, |seen| seen.contains("event: chat_you")).await;
    assert_eq!(last_event(&seen, "chat_you")["name"], "Old Tom");

    // Tab B reloads: its name is kept for it (it is not said to have left for a few minutes).
    drop(b);
    let mut b = app.clone().oneshot(authed("/api/stream?viewer=tabB")).await.unwrap().into_body();
    let seen_b = read_events(&mut b, |seen| seen.contains("event: chat_welcome")).await;
    assert_eq!(last_event(&seen_b, "chat_welcome")["you"], name_b.as_str());
    let reuse = format!(r#"{{"viewer":"tabA","name":"{name_b}"}}"#);
    let reuse = app.clone().oneshot(json_request("POST", "/api/chat/name", &reuse)).await.unwrap();
    assert_eq!(reuse.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn admins_put_text_on_a_billboard_and_every_viewer_sees_it() {
    let (app, _, _data) = app("billboards").await;
    let mut body = app.clone().oneshot(authed("/api/stream?viewer=tabA")).await.unwrap().into_body();
    // A tick after the snapshot means the snapshot has arrived whole.
    let seen = read_events(&mut body, |seen| seen.contains("event: snapshot") && seen.contains("event: tick")).await;
    // The snapshot carries an `id:` line before its data.
    let snapshot = seen.split("event: snapshot\n").nth(1).unwrap();
    let data = snapshot.lines().find_map(|line| line.strip_prefix("data: ")).unwrap();
    let boards = serde_json::from_str::<serde_json::Value>(data).unwrap()["billboards"].clone();
    assert_eq!(boards.as_array().unwrap().len(), 7, "{boards}");
    assert_eq!(boards[1]["id"], "green_board");
    assert_eq!(boards[1]["place"], "the village green");
    assert_eq!(boards[1]["custom"], false);

    let post = |body: &str| json_request("POST", "/api/admin/billboards", body);
    let missing = app.clone().oneshot(post(r#"{"id":"moon_board","text":"Hi"}"#)).await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let blank = app.clone().oneshot(post(r#"{"id":"green_board","text":"  \n "}"#)).await.unwrap();
    assert_eq!(blank.status(), StatusCode::BAD_REQUEST);
    let long = format!(r#"{{"id":"green_board","text":"{}"}}"#, "a".repeat(91));
    assert_eq!(app.clone().oneshot(post(&long)).await.unwrap().status(), StatusCode::BAD_REQUEST);
    let form = Request::post("/api/admin/billboards")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("id=green_board&text=hi"))
        .unwrap();
    assert_eq!(app.clone().oneshot(form).await.unwrap().status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let set = post(r#"{"id":"green_board","text":"Fresh bread at the market!","tone":"pushy"}"#);
    assert_eq!(app.clone().oneshot(set).await.unwrap().status(), StatusCode::NO_CONTENT);
    let seen = read_events(&mut body, |seen| seen.contains(r#""type":"billboard""#)).await;
    assert!(
        seen.contains(r#"{"type":"billboard","billboard":{"id":"green_board","place":"the village green","text":"Fresh bread at the market!","tone":"pushy","custom":true}}"#),
        "{seen}"
    );

    let back = post(r#"{"id":"green_board"}"#);
    assert_eq!(app.clone().oneshot(back).await.unwrap().status(), StatusCode::NO_CONTENT);
    let seen = read_events(&mut body, |seen| seen.contains(r#""custom":false"#)).await;
    assert!(seen.contains(r#""id":"green_board""#), "{seen}");
}

// ---------------------------------------------------------------------------
// Guests
// ---------------------------------------------------------------------------

/// The router in token mode with these guest limits.
async fn guest_app(name: &str, guests: GuestLimits) -> (axum::Router, CancellationToken, tempfile::TempDir) {
    let (state, shutdown, data) = state_with_guests(name, &[("AUTH_TOKEN", TOKEN)], guests).await;
    (router(state), shutdown, data)
}

/// A JSON request with only the guest cookie (if any): no token.
fn as_guest(method: &str, uri: &str, cookie: &str, body: &str) -> Request<Body> {
    let mut request = Request::builder().method(method).uri(uri);
    if !cookie.is_empty() {
        request = request.header(header::COOKIE, cookie);
    }
    if method != "GET" {
        request = request.header(header::CONTENT_TYPE, "application/json");
    }
    request.body(Body::from(body.to_owned())).unwrap()
}

/// Asks for a guest seat; the answer, its JSON and the `name=value` cookie it set, if any.
async fn ask_for_a_seat(app: &axum::Router, body: &str) -> (StatusCode, serde_json::Value, String) {
    let response = app.clone().oneshot(as_guest("POST", "/auth/guest", "", body)).await.unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned())
        .unwrap_or_default();
    let json = serde_json::from_str(&text(response).await).unwrap_or_default();
    (status, json, cookie)
}

#[tokio::test]
async fn guests_watch_and_chat_without_signing_in_but_do_not_run_the_world() {
    let (app, _, _data) = app("guest").await;
    assert_eq!(app.clone().oneshot(get("/api/stream")).await.unwrap().status(), StatusCode::UNAUTHORIZED);

    let response = app.clone().oneshot(as_guest("POST", "/auth/guest", "", "{}")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let set_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap().to_owned();
    assert!(set_cookie.starts_with("guest="), "{set_cookie}");
    assert!(set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Lax") && set_cookie.contains("Max-Age="));
    assert_eq!(text(response).await, r#"{"kind":"admitted"}"#);
    let cookie = set_cookie.split(';').next().unwrap().to_owned();

    let me = app.clone().oneshot(as_guest("GET", "/api/me", &cookie, "")).await.unwrap();
    assert_eq!(text(me).await, r#"{"role":"guest"}"#);

    let stream = app.clone().oneshot(as_guest("GET", "/api/stream?viewer=tabG", &cookie, "")).await.unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    let mut body = stream.into_body();
    let seen = read_events(&mut body, |seen| seen.contains("event: chat_welcome")).await;
    assert!(seen.starts_with("event: hello\n") && seen.contains(r#""role":"guest"}"#), "{seen}");
    let name = last_event(&seen, "chat_welcome")["you"].clone();
    assert!(name.is_string(), "a guest has a chat name: {seen}");
    let character = app.clone().oneshot(as_guest("GET", "/api/characters/c1", &cookie, "")).await.unwrap();
    assert_eq!(character.status(), StatusCode::OK);
    let site = app.clone().oneshot(as_guest("GET", "/api/sites/b:tavern", &cookie, "")).await.unwrap();
    assert_eq!(site.status(), StatusCode::OK);
    let monster = app.clone().oneshot(as_guest("GET", "/api/monsters/m1", &cookie, "")).await.unwrap();
    assert_eq!(monster.status(), StatusCode::NOT_FOUND, "none abroad yet, but the route is open to guests");
    let ghost = app.clone().oneshot(as_guest("GET", "/api/ghosts/g1", &cookie, "")).await.unwrap();
    assert_eq!(ghost.status(), StatusCode::NOT_FOUND, "no ghost walks yet, but the route is open to guests");
    let watch = as_guest("POST", "/api/watch", &cookie, r#"{"viewer":"tabG","id":"c1"}"#);
    assert_eq!(app.clone().oneshot(watch).await.unwrap().status(), StatusCode::NO_CONTENT);

    // Guests talk, and pick their name.
    let post = as_guest("POST", "/api/chat", &cookie, r#"{"viewer":"tabG","text":"hello"}"#);
    assert_eq!(app.clone().oneshot(post).await.unwrap().status(), StatusCode::NO_CONTENT);
    let rename = as_guest("POST", "/api/chat/name", &cookie, r#"{"viewer":"tabG","name":"Sneaky"}"#);
    let renamed = app.clone().oneshot(rename).await.unwrap();
    assert_eq!(renamed.status(), StatusCode::OK);
    assert_eq!(text(renamed).await, r#"{"name":"Sneaky"}"#);
    // A second tab of the same browser shares the pass, and so the name.
    let mut second =
        app.clone().oneshot(as_guest("GET", "/api/stream?viewer=tabH", &cookie, "")).await.unwrap().into_body();
    let seen = read_events(&mut second, |seen| seen.contains("event: chat_welcome")).await;
    assert_eq!(last_event(&seen, "chat_welcome")["you"], "Sneaky");
    // Another browser is someone else.
    let (_, _, other) = ask_for_a_seat(&app, "{}").await;
    let mut other_tab =
        app.clone().oneshot(as_guest("GET", "/api/stream?viewer=tabX", &other, "")).await.unwrap().into_body();
    let seen = read_events(&mut other_tab, |seen| seen.contains("event: chat_welcome")).await;
    assert_ne!(last_event(&seen, "chat_welcome")["you"], "Sneaky");

    // But running the world is for admins.
    for (method, uri, body) in [
        ("GET", "/api/admin/health", ""),
        ("POST", "/api/admin/speed", r#"{"speed":0}"#),
        ("POST", "/api/admin/raid", "{}"),
        ("POST", "/api/admin/dragon", "{}"),
    ] {
        let response = app.clone().oneshot(as_guest(method, uri, &cookie, body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {uri}");
        assert_eq!(text(response).await, r#"{"error":"admins only"}"#);
    }

    // Leaving ends the stream and the pass, and clears the cookie.
    let logout = app.clone().oneshot(as_guest("POST", "/api/logout", &cookie, "{}")).await.unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(logout.headers()[header::SET_COOKIE].to_str().unwrap().starts_with("guest=; "));
    let seen = read_events(&mut second, |seen| seen.contains("event: guest_end")).await;
    assert_eq!(last_event(&seen, "guest_end")["reason"], "left");
    let me = app.clone().oneshot(as_guest("GET", "/api/me", &cookie, "")).await.unwrap();
    assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn guests_wait_in_line_when_the_seats_are_full() {
    let (app, _, _data) = guest_app("guest-line", GuestLimits { seats: 1 }).await;
    let (status, _, cookie) = ask_for_a_seat(&app, "{}").await;
    assert_eq!(status, StatusCode::OK);
    let _seated = app.clone().oneshot(as_guest("GET", "/api/stream?viewer=t1", &cookie, "")).await.unwrap();

    let (status, answer, cookie) = ask_for_a_seat(&app, "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert!(cookie.is_empty(), "no pass while in line");
    assert_eq!((answer["kind"].as_str(), answer["position"].as_u64()), (Some("queued"), Some(1)));
    assert_eq!((answer["seats"].as_u64(), answer["retryAfterSeconds"].as_u64()), (Some(1), Some(60)));
    let ticket = answer["ticket"].as_str().unwrap().to_owned();

    // Asking again straight away is refused, without losing the place.
    let again = format!(r#"{{"ticket":"{ticket}"}}"#);
    let response = app.clone().oneshot(as_guest("POST", "/auth/guest", "", &again)).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let wait: u64 = response.headers()[header::RETRY_AFTER].to_str().unwrap().parse().unwrap();
    assert!((50..=60).contains(&wait), "{wait}");
    let (_, answer, _) = ask_for_a_seat(&app, "{}").await;
    assert_eq!(answer["position"], 2);

    // The cross-site rule applies here too.
    let foreign = Request::post("/auth/guest")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "https://evil.example")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(app.clone().oneshot(foreign).await.unwrap().status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_newer_tab_takes_the_guest_stream_over() {
    let (app, _, _data) = app("guest-tabs").await;
    let (_, _, cookie) = ask_for_a_seat(&app, "{}").await;
    let mut first =
        app.clone().oneshot(as_guest("GET", "/api/stream?viewer=t1", &cookie, "")).await.unwrap().into_body();
    read_events(&mut first, |seen| seen.contains("event: chat_welcome")).await;
    let mut second =
        app.clone().oneshot(as_guest("GET", "/api/stream?viewer=t2", &cookie, "")).await.unwrap().into_body();
    let seen = read_events(&mut first, |seen| seen.contains("event: guest_end")).await;
    assert_eq!(last_event(&seen, "guest_end")["reason"], "elsewhere");
    let seen = read_events(&mut second, |seen| seen.matches("event: tick").count() >= 2).await;
    assert!(!seen.contains("guest_end"), "{seen}");
}

#[tokio::test]
async fn signing_in_wins_over_a_guest_pass() {
    let (app, _, _data) = app("guest-token").await;
    let (_, _, cookie) = ask_for_a_seat(&app, "{}").await;
    let me = Request::get("/api/me")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    assert!(text(app.oneshot(me).await.unwrap()).await.contains(r#""role":"admin""#));
}

#[tokio::test]
async fn without_a_cap_everyone_is_seated_and_with_sign_in_off_nobody_needs_a_seat() {
    let (app, _, _data) = guest_app("guest-uncapped", GuestLimits { seats: 0 }).await;
    for _ in 0..3 {
        let (status, answer, cookie) = ask_for_a_seat(&app, "{}").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer["kind"], "admitted");
        assert!(cookie.starts_with("guest="), "{cookie}");
    }

    let (app, _, _data) = app_with("guest-off", &[("AUTH_MODE", "off")]).await;
    assert_eq!(ask_for_a_seat(&app, "{}").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admins_call_a_raid_up_the_road_once() {
    let (state, shutdown, _data) = state_with("raid", &[("AUTH_TOKEN", TOKEN)]).await;
    let app = router(state.clone());
    let call = || json_request("POST", "/api/admin/raid", "{}");
    let form = Request::post("/api/admin/raid")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("now=1"))
        .unwrap();
    assert_eq!(app.clone().oneshot(form).await.unwrap().status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Not while the village is still being settled.
    state.world.core.write().sim.state.director.founding = true;
    let settling = app.clone().oneshot(call()).await.unwrap();
    assert_eq!(settling.status(), StatusCode::CONFLICT);
    assert!(text(settling).await.contains("still being settled"));
    state.world.core.write().sim.state.director.founding = false;

    let called = app.clone().oneshot(call()).await.unwrap();
    assert_eq!(called.status(), StatusCode::ACCEPTED);
    let answer = text(called).await;
    assert!(answer.contains("The orcs"), "{answer}");

    // The call reaches the sim as a command at a tick boundary.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        {
            let core = state.world.core.read();
            let state = &core.sim.state;
            if state.director.raid_called || state.war_band.is_some() {
                break;
            }
        }
        assert!(tokio::time::Instant::now() < deadline, "the call never reached the world");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let again = app.clone().oneshot(call()).await.unwrap();
    assert_eq!(again.status(), StatusCode::CONFLICT, "one raid at a time");
    shutdown.cancel();
}

#[tokio::test]
async fn admins_call_the_dragon_over_the_hills_once() {
    let (state, shutdown, _data) = state_with("dragon", &[("AUTH_TOKEN", TOKEN)]).await;
    let app = router(state.clone());
    let call = || json_request("POST", "/api/admin/dragon", "{}");

    // Not while the village is still being settled.
    state.world.core.write().sim.state.director.founding = true;
    let settling = app.clone().oneshot(call()).await.unwrap();
    assert_eq!(settling.status(), StatusCode::CONFLICT);
    assert!(text(settling).await.contains("still being settled"));
    state.world.core.write().sim.state.director.founding = false;

    let called = app.clone().oneshot(call()).await.unwrap();
    assert_eq!(called.status(), StatusCode::ACCEPTED);
    let answer = text(called).await;
    assert!(answer.contains("The dragon"), "{answer}");

    // The call reaches the sim as a command at a tick boundary.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        {
            let core = state.world.core.read();
            let state = &core.sim.state;
            if state.director.dragon_called || fsw_sim::systems::dragon::dragon(state).is_some() {
                break;
            }
        }
        assert!(tokio::time::Instant::now() < deadline, "the call never reached the world");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let again = app.clone().oneshot(call()).await.unwrap();
    assert_eq!(again.status(), StatusCode::CONFLICT, "one dragon at a time");
    shutdown.cancel();
}
