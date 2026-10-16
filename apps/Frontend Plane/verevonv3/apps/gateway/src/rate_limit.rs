//! Inbound token-bucket rate limiter for the verevonv3 BFF gateway.
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
//!
//! ## Rate classes
//!
//! The limiter above is one bucket per caller for everything, keyed by IP
//! before authentication. On top of it, routes that differ a lot in cost carry
//! a **class** limiter ([`RateClass`], applied with [`with_rate_class`] inside
//! `require_session`), keyed by the organization and kept in its own bucket and
//! its own Dragonfly key (`verevon:gw:ratelimit:{class}:{key}`), so spending one
//! class never spends another:
//!
//! | class | covers | default per org per minute | env |
//! |---|---|---|---|
//! | `interactive` | everything inside an open browser session | 1200 | `GATEWAY_RATE_LIMIT_RPM_INTERACTIVE` |
//! | `browser_start` | creating a session, starting an AI run | 30 | `GATEWAY_RATE_LIMIT_RPM_BROWSER_START` |
//! | `job_submit` | starting or discovering a crawl | 20 | `GATEWAY_RATE_LIMIT_RPM_JOB_SUBMIT` |
//! | `search_scrape` | search, scrape, product extraction | 120 | `GATEWAY_RATE_LIMIT_RPM_SEARCH_SCRAPE` |
//!
//! A value that is not a positive number keeps the class default (it never
//! disables a limit). `GATEWAY_RATE_CLASSES=off` removes the class layers and
//! leaves only the default limiter. A denied request is `429` with
//! `Retry-After`, as before.

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
    /// `None` for the default (per-IP, pre-auth) limiter; `Some` for a class
    /// limiter, which owns its buckets and its Dragonfly keys so one class can
    /// never spend another's budget.
    class: Option<RateClass>,
}

/// A kind of request with its own per-organization budget.
///
/// The default limiter is one bucket per caller for everything, so a burst of
/// session creation or crawl submissions can starve interactive browsing, and
/// the other way round. Each class below has its own bucket and its own key
/// (`verevon:gw:ratelimit:{class}:{key}`), and is applied inside
/// `require_session`, so the key is the organization, not the IP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateClass {
    /// Everything a user does inside an open browser session.
    Interactive,
    /// Creating a browser session or starting an AI browser run.
    BrowserStart,
    /// Submitting a crawl.
    JobSubmit,
    /// Search and scrape requests.
    SearchScrape,
}

impl RateClass {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::BrowserStart => "browser_start",
            Self::JobSubmit => "job_submit",
            Self::SearchScrape => "search_scrape",
        }
    }

    /// Requests per organization per minute when the env var is unset.
    pub(crate) const fn default_rpm(self) -> f64 {
        match self {
            Self::Interactive => 1200.0,
            Self::BrowserStart => 30.0,
            Self::JobSubmit => 20.0,
            Self::SearchScrape => 120.0,
        }
    }

    fn env_name(self) -> String {
        format!(
            "GATEWAY_RATE_LIMIT_RPM_{}",
            self.name().to_ascii_uppercase()
        )
    }
}

/// `GATEWAY_RATE_CLASSES=off` removes the class layers and leaves only the
/// default limiter. Anything else (including unset) keeps them on.
fn rate_classes_enabled() -> bool {
    !std::env::var("GATEWAY_RATE_CLASSES").is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        )
    })
}

fn positive_rpm(raw: &str) -> Option<f64> {
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
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
            class: None,
        }
    }

    /// A limiter for `class` that shares this one's Dragonfly connection but
    /// not its buckets or keys. The rate comes from
    /// `GATEWAY_RATE_LIMIT_RPM_<CLASS>` (a non-positive or unparseable value
    /// keeps the class default rather than disabling it). `None` when
    /// `GATEWAY_RATE_CLASSES=off`.
    pub(crate) fn for_class(&self, class: RateClass) -> Option<Self> {
        if !rate_classes_enabled() {
            return None;
        }
        let rpm = std::env::var(class.env_name())
            .ok()
            .and_then(|raw| positive_rpm(&raw))
            .unwrap_or_else(|| class.default_rpm());
        Some(Self {
            conn: self.conn.clone(),
            buckets: Arc::new(DashMap::new()),
            rpm,
            class: Some(class),
        })
    }

    /// Build a limiter sharing the same distributed backend as [`from_cache`]
    /// but with its own fixed rate rather than `GATEWAY_RATE_LIMIT_RPM` — for a
    /// narrower caller (e.g. mail-task's per-user ingest ceiling) that needs a
    /// much lower bound than the fleet-wide inbound limit. `rpm` must be > 0;
    /// non-positive values fall back to [`DEFAULT_RPM`] like the env parser
    /// does, rather than silently disabling the limit.
    pub(crate) fn from_cache_with_rpm(cache: &crate::cache::ResultCache, rpm: f64) -> Self {
        Self {
            conn: cache.connection(),
            buckets: Arc::new(DashMap::new()),
            rpm: if rpm.is_finite() && rpm > 0.0 {
                rpm
            } else {
                DEFAULT_RPM
            },
            class: None,
        }
    }

    /// Try to consume one token for `key`. Returns `Ok(())` if allowed, or
    /// `Err(retry_after_secs)` (>= 1) if the bucket is exhausted.
    ///
    /// Prefers the distributed (Dragonfly) bucket so the limit holds fleet-wide.
    /// Any connection error — or no connection at all — transparently falls back
    /// to the in-process bucket (fail-open: a Redis blip never rejects traffic).
    pub(crate) async fn try_acquire(&self, key: &str) -> Result<(), u64> {
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
        let redis_key = distributed_key_for(self.class, key);

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

/// Namespaced Dragonfly key for a caller's bucket. Shares the `verevon:gw:`
/// prefix convention with [`crate::cache::cache_key`] and carries `ratelimit`
/// so operators can scan/inspect throttling state (`KEYS '*ratelimit*'`).
/// The default limiter's key is unchanged (so live buckets survive a deploy); a
/// class limiter inserts its class name: `verevon:gw:ratelimit:{class}:{key}`.
fn distributed_key_for(class: Option<RateClass>, key: &str) -> String {
    match class {
        None => format!("verevon:gw:ratelimit:{key}"),
        Some(class) => format!("verevon:gw:ratelimit:{}:{key}", class.name()),
    }
}

/// Put `class`'s limiter in front of `routes`. It must be applied to routes
/// that are later wrapped in `require_session`, so `AuthenticatedUser` is
/// present and the key is the organization. Layer order matters: the
/// middleware is innermost, the extension that selects the class limiter wraps
/// it, and `require_session` (added by the caller) is outermost.
pub(crate) fn with_rate_class<S>(
    routes: axum::Router<S>,
    default_limiter: &RateLimiter,
    class: RateClass,
) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    match default_limiter.for_class(class) {
        None => routes,
        Some(limiter) => routes
            .route_layer(axum::middleware::from_fn(rate_limit_middleware))
            .route_layer(axum::Extension(limiter)),
    }
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
    matches!(
        path,
        "/health" | "/version" | "/metrics" | "/api/v1/auth/session"
    )
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
            class: None,
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
        assert!(is_rate_limit_exempt_path("/version"));
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
            distributed_key_for(None, "org:org_42"),
            "verevon:gw:ratelimit:org:org_42"
        );
        assert!(distributed_key_for(None, "ip:1.2.3.4").contains("ratelimit"));
    }

    #[test]
    fn distributed_key_includes_the_class_and_the_default_key_is_unchanged() {
        // The default key must not move, or every live bucket resets on deploy.
        assert_eq!(
            distributed_key_for(None, "org:org_42"),
            "verevon:gw:ratelimit:org:org_42"
        );
        for (class, name) in [
            (RateClass::Interactive, "interactive"),
            (RateClass::BrowserStart, "browser_start"),
            (RateClass::JobSubmit, "job_submit"),
            (RateClass::SearchScrape, "search_scrape"),
        ] {
            let key = distributed_key_for(Some(class), "org:org_42");
            assert_eq!(key, format!("verevon:gw:ratelimit:{name}:org:org_42"));
            // Still found by the operators' `KEYS '*ratelimit*'` scan, and never
            // equal to the default key a class could otherwise collide with.
            assert!(key.contains("ratelimit"));
            assert_ne!(key, distributed_key_for(None, "org:org_42"));
        }
        // A class name cannot be forged into another class's key through the
        // caller key: the caller key is always `org:`, `user:`, `ip:` or `anonymous`.
        assert_ne!(
            distributed_key_for(Some(RateClass::JobSubmit), "x"),
            distributed_key_for(Some(RateClass::SearchScrape), "x")
        );
    }

    fn classed(class: RateClass, rpm: f64) -> RateLimiter {
        RateLimiter {
            conn: None,
            buckets: Arc::new(DashMap::new()),
            rpm,
            class: Some(class),
        }
    }

    #[tokio::test]
    async fn class_limiters_have_independent_buckets_for_the_same_org() {
        let start = classed(RateClass::BrowserStart, 1.0);
        let interactive = classed(RateClass::Interactive, 1.0);
        assert!(start.try_acquire("org:org_a").await.is_ok());
        assert!(start.try_acquire("org:org_a").await.is_err());
        assert!(
            interactive.try_acquire("org:org_a").await.is_ok(),
            "exhausting browser_start must not spend interactive"
        );
    }

    #[test]
    fn class_defaults_and_names_are_the_documented_ones() {
        assert_eq!(RateClass::Interactive.default_rpm(), 1200.0);
        assert_eq!(RateClass::BrowserStart.default_rpm(), 30.0);
        assert_eq!(RateClass::JobSubmit.default_rpm(), 20.0);
        assert_eq!(RateClass::SearchScrape.default_rpm(), 120.0);
        assert_eq!(
            RateClass::BrowserStart.env_name(),
            "GATEWAY_RATE_LIMIT_RPM_BROWSER_START"
        );
    }

    #[test]
    fn for_class_reads_env_ignores_garbage_and_honours_the_off_switch() {
        let _env = crate::config::TEST_ENV_LOCK.blocking_lock();
        let default = limiter(600.0);
        std::env::remove_var("GATEWAY_RATE_CLASSES");

        std::env::remove_var("GATEWAY_RATE_LIMIT_RPM_JOB_SUBMIT");
        assert_eq!(default.for_class(RateClass::JobSubmit).unwrap().rpm, 20.0);

        std::env::set_var("GATEWAY_RATE_LIMIT_RPM_JOB_SUBMIT", "3");
        let limiter = default.for_class(RateClass::JobSubmit).unwrap();
        assert_eq!(
            (limiter.rpm, limiter.class),
            (3.0, Some(RateClass::JobSubmit))
        );

        for bad in ["0", "-4", "nope", "NaN", ""] {
            std::env::set_var("GATEWAY_RATE_LIMIT_RPM_JOB_SUBMIT", bad);
            assert_eq!(
                default.for_class(RateClass::JobSubmit).unwrap().rpm,
                20.0,
                "{bad:?} must keep the class default, not disable the limit"
            );
        }
        std::env::remove_var("GATEWAY_RATE_LIMIT_RPM_JOB_SUBMIT");

        for off in ["off", "OFF", "0", "false", "no"] {
            std::env::set_var("GATEWAY_RATE_CLASSES", off);
            assert!(default.for_class(RateClass::Interactive).is_none(), "{off}");
        }
        for on in ["on", "1", "", "anything-else"] {
            std::env::set_var("GATEWAY_RATE_CLASSES", on);
            assert!(
                default.for_class(RateClass::Interactive).is_some(),
                "{on:?}"
            );
        }
        std::env::remove_var("GATEWAY_RATE_CLASSES");
    }

    // ---- the layered stack ----

    mod stack {
        use super::*;
        use axum::{
            body::Body,
            http::{Request as HttpRequest, StatusCode},
            routing::{get, post},
            Router,
        };
        use tower::ServiceExt;

        fn user(org: &str) -> AuthenticatedUser {
            AuthenticatedUser {
                user_id: format!("user-of-{org}"),
                user_email: String::new(),
                user_name: String::new(),
                user_image: None,
                email_verified: true,
                auth_role: None,
                active_org_id: Some(org.to_owned()),
                authorized_membership: Some(crate::middleware::AuthorizedMembership {
                    organization_id: org.to_owned(),
                    role: "member".into(),
                }),
            }
        }

        /// A stand-in for `require_session`: the org comes from a test header and
        /// is attached as the validated identity, outermost, as in production.
        async fn fake_session(
            mut request: axum::extract::Request,
            next: axum::middleware::Next,
        ) -> axum::response::Response {
            if let Some(org) = request
                .headers()
                .get("x-test-org")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
            {
                request.extensions_mut().insert(user(&org));
            }
            next.run(request).await
        }

        async fn ok() -> &'static str {
            "ok"
        }

        /// Two classes behind one session layer, built the way the domain
        /// routers build them. The env is read at construction, under the lock.
        async fn app(rpm_env: &[(&str, &str)]) -> Router {
            let _env = crate::config::TEST_ENV_LOCK.lock().await;
            std::env::remove_var("GATEWAY_RATE_CLASSES");
            for (name, value) in rpm_env {
                std::env::set_var(name, value);
            }
            let default = limiter(600.0);
            let job = Router::new().route("/job", post(ok));
            let interactive = Router::new().route("/frame", get(ok));
            let app = Router::new()
                .merge(with_rate_class(job, &default, RateClass::JobSubmit))
                .merge(with_rate_class(
                    interactive,
                    &default,
                    RateClass::Interactive,
                ))
                .route_layer(axum::middleware::from_fn(fake_session));
            for (name, _) in rpm_env {
                std::env::remove_var(name);
            }
            app
        }

        async fn call(
            app: &Router,
            method: &str,
            path: &str,
            org: &str,
            ip: &str,
        ) -> (StatusCode, Option<String>) {
            let response = app
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .method(method)
                        .uri(path)
                        .header("x-test-org", org)
                        .header("x-forwarded-for", ip)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let retry = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            (response.status(), retry)
        }

        #[tokio::test]
        async fn interactive_class_is_not_exhausted_by_job_submit() {
            let app = app(&[("GATEWAY_RATE_LIMIT_RPM_JOB_SUBMIT", "2")]).await;
            assert_eq!(
                call(&app, "POST", "/job", "org_a", "198.51.100.1").await.0,
                StatusCode::OK
            );
            assert_eq!(
                call(&app, "POST", "/job", "org_a", "198.51.100.1").await.0,
                StatusCode::OK
            );
            let (status, retry) = call(&app, "POST", "/job", "org_a", "198.51.100.1").await;
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
            assert!(retry.is_some_and(|value| value.parse::<u64>().is_ok_and(|secs| secs >= 1)));
            for _ in 0..20 {
                assert_eq!(
                    call(&app, "GET", "/frame", "org_a", "198.51.100.1").await.0,
                    StatusCode::OK,
                    "interactive browsing continues while job_submit is exhausted"
                );
            }
        }

        #[tokio::test]
        async fn class_key_is_the_org_not_the_ip_after_require_session() {
            let app = app(&[("GATEWAY_RATE_LIMIT_RPM_JOB_SUBMIT", "1")]).await;
            assert_eq!(
                call(&app, "POST", "/job", "org_a", "198.51.100.1").await.0,
                StatusCode::OK
            );
            // Same org from a different IP is the same bucket.
            assert_eq!(
                call(&app, "POST", "/job", "org_a", "203.0.113.77").await.0,
                StatusCode::TOO_MANY_REQUESTS
            );
            // A different org behind the same IP is its own bucket.
            assert_eq!(
                call(&app, "POST", "/job", "org_b", "198.51.100.1").await.0,
                StatusCode::OK
            );
        }

        #[tokio::test]
        async fn session_create_is_limited_per_org_while_polling_continues() {
            let app = app(&[
                ("GATEWAY_RATE_LIMIT_RPM_JOB_SUBMIT", "3"),
                ("GATEWAY_RATE_LIMIT_RPM_INTERACTIVE", "1000"),
            ])
            .await;
            let mut statuses = Vec::new();
            for _ in 0..4 {
                statuses.push(call(&app, "POST", "/job", "org_a", "198.51.100.1").await.0);
            }
            assert_eq!(
                statuses,
                [
                    StatusCode::OK,
                    StatusCode::OK,
                    StatusCode::OK,
                    StatusCode::TOO_MANY_REQUESTS
                ]
            );
            assert_eq!(
                call(&app, "GET", "/frame", "org_a", "198.51.100.1").await.0,
                StatusCode::OK
            );
            assert_eq!(
                call(&app, "POST", "/job", "org_b", "198.51.100.1").await.0,
                StatusCode::OK
            );
        }

        #[tokio::test]
        async fn off_switch_removes_the_class_layers() {
            let _env = crate::config::TEST_ENV_LOCK.lock().await;
            std::env::set_var("GATEWAY_RATE_CLASSES", "off");
            std::env::set_var("GATEWAY_RATE_LIMIT_RPM_JOB_SUBMIT", "1");
            let default = limiter(600.0);
            let job = Router::new().route("/job", post(ok));
            let app = with_rate_class(job, &default, RateClass::JobSubmit)
                .route_layer(axum::middleware::from_fn(fake_session));
            std::env::remove_var("GATEWAY_RATE_CLASSES");
            std::env::remove_var("GATEWAY_RATE_LIMIT_RPM_JOB_SUBMIT");
            for _ in 0..5 {
                assert_eq!(
                    call(&app, "POST", "/job", "org_a", "198.51.100.1").await.0,
                    StatusCode::OK
                );
            }
        }
    }

    #[test]
    fn from_env_rejects_non_positive_and_keeps_default() {
        // GATEWAY_RATE_LIMIT_RPM is process-global: without this lock a
        // concurrent test's request is limited against the bogus value set
        // below and fails with 503. `blocking_lock` because this is a plain
        // `#[test]` with no runtime to await on.
        let _env = crate::config::TEST_ENV_LOCK.blocking_lock();
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
