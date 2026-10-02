//! Who may watch. Phase 0 has the shared-token mode only; Google and Apple
//! sign-in with an allowlist replace it in phase 5.

use std::sync::Arc;

use axum::Form;
use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::AppState;
use crate::api::wire::{ApiError, Me, Role};
use crate::config::AuthMode;

pub mod provider;

const COOKIE_MAX_AGE_SECONDS: u32 = 30 * 24 * 3600;

/// `__Host-` cookies must be `Secure`, which browsers refuse on plain http (local development).
fn cookie_name(app: &AppState) -> &'static str {
    if app.config.secure_cookies() { "__Host-fsw_token" } else { "fsw_token" }
}

/// Compares without an early exit, so response timing says nothing about the token.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn presented_token<'a>(app: &AppState, headers: &'a HeaderMap) -> Option<&'a str> {
    if let Some(bearer) =
        headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "))
    {
        return Some(bearer.trim());
    }
    let name = cookie_name(app);
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

fn authorized(app: &AppState, headers: &HeaderMap) -> bool {
    let AuthMode::Token(expected) = &app.config.auth;
    presented_token(app, headers).is_some_and(|token| same(token.as_bytes(), expected.as_bytes()))
}

/// Middleware for `/api`: 401 unless the request carries the token.
pub async fn require_viewer(State(app): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    if authorized(&app, request.headers()) {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, Json(ApiError { error: "not signed in".into() })).into_response()
    }
}

/// `GET /api/me`
pub async fn me() -> Json<Me> {
    Json(Me { email: String::new(), display_name: "Token holder".into(), role: Role::Admin })
}

/// `GET /auth/token`: a form, so the token never ends up in a URL (and so in proxy logs).
pub async fn token_form() -> Html<&'static str> {
    Html(concat!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'>",
        "<title>Sign in</title><body style='font:16px system-ui;max-width:24rem;margin:4rem auto;padding:0 1rem'>",
        "<form method=post action=/auth/token><p><label>Access token<br>",
        "<input name=token type=password autofocus required style='width:100%;font:inherit'></label></p>",
        "<p><button>Enter the valley</button></p></form>"
    ))
}

#[derive(Deserialize)]
pub struct TokenLogin {
    token: String,
    #[serde(default)]
    return_to: Option<String>,
}

/// Only paths on this site: "/x" but not "//evil.example" or "/\evil.example".
fn safe_return(path: Option<&str>) -> &str {
    match path {
        Some(p) if p.starts_with('/') && !p.starts_with("//") && !p.starts_with("/\\") => p,
        _ => "/",
    }
}

/// `POST /auth/token`: sets the cookie and goes back to the app.
pub async fn token_login(State(app): State<Arc<AppState>>, Form(login): Form<TokenLogin>) -> Response {
    let AuthMode::Token(expected) = &app.config.auth;
    if !same(login.token.trim().as_bytes(), expected.as_bytes()) {
        tracing::info!(event = "token_refused", "wrong access token");
        return (StatusCode::UNAUTHORIZED, Html("Wrong token. <a href=/auth/token>Try again</a>.")).into_response();
    }
    let secure = if app.config.secure_cookies() { "; Secure" } else { "" };
    let cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={COOKIE_MAX_AGE_SECONDS}{secure}",
        cookie_name(&app),
        expected
    );
    let mut response = Redirect::to(safe_return(login.return_to.as_deref())).into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).expect("token is a valid header value"));
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
        assert_eq!(safe_return(Some("https://evil.example")), "/");
        assert_eq!(safe_return(None), "/");
    }
}
