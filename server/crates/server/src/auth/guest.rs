//! Guest passes: `POST /auth/guest` hands out a seat (or a place in line), and the pass in the
//! `__Host-guest` cookie (plain `guest` over http://) lets a guest into the routes that only watch.
//! The seats themselves are kept by [`crate::guests::GuestHub`].

use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::{Viewer, cookie};
use crate::AppState;
use crate::api::error;
use crate::api::wire::{GuestAnswer, GuestRequest, Role};
use crate::config::Config;
use crate::guests::{Admission, POLL_EVERY};

pub fn cookie_name(config: &Config) -> &'static str {
    if config.secure_cookies() { "__Host-guest" } else { "guest" }
}

fn set_cookie(config: &Config, token: &str, max_age_seconds: u64) -> HeaderValue {
    let secure = if config.secure_cookies() { "; Secure" } else { "" };
    let value =
        format!("{}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}{secure}", cookie_name(config));
    HeaderValue::from_str(&value).expect("tokens are base64url")
}

pub fn clear_cookie(config: &Config) -> HeaderValue {
    set_cookie(config, "", 0)
}

/// The guest behind the cookie, while their pass lasts.
pub fn viewer(app: &AppState, headers: &HeaderMap) -> Option<Viewer> {
    if !app.config.guests_on() {
        return None;
    }
    let pass = app.guests.pass(cookie(headers, cookie_name(&app.config))?, Instant::now())?;
    Some(Viewer {
        user_id: None,
        email: String::new(),
        display_name: "Guest".into(),
        role: Role::Guest,
        session: None,
        guest: Some(pass),
    })
}

/// `POST /auth/guest`: a seat for a guest, or their place in line.
pub async fn ask(State(app): State<Arc<AppState>>, headers: HeaderMap, Json(body): Json<GuestRequest>) -> Response {
    if !app.config.guests_on() {
        return error(StatusCode::NOT_FOUND, "guest viewing is off");
    }
    let now = Instant::now();
    let pass = cookie(&headers, cookie_name(&app.config));
    match app.guests.ask(pass, body.ticket.as_deref(), now) {
        Admission::Admitted { token, ends } => {
            let left = ends.saturating_duration_since(now).as_secs();
            let mut response =
                Json(GuestAnswer::Admitted { seconds_left: left.try_into().unwrap_or(u32::MAX) }).into_response();
            if let Some(token) = token {
                let (seated, waiting) = app.guests.counts(now);
                tracing::info!(event = "guest_admitted", seated, waiting);
                response.headers_mut().append(header::SET_COOKIE, set_cookie(&app.config, &token, left));
            }
            response
        }
        Admission::Queued { ticket, position } => {
            let (seated, waiting) = app.guests.counts(now);
            tracing::info!(event = "guest_queued", position, seated, waiting);
            Json(GuestAnswer::Queued {
                ticket,
                position,
                seats: app.guests.limits().seats,
                retry_after_seconds: POLL_EVERY.as_secs() as u32,
            })
            .into_response()
        }
        Admission::TooSoon { retry_after } => {
            let mut response = error(StatusCode::TOO_MANY_REQUESTS, "ask again in a minute");
            let seconds = retry_after.as_secs().max(1);
            response.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(seconds));
            response
        }
        Admission::Off => error(StatusCode::NOT_FOUND, "guest viewing is off"),
    }
}
