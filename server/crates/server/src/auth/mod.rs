//! Who may watch.
//!
//! `AUTH_MODE=token`: one shared secret, everyone is an admin (local work,
//! first deployment). `AUTH_MODE=oidc`: sign-in with Google, Apple, Microsoft,
//! LinkedIn, GitHub and/or Discord, limited to the allowlist, with server-side
//! sessions. `AUTH_MODE=off`: no sign-in,
//! everyone is an admin; the configuration allows it on this machine only. Every `/api` route needs a
//! viewer ([`middleware::require_viewer`]), `/api/admin` an admin, and every
//! mutating `/api` request passes the CSRF check. In the first two modes people may also watch
//! as guests for a while ([`guest`]), which lets them into the routes that only watch.

use std::time::Instant;

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::api::wire::{Me, Role, SignInOptions};
use crate::config::{AuthMode, Config, ProviderConfig};
use crate::guests::GuestPass;

pub mod allowlist;
pub mod apple;
pub mod discord;
pub mod discovered;
pub mod flow;
pub mod github;
pub mod guest;
pub mod microsoft;
pub mod middleware;
pub mod provider;
pub mod rate_limit;
pub mod session;
pub mod token;

use provider::{Provider, ProviderKind};

/// How long sign-in HTTP calls (discovery, keys, token exchange) may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Who is asking; in the request extensions of every `/api` handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Viewer {
    /// `None` for the token holder
    pub user_id: Option<i64>,
    /// empty for the token holder
    pub email: String,
    pub display_name: String,
    pub role: Role,
    /// the session's database key; `None` in token mode
    pub session: Option<[u8; 32]>,
    /// a guest's pass; `None` for everyone who signed in
    pub guest: Option<GuestPass>,
}

/// Sign-in state shared by the handlers.
pub struct Auth {
    providers: Vec<Provider>,
    pub flows: flow::Flows,
    pub limiter: rate_limit::RateLimiter,
    /// for discovery, keys and token exchanges (rustls on aws-lc-rs, no redirects)
    pub http: reqwest::Client,
}

impl Auth {
    pub fn new(config: &Config) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("fantasy-sim-world")
            .build()
            .map_err(|e| format!("HTTP client: {e}"))?;
        let mut providers = Vec::new();
        if let AuthMode::Oidc(oidc) = &config.auth {
            for provider in &oidc.providers {
                let kind = provider.kind();
                providers.push(match provider.clone() {
                    ProviderConfig::Google(c) | ProviderConfig::Microsoft(c) | ProviderConfig::LinkedIn(c) => {
                        Provider::Oidc(discovered::Discovered::new(kind, c))
                    }
                    ProviderConfig::Apple(c) => Provider::Apple(apple::Apple::new(c).map_err(|e| e.to_string())?),
                    ProviderConfig::GitHub(c) => Provider::GitHub(c),
                    ProviderConfig::Discord(c) => Provider::Discord(c),
                });
            }
        }
        Ok(Self { providers, flows: flow::Flows::default(), limiter: rate_limit::RateLimiter::default(), http })
    }

    /// An enabled provider.
    pub fn provider(&self, kind: ProviderKind) -> Option<&Provider> {
        self.providers.iter().find(|p| p.kind() == kind)
    }

    /// The enabled providers, in `AUTH_PROVIDERS` order.
    pub fn provider_kinds(&self) -> Vec<ProviderKind> {
        self.providers.iter().map(Provider::kind).collect()
    }
}

/// The viewer behind a request, by the configured mode, or else a guest; plus a renewed
/// session cookie, if any. Signing in wins over a guest pass left in the browser.
pub async fn authenticate(
    app: &AppState,
    headers: &HeaderMap,
) -> crate::store::Result<Option<(Viewer, Option<HeaderValue>)>> {
    let signed_in = match &app.config.auth {
        AuthMode::Off => Some((open_viewer(), None)),
        AuthMode::Token(_) => token::viewer(&app.config, headers).map(|v| (v, None)),
        AuthMode::Oidc(_) => session::authenticate(app, headers).await?,
    };
    Ok(signed_in.or_else(|| guest::viewer(app, headers).map(|v| (v, None))))
}

/// Everyone, when sign-in is off: an admin with no database row, like the token holder.
fn open_viewer() -> Viewer {
    Viewer {
        user_id: None,
        email: String::new(),
        display_name: "Local developer".into(),
        role: Role::Admin,
        session: None,
        guest: None,
    }
}

/// A cookie's value by name.
pub fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// Compares without an early exit, so response timing says nothing about a secret.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Only paths on this site: "/x" but not "//evil.example", "/\evil.example" or anything with control characters.
pub fn safe_return(path: Option<&str>) -> &str {
    match path {
        Some(p)
            if p.starts_with('/')
                && !p.starts_with("//")
                && !p.contains('\\')
                && !p.chars().any(|c| c.is_control() || c.is_whitespace()) =>
        {
            p
        }
        _ => "/",
    }
}

/// `GET /auth/providers`
pub async fn providers(State(app): State<Arc<AppState>>) -> Json<SignInOptions> {
    Json(match &app.config.auth {
        AuthMode::Off => SignInOptions { open: true, token: false, providers: Vec::new(), guests: false },
        AuthMode::Token(_) => {
            SignInOptions { open: false, token: true, providers: Vec::new(), guests: app.config.guests_on() }
        }
        AuthMode::Oidc(_) => SignInOptions {
            open: false,
            token: false,
            providers: app.auth.provider_kinds(),
            guests: app.config.guests_on(),
        },
    })
}

/// `GET /api/me`
pub async fn me(Extension(viewer): Extension<Viewer>) -> Json<Me> {
    let guest_seconds_left =
        viewer.guest.map(|pass| pass.ends.saturating_duration_since(Instant::now()).as_secs().try_into().unwrap_or(0));
    Json(Me { email: viewer.email, display_name: viewer.display_name, role: viewer.role, guest_seconds_left })
}

/// `POST /api/logout`: ends this session (or guest pass, freeing the seat) and clears the cookie.
pub async fn logout(State(app): State<Arc<AppState>>, Extension(viewer): Extension<Viewer>) -> Response {
    if let Some(pass) = viewer.guest {
        app.guests.end(pass.id);
        tracing::info!(event = "guest_left");
        let mut response = StatusCode::NO_CONTENT.into_response();
        response.headers_mut().append(header::SET_COOKIE, guest::clear_cookie(&app.config));
        return response;
    }
    if let Some(id) = viewer.session
        && let Err(error) = app.store.delete_session(id).await
    {
        tracing::error!(%error, "could not delete a session");
        return crate::api::error(StatusCode::SERVICE_UNAVAILABLE, "try again");
    }
    let clear = match &app.config.auth {
        AuthMode::Off => None,
        AuthMode::Token(_) => Some(token::clear_cookie(&app.config)),
        AuthMode::Oidc(_) => Some(session::clear_cookie(&app.config)),
    };
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Some(clear) = clear {
        response.headers_mut().append(header::SET_COOKIE, clear);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_compare() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"abcd"));
    }

    #[test]
    fn return_paths_stay_on_site() {
        assert_eq!(safe_return(Some("/world?x=1")), "/world?x=1");
        assert_eq!(safe_return(Some("//evil.example")), "/");
        assert_eq!(safe_return(Some("/\\evil.example")), "/");
        assert_eq!(safe_return(Some("/a\\b")), "/");
        assert_eq!(safe_return(Some("/\tevil")), "/");
        assert_eq!(safe_return(Some("https://evil.example")), "/");
        assert_eq!(safe_return(Some("world")), "/");
        assert_eq!(safe_return(None), "/");
    }

    #[test]
    fn cookies_by_name() {
        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, HeaderValue::from_static("a=1; session=xyz"));
        headers.append(header::COOKIE, HeaderValue::from_static("b=2"));
        assert_eq!(cookie(&headers, "session"), Some("xyz"));
        assert_eq!(cookie(&headers, "b"), Some("2"));
        assert_eq!(cookie(&headers, "sess"), None);
    }
}
