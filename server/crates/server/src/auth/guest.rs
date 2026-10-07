//! Guest passes: `POST /auth/guest` hands out a seat (or a place in line), and the pass in the
//! `__Host-guest` cookie (plain `guest` over http://) is what lets a viewer in. The seats
//! themselves are kept by [`crate::guests::GuestHub`].

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
use crate::config::{AuthMode, Config};
use crate::guests::{Admission, POLL_EVERY};

/// The cookie outlives the pass, which lives in the server's memory: a browser that comes back
/// after a restart simply asks for a new seat.
const COOKIE_MAX_AGE_SECONDS: u64 = 30 * 24 * 3600;

pub fn cookie_name(config: &Config) -> &'static str {
    if config.secure_cookies() { "__Host-guest" } else { "guest" }
}

fn set_cookie(config: &Config, token: &str, max_age_seconds: u64) -> HeaderValue {
    let secure = if config.secure_cookies() { "; Secure" } else { "" };
    let value =
        format!("{}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}{secure}", cookie_name(config));
    HeaderValue::from_str(&value).expect("tokens are hex")
}

pub fn clear_cookie(config: &Config) -> HeaderValue {
    set_cookie(config, "", 0)
}

/// The guest behind the cookie, while their pass holds a seat.
pub fn viewer(app: &AppState, headers: &HeaderMap) -> Option<Viewer> {
    if app.config.auth == AuthMode::Off {
        return None;
    }
    let pass = app.guests.pass(cookie(headers, cookie_name(&app.config))?, Instant::now())?;
    Some(Viewer { role: Role::Guest, guest: Some(pass) })
}

/// Whether a viewer still holds what let them in: a guest their seat; an admin always.
pub fn still_valid(app: &AppState, viewer: &Viewer) -> bool {
    match &viewer.guest {
        Some(pass) => app.guests.holds(pass.id, Instant::now()),
        None => true,
    }
}

/// `POST /auth/guest`: a seat for a guest, or their place in line.
pub async fn ask(State(app): State<Arc<AppState>>, headers: HeaderMap, Json(body): Json<GuestRequest>) -> Response {
    if app.config.auth == AuthMode::Off {
        return error(StatusCode::NOT_FOUND, "everyone is an admin on this server");
    }
    let now = Instant::now();
    let pass = cookie(&headers, cookie_name(&app.config));
    match app.guests.ask(pass, body.ticket.as_deref(), now) {
        Admission::Admitted { token } => {
            let mut response = Json(GuestAnswer::Admitted).into_response();
            if let Some(token) = token {
                let (seated, waiting) = app.guests.counts(now);
                tracing::info!(event = "guest_admitted", seated, waiting);
                response
                    .headers_mut()
                    .append(header::SET_COOKIE, set_cookie(&app.config, &token, COOKIE_MAX_AGE_SECONDS));
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
    }
}
