//! `/auth/{provider}/start` and `/auth/{provider}/callback`.
//!
//! Start keeps `{state, nonce, pkce verifier, return_to}` in memory under a
//! random flow id (10 minutes) and puts the id in `__Secure-auth_flow`
//! (`SameSite=None`, because Apple's callback is a cross-site POST). The
//! callback takes the flow (once), checks `state`, exchanges the code and
//! resolves the identity: known identity, then allowlist, else `/no-access`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use oauth2::PkceCodeChallenge;
use serde::Deserialize;

use super::provider::{AuthorizeRequest, Callback, Provider, ProviderKind};
use super::{cookie, safe_return, same, session};
use crate::AppState;
use crate::config::Config;
use crate::store::LoginOutcome;

pub const FLOW_TTL: Duration = Duration::from_secs(600);
/// Flows in progress at most (each is a few hundred bytes).
const MAX_FLOWS: usize = 10_000;

struct Flow {
    provider: ProviderKind,
    state: String,
    nonce: String,
    pkce_verifier: String,
    return_to: String,
    started: Instant,
}

/// Sign-ins in progress, by flow id.
#[derive(Default)]
pub struct Flows {
    flows: Mutex<HashMap<String, Flow>>,
}

impl Flows {
    fn insert(&self, flow: Flow) -> Option<String> {
        let mut flows = self.flows.lock().unwrap_or_else(|e| e.into_inner());
        flows.retain(|_, f| f.started.elapsed() < FLOW_TTL);
        if flows.len() >= MAX_FLOWS {
            return None;
        }
        let id = session::random_token();
        flows.insert(id.clone(), flow);
        Some(id)
    }

    /// Removes and returns a live flow: each one can finish once.
    fn take(&self, id: &str) -> Option<Flow> {
        let flow = self.flows.lock().unwrap_or_else(|e| e.into_inner()).remove(id)?;
        (flow.started.elapsed() < FLOW_TTL).then_some(flow)
    }

    pub fn len(&self) -> usize {
        self.flows.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn flow_cookie_name(config: &Config) -> &'static str {
    if config.secure_cookies() { "__Secure-auth_flow" } else { "auth_flow" }
}

fn flow_cookie(config: &Config, value: &str, max_age: u64) -> HeaderValue {
    // SameSite=None needs Secure; plain-http development falls back to Lax (Google's GET callback still works).
    let attributes = if config.secure_cookies() { "Secure; HttpOnly; SameSite=None" } else { "HttpOnly; SameSite=Lax" };
    let value = format!("{}={value}; Path=/auth/; {attributes}; Max-Age={max_age}", flow_cookie_name(config));
    HeaderValue::from_str(&value).expect("flow ids are base64url")
}

fn redirect_uri(config: &Config, provider: ProviderKind) -> String {
    format!("{}/auth/{}/callback", config.public_origin, provider.as_str())
}

/// A small page for the cases that end the flow without signing in.
fn failed(app: &AppState, status: StatusCode, message: &str) -> Response {
    let body = format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'><title>Sign-in failed</title>\
         <body style='font:16px system-ui;max-width:28rem;margin:4rem auto;padding:0 1rem'>\
         <p>{message}</p><p><a href=/login>Try again</a></p>"
    );
    let mut response = (status, Html(body)).into_response();
    response.headers_mut().append(header::SET_COOKIE, flow_cookie(&app.config, "", 0));
    response
}

#[derive(Deserialize)]
pub struct StartQuery {
    #[serde(default)]
    return_to: Option<String>,
}

/// `GET /auth/{provider}/start?return_to=/path`
pub async fn start(
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(query): Query<StartQuery>,
) -> Response {
    let Some(provider) = ProviderKind::parse(&name).and_then(|kind| app.auth.provider(kind)) else {
        return (StatusCode::NOT_FOUND, "no such sign-in provider").into_response();
    };
    let (state, nonce) = (session::random_token(), session::random_token());
    let (pkce, verifier) = PkceCodeChallenge::new_random_sha256();
    let redirect = redirect_uri(&app.config, provider.kind());
    let request = AuthorizeRequest { redirect_uri: &redirect, state: &state, nonce: &nonce, pkce };
    let url = match provider.authorize_url(&app.auth.http, request).await {
        Ok(url) => url,
        Err(error) => {
            tracing::error!(%error, provider = name, "cannot start sign-in");
            return failed(&app, StatusCode::BAD_GATEWAY, "Sign-in is unavailable right now.");
        }
    };
    let flow = Flow {
        provider: provider.kind(),
        state,
        nonce,
        pkce_verifier: verifier.into_secret(),
        return_to: safe_return(query.return_to.as_deref()).to_owned(),
        started: Instant::now(),
    };
    let Some(id) = app.auth.flows.insert(flow) else {
        return failed(&app, StatusCode::SERVICE_UNAVAILABLE, "Too many sign-ins at once. Try again in a minute.");
    };
    let mut response = Redirect::to(&url).into_response();
    response.headers_mut().append(header::SET_COOKIE, flow_cookie(&app.config, &id, FLOW_TTL.as_secs()));
    response
}

/// What a provider sends back, as query (Google) or form (Apple) fields.
#[derive(Deserialize)]
pub struct CallbackParams {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
    /// Apple, first sign-in only
    #[serde(default)]
    user: Option<String>,
}

/// `GET /auth/{provider}/callback` (Google)
pub async fn callback_get(
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Query(params): Query<CallbackParams>,
) -> Response {
    finish(app, &name, &headers, params).await
}

/// `POST /auth/{provider}/callback` (Apple's `response_mode=form_post`)
pub async fn callback_post(
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Form(params): Form<CallbackParams>,
) -> Response {
    finish(app, &name, &headers, params).await
}

async fn finish(app: Arc<AppState>, name: &str, headers: &HeaderMap, params: CallbackParams) -> Response {
    let Some(provider) = ProviderKind::parse(name).and_then(|kind| app.auth.provider(kind)) else {
        return (StatusCode::NOT_FOUND, "no such sign-in provider").into_response();
    };
    let flow = cookie(headers, flow_cookie_name(&app.config)).and_then(|id| app.auth.flows.take(id));
    let Some(flow) = flow.filter(|f| f.provider == provider.kind()) else {
        return failed(&app, StatusCode::BAD_REQUEST, "This sign-in has expired or was already used.");
    };
    if let Some(error) = params.error {
        // The person cancelled, or the provider refused; back to the login page with the reason.
        tracing::info!(event = "sign_in_error", provider = name, error, "provider returned an error");
        let reason: String = oauth2::url::form_urlencoded::byte_serialize(error.as_bytes()).collect();
        let mut response = Redirect::to(&format!("/login?error={reason}")).into_response();
        response.headers_mut().append(header::SET_COOKIE, flow_cookie(&app.config, "", 0));
        return response;
    }
    let (Some(code), Some(state)) = (params.code, params.state) else {
        return failed(&app, StatusCode::BAD_REQUEST, "The sign-in answer was incomplete.");
    };
    if !same(state.as_bytes(), flow.state.as_bytes()) {
        tracing::warn!(event = "sign_in_bad_state", provider = name, "state does not match the flow");
        return failed(&app, StatusCode::BAD_REQUEST, "The sign-in answer did not match this browser.");
    }
    let redirect = redirect_uri(&app.config, provider.kind());
    let callback = Callback {
        redirect_uri: &redirect,
        code: &code,
        pkce_verifier: &flow.pkce_verifier,
        nonce: &flow.nonce,
        apple_user: params.user.as_deref(),
    };
    let identity = match provider.exchange(&app.auth.http, callback).await {
        Ok(identity) => identity,
        Err(error) => {
            tracing::warn!(%error, provider = name, "sign-in failed");
            let status = match error {
                super::provider::AuthError::Upstream(_) => StatusCode::BAD_GATEWAY,
                _ => StatusCode::BAD_REQUEST,
            };
            return failed(&app, status, "The provider's answer could not be verified.");
        }
    };
    let user_agent = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    complete(&app, provider, identity, user_agent, &flow.return_to).await
}

async fn complete(
    app: &AppState,
    provider: &Provider,
    identity: super::provider::Identity,
    user_agent: &str,
    return_to: &str,
) -> Response {
    let outcome = match app.store.resolve_login(identity).await {
        Ok(outcome) => outcome,
        Err(error) => {
            tracing::error!(%error, "resolving a sign-in failed");
            return failed(app, StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong on our side.");
        }
    };
    let user = match outcome {
        LoginOutcome::Existing(user) | LoginOutcome::Linked(user) => user,
        LoginOutcome::Refused { outcome, email } => {
            tracing::info!(event = "sign_in_refused", provider = provider.kind().as_str(), email, outcome);
            let email: String = oauth2::url::form_urlencoded::byte_serialize(email.as_bytes()).collect();
            let mut response = Redirect::to(&format!("/no-access?email={email}")).into_response();
            response.headers_mut().append(header::SET_COOKIE, flow_cookie(&app.config, "", 0));
            return response;
        }
    };
    let session_cookie = match session::start(app, user.id, user_agent).await {
        Ok(cookie) => cookie,
        Err(error) => {
            tracing::error!(%error, "starting a session failed");
            return failed(app, StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong on our side.");
        }
    };
    tracing::info!(event = "sign_in", provider = provider.kind().as_str(), user = user.id, role = ?user.role);
    let mut response = Redirect::to(return_to).into_response();
    response.headers_mut().append(header::SET_COOKIE, session_cookie);
    response.headers_mut().append(header::SET_COOKIE, flow_cookie(&app.config, "", 0));
    response
}
