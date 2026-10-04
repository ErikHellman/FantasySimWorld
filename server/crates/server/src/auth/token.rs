//! `AUTH_MODE=token`: one shared secret, for local work and the first
//! deployment. Whoever holds it is an admin (a pseudo-user with no database row).

use std::sync::Arc;

use axum::Form;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::{Viewer, cookie, safe_return, same};
use crate::AppState;
use crate::api::wire::Role;
use crate::config::{AuthMode, Config};

const COOKIE_MAX_AGE_SECONDS: u32 = 30 * 24 * 3600;

/// `__Host-` cookies must be `Secure`, which browsers refuse on plain http (local development).
pub fn cookie_name(config: &Config) -> &'static str {
    if config.secure_cookies() { "__Host-fsw_token" } else { "fsw_token" }
}

fn expected(config: &Config) -> Option<&str> {
    match &config.auth {
        AuthMode::Token(token) => Some(token),
        AuthMode::Oidc(_) | AuthMode::Off => None,
    }
}

fn presented<'a>(config: &Config, headers: &'a HeaderMap) -> Option<&'a str> {
    if let Some(bearer) =
        headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "))
    {
        return Some(bearer.trim());
    }
    cookie(headers, cookie_name(config))
}

/// The token holder, when the request carries the token (cookie or `Authorization: Bearer`).
pub fn viewer(config: &Config, headers: &HeaderMap) -> Option<Viewer> {
    let expected = expected(config)?;
    let token = presented(config, headers)?;
    same(token.as_bytes(), expected.as_bytes()).then(|| Viewer {
        user_id: None,
        email: String::new(),
        display_name: "Token holder".into(),
        role: Role::Admin,
        guest: None,
        session: None,
    })
}

pub fn clear_cookie(config: &Config) -> HeaderValue {
    let secure = if config.secure_cookies() { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}", cookie_name(config)))
        .expect("static cookie")
}

/// `GET /auth/token`: a form, so the token never ends up in a URL (and so in proxy logs).
pub async fn form(State(app): State<Arc<AppState>>) -> Response {
    if expected(&app.config).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    Html(concat!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'>",
        "<title>Sign in</title><body style='font:16px system-ui;max-width:24rem;margin:4rem auto;padding:0 1rem'>",
        "<form method=post action=/auth/token><p><label>Access token<br>",
        "<input name=token type=password autofocus required style='width:100%;font:inherit'></label></p>",
        "<p><button>Enter the valley</button></p></form>"
    ))
    .into_response()
}

#[derive(Deserialize)]
pub struct TokenLogin {
    token: String,
    #[serde(default)]
    return_to: Option<String>,
}

/// `POST /auth/token`: sets the cookie and goes back to the app.
pub async fn login(State(app): State<Arc<AppState>>, Form(login): Form<TokenLogin>) -> Response {
    let Some(expected) = expected(&app.config) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !same(login.token.trim().as_bytes(), expected.as_bytes()) {
        tracing::info!(event = "token_refused", "wrong access token");
        return (StatusCode::UNAUTHORIZED, Html("Wrong token. <a href=/auth/token>Try again</a>.")).into_response();
    }
    let secure = if app.config.secure_cookies() { "; Secure" } else { "" };
    let cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={COOKIE_MAX_AGE_SECONDS}{secure}",
        cookie_name(&app.config),
        expected
    );
    let mut response = Redirect::to(safe_return(login.return_to.as_deref())).into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).expect("token is a valid header value"));
    response
}
