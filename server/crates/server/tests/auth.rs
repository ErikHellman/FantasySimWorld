//! Sign-in end to end against fake providers: an axum server on 127.0.0.1
//! serving discovery documents for "Google", "Microsoft" and "LinkedIn", a
//! JWKS, token endpoints for those and "Apple", "GitHub" and "Discord", and the
//! last two's profile APIs, with keys generated here. The app under test is
//! driven without a socket; its HTTP client talks to the fake over the loopback.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
use axum::Form;
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use fsw_server::config::{ApiEndpoints, AppleEndpoints, AuthMode, Config, ProviderConfig};
use fsw_server::store::Store;
use fsw_server::{AppState, brain_task, router, world_task};
use http_body_util::BodyExt;
use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const ORIGIN: &str = "https://world.example.com";
const GOOGLE_CLIENT: &str = "google-client.apps.example";
const APPLE_SERVICES_ID: &str = "com.example.world.web";
const APPLE_TEAM: &str = "TEAM123456";
const APPLE_KEY_ID: &str = "KEY1234567";
const ADMIN: &str = "admin@example.com";
const GOOGLE_ISSUER: &str = "https://accounts.google.com";
const APPLE_ISSUER: &str = "https://appleid.apple.com";
const MICROSOFT_CLIENT: &str = "00000000-aaaa-bbbb-cccc-000000000001";
/// What Microsoft's multi-tenant discovery document names as the issuer.
const MICROSOFT_ISSUER: &str = "https://login.microsoftonline.com/{tenantid}/v2.0";
const MICROSOFT_PERSONAL: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";
const MICROSOFT_WORK: &str = "72f988bf-0000-0000-0000-000000000000";
const LINKEDIN_CLIENT: &str = "86linkedin";
const LINKEDIN_ISSUER: &str = "https://www.linkedin.com/oauth";
const GITHUB_CLIENT: &str = "Iv1.github";
const DISCORD_CLIENT: &str = "1234567890";

// ---------------------------------------------------------------------------
// The fake provider
// ---------------------------------------------------------------------------

struct Grant {
    /// the id_token's claims; `None` for providers without one (GitHub, Discord)
    claims: Option<Value>,
    /// `None` when the authorize URL had no PKCE challenge (LinkedIn)
    challenge: Option<String>,
    /// what the profile API answers for this grant's access token (GitHub, Discord)
    profile: Value,
    client_id: String,
    redirect_uri: String,
}

struct Fake {
    signing: EncodingKey,
    jwks: JwkSet,
    grants: Mutex<HashMap<String, Grant>>,
    /// form fields of every token request
    token_requests: Mutex<Vec<HashMap<String, String>>>,
    /// profile API answers by access token
    profiles: Mutex<HashMap<String, Value>>,
    base: String,
}

fn p256_pkcs8() -> Vec<u8> {
    EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new()).unwrap().as_ref().to_vec()
}

fn pem(der: &[u8]) -> String {
    let body = STANDARD.encode(der);
    let lines: Vec<&str> = body.as_bytes().chunks(64).map(|c| std::str::from_utf8(c).unwrap()).collect();
    format!("-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n", lines.join("\n"))
}

impl Fake {
    async fn start() -> Arc<Self> {
        let signing = EncodingKey::from_ec_der(&p256_pkcs8());
        let mut jwk = Jwk::from_encoding_key(&signing, Algorithm::ES256).unwrap();
        jwk.common.key_id = Some("fake-key-1".into());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fake = Arc::new(Self {
            signing,
            jwks: JwkSet { keys: vec![jwk] },
            grants: Mutex::default(),
            token_requests: Mutex::default(),
            profiles: Mutex::default(),
            base,
        });
        let routes = axum::Router::new()
            .route("/{provider}/.well-known/openid-configuration", get(discovery))
            .route("/jwks", get(|State(fake): State<Arc<Fake>>| async move { Json(fake.jwks.clone()) }))
            .route("/{provider}/token", post(token))
            .route("/github/user", get(github_user))
            .route("/github/user/emails", get(github_emails))
            .route("/discord/users/@me", get(discord_me))
            .with_state(fake.clone());
        tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
        fake
    }

    /// Hands out `code`: the next token request with it gets an id_token with these claims (if any),
    /// and an access token the profile API answers with `profile`.
    fn grant(&self, code: &str, claims: Option<Value>, profile: Value, authorize_url: &reqwest::Url) {
        let param = |name: &str| authorize_url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
        let challenge = param("code_challenge");
        if challenge.is_some() {
            assert_eq!(param("code_challenge_method").as_deref(), Some("S256"));
        }
        let grant = Grant {
            claims,
            challenge,
            profile,
            client_id: param("client_id").unwrap(),
            redirect_uri: param("redirect_uri").unwrap(),
        };
        self.grants.lock().unwrap().insert(code.into(), grant);
    }

    fn last_token_request(&self) -> HashMap<String, String> {
        self.token_requests.lock().unwrap().last().cloned().unwrap()
    }
}

async fn discovery(State(fake): State<Arc<Fake>>, Path(provider): Path<String>) -> Json<Value> {
    let issuer = match provider.as_str() {
        "google" => GOOGLE_ISSUER,
        "microsoft" => MICROSOFT_ISSUER,
        "linkedin" => LINKEDIN_ISSUER,
        other => panic!("no discovery for {other}"),
    };
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{}/{provider}/authorize", fake.base),
        "token_endpoint": format!("{}/{provider}/token", fake.base),
        "jwks_uri": format!("{}/jwks", fake.base),
    }))
}

async fn token(State(fake): State<Arc<Fake>>, Form(form): Form<HashMap<String, String>>) -> Response {
    fake.token_requests.lock().unwrap().push(form.clone());
    let refuse = || (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant"}))).into_response();
    let Some(grant) = form.get("code").and_then(|code| fake.grants.lock().unwrap().remove(code)) else {
        return refuse();
    };
    // Without a challenge at the start, a verifier now is as wrong as a missing one.
    let pkce_ok = match (&grant.challenge, form.get("code_verifier")) {
        (Some(challenge), Some(verifier)) => {
            let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes());
            &URL_SAFE_NO_PAD.encode(digest) == challenge
        }
        (None, None) => true,
        _ => false,
    };
    if form.get("grant_type").map(String::as_str) != Some("authorization_code")
        || form.get("client_id") != Some(&grant.client_id)
        || form.get("redirect_uri") != Some(&grant.redirect_uri)
        || !pkce_ok
        || form.get("client_secret").is_none_or(String::is_empty)
    {
        return refuse();
    }
    let access_token = format!("at-{}", session_safe(&form["code"]));
    fake.profiles.lock().unwrap().insert(access_token.clone(), grant.profile);
    let mut answer = json!({"access_token": access_token, "token_type": "Bearer", "expires_in": 3600});
    if let Some(claims) = grant.claims {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some("fake-key-1".into());
        answer["id_token"] = json!(jsonwebtoken::encode(&header, &claims, &fake.signing).unwrap());
    }
    Json(answer).into_response()
}

fn session_safe(code: &str) -> String {
    code.chars().filter(char::is_ascii_alphanumeric).collect()
}

/// The profile behind the request's bearer token.
fn profile(fake: &Fake, headers: &HeaderMap) -> Result<Value, StatusCode> {
    let token =
        headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    token.and_then(|t| fake.profiles.lock().unwrap().get(t).cloned()).ok_or(StatusCode::UNAUTHORIZED)
}

fn github_profile(fake: &Fake, headers: &HeaderMap) -> Result<Value, StatusCode> {
    // GitHub's API wants its media type; check that we send it.
    if headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) != Some("application/vnd.github+json") {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    profile(fake, headers)
}

async fn github_user(State(fake): State<Arc<Fake>>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    Ok(Json(github_profile(&fake, &headers)?["user"].clone()))
}

async fn github_emails(State(fake): State<Arc<Fake>>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    Ok(Json(github_profile(&fake, &headers)?["emails"].clone()))
}

async fn discord_me(State(fake): State<Arc<Fake>>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    Ok(Json(profile(&fake, &headers)?))
}

// ---------------------------------------------------------------------------
// The app under test
// ---------------------------------------------------------------------------

struct TestApp {
    router: axum::Router,
    store: Store,
    fake: Arc<Fake>,
    apple_key: Vec<u8>,
    _data: tempfile::TempDir,
}

/// Every provider on.
async fn app() -> TestApp {
    app_with("google,apple,microsoft,linkedin,github,discord").await
}

async fn app_with(providers: &str) -> TestApp {
    let fake = Fake::start().await;
    let apple_key = p256_pkcs8();
    let data = tempfile::tempdir().unwrap();
    let vars = [
        ("AUTH_MODE", "oidc".to_string()),
        ("AUTH_PROVIDERS", providers.into()),
        ("GOOGLE_CLIENT_ID", GOOGLE_CLIENT.into()),
        ("GOOGLE_CLIENT_SECRET", "google-secret".into()),
        ("MICROSOFT_CLIENT_ID", MICROSOFT_CLIENT.into()),
        ("MICROSOFT_CLIENT_SECRET", "microsoft-secret".into()),
        ("LINKEDIN_CLIENT_ID", LINKEDIN_CLIENT.into()),
        ("LINKEDIN_CLIENT_SECRET", "linkedin-secret".into()),
        ("GITHUB_CLIENT_ID", GITHUB_CLIENT.into()),
        ("GITHUB_CLIENT_SECRET", "github-secret".into()),
        ("DISCORD_CLIENT_ID", DISCORD_CLIENT.into()),
        ("DISCORD_CLIENT_SECRET", "discord-secret".into()),
        ("APPLE_TEAM_ID", APPLE_TEAM.into()),
        ("APPLE_KEY_ID", APPLE_KEY_ID.into()),
        ("APPLE_SERVICES_ID", APPLE_SERVICES_ID.into()),
        // As some platforms store multi-line secrets: with "\n" written out.
        ("APPLE_PRIVATE_KEY", pem(&apple_key).replace('\n', "\\n")),
        ("ADMIN_EMAILS", ADMIN.into()),
        ("PUBLIC_ORIGIN", ORIGIN.into()),
        ("DATA_DIR", data.path().display().to_string()),
        ("STATIC_DIR", data.path().join("no-web-build").display().to_string()),
    ];
    let mut config = Config::from_vars(vars.into_iter().map(|(k, v)| (k.to_string(), v))).unwrap();
    let AuthMode::Oidc(oidc) = &mut config.auth else { panic!("oidc mode") };
    let base = &fake.base;
    let discovery = |name: &str| format!("{base}/{name}/.well-known/openid-configuration");
    let api = |name: &str| ApiEndpoints {
        authorize_url: format!("{base}/{name}/authorize"),
        token_url: format!("{base}/{name}/token"),
        api_url: format!("{base}/{name}"),
    };
    for provider in &mut oidc.providers {
        match provider {
            ProviderConfig::Google(c) => c.discovery_url = discovery("google"),
            ProviderConfig::Microsoft(c) => c.discovery_url = discovery("microsoft"),
            ProviderConfig::LinkedIn(c) => c.discovery_url = discovery("linkedin"),
            ProviderConfig::GitHub(c) => c.endpoints = api("github"),
            ProviderConfig::Discord(c) => c.endpoints = api("discord"),
            ProviderConfig::Apple(c) => {
                c.endpoints = AppleEndpoints {
                    issuer: APPLE_ISSUER.into(),
                    authorize_url: format!("{base}/apple/authorize"),
                    token_url: format!("{base}/apple/token"),
                    jwks_url: format!("{base}/jwks"),
                }
            }
        }
    }
    let store = Store::open(&config.data_dir).await.unwrap();
    store.seed_admins(&config.admin_emails).await.unwrap();
    let shutdown = CancellationToken::new();
    let (world, _task) = world_task::start(store.clone(), 1, shutdown.clone()).await.unwrap();
    let brain = brain_task::start(world.clone(), store.clone(), config.llm.clone(), config.llm_api_key.clone()).await;
    let state = AppState::new(config, world, store.clone(), brain, shutdown).unwrap();
    TestApp { router: router(state), store, fake, apple_key, _data: data }
}

fn now() -> u64 {
    jsonwebtoken::get_current_timestamp()
}

/// The `name=value` part of a `Set-Cookie` with this name, if the response sets it.
fn set_cookie(response: &Response, name: &str) -> Option<String> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .find(|c| c.starts_with(&format!("{name}=")))
}

fn pair(set_cookie: &str) -> String {
    set_cookie.split(';').next().unwrap().to_owned()
}

fn location(response: &Response) -> String {
    response.headers()[header::LOCATION].to_str().unwrap().to_owned()
}

async fn text(response: Response) -> String {
    String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
}

/// How a test sign-in should go.
struct SignIn<'a> {
    provider: &'a str,
    sub: &'a str,
    email: &'a str,
    email_verified: Value,
    /// replace the nonce in the id_token
    nonce: Option<&'a str>,
    /// replace the state on the callback
    state: Option<&'a str>,
    aud: Option<&'a str>,
    /// replace the issuer in the id_token
    iss: Option<&'a str>,
    /// leave the nonce out of the id_token
    no_nonce: bool,
    /// Microsoft: the tenant (`tid`) and `xms_edov`
    tid: Option<&'a str>,
    xms_edov: Option<bool>,
}

impl<'a> SignIn<'a> {
    fn google(sub: &'a str, email: &'a str) -> Self {
        Self {
            provider: "google",
            sub,
            email,
            email_verified: json!(true),
            nonce: None,
            state: None,
            aud: None,
            iss: None,
            no_nonce: false,
            tid: None,
            xms_edov: None,
        }
    }

    fn apple(sub: &'a str, email: &'a str) -> Self {
        // Apple sends email_verified as a string.
        Self { provider: "apple", email_verified: json!("true"), ..Self::google(sub, email) }
    }

    /// A personal Microsoft account; Microsoft sends no `email_verified`.
    fn microsoft(sub: &'a str, email: &'a str) -> Self {
        Self {
            provider: "microsoft",
            email_verified: Value::Null,
            tid: Some(MICROSOFT_PERSONAL),
            ..Self::google(sub, email)
        }
    }

    fn linkedin(sub: &'a str, email: &'a str) -> Self {
        Self { provider: "linkedin", ..Self::google(sub, email) }
    }

    /// GitHub and Discord: `sub` is the account id, `email_verified` what the profile API says.
    fn github(sub: &'a str, email: &'a str) -> Self {
        Self { provider: "github", ..Self::google(sub, email) }
    }

    fn discord(sub: &'a str, email: &'a str) -> Self {
        Self { provider: "discord", ..Self::google(sub, email) }
    }

    /// What the profile API answers (GitHub, Discord).
    fn profile(&self) -> Value {
        let verified = self.email_verified == json!(true);
        match self.provider {
            "github" => json!({
                "user": {"id": self.sub.parse::<u64>().unwrap(), "login": "octocat", "name": null},
                "emails": [
                    {"email": "old@example.com", "primary": false, "verified": true, "visibility": null},
                    {"email": self.email, "primary": true, "verified": verified, "visibility": "private"},
                ],
            }),
            "discord" => json!({
                "id": self.sub,
                "username": "dusk",
                "global_name": "Dusk Rider",
                "email": self.email,
                "verified": verified,
            }),
            _ => Value::Null,
        }
    }
}

impl TestApp {
    async fn send(&self, request: Request<Body>) -> Response {
        self.router.clone().oneshot(request).await.unwrap()
    }

    /// Runs the whole flow: start, the provider's part, the callback. Returns the callback's response.
    async fn sign_in(&self, how: SignIn<'_>) -> Response {
        let start = self
            .send(Request::get(format!("/auth/{}/start?return_to=/world", how.provider)).body(Body::empty()).unwrap())
            .await;
        assert_eq!(start.status(), StatusCode::SEE_OTHER);
        let flow_cookie = set_cookie(&start, "__Secure-auth_flow").expect("flow cookie");
        assert!(flow_cookie.ends_with("; Path=/auth/; Secure; HttpOnly; SameSite=None; Max-Age=600"), "{flow_cookie}");
        let authorize = reqwest::Url::parse(&location(&start)).unwrap();
        let param = |name: &str| authorize.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
        assert_eq!(param("redirect_uri").unwrap(), format!("{ORIGIN}/auth/{}/callback", how.provider));
        let microsoft_issuer = MICROSOFT_ISSUER.replace("{tenantid}", how.tid.unwrap_or_default());
        let (client_id, issuer) = match how.provider {
            "google" => (GOOGLE_CLIENT, GOOGLE_ISSUER),
            "apple" => (APPLE_SERVICES_ID, APPLE_ISSUER),
            "microsoft" => (MICROSOFT_CLIENT, microsoft_issuer.as_str()),
            "linkedin" => (LINKEDIN_CLIENT, LINKEDIN_ISSUER),
            "github" => (GITHUB_CLIENT, ""),
            "discord" => (DISCORD_CLIENT, ""),
            other => panic!("no provider {other}"),
        };
        assert_eq!(param("client_id").unwrap(), client_id);

        let code = format!("code-{}", how.sub);
        let claims = (!issuer.is_empty()).then(|| {
            let mut claims = json!({
                "iss": how.iss.unwrap_or(issuer),
                "aud": how.aud.unwrap_or(client_id),
                "sub": how.sub,
                "email": how.email,
                "email_verified": how.email_verified,
                "nonce": how.nonce.map_or_else(|| param("nonce").unwrap(), str::to_owned),
                "iat": now(),
                "exp": now() + 600,
            });
            if how.no_nonce {
                claims.as_object_mut().unwrap().remove("nonce");
            }
            if let Some(tid) = how.tid {
                claims["tid"] = json!(tid);
            }
            if let Some(edov) = how.xms_edov {
                claims["xms_edov"] = json!(edov);
            }
            claims
        });
        self.fake.grant(&code, claims, how.profile(), &authorize);

        let state = how.state.map_or_else(|| param("state").unwrap(), str::to_owned);
        let request = match how.provider {
            "apple" => {
                let user =
                    json!({"name": {"firstName": "Grace", "lastName": "Hopper"}, "email": how.email}).to_string();
                let user: String = url_encode(&user);
                // A cross-site POST from Apple's page, as response_mode=form_post makes it.
                Request::post("/auth/apple/callback")
                    .header(header::COOKIE, pair(&flow_cookie))
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::ORIGIN, APPLE_ISSUER)
                    .header("sec-fetch-site", "cross-site")
                    .body(Body::from(format!("code={code}&state={state}&user={user}")))
                    .unwrap()
            }
            provider => Request::get(format!("/auth/{provider}/callback?code={code}&state={state}"))
                .header(header::COOKIE, pair(&flow_cookie))
                .body(Body::empty())
                .unwrap(),
        };
        self.send(request).await
    }

    async fn session_for(&self, how: SignIn<'_>) -> String {
        let response = self.sign_in(how).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{}", text(response).await);
        assert_eq!(location(&response), "/world");
        pair(&set_cookie(&response, "__Host-session").expect("session cookie"))
    }

    async fn get(&self, uri: &str, cookie: &str) -> Response {
        self.send(Request::get(uri).header(header::COOKIE, cookie).body(Body::empty()).unwrap()).await
    }

    /// A request from our own page: same origin, JSON.
    async fn mutate(&self, method: &str, uri: &str, cookie: &str, body: Value) -> Response {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .body(Body::from(body.to_string()))
            .unwrap();
        self.send(request).await
    }

    async fn me(&self, cookie: &str) -> (StatusCode, Value) {
        let response = self.get("/api/me", cookie).await;
        let status = response.status();
        (status, serde_json::from_str(&text(response).await).unwrap())
    }
}

fn url_encode(text: &str) -> String {
    reqwest::Url::parse_with_params("http://x/", [("v", text)]).unwrap().query().unwrap()[2..].to_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn allowlisted_google_sign_in_creates_a_user_and_a_session() {
    let app = app().await;
    let response = app.sign_in(SignIn::google("g-admin", "Admin@Example.com")).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), "/world");
    let session = set_cookie(&response, "__Host-session").unwrap();
    assert!(session.ends_with("; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000; Secure"), "{session}");
    // The flow cookie is cleared.
    assert!(set_cookie(&response, "__Secure-auth_flow").unwrap().contains("Max-Age=0"));

    let (status, me) = app.me(&pair(&session)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me, json!({"email": ADMIN, "displayName": "admin", "role": "admin"}));
    let user = app.store.user_by_email(ADMIN).await.unwrap().unwrap();
    assert!(user.last_login_at.is_some());

    // The token request used the PKCE verifier and the client secret in the body.
    let request = app.fake.last_token_request();
    assert_eq!(request["client_secret"], "google-secret");

    // Signing in again finds the linked identity, even with another e-mail claim.
    let again = app.session_for(SignIn::google("g-admin", "renamed@example.com")).await;
    assert_eq!(app.me(&again).await.1["email"], ADMIN);
}

#[tokio::test]
async fn unknown_email_is_refused_recorded_and_can_be_allowed() {
    let app = app().await;
    let refused = app.sign_in(SignIn::google("g-stranger", "stranger@example.com")).await;
    assert_eq!(refused.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&refused), "/no-access?email=stranger%40example.com");
    assert!(set_cookie(&refused, "__Host-session").is_none());

    let admin = app.session_for(SignIn::google("g-admin", ADMIN)).await;
    let attempts = app.get("/api/admin/attempts", &admin).await;
    assert_eq!(attempts.status(), StatusCode::OK);
    let attempts: Value = serde_json::from_str(&text(attempts).await).unwrap();
    assert_eq!(attempts[0]["email"], "stranger@example.com");
    assert_eq!(attempts[0]["provider"], "google");
    assert_eq!(attempts[0]["outcome"], "not_allowlisted");

    // The Allow button.
    let allowed = app
        .mutate("POST", "/api/admin/allowlist", &admin, json!({"email": "stranger@example.com", "role": "viewer"}))
        .await;
    assert_eq!(allowed.status(), StatusCode::OK);
    let entry: Value = serde_json::from_str(&text(allowed).await).unwrap();
    assert_eq!(entry["addedBy"], ADMIN);

    let viewer = app.session_for(SignIn::google("g-stranger", "stranger@example.com")).await;
    assert_eq!(app.me(&viewer).await.1["role"], "viewer");
    // Viewers stay out of the admin routes.
    let forbidden = app.get("/api/admin/allowlist", &viewer).await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(text(forbidden).await, r#"{"error":"admins only"}"#);
    let billboard = app
        .mutate("POST", "/api/admin/billboards", &viewer, json!({"id": "green_board", "text": "Viewers rule"}))
        .await;
    assert_eq!(billboard.status(), StatusCode::FORBIDDEN);
    let list: Value = serde_json::from_str(&text(app.get("/api/admin/allowlist", &admin).await).await).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn apple_posts_back_with_a_string_email_verified_and_the_name() {
    let app = app().await;
    let relay = "x7q2@privaterelay.appleid.com";
    app.store.allow(relay, fsw_server::api::wire::Role::Viewer, ADMIN, "Apple relay").await.unwrap();

    let start = app.send(Request::get("/auth/apple/start").body(Body::empty()).unwrap()).await;
    let authorize = reqwest::Url::parse(&location(&start)).unwrap();
    let params: HashMap<_, _> = authorize.query_pairs().into_owned().collect();
    assert_eq!(params["response_mode"], "form_post");
    assert_eq!(params["scope"], "name email");
    assert!(authorize.as_str().starts_with(&format!("{}/apple/authorize?", app.fake.base)));

    let cookie = app.session_for(SignIn::apple("apple-001", relay)).await;
    let (_, me) = app.me(&cookie).await;
    assert_eq!(me, json!({"email": relay, "displayName": "Grace Hopper", "role": "viewer"}));

    // The client secret: an ES256 JWT from the .p8 key, for the Services ID.
    let secret = app.fake.last_token_request()["client_secret"].clone();
    let header = jsonwebtoken::decode_header(&secret).unwrap();
    assert_eq!((header.alg, header.kid.as_deref()), (Algorithm::ES256, Some(APPLE_KEY_ID)));
    let public = Jwk::from_encoding_key(&EncodingKey::from_ec_der(&app.apple_key), Algorithm::ES256).unwrap();
    let mut validation = Validation::new(Algorithm::ES256);
    validation.set_audience(&[APPLE_ISSUER]);
    validation.set_issuer(&[APPLE_TEAM]);
    let claims =
        jsonwebtoken::decode::<Value>(&secret, &DecodingKey::from_jwk(&public).unwrap(), &validation).unwrap().claims;
    assert_eq!(claims["sub"], APPLE_SERVICES_ID);
    let lifetime = claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap();
    assert_eq!(lifetime, 30 * 24 * 3600);
}

#[tokio::test]
async fn bad_state_nonce_or_audience_is_rejected() {
    let app = app().await;
    let bad_state = app.sign_in(SignIn { state: Some("forged"), ..SignIn::google("g-admin", ADMIN) }).await;
    assert_eq!(bad_state.status(), StatusCode::BAD_REQUEST);
    assert!(set_cookie(&bad_state, "__Host-session").is_none());

    let bad_nonce = app.sign_in(SignIn { nonce: Some("replayed"), ..SignIn::google("g-admin", ADMIN) }).await;
    assert_eq!(bad_nonce.status(), StatusCode::BAD_REQUEST);
    assert!(set_cookie(&bad_nonce, "__Host-session").is_none());

    let bad_aud = app.sign_in(SignIn { aud: Some("someone-else"), ..SignIn::google("g-admin", ADMIN) }).await;
    assert_eq!(bad_aud.status(), StatusCode::BAD_REQUEST);

    let unverified = app.sign_in(SignIn { email_verified: json!("false"), ..SignIn::apple("a-1", ADMIN) }).await;
    assert_eq!(location(&unverified), "/no-access?email=admin%40example.com");

    // A callback without its flow cookie, or replaying a finished flow, is refused.
    let no_flow = app.send(Request::get("/auth/google/callback?code=x&state=y").body(Body::empty()).unwrap()).await;
    assert_eq!(no_flow.status(), StatusCode::BAD_REQUEST);
    let start = app.send(Request::get("/auth/google/start").body(Body::empty()).unwrap()).await;
    let flow = pair(&set_cookie(&start, "__Secure-auth_flow").unwrap());
    let state = reqwest::Url::parse(&location(&start))
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    for _ in 0..2 {
        let replay = app.get(&format!("/auth/google/callback?code=unknown&state={state}"), &flow).await;
        assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    }
    // A provider this server does not know does not exist.
    let other = app.send(Request::get("/auth/facebook/start").body(Body::empty()).unwrap()).await;
    assert_eq!(other.status(), StatusCode::NOT_FOUND);
    // The start link only returns to this site.
    let evil = app.send(Request::get("/auth/google/start?return_to=//evil.example").body(Body::empty()).unwrap()).await;
    assert_eq!(evil.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn stream_needs_a_session_and_says_the_viewers_role() {
    let app = app().await;
    let anonymous = app.send(Request::get("/api/stream").body(Body::empty()).unwrap()).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(text(anonymous).await, r#"{"error":"not signed in"}"#);
    let forged = app.get("/api/stream", "__Host-session=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").await;
    assert_eq!(forged.status(), StatusCode::UNAUTHORIZED);
    // The token mode's bearer does nothing here.
    let bearer = app
        .send(Request::get("/api/me").header(header::AUTHORIZATION, "Bearer anything").body(Body::empty()).unwrap())
        .await;
    assert_eq!(bearer.status(), StatusCode::UNAUTHORIZED);

    app.store.allow("viewer@example.com", fsw_server::api::wire::Role::Viewer, ADMIN, "").await.unwrap();
    let viewer = app.session_for(SignIn::google("g-viewer", "viewer@example.com")).await;
    let response = app.get("/api/stream", &viewer).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let frame = tokio::time::timeout(Duration::from_secs(5), body.frame()).await.unwrap().unwrap().unwrap();
    let hello = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(hello.starts_with("event: hello\n"), "{hello}");
    assert!(hello.contains(r#""role":"viewer""#), "{hello}");
}

#[tokio::test]
async fn cross_site_requests_to_admin_routes_are_rejected() {
    let app = app().await;
    let admin = app.session_for(SignIn::google("g-admin", ADMIN)).await;
    // A form on another site posting to us with the admin's cookie.
    let form = Request::post("/api/admin/allowlist")
        .header(header::COOKIE, &admin)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::ORIGIN, "https://evil.example")
        .header("sec-fetch-site", "cross-site")
        .body(Body::from("email=mallory%40evil.example&role=admin"))
        .unwrap();
    let response = app.send(form).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // Even with a JSON body, a foreign origin is refused…
    let json_from_elsewhere = Request::post("/api/admin/allowlist")
        .header(header::COOKIE, &admin)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "https://evil.example")
        .body(Body::from(r#"{"email":"mallory@evil.example","role":"admin"}"#))
        .unwrap();
    assert_eq!(app.send(json_from_elsewhere).await.status(), StatusCode::FORBIDDEN);
    let billboard_from_elsewhere = Request::post("/api/admin/billboards")
        .header(header::COOKIE, &admin)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "https://evil.example")
        .body(Body::from(r#"{"id":"green_board","text":"Buy evil"}"#))
        .unwrap();
    assert_eq!(app.send(billboard_from_elsewhere).await.status(), StatusCode::FORBIDDEN);
    // …and a same-origin form body is not JSON.
    let same_site_form = Request::post("/api/admin/allowlist")
        .header(header::COOKIE, &admin)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .body(Body::from("email=mallory%40evil.example&role=admin"))
        .unwrap();
    assert_eq!(app.send(same_site_form).await.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(app.store.user_by_email("mallory@evil.example").await.unwrap().is_none());
    let list = app.store.allowlist().await.unwrap();
    assert_eq!(list.len(), 1, "nothing was added");
}

#[tokio::test]
async fn removing_from_the_allowlist_revokes_sessions() {
    let app = app().await;
    let admin = app.session_for(SignIn::google("g-admin", ADMIN)).await;
    app.mutate("POST", "/api/admin/allowlist", &admin, json!({"email": "temp@example.com", "role": "viewer"})).await;
    let first = app.session_for(SignIn::google("g-temp", "temp@example.com")).await;
    let second = app.session_for(SignIn::apple("a-temp", "temp@example.com")).await;
    assert_eq!(app.me(&first).await.0, StatusCode::OK);
    assert_eq!(app.me(&second).await.0, StatusCode::OK);

    let removed = app.mutate("DELETE", "/api/admin/allowlist?email=Temp%40example.com", &admin, json!({})).await;
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);
    assert_eq!(app.me(&first).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(app.me(&second).await.0, StatusCode::UNAUTHORIZED);
    // And the linked identity no longer lets them in.
    let again = app.sign_in(SignIn::google("g-temp", "temp@example.com")).await;
    assert_eq!(location(&again), "/no-access?email=temp%40example.com");
    let missing = app.mutate("DELETE", "/api/admin/allowlist?email=temp%40example.com", &admin, json!({})).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn logout_ends_the_session() {
    let app = app().await;
    let admin = app.session_for(SignIn::google("g-admin", ADMIN)).await;
    let response = app.mutate("POST", "/api/logout", &admin, json!({})).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(set_cookie(&response, "__Host-session").unwrap().contains("Max-Age=0"));
    assert_eq!(app.me(&admin).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sign_in_is_rate_limited_per_address() {
    let app = app().await;
    let start = || Request::get("/auth/google/start").body(Body::empty()).unwrap();
    let mut statuses = Vec::new();
    for _ in 0..40 {
        statuses.push(app.send(start()).await.status());
    }
    assert!(statuses[..30].iter().all(|s| *s == StatusCode::SEE_OTHER));
    assert_eq!(statuses[39], StatusCode::TOO_MANY_REQUESTS);
    // The API is not affected.
    assert_eq!(app.send(Request::get("/api/me").body(Body::empty()).unwrap()).await.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_login_page_learns_which_providers_are_on() {
    let app = app().await;
    let response = app.get("/auth/providers", "").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        text(response).await,
        r#"{"open":false,"token":false,"providers":["google","apple","microsoft","linkedin","github","discord"]}"#
    );

    // In the order AUTH_PROVIDERS gives, and only those.
    let app = app_with("github, google").await;
    let response = app.get("/auth/providers", "").await;
    assert_eq!(text(response).await, r#"{"open":false,"token":false,"providers":["github","google"]}"#);
    let off = app.send(Request::get("/auth/discord/start").body(Body::empty()).unwrap()).await;
    assert_eq!(off.status(), StatusCode::NOT_FOUND);
}

/// The latest refused sign-in, as the admin sees it.
async fn last_attempt(app: &TestApp) -> Value {
    let admin = app.session_for(SignIn::google("g-admin", ADMIN)).await;
    let attempts: Value = serde_json::from_str(&text(app.get("/api/admin/attempts", &admin).await).await).unwrap();
    attempts[0].clone()
}

#[tokio::test]
async fn microsoft_vouches_for_personal_accounts_and_domain_verified_tenants_only() {
    let app = app().await;
    let start = app.send(Request::get("/auth/microsoft/start").body(Body::empty()).unwrap()).await;
    let params: HashMap<_, _> = reqwest::Url::parse(&location(&start)).unwrap().query_pairs().into_owned().collect();
    assert_eq!(params["scope"], "openid email profile");

    // A personal account: the issuer template takes the consumer tenant.
    let personal = app.session_for(SignIn::microsoft("ms-personal", ADMIN)).await;
    assert_eq!(app.me(&personal).await.1["role"], "admin");
    assert_eq!(app.fake.last_token_request()["client_secret"], "microsoft-secret");

    let worker = "ada@contoso.example";
    app.store.allow(worker, fsw_server::api::wire::Role::Viewer, ADMIN, "").await.unwrap();
    // A work account whose tenant has not proven it owns the domain: refused (nOAuth).
    let unproven = SignIn { tid: Some(MICROSOFT_WORK), ..SignIn::microsoft("ms-work", worker) };
    let refused = app.sign_in(unproven).await;
    assert_eq!(location(&refused), "/no-access?email=ada%40contoso.example");
    let attempt = last_attempt(&app).await;
    assert_eq!(
        (attempt["provider"].as_str(), attempt["outcome"].as_str()),
        (Some("microsoft"), Some("email_unverified"))
    );
    // `email_verified` from a tenant counts for nothing.
    let claimed =
        SignIn { tid: Some(MICROSOFT_WORK), email_verified: json!(true), ..SignIn::microsoft("ms-work", worker) };
    assert_eq!(location(&app.sign_in(claimed).await), "/no-access?email=ada%40contoso.example");

    // With xms_edov the same account gets in.
    let proven = SignIn { tid: Some(MICROSOFT_WORK), xms_edov: Some(true), ..SignIn::microsoft("ms-work", worker) };
    let cookie = app.session_for(proven).await;
    assert_eq!(app.me(&cookie).await.1["role"], "viewer");

    // An issuer from another tenant than the token's `tid` is rejected.
    let other_tenant = "https://login.microsoftonline.com/11111111-2222-3333-4444-555555555555/v2.0";
    let forged = SignIn { iss: Some(other_tenant), ..SignIn::microsoft("ms-personal", ADMIN) };
    assert_eq!(app.sign_in(forged).await.status(), StatusCode::BAD_REQUEST);
    let no_nonce = SignIn { no_nonce: true, ..SignIn::microsoft("ms-personal", ADMIN) };
    assert_eq!(app.sign_in(no_nonce).await.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn linkedin_goes_without_pkce_and_may_leave_the_nonce_out() {
    let app = app().await;
    let start = app.send(Request::get("/auth/linkedin/start").body(Body::empty()).unwrap()).await;
    let params: HashMap<_, _> = reqwest::Url::parse(&location(&start)).unwrap().query_pairs().into_owned().collect();
    assert!(!params.contains_key("code_challenge"), "LinkedIn web apps get no PKCE");
    assert!(params.contains_key("nonce"));

    // The fake refuses a verifier without a challenge, so getting in shows none was sent.
    let cookie = app.session_for(SignIn { no_nonce: true, ..SignIn::linkedin("li-admin", ADMIN) }).await;
    assert_eq!(app.me(&cookie).await.1["email"], ADMIN);
    assert!(!app.fake.last_token_request().contains_key("code_verifier"));
    // The issuer LinkedIn's documentation names is accepted too.
    let documented = SignIn { iss: Some("https://www.linkedin.com"), ..SignIn::linkedin("li-admin", ADMIN) };
    app.session_for(documented).await;

    // A nonce that is there must match.
    let replayed = app.sign_in(SignIn { nonce: Some("replayed"), ..SignIn::linkedin("li-admin", ADMIN) }).await;
    assert_eq!(replayed.status(), StatusCode::BAD_REQUEST);
    let unverified = app.sign_in(SignIn { email_verified: json!(false), ..SignIn::linkedin("li-2", ADMIN) }).await;
    assert_eq!(location(&unverified), "/no-access?email=admin%40example.com");
}

#[tokio::test]
async fn github_signs_in_with_the_verified_primary_address() {
    let app = app().await;
    let start = app.send(Request::get("/auth/github/start").body(Body::empty()).unwrap()).await;
    let params: HashMap<_, _> = reqwest::Url::parse(&location(&start)).unwrap().query_pairs().into_owned().collect();
    assert_eq!(params["scope"], "read:user user:email");
    assert!(params.contains_key("code_challenge"));
    assert!(!params.contains_key("nonce"));

    let cookie = app.session_for(SignIn::github("583231", ADMIN)).await;
    // No name on the account: the login stands in.
    assert_eq!(app.me(&cookie).await.1, json!({"email": ADMIN, "displayName": "octocat", "role": "admin"}));
    let request = app.fake.last_token_request();
    assert_eq!(request["client_secret"], "github-secret");
    assert!(request.contains_key("code_verifier"));

    // An unverified primary address is refused, even with a verified secondary one.
    let unverified = app.sign_in(SignIn { email_verified: json!(false), ..SignIn::github("777", ADMIN) }).await;
    assert_eq!(location(&unverified), "/no-access?email=admin%40example.com");
    let attempt = last_attempt(&app).await;
    assert_eq!((attempt["provider"].as_str(), attempt["outcome"].as_str()), (Some("github"), Some("email_unverified")));
}

#[tokio::test]
async fn discord_needs_a_verified_address() {
    let app = app().await;
    let guest = "dusk@example.com";
    app.store.allow(guest, fsw_server::api::wire::Role::Viewer, ADMIN, "").await.unwrap();
    let cookie = app.session_for(SignIn::discord("80351110224678912", guest)).await;
    assert_eq!(app.me(&cookie).await.1, json!({"email": guest, "displayName": "Dusk Rider", "role": "viewer"}));
    assert_eq!(app.fake.last_token_request()["client_secret"], "discord-secret");

    let unverified = app.sign_in(SignIn { email_verified: json!(false), ..SignIn::discord("999", guest) }).await;
    assert_eq!(location(&unverified), "/no-access?email=dusk%40example.com");
}

/// Reads `body` until a `chat_welcome` arrives and returns its JSON.
async fn chat_welcome(body: &mut Body) -> Value {
    let mut seen = String::new();
    let marker = "event: chat_welcome\ndata: ";
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !seen.contains(marker) || !seen[seen.find(marker).unwrap()..].contains("\n\n") {
        let frame = tokio::time::timeout_at(deadline, body.frame()).await.expect(&seen).unwrap().unwrap();
        if let Ok(data) = frame.into_data() {
            seen.push_str(std::str::from_utf8(&data).unwrap());
        }
    }
    let start = seen.find(marker).unwrap() + marker.len();
    serde_json::from_str(seen[start..].lines().next().unwrap()).unwrap()
}

#[tokio::test]
async fn signed_in_people_each_get_a_chat_name_and_viewers_may_talk() {
    let app = app().await;
    app.store.allow("viewer@example.com", fsw_server::api::wire::Role::Viewer, ADMIN, "").await.unwrap();
    let admin = app.session_for(SignIn::google("g-admin", ADMIN)).await;
    let viewer = app.session_for(SignIn::google("g-viewer", "viewer@example.com")).await;

    let mut admin_tab = app.get("/api/stream?viewer=t1", &admin).await.into_body();
    let mut viewer_tab = app.get("/api/stream?viewer=t2", &viewer).await.into_body();
    // A second tab of the same account shares its name.
    let mut viewer_tab2 = app.get("/api/stream?viewer=t3", &viewer).await.into_body();
    let admin_name = chat_welcome(&mut admin_tab).await["you"].clone();
    let viewer_name = chat_welcome(&mut viewer_tab).await["you"].clone();
    assert_ne!(admin_name, viewer_name);
    assert_eq!(chat_welcome(&mut viewer_tab2).await["you"], viewer_name);

    let post = app.mutate("POST", "/api/chat", &viewer, json!({"viewer": "t2", "text": "hello"})).await;
    assert_eq!(post.status(), StatusCode::NO_CONTENT);
}
