//! Inbound token-bucket rate limiter for the velionv3 BFF gateway.
//!
//! Ported from the Model Plane `model-gateway` limiter (per-key token bucket,
//! configurable RPM, `429 TOO_MANY_REQUESTS` + `Retry-After`).
//!
//! ## Key selection
//!
//! 1. Validated session identity when present — `AuthenticatedUser` is inserted
//!    into request extensions by [`crate::middleware::require_session`]. We key
//!    by the live canonical membership's organization when present, otherwise
//!    the trusted `user_id`. Neither scope can originate from the client.
//! 2. Client IP for pre-auth routes (auth / onboarding) where no session exists,
//!    read from a validated forwarded-for header. The browser cannot present an
//!    `AuthenticatedUser` extension, so spoofing the key requires spoofing the
//!    forwarded header — which only the trusted reverse proxy in front of the
//!    gateway should be able to set.
//! 3. A single shared `anonymous` bucket as a last resort when neither an
//!    identity nor a parseable client IP is available, so an attacker cannot
//!    escape the limiter by simply omitting the forwarded header.
//!
//! ## Bucket storage: distributed, with an in-process fallback
//!
//! When the gateway's Dragonfly connection (the same `GATEWAY_CACHE_REDIS_URL`
//! link the [`crate::cache::ResultCache`] uses) is available, buckets live in
//! Dragonfly and are refilled+consumed atomically by a single Lua `EVAL` per
//! request. Refill is driven by Redis server `TIME`, so the limit holds
//! fleet-wide across every horizontally-scaled gateway replica regardless of any
//! per-process clock.
//!
//! If the connection is absent (cache disabled) or any Redis call errors, the
//! limiter transparently falls back to a per-process [`DashMap`] token bucket —
//! fail-open by design: a Dragonfly blip must never reject legitimate traffic.
//! The DashMap path is the original per-instance behaviour and remains fully
//! covered by tests.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use redis::Script;

use crate::middleware::AuthenticatedUser;

/// Default requests-per-minute when `GATEWAY_RATE_LIMIT_RPM` is unset/invalid.
///
/// Generous because this limiter runs BEFORE `require_session`, so it keys by
/// client IP rather than user: a single SPA page load fans out ~10-15 authed
/// calls across components, and multiple users can share one egress IP (NAT).
/// The previous 120 was low enough that a normal burst — or co-located clients
/// (e.g. dev tooling on the same host) — tripped a 429 and cascaded to 401s.
const DEFAULT_RPM: f64 = 600.0;

/// Forwarded-for header values longer than this are rejected outright — a sane
/// upper bound that defends against an unbounded-allocation key from a hostile
/// proxy hop while comfortably fitting a realistic proxy chain.
const MAX_FORWARDED_LEN: usize = 256;

/// A cache optimization must never hold an HTTP request open while the shared
/// connection manager reconnects. After this deadline the local bucket decides.
const DISTRIBUTED_RATE_LIMIT_TIMEOUT: Duration = Duration::from_millis(500);

/// Atomic token-bucket refill+consume, evaluated server-side in one round trip.
///
/// `KEYS[1]` is the bucket key. `ARGV[1]` is the capacity / refill burst (rpm),
/// `ARGV[2]` the refill rate in tokens/second, `ARGV[3]` the TTL to set on the
/// key (seconds). The bucket's `tokens` + `ts` (last-refill unix seconds, with
/// microsecond fraction) live in a hash; refill is driven by the Redis server's
/// own `TIME`, so it is correct no matter which replica runs the script and
/// independent of any client clock. Returns `{allowed, retry_after_secs}` where
/// `allowed` is 1/0 and `retry_after_secs` is a ceil'd >=1 hint when denied.
const TOKEN_BUCKET_LUA: &str = r#"
local capacity = tonumber(ARGV[1])
local refill_rate = tonumber(ARGV[2])
local ttl = tonumber(ARGV[3])
local t = redis.call('TIME')
local now = tonumber(t[1]) + (tonumber(t[2]) / 1000000)
local data = redis.call('HMGET', KEYS[1], 'tokens', 'ts')
local tokens = tonumber(data[1])
local ts = tonumber(data[2])
if tokens == nil then
  tokens = capacity
  ts = now
end
local elapsed = now - ts
if elapsed < 0 then elapsed = 0 end
tokens = math.min(capacity, tokens + elapsed * refill_rate)
local allowed = 0
local retry_after = 0
if tokens >= 1.0 then
  tokens = tokens - 1.0
  allowed = 1
else
  local deficit = 1.0 - tokens
  retry_after = math.ceil(deficit / refill_rate)
  if retry_after < 1 then retry_after = 1 end
end
redis.call('HSET', KEYS[1], 'tokens', tokens, 'ts', now)
redis.call('EXPIRE', KEYS[1], ttl)
return {allowed, retry_after}
"#;

/// Tracks token count and last refill timestamp for a single bucket.
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

/// Shared rate-limiter state keyed by caller identity (org / user / client IP).
///
/// `conn` is the optional shared Dragonfly link (distributed buckets); `buckets`
/// is the always-present in-process fallback used when `conn` is `None` or a
/// Redis call errors.
#[derive(Clone)]
pub(crate) struct RateLimiter {
    conn: Option<redis::aio::ConnectionManager>,
    buckets: Arc<DashMap<String, TokenBucket>>,
    rpm: f64,
}

/// Read `GATEWAY_RATE_LIMIT_RPM` (requests per minute), defaulting to
/// [`DEFAULT_RPM`]. A non-positive or unparseable value falls back to the
/// default rather than disabling the limiter.
fn rpm_from_env() -> f64 {
    std::env::var("GATEWAY_RATE_LIMIT_RPM")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_RPM)
}

impl RateLimiter {
    /// Build a limiter that backs its buckets with the gateway's existing
    /// Dragonfly connection (reused from the [`crate::cache::ResultCache`]).
    ///
    /// When the cache is disabled (connection `None`) the limiter runs purely
    /// in-process — the fail-open fallback path. The RPM is read from
    /// `GATEWAY_RATE_LIMIT_RPM` ([`DEFAULT_RPM`] otherwise).
    pub(crate) fn from_cache(cache: &crate::cache::ResultCache) -> Self {
        Self {
            conn: cache.connection(),
            buckets: Arc::new(DashMap::new()),
            rpm: rpm_from_env(),
        }
    }

    /// Try to consume one token for `key`. Returns `Ok(())` if allowed, or
    /// `Err(retry_after_secs)` (>= 1) if the bucket is exhausted.
    ///
    /// Prefers the distributed (Dragonfly) bucket so the limit holds fleet-wide.
    /// Any connection error — or no connection at all — transparently falls back
    /// to the in-process bucket (fail-open: a Redis blip never rejects traffic).
    async fn try_acquire(&self, key: &str) -> Result<(), u64> {
        if let Some(conn) = self.conn.clone() {
            match tokio::time::timeout(
                DISTRIBUTED_RATE_LIMIT_TIMEOUT,
                self.try_acquire_distributed(conn, key),
            )
            .await
            {
                Ok(Ok(result)) => return result,
                Ok(Err(error)) => {
                    // Degrade to the in-process bucket rather than failing the
                    // request. Log the error class only — never the key value.
                    tracing::warn!(
                        %error,
                        "distributed rate-limit backend errored; falling back to in-process bucket"
                    );
                }
                Err(_) => {
                    tracing::warn!(
                        timeout_ms = DISTRIBUTED_RATE_LIMIT_TIMEOUT.as_millis(),
                        "distributed rate-limit backend timed out; falling back to in-process bucket"
                    );
                }
            }
        }
        self.try_acquire_local(key)
    }

    /// Distributed token bucket: one atomic Lua `EVAL` against Dragonfly.
    async fn try_acquire_distributed(
        &self,
        mut conn: redis::aio::ConnectionManager,
        key: &str,
    ) -> Result<Result<(), u64>, redis::RedisError> {
        let refill_rate = self.rpm / 60.0; // tokens per second
                                           // TTL: long enough that an idle bucket isn't reaped mid-burst, bounded
                                           // so abandoned keys self-evict. One full refill window + a margin.
        let ttl_secs = (self.rpm / refill_rate).ceil() as u64 + 60;
        let redis_key = distributed_key(key);

        let (allowed, retry_after): (i64, i64) = Script::new(TOKEN_BUCKET_LUA)
            .key(redis_key)
            .arg(self.rpm)
            .arg(refill_rate)
            .arg(ttl_secs)
            .invoke_async(&mut conn)
            .await?;

        if allowed == 1 {
            Ok(Ok(()))
        } else {
            Ok(Err((retry_after.max(1)) as u64))
        }
    }

    /// In-process token bucket (per-instance fallback). The original behaviour.
    fn try_acquire_local(&self, key: &str) -> Result<(), u64> {
        let now = Instant::now();
        let refill_rate = self.rpm / 60.0; // tokens per second

        let mut entry = self
            .buckets
            .entry(key.to_owned())
            .or_insert_with(|| TokenBucket {
                tokens: self.rpm,
                last_refill: now,
            });

        let bucket = entry.value_mut();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * refill_rate).min(self.rpm);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let deficit = 1.0 - bucket.tokens;
            let retry_after = Duration::from_secs_f64((deficit / refill_rate).max(0.0)).as_secs();
            Err(retry_after.max(1))
        }
    }
}

/// Namespaced Dragonfly key for a caller's bucket. Shares the `velion:gw:`
/// prefix convention with [`crate::cache::cache_key`] and carries `ratelimit`
/// so operators can scan/inspect throttling state (`KEYS '*ratelimit*'`).
fn distributed_key(key: &str) -> String {
    format!("velion:gw:ratelimit:{key}")
}

/// Derive the rate-limit key for a request, preferring validated identity.
///
/// `user` is the `AuthenticatedUser` extension when `require_session` ran before
/// this layer; `None` on pre-auth routes. Returns an owned key plus a static
/// label describing its source (for structured logging only — never the value).
fn rate_limit_key(user: Option<&AuthenticatedUser>, headers: &HeaderMap) -> (String, &'static str) {
    if let Some(user) = user {
        if let Some(org) = user
            .authorized_membership
            .as_ref()
            .map(|membership| membership.organization_id.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return (format!("org:{org}"), "org");
        }
        if !user.user_id.trim().is_empty() {
            return (format!("user:{}", user.user_id.trim()), "user");
        }
    }

    match client_ip(headers) {
        Some(ip) => (format!("ip:{ip}"), "ip"),
        None => ("anonymous".to_owned(), "anonymous"),
    }
}

/// Extract a validated client IP from the forwarded-for chain.
///
/// Prefers the first entry of `x-forwarded-for` (the original client per the de
/// facto `client, proxy1, proxy2` convention), falling back to `x-real-ip`. The
/// candidate must parse as a real `IpAddr`; anything else is rejected so a
/// hostile value cannot become an arbitrary bucket key. The header itself is
/// only trusted because untrusted inbound identity headers are stripped at
/// ingress and the gateway sits behind a reverse proxy that sets it.
fn client_ip(headers: &HeaderMap) -> Option<IpAddr> {
    let from_forwarded = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= MAX_FORWARDED_LEN)
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(parse_ip);

    from_forwarded.or_else(|| {
        headers
            .get("x-real-ip")
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() <= MAX_FORWARDED_LEN)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(parse_ip)
    })
}

/// Parse a forwarded-for entry into an `IpAddr`, tolerating a `[v6]:port` or
/// `v4:port` suffix that some proxies append.
fn parse_ip(candidate: &str) -> Option<IpAddr> {
    if let Ok(ip) = candidate.parse::<IpAddr>() {
        return Some(ip);
    }
    // `[2001:db8::1]:443` form.
    if let Some(inner) = candidate
        .strip_prefix('[')
        .and_then(|rest| rest.split(']').next())
    {
        if let Ok(ip) = inner.parse::<IpAddr>() {
            return Some(ip);
        }
    }
    // `203.0.113.7:443` form — strip a trailing `:port` only for IPv4-looking
    // values (an unbracketed colon in IPv6 is ambiguous, so leave those alone).
    if let Some((host, _port)) = candidate.rsplit_once(':') {
        if let Ok(ip @ IpAddr::V4(_)) = host.parse::<IpAddr>() {
            return Some(ip);
        }
    }
    None
}

/// Axum middleware enforcing the per-key inbound rate limit.
///
/// Pass-through when no [`RateLimiter`] is present in extensions (limiter not
/// wired) so the gateway fails open rather than rejecting all traffic on a
/// wiring mistake.
pub(crate) async fn rate_limit_middleware(request: Request, next: Next) -> Response {
    let Some(limiter) = request.extensions().get::<RateLimiter>().cloned() else {
        return next.run(request).await;
    };

    // Never rate-limit the session status/recovery poll. The SPA calls it to
    // discover that it is logged out and to recover; because this layer keys by
    // IP (it runs before `require_session`), letting a burst from one IP 429 it
    // would lock every co-located client out of the very endpoint they need to
    // log back in. It is a cheap, side-effect-free read auth-core can absorb.
    if is_rate_limit_exempt_path(request.uri().path()) {
        return next.run(request).await;
    }

    let user = request.extensions().get::<AuthenticatedUser>().cloned();
    let (key, source) = rate_limit_key(user.as_ref(), request.headers());

    match limiter.try_acquire(&key).await {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            // Log the key SOURCE, never the key value (it can be a user/org id).
            tracing::warn!(
                key_source = source,
                retry_after = retry_after,
                "inbound rate limit exceeded"
            );
            (
                StatusCode::TOO_MANY_REQUESTS,
                [("Retry-After", retry_after.to_string())],
                "rate limit exceeded",
            )
                .into_response()
        }
    }
}

fn is_rate_limit_exempt_path(path: &str) -> bool {
    matches!(path, "/health" | "/metrics" | "/api/v1/auth/session")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    /// In-process (fallback-path) limiter for the bucket-arithmetic tests. The
    /// distributed path is exercised live against Dragonfly in the rebuild/verify
    /// step; the Lua arithmetic mirrors `try_acquire_local` exactly.
    fn limiter(rpm: f64) -> RateLimiter {
        RateLimiter {
            conn: None,
            buckets: Arc::new(DashMap::new()),
            rpm,
        }
    }

    #[test]
    fn allows_requests_within_limit() {
        let limiter = limiter(10.0);
        for _ in 0..10 {
            assert!(limiter.try_acquire_local("org:org_1").is_ok());
        }
    }

    #[test]
    fn rejects_when_exhausted_with_retry_after() {
        let limiter = limiter(2.0);
        assert!(limiter.try_acquire_local("user:u1").is_ok());
        assert!(limiter.try_acquire_local("user:u1").is_ok());
        let retry_after = limiter
            .try_acquire_local("user:u1")
            .expect_err("third request must be throttled");
        assert!(retry_after >= 1, "Retry-After must be at least 1 second");
    }

    #[test]
    fn isolates_keys() {
        let limiter = limiter(1.0);
        assert!(limiter.try_acquire_local("ip:1.1.1.1").is_ok());
        assert!(limiter.try_acquire_local("ip:2.2.2.2").is_ok());
        assert!(limiter.try_acquire_local("ip:1.1.1.1").is_err());
    }

    #[test]
    fn refills_over_time() {
        // rpm == 1 means the bucket starts with a single token and refills at
        // 1/60 token per second. Exhaust it, rewind the refill clock by a full
        // minute to simulate elapsed time, and confirm a token has returned.
        let limiter = limiter(1.0);
        assert!(limiter.try_acquire_local("org:refill").is_ok());
        assert!(limiter.try_acquire_local("org:refill").is_err());

        if let Some(mut bucket) = limiter.buckets.get_mut("org:refill") {
            bucket.last_refill = Instant::now() - Duration::from_secs(60);
        }
        assert!(
            limiter.try_acquire_local("org:refill").is_ok(),
            "bucket should refill after elapsed time"
        );
    }

    #[tokio::test]
    async fn try_acquire_falls_back_to_local_without_conn() {
        // No Dragonfly connection ⇒ the async entrypoint must transparently use
        // the in-process bucket and still throttle at the limit.
        let limiter = limiter(1.0);
        assert!(limiter.try_acquire("org:fallback").await.is_ok());
        assert!(limiter.try_acquire("org:fallback").await.is_err());
    }

    #[test]
    fn operational_endpoints_never_depend_on_the_rate_limit_backend() {
        assert!(is_rate_limit_exempt_path("/health"));
        assert!(is_rate_limit_exempt_path("/metrics"));
        assert!(is_rate_limit_exempt_path("/api/v1/auth/session"));
        assert!(!is_rate_limit_exempt_path("/api/v1/sessions/bootstrap"));
    }

    #[test]
    fn distributed_rate_limit_call_has_a_short_deadline() {
        assert!(DISTRIBUTED_RATE_LIMIT_TIMEOUT <= Duration::from_secs(1));
    }

    #[test]
    fn distributed_key_is_namespaced_and_scannable() {
        // Operators scan throttling state with `KEYS '*ratelimit*'`.
        assert_eq!(
            distributed_key("org:org_42"),
            "velion:gw:ratelimit:org:org_42"
        );
        assert!(distributed_key("ip:1.2.3.4").contains("ratelimit"));
    }

    #[test]
    fn from_env_rejects_non_positive_and_keeps_default() {
        // Set a bogus value; RPM parsing must fall back to DEFAULT_RPM. Built via
        // from_cache against a disabled cache (no connection ⇒ in-process path).
        std::env::set_var("GATEWAY_RATE_LIMIT_RPM", "not-a-number");
        let limiter = RateLimiter::from_cache(&crate::cache::ResultCache::disabled());
        assert_eq!(limiter.rpm, DEFAULT_RPM);
        std::env::remove_var("GATEWAY_RATE_LIMIT_RPM");
    }

    #[test]
    fn prefers_org_then_user_over_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.7"));

        let with_org = AuthenticatedUser {
            user_id: "u1".into(),
            user_email: String::new(),
            user_name: String::new(),
            user_image: None,
            email_verified: true,
            auth_role: None,
            active_org_id: Some("org_42".into()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org_42".into(),
                role: "member".into(),
            }),
        };
        let (key, source) = rate_limit_key(Some(&with_org), &headers);
        assert_eq!(key, "org:org_42");
        assert_eq!(source, "org");

        let no_org = AuthenticatedUser {
            active_org_id: None,
            authorized_membership: None,
            ..with_org
        };
        let (key, source) = rate_limit_key(Some(&no_org), &headers);
        assert_eq!(key, "user:u1");
        assert_eq!(source, "user");
    }

    #[test]
    fn falls_back_to_client_ip_pre_auth() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("198.51.100.9, 10.0.0.1, 10.0.0.2"),
        );
        let (key, source) = rate_limit_key(None, &headers);
        assert_eq!(key, "ip:198.51.100.9");
        assert_eq!(source, "ip");
    }

    #[test]
    fn falls_back_to_real_ip_then_anonymous() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.42"));
        let (key, _) = rate_limit_key(None, &headers);
        assert_eq!(key, "ip:198.51.100.42");

        let (key, source) = rate_limit_key(None, &HeaderMap::new());
        assert_eq!(key, "anonymous");
        assert_eq!(source, "anonymous");
    }

    #[test]
    fn rejects_garbage_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("not-an-ip; rm -rf"),
        );
        // Garbage candidate is rejected → no valid IP → anonymous bucket.
        let (key, source) = rate_limit_key(None, &headers);
        assert_eq!(key, "anonymous");
        assert_eq!(source, "anonymous");
    }

    #[test]
    fn strips_port_suffix() {
        assert_eq!(parse_ip("203.0.113.7:443"), "203.0.113.7".parse().ok());
        assert_eq!(parse_ip("[2001:db8::1]:443"), "2001:db8::1".parse().ok());
        assert_eq!(parse_ip("2001:db8::1"), "2001:db8::1".parse().ok());
    }
}
