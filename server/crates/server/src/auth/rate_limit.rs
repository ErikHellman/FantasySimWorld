//! A per-address token bucket for `/auth/*`.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{ConnectInfo, Request};

/// Requests one address may make in a burst.
pub const BURST: f64 = 30.0;
/// Sustained rate: one request every two seconds.
pub const PER_SECOND: f64 = 0.5;
/// Past this many tracked addresses, full buckets are forgotten.
const TRACKED: usize = 10_000;

#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<Option<IpAddr>, (f64, Instant)>>,
}

impl RateLimiter {
    /// Takes one token for `ip` (`None`: address unknown, shared by all such requests).
    pub fn allow(&self, ip: Option<IpAddr>, now: Instant) -> bool {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if buckets.len() > TRACKED {
            buckets.retain(|_, (tokens, at)| *tokens + now.duration_since(*at).as_secs_f64() * PER_SECOND < BURST);
        }
        let (tokens, at) = buckets.entry(ip).or_insert((BURST, now));
        *tokens = (*tokens + now.duration_since(*at).as_secs_f64() * PER_SECOND).min(BURST);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// The client's address: from the proxy's headers when `trust_proxy`, else the socket peer.
pub fn client_ip(request: &Request, trust_proxy: bool) -> Option<IpAddr> {
    if trust_proxy {
        let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok());
        if let Some(ip) = header("fly-client-ip").and_then(|v| v.trim().parse().ok()) {
            return Some(ip);
        }
        // The trusted proxy appends the address it saw; earlier entries are the client's own claims.
        if let Some(ip) =
            header("x-forwarded-for").and_then(|v| v.rsplit(',').next()).and_then(|v| v.trim().parse().ok())
        {
            return Some(ip);
        }
    }
    request.extensions().get::<ConnectInfo<SocketAddr>>().map(|info| info.0.ip())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;

    use super::*;

    #[test]
    fn bursts_then_refills() {
        let limiter = RateLimiter::default();
        let ip = Some(IpAddr::from([10, 0, 0, 1]));
        let start = Instant::now();
        for _ in 0..BURST as usize {
            assert!(limiter.allow(ip, start));
        }
        assert!(!limiter.allow(ip, start));
        assert!(limiter.allow(Some(IpAddr::from([10, 0, 0, 2])), start), "others are not affected");
        assert!(limiter.allow(ip, start + Duration::from_secs(2)));
        assert!(!limiter.allow(ip, start + Duration::from_secs(2)));
    }

    #[test]
    fn proxy_headers_only_when_trusted() {
        let request = Request::builder()
            .header("x-forwarded-for", "6.6.6.6, 203.0.113.9")
            .extension(ConnectInfo(SocketAddr::from(([10, 1, 2, 3], 4000))))
            .body(Body::empty())
            .unwrap();
        assert_eq!(client_ip(&request, false), Some(IpAddr::from([10, 1, 2, 3])));
        assert_eq!(client_ip(&request, true), Some(IpAddr::from([203, 0, 113, 9])));
        let fly = Request::builder().header("fly-client-ip", "198.51.100.4").body(Body::empty()).unwrap();
        assert_eq!(client_ip(&fly, true), Some(IpAddr::from([198, 51, 100, 4])));
        assert_eq!(client_ip(&fly, false), None);
    }
}
