//! Request guards: who is asking, whether they may, and whether the request
//! really comes from our own pages.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::{Viewer, authenticate, rate_limit};
use crate::AppState;
use crate::api::error;
use crate::api::wire::Role;

/// For `/api`: 401 unless signed in; puts the [`Viewer`] in the request extensions.
pub async fn require_viewer(State(app): State<Arc<AppState>>, mut request: Request, next: Next) -> Response {
    match authenticate(&app, request.headers()).await {
        Ok(Some((viewer, renewed))) => {
            request.extensions_mut().insert(viewer);
            let mut response = next.run(request).await;
            if let Some(cookie) = renewed {
                response.headers_mut().append(header::SET_COOKIE, cookie);
            }
            response
        }
        Ok(None) => error(StatusCode::UNAUTHORIZED, "not signed in"),
        Err(e) => {
            tracing::error!(error = %e, "session lookup failed");
            error(StatusCode::SERVICE_UNAVAILABLE, "sign-in check unavailable")
        }
    }
}

/// For the `/api` routes beyond watching (chat, credits, admin): 403 for guests. Runs inside
/// [`require_viewer`].
pub async fn members_only(request: Request, next: Next) -> Response {
    match request.extensions().get::<Viewer>() {
        Some(viewer) if viewer.role == Role::Guest => error(StatusCode::FORBIDDEN, "guests can only watch"),
        _ => next.run(request).await,
    }
}

/// For `/api/admin`: 403 unless the viewer is an admin. Runs inside [`require_viewer`].
pub async fn require_admin(request: Request, next: Next) -> Response {
    match request.extensions().get::<Viewer>() {
        Some(viewer) if viewer.role == Role::Admin => next.run(request).await,
        Some(_) => error(StatusCode::FORBIDDEN, "admins only"),
        None => error(StatusCode::UNAUTHORIZED, "not signed in"),
    }
}

/// Why a mutating request was turned away.
#[derive(Debug, PartialEq, Eq)]
pub struct CsrfRefusal {
    pub status: StatusCode,
    pub reason: &'static str,
}

/// The CSRF rule for state-changing requests: `Sec-Fetch-Site` same-origin or
/// none when sent, `Origin` equal to `PUBLIC_ORIGIN` when sent, and a JSON body
/// type (which a cross-site HTML form cannot send). Pure.
pub fn check_csrf(public_origin: &str, headers: &HeaderMap) -> Result<(), CsrfRefusal> {
    let text = |name| headers.get(name).map(|v| v.to_str().unwrap_or("\u{0}"));
    if let Some(site) = text(header::HeaderName::from_static("sec-fetch-site"))
        && !matches!(site, "same-origin" | "none")
    {
        return Err(CsrfRefusal { status: StatusCode::FORBIDDEN, reason: "cross-site request" });
    }
    if let Some(origin) = text(header::ORIGIN)
        && origin != public_origin
    {
        return Err(CsrfRefusal { status: StatusCode::FORBIDDEN, reason: "foreign origin" });
    }
    let json = text(header::CONTENT_TYPE)
        .and_then(|v| v.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"));
    if !json {
        return Err(CsrfRefusal { status: StatusCode::UNSUPPORTED_MEDIA_TYPE, reason: "send application/json" });
    }
    Ok(())
}

/// For `/api`: applies [`check_csrf`] to every method that changes something.
pub async fn csrf(State(app): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    if request.method().is_safe() {
        return next.run(request).await;
    }
    match check_csrf(&app.config.public_origin, request.headers()) {
        Ok(()) => next.run(request).await,
        Err(refusal) => {
            tracing::info!(event = "csrf_refused", reason = refusal.reason, path = %request.uri().path());
            error(refusal.status, refusal.reason)
        }
    }
}

/// For `/auth`: per-address rate limit.
pub async fn rate_limit(State(app): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let ip = rate_limit::client_ip(&request, app.config.trust_proxy);
    if app.auth.limiter.allow(ip, Instant::now()) {
        next.run(request).await
    } else {
        tracing::warn!(event = "rate_limited", ip = ?ip, "too many sign-in requests");
        (StatusCode::TOO_MANY_REQUESTS, [(header::RETRY_AFTER, "30")], "Too many requests; try again shortly.")
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    const ORIGIN: &str = "https://world.example.com";

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        pairs.iter().map(|(k, v)| (header::HeaderName::from_static(k), HeaderValue::from_static(v))).collect()
    }

    #[test]
    fn csrf_rule() {
        let json = ("content-type", "application/json; charset=utf-8");
        assert_eq!(check_csrf(ORIGIN, &headers(&[json])), Ok(()));
        assert_eq!(
            check_csrf(ORIGIN, &headers(&[json, ("sec-fetch-site", "same-origin"), ("origin", ORIGIN)])),
            Ok(())
        );
        assert_eq!(check_csrf(ORIGIN, &headers(&[json, ("sec-fetch-site", "none")])), Ok(()));
        let refused = |pairs: &[(&'static str, &'static str)]| check_csrf(ORIGIN, &headers(pairs)).unwrap_err().status;
        assert_eq!(refused(&[json, ("sec-fetch-site", "cross-site")]), StatusCode::FORBIDDEN);
        assert_eq!(refused(&[json, ("sec-fetch-site", "same-site")]), StatusCode::FORBIDDEN);
        assert_eq!(refused(&[json, ("origin", "https://evil.example")]), StatusCode::FORBIDDEN);
        assert_eq!(refused(&[json, ("origin", "null")]), StatusCode::FORBIDDEN);
        assert_eq!(
            refused(&[("content-type", "application/x-www-form-urlencoded")]),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert_eq!(refused(&[("content-type", "text/plain")]), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(refused(&[]), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
}
