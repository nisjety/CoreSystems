//! §16.5.1 — per-org rate limiting.
//!
//! Global concurrency caps don't stop one noisy tenant from starving every
//! other org. We key a `governor::RateLimiter` by `org_id` so each tenant
//! gets its own token bucket. Defaults can be overridden per env var; on
//! quota exhaustion we return HTTP 429 with a `Retry-After` header.
//!
//! - `DPV2_RATE_LIMIT_PER_ORG_RPS` — sustained requests/sec per org (default 20)
//! - `DPV2_RATE_LIMIT_PER_ORG_BURST` — burst size per org (default 40)
//!
//! Anonymous / unauthenticated callers all share the bucket keyed by the
//! literal string `"anonymous"`, which is what we want — a flood of
//! unauth'd requests must not crowd out logged-in orgs.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};

use crate::authz::AuthContext;

type OrgKeyedLimiter = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

#[derive(Clone)]
pub struct PerOrgLimiter {
    inner: Arc<OrgKeyedLimiter>,
}

impl PerOrgLimiter {
    pub fn from_env() -> Self {
        let rps: u32 = std::env::var("DPV2_RATE_LIMIT_PER_ORG_RPS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20);
        let burst: u32 = std::env::var("DPV2_RATE_LIMIT_PER_ORG_BURST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(40);
        let rps_nz = NonZeroU32::new(rps.max(1)).unwrap();
        let burst_nz = NonZeroU32::new(burst.max(1)).unwrap();
        let quota = Quota::per_second(rps_nz).allow_burst(burst_nz);
        Self {
            inner: Arc::new(RateLimiter::keyed(quota)),
        }
    }

    fn check(&self, key: &str) -> bool {
        self.inner.check_key(&key.to_string()).is_ok()
    }
}

/// Axum middleware: looks up `AuthContext` (set by `auth_middleware`),
/// keys the limiter on `org_id`, and on rejection returns 429 with a
/// `Retry-After: 1` header (best-effort — governor's wait estimate is
/// monotonic-clock-based and changes shape between releases, so a
/// constant 1s retry is a safe minimum).
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

    if !limiter.check(&key) {
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
