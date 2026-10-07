//! Who may watch, and who may run the world.
//!
//! Everyone may watch: a browser without a cookie asks for a guest seat
//! ([`guest`]), which lets it into every `/api` route but the admin's, chat
//! included. Admins come in one of two ways: `AUTH_MODE=token` has one shared
//! secret posted to `/auth/token` once for a cookie; `AUTH_MODE=off` makes
//! everyone an admin and is allowed on this machine only. Every `/api` route
//! needs a viewer ([`middleware::require_viewer`]), `/api/admin` an admin, and
//! every mutating `/api` request passes the CSRF check.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::api::wire::{Me, Role};
use crate::config::AuthMode;
use crate::guests::GuestPass;

pub mod guest;
pub mod middleware;
pub mod rate_limit;
pub mod token;

/// Who is asking; in the request extensions of every `/api` handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Viewer {
    pub role: Role,
    /// a guest's pass; `None` for an admin
    pub guest: Option<GuestPass>,
}

/// What the handlers share: the per-address limit on the sign-in routes.
#[derive(Default)]
pub struct Auth {
    pub limiter: rate_limit::RateLimiter,
}

/// The viewer behind a request: an admin by the configured mode, or else the guest behind
/// the guest cookie. An admin's token wins over a guest pass left in the browser.
pub fn authenticate(app: &AppState, headers: &HeaderMap) -> Option<Viewer> {
    let admin = match &app.config.auth {
        AuthMode::Off => Some(Viewer { role: Role::Admin, guest: None }),
        AuthMode::Token(_) => token::viewer(&app.config, headers),
    };
    admin.or_else(|| guest::viewer(app, headers))
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

/// `GET /api/me`
pub async fn me(Extension(viewer): Extension<Viewer>) -> Json<Me> {
    Json(Me { role: viewer.role })
}

/// `POST /api/logout`: gives up a guest seat (ending its stream) or the admin's cookie.
pub async fn logout(State(app): State<Arc<AppState>>, Extension(viewer): Extension<Viewer>) -> Response {
    let clear: Option<HeaderValue> = match viewer.guest {
        Some(pass) => {
            app.guests.end(pass.id);
            tracing::info!(event = "guest_left");
            Some(guest::clear_cookie(&app.config))
        }
        None => match &app.config.auth {
            AuthMode::Off => None,
            AuthMode::Token(_) => Some(token::clear_cookie(&app.config)),
        },
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
        headers.append(header::COOKIE, HeaderValue::from_static("a=1; guest=xyz"));
        headers.append(header::COOKIE, HeaderValue::from_static("b=2"));
        assert_eq!(cookie(&headers, "guest"), Some("xyz"));
        assert_eq!(cookie(&headers, "b"), Some("2"));
        assert_eq!(cookie(&headers, "gue"), None);
    }
}
