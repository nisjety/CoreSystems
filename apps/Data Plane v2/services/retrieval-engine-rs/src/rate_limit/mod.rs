//! §16.5.1 / P2-6 — per-org rate limiting, backed by Dragonfly.
//!
//! The previous version keyed an in-process `governor::RateLimiter` by
//! `org_id` — correct for a single replica, wrong the moment there is more
//! than one: each replica held its own independent token bucket, unaware of
//! the others, so the ACTUAL cluster-wide limit for an org was
//! `configured_limit * replica_count`, silently scaling with however many
//! pods happened to be up. Moving the bucket state into Dragonfly (already
//! this service's shared cache backend) gives every replica the same view.
//!
//! **Algorithm.** A distributed token bucket, evaluated atomically by a Lua
//! script (`EVAL` on Redis/Dragonfly runs the whole script as one atomic
//! step, so two replicas checking the same org concurrently cannot both
//! observe the same pre-decrement token count). Tokens refill continuously
//! and proportionally to elapsed wall-clock time rather than in discrete
//! windows, so there is no window-boundary double-burst the way a naive
//! fixed-window counter would have — this reproduces the steady-rate + burst
//! shape of the `governor::Quota::per_second(rps).allow_burst(burst)` it
//! replaces, just evaluated centrally instead of per-replica.
//!
//! - `DPV2_RATE_LIMIT_PER_ORG_RPS` — sustained requests/sec per org (default 20)
//! - `DPV2_RATE_LIMIT_PER_ORG_BURST` — burst size per org (default 40)
//! - `DRAGONFLY_URL` / `CACHE_URL` — same lookup order as `Config::redis_url`,
//!   so this points at the same Dragonfly instance without a dedicated env var
//!
//! Anonymous / unauthenticated callers all share the bucket keyed by the
//! literal string `"anonymous"`, which is what we want — a flood of
//! unauth'd requests must not crowd out logged-in orgs.
//!
//! **Fail-open.** Any Dragonfly error — unreachable, connection refused,
//! script failure — allows the request through rather than rejecting every
//! caller because an ancillary fairness mechanism is degraded. This matches
//! the crate's existing convention for Dragonfly dependencies (`CacheLayer`
//! degrades to a no-op cache the same way; see `main.rs`). A rate limiter is
//! a defense against one noisy tenant, not a security boundary — availability
//! is the right thing to preserve when it is unavailable, not strict fairness.
//! `record_rate_limit_backend_unavailable` makes this observable rather than
//! a silent, permanent no-op.
//!
//! Fail-open only helps if it happens *fast*. The connect-and-check round
//! trip is wrapped in a 250ms `tokio::time::timeout` (`PerOrgLimiter::
//! BACKEND_TIMEOUT`) for exactly this reason: measured empirically against a
//! closed port, `redis::aio::ConnectionManager::new` took **473 seconds** to
//! give up on its own internal reconnect attempts before returning an error.
//! Without the outer bound, "fail open" would still mean an 8-minute stall on
//! every request through a dead Dragonfly — indistinguishable from a total
//! outage, not the graceful degradation this design is supposed to provide.
//!
//! The connection itself is established lazily, on the first request that
//! needs it, via `tokio::sync::OnceCell` — `get_or_try_init` does not cache a
//! failure, so a transient outage during startup retries on the very next
//! request instead of disabling the limiter for the service's remaining
//! lifetime. This also keeps `from_env()` synchronous, so the router's
//! existing `middleware::from_fn_with_state(PerOrgLimiter::from_env(), ...)`
//! call site needed no change.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use redis::aio::ConnectionManager;
use tokio::sync::OnceCell;

use crate::authz::AuthContext;

/// KEYS[1] = bucket key. ARGV: now_ms (i64), rps (f64), burst (f64),
/// ttl_seconds (i64). Returns 1 (allowed, and a token was consumed) or 0
/// (denied, bucket left untouched aside from the refill-and-timestamp update).
const TOKEN_BUCKET_SCRIPT: &str = r#"
local now = tonumber(ARGV[1])
local rps = tonumber(ARGV[2])
local burst = tonumber(ARGV[3])
local ttl = tonumber(ARGV[4])

local data = redis.call('HMGET', KEYS[1], 'tokens', 'ts')
local tokens = tonumber(data[1])
local ts = tonumber(data[2])
if tokens == nil then
  tokens = burst
  ts = now
end

local elapsed_ms = now - ts
if elapsed_ms < 0 then elapsed_ms = 0 end
tokens = math.min(burst, tokens + (elapsed_ms / 1000.0) * rps)

local allowed = 0
if tokens >= 1.0 then
  tokens = tokens - 1.0
  allowed = 1
end

redis.call('HSET', KEYS[1], 'tokens', tostring(tokens), 'ts', tostring(now))
redis.call('EXPIRE', KEYS[1], ttl)
return allowed
"#;

struct LimiterInner {
    redis_url: String,
    rps: f64,
    burst: f64,
    ttl_seconds: i64,
    conn: OnceCell<ConnectionManager>,
    script: redis::Script,
}

#[derive(Clone)]
pub struct PerOrgLimiter {
    inner: Arc<LimiterInner>,
}

impl PerOrgLimiter {
    pub fn from_env() -> Self {
        let rps = std::env::var("DPV2_RATE_LIMIT_PER_ORG_RPS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(20.0_f64);
        let burst = std::env::var("DPV2_RATE_LIMIT_PER_ORG_BURST")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(40.0_f64);
        let redis_url = std::env::var("DRAGONFLY_URL")
            .or_else(|_| std::env::var("CACHE_URL"))
            .unwrap_or_else(|_| "redis://localhost:6379".to_string());
        Self::with_config(rps, burst, redis_url)
    }

    /// Separated from `from_env` so the clamping/TTL math is unit-testable
    /// as a pure function of its arguments. `std::env::var` is process-global
    /// — the previous test suite drove this through `from_env` with
    /// `std::env::set_var`, which raced under Rust's default parallel test
    /// runner (two tests' env vars would interleave mid-read) and produced a
    /// flaky, occasionally-nonsensical TTL. No test touches this directly;
    /// they all call `with_config`.
    fn with_config(rps: f64, burst: f64, redis_url: String) -> Self {
        // A zero/negative misconfiguration must not make every request wait
        // forever for a refill that never accumulates meaningfully.
        let rps = rps.max(0.001_f64);
        let burst = burst.max(1.0_f64);
        // Long enough that a moderately active org's bucket survives between
        // requests; short enough not to accumulate keys for orgs that never
        // come back. An idle bucket expiring and resetting to full burst is
        // harmless — the token-bucket math would have refilled it to full by
        // then regardless of whether the key survived.
        let ttl_seconds = (((burst / rps).ceil() as i64) * 2).max(60);

        Self {
            inner: Arc::new(LimiterInner {
                redis_url,
                rps,
                burst,
                ttl_seconds,
                conn: OnceCell::new(),
                script: redis::Script::new(TOKEN_BUCKET_SCRIPT),
            }),
        }
    }

    async fn connection(&self) -> anyhow::Result<ConnectionManager> {
        let conn_ref = self
            .inner
            .conn
            .get_or_try_init(|| async {
                let client = redis::Client::open(self.inner.redis_url.as_str())?;
                ConnectionManager::new(client).await
            })
            .await?;
        Ok(conn_ref.clone())
    }

    /// Bounds the ENTIRE connect-and-check round trip, not just the script
    /// call. Without this, a hung or unreachable Dragonfly does not fail
    /// open quickly — it fails open *eventually*, after `ConnectionManager`
    /// exhausts its own internal reconnect attempts. Measured empirically at
    /// ~473s against a closed port in this crate's own test, which is an
    /// 8-minute request stall in production, not graceful degradation.
    const BACKEND_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

    async fn check(&self, key: &str) -> bool {
        match tokio::time::timeout(Self::BACKEND_TIMEOUT, self.check_inner(key)).await {
            Ok(allowed) => allowed,
            Err(_) => {
                tracing::warn!("rate limiter backend timed out; failing open");
                crate::metrics::record_rate_limit_backend_unavailable();
                true
            }
        }
    }

    async fn check_inner(&self, key: &str) -> bool {
        let mut conn = match self.connection().await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "rate limiter backend unavailable; failing open");
                crate::metrics::record_rate_limit_backend_unavailable();
                return true;
            }
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let redis_key = format!("dpv2:ratelimit:{key}");
        let result: Result<i64, redis::RedisError> = self
            .inner
            .script
            .key(&redis_key)
            .arg(now_ms)
            .arg(self.inner.rps)
            .arg(self.inner.burst)
            .arg(self.inner.ttl_seconds)
            .invoke_async(&mut conn)
            .await;
        match result {
            Ok(allowed) => allowed == 1,
            Err(e) => {
                tracing::warn!(error = %e, "rate limiter script failed; failing open");
                crate::metrics::record_rate_limit_backend_unavailable();
                true
            }
        }
    }
}

/// Axum middleware: looks up `AuthContext` (set by `auth_middleware`),
/// keys the limiter on `org_id`, and on rejection returns 429 with a
/// `Retry-After: 1` header (best-effort — the token bucket's true wait time
/// depends on the org's own refill rate, and a constant 1s retry is a safe
/// minimum regardless of configured rps).
pub async fn per_org_rate_limit(
    State(limiter): State<PerOrgLimiter>,
    req: Request,
    next: Next,
) -> Response {
    let key = req
        .extensions()
        .get::<AuthContext>()
        .map(|c| c.org_id.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "anonymous".to_string());

    if !limiter.check(&key).await {
        crate::metrics::record_rate_limit_denied(&key);
        let mut resp = (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(serde_json::json!({
                "error": "rate_limited",
                "message": "per-org rate limit exceeded",
                "retry_after_seconds": 1,
            })),
        )
            .into_response();
        resp.headers_mut()
            .insert("Retry-After", HeaderValue::from_static("1"));
        return resp;
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Proves the TTL heuristic keeps its floor and never rounds down to
    /// something shorter than a full refill cycle.
    #[test]
    fn ttl_has_a_sixty_second_floor_regardless_of_a_fast_refill_config() {
        let limiter = PerOrgLimiter::with_config(1000.0, 1.0, "redis://localhost:6379".into());
        assert_eq!(limiter.inner.ttl_seconds, 60);
    }

    /// A zero rps must not divide-by-zero or produce a NaN/infinite TTL —
    /// the `.max(0.001)` guard on rps exists specifically for this input.
    #[test]
    fn zero_rps_is_clamped_rather_than_dividing_by_zero() {
        let limiter = PerOrgLimiter::with_config(0.0, 40.0, "redis://localhost:6379".into());
        assert!(limiter.inner.rps > 0.0);
        assert!(limiter.inner.ttl_seconds.is_positive());
    }

    /// A request against an unreachable Dragonfly must be ALLOWED (fail
    /// open), not rejected — the whole point of the fail-open design. Bounds
    /// its own execution well under `BACKEND_TIMEOUT`'s multiple-of margin so
    /// a regression that removes the internal timeout (reintroducing the
    /// empirically-measured 473s hang) fails this test loudly instead of
    /// just making the suite slow.
    #[tokio::test]
    async fn an_unreachable_backend_fails_open_and_does_so_quickly() {
        let limiter = PerOrgLimiter::with_config(20.0, 40.0, "redis://127.0.0.1:1".into());
        let outcome = tokio::time::timeout(
            PerOrgLimiter::BACKEND_TIMEOUT * 4,
            limiter.check("org-under-test"),
        )
        .await;
        assert_eq!(
            outcome,
            Ok(true),
            "an unreachable rate-limit backend must fail open within a small \
             multiple of BACKEND_TIMEOUT, not hang (this exact class of bug \
             measured at 473s before BACKEND_TIMEOUT was added)"
        );
    }
}
