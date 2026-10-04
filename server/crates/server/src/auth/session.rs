//! Sessions: 32 random bytes in an HttpOnly cookie, only their SHA-256 in the database.
//!
//! The cookie is `__Host-session` (plain `session` without `Secure` when
//! `PUBLIC_ORIGIN` is http://). Sessions slide: each use more than an hour
//! after the last pushes the expiry to 30 days out, but never past 90 days
//! from sign-in.

use axum::http::{HeaderMap, HeaderValue};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use super::{Viewer, cookie};
use crate::AppState;
use crate::config::Config;
use crate::store::{DAY_MS, Result, now_ms};

pub const IDLE_MS: i64 = 30 * DAY_MS;
pub const ABSOLUTE_MS: i64 = 90 * DAY_MS;
/// A session is written back (and its cookie renewed) at most this often.
pub const TOUCH_AFTER_MS: i64 = 3600 * 1000;

/// A fresh random token, base64url.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The database key of a token.
pub fn hash(token: &str) -> [u8; 32] {
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, token.as_bytes());
    digest.as_ref().try_into().expect("SHA-256 is 32 bytes")
}

/// Sliding expiry, capped at the absolute lifetime. Pure.
pub fn expiry(created_at: i64, now: i64) -> i64 {
    (now + IDLE_MS).min(created_at + ABSOLUTE_MS)
}

pub fn cookie_name(config: &Config) -> &'static str {
    if config.secure_cookies() { "__Host-session" } else { "session" }
}

pub fn set_cookie(config: &Config, token: &str, max_age_seconds: i64) -> HeaderValue {
    let secure = if config.secure_cookies() { "; Secure" } else { "" };
    let value = format!(
        "{}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        cookie_name(config),
        max_age_seconds.max(0)
    );
    HeaderValue::from_str(&value).expect("tokens are base64url")
}

pub fn clear_cookie(config: &Config) -> HeaderValue {
    set_cookie(config, "", 0)
}

/// Starts a session for a signed-in user; returns the `Set-Cookie` value.
pub async fn start(app: &AppState, user_id: i64, user_agent: &str) -> Result<HeaderValue> {
    let token = random_token();
    let now = now_ms();
    let expires_at = expiry(now, now);
    app.store.create_session(hash(&token), user_id, user_agent, expires_at).await?;
    Ok(set_cookie(&app.config, &token, (expires_at - now) / 1000))
}

/// The viewer behind the session cookie, if it is valid; plus a renewed cookie when the session slid.
pub async fn authenticate(app: &AppState, headers: &HeaderMap) -> Result<Option<(Viewer, Option<HeaderValue>)>> {
    let Some(token) = cookie(headers, cookie_name(&app.config)) else { return Ok(None) };
    let id = hash(token);
    let Some(session) = app.store.session(id).await? else { return Ok(None) };
    let now = now_ms();
    let mut renewed = None;
    if now - session.last_seen_at > TOUCH_AFTER_MS {
        let expires_at = expiry(session.created_at, now);
        app.store.touch_session(id, expires_at).await?;
        renewed = Some(set_cookie(&app.config, token, (expires_at - now) / 1000));
    }
    let user = session.user;
    let viewer = Viewer {
        user_id: Some(user.id),
        email: user.email,
        display_name: user.display_name,
        role: user.role,
        session: Some(id),
        guest: None,
    };
    Ok(Some((viewer, renewed)))
}

/// False once the viewer's session is gone (signed out, expired, removed from the allowlist),
/// or a guest's pass has. A database hiccup keeps the viewer in.
pub async fn still_valid(app: &AppState, viewer: &Viewer) -> bool {
    if let Some(pass) = viewer.guest {
        return app.guests.holds(pass.id, std::time::Instant::now());
    }
    let Some(id) = viewer.session else { return true };
    match app.store.session(id).await {
        Ok(found) => found.is_some(),
        Err(error) => {
            tracing::warn!(%error, "could not re-check a session");
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_slides_up_to_the_cap() {
        assert_eq!(expiry(0, 0), IDLE_MS);
        assert_eq!(expiry(0, 10 * DAY_MS), 40 * DAY_MS);
        assert_eq!(expiry(0, 80 * DAY_MS), ABSOLUTE_MS);
    }

    #[test]
    fn tokens_are_random_and_hashed() {
        let (a, b) = (random_token(), random_token());
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert_eq!(hash(&a), hash(&a));
        assert_ne!(hash(&a), hash(&b));
    }
}
