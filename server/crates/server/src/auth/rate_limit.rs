//! A token bucket per key: per address for `/auth/*`, per person for chat.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{ConnectInfo, Request};

/// Requests one address may make to `/auth/*` in a burst.
pub const BURST: f64 = 30.0;
/// Sustained `/auth/*` rate: one request every two seconds.
pub const PER_SECOND: f64 = 0.5;
/// Past this many tracked keys, full buckets are forgotten.
const TRACKED: usize = 10_000;

/// Allows `burst` requests at once per key, refilling at `per_second`.
pub struct RateLimiter<K = Option<IpAddr>> {
    buckets: Mutex<HashMap<K, (f64, Instant)>>,
    burst: f64,
    per_second: f64,
}

/// The `/auth/*` limiter, keyed by client address.
impl Default for RateLimiter<Option<IpAddr>> {
    fn default() -> Self {
        Self::new(BURST, PER_SECOND)
    }
}

impl<K: Hash + Eq + Clone> RateLimiter<K> {
    pub fn new(burst: f64, per_second: f64) -> Self {
        Self { buckets: Mutex::new(HashMap::new()), burst, per_second }
    }

    /// Takes one token for `key` (for addresses, `None` means unknown: shared by all such requests).
    pub fn allow(&self, key: K, now: Instant) -> bool {
        let (burst, rate) = (self.burst, self.per_second);
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if buckets.len() > TRACKED {
            buckets.retain(|_, (tokens, at)| *tokens + now.duration_since(*at).as_secs_f64() * rate < burst);
        }
        let (tokens, at) = buckets.entry(key).or_insert((burst, now));
        *tokens = (*tokens + now.duration_since(*at).as_secs_f64() * rate).min(burst);
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
