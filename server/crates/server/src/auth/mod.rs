//! Who may watch.
//!
//! `AUTH_MODE=token`: one shared secret, everyone is an admin (local work,
//! first deployment). `AUTH_MODE=oidc`: Google and/or Apple sign-in, limited
//! to the allowlist, with server-side sessions. Every `/api` route needs a
//! viewer ([`middleware::require_viewer`]), `/api/admin` an admin, and every
//! mutating `/api` request passes the CSRF check.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::api::wire::{Me, Role, SignInOptions};
use crate::config::{AuthMode, Config};

pub mod allowlist;
pub mod apple;
pub mod flow;
pub mod google;
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
            if let Some(google) = &oidc.google {
                providers.push(Provider::Google(google::Google::new(google.clone())));
            }
            if let Some(apple) = &oidc.apple {
                providers.push(Provider::Apple(apple::Apple::new(apple.clone()).map_err(|e| e.to_string())?));
            }
        }
        Ok(Self { providers, flows: flow::Flows::default(), limiter: rate_limit::RateLimiter::default(), http })
    }

    /// An enabled provider.
    pub fn provider(&self, kind: ProviderKind) -> Option<&Provider> {
        self.providers.iter().find(|p| p.kind() == kind)
    }
}

/// The viewer behind a request, by the configured mode; plus a renewed session cookie, if any.
pub async fn authenticate(
    app: &AppState,
    headers: &HeaderMap,
) -> crate::store::Result<Option<(Viewer, Option<HeaderValue>)>> {
    match &app.config.auth {
        AuthMode::Token(_) => Ok(token::viewer(&app.config, headers).map(|v| (v, None))),
        AuthMode::Oidc(_) => session::authenticate(app, headers).await,
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
        AuthMode::Token(_) => SignInOptions { token: true, google: false, apple: false },
        AuthMode::Oidc(oidc) => {
            SignInOptions { token: false, google: oidc.google.is_some(), apple: oidc.apple.is_some() }
        }
    })
}

/// `GET /api/me`
pub async fn me(Extension(viewer): Extension<Viewer>) -> Json<Me> {
    Json(Me { email: viewer.email, display_name: viewer.display_name, role: viewer.role })
}

/// `POST /api/logout`: ends this session and clears the cookie.
pub async fn logout(State(app): State<Arc<AppState>>, Extension(viewer): Extension<Viewer>) -> Response {
    if let Some(id) = viewer.session
        && let Err(error) = app.store.delete_session(id).await
    {
        tracing::error!(%error, "could not delete a session");
        return crate::api::error(StatusCode::SERVICE_UNAVAILABLE, "try again");
    }
    let clear = match &app.config.auth {
        AuthMode::Token(_) => token::clear_cookie(&app.config),
        AuthMode::Oidc(_) => session::clear_cookie(&app.config),
    };
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(header::SET_COOKIE, clear);
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
