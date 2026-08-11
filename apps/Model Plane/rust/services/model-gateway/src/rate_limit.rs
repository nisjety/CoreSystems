//! Token-bucket rate limiter middleware per `org_id`.
//!
//! Configurable via `RATE_LIMIT_RPM` (requests per minute). Defaults to 100.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;

use crate::auth::Claims;

/// Tracks token count and last refill timestamp for a single bucket.
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

/// Shared rate-limiter state keyed by `org_id`.
#[derive(Clone)]
pub struct RateLimiter {
    buckets: Arc<DashMap<String, TokenBucket>>,
    rpm: f64,
    /// Per-org RPM overrides. Rate limiting is the one part of the old
    /// gateway-owned `OrgPolicy` that genuinely belongs at the edge — it
    /// protects THIS process, so no plane can enforce it on our behalf.
    org_rpm: Arc<DashMap<String, f64>>,
}

impl RateLimiter {
    /// Create a new rate limiter from environment configuration.
    ///
    /// Reads `RATE_LIMIT_RPM` env var, defaulting to 100.
    pub fn from_env() -> Self {
        let rpm: f64 = std::env::var("RATE_LIMIT_RPM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100.0);

        Self {
            buckets: Arc::new(DashMap::new()),
            rpm,
            org_rpm: Arc::new(DashMap::new()),
        }
    }

    /// Set (or clear, with `None`) this org's requests-per-minute ceiling.
    ///
    /// Clearing returns the org to the process-wide `RATE_LIMIT_RPM`. The
    /// existing bucket is dropped so the new ceiling applies from the next
    /// request rather than after the old bucket happens to drain — an operator
    /// who lowers a limit expects it to bite now.
    pub fn set_org_rpm(&self, org_id: &str, rpm: Option<f64>) {
        match rpm {
            Some(rpm) if rpm > 0.0 => {
                self.org_rpm.insert(org_id.to_owned(), rpm);
            }
            _ => {
                self.org_rpm.remove(org_id);
            }
        }
        self.buckets.remove(org_id);
    }

    /// This org's effective ceiling: its override, else the process default.
    #[must_use]
    pub fn org_rpm(&self, org_id: &str) -> f64 {
        self.org_rpm.get(org_id).map_or(self.rpm, |entry| *entry)
    }

    /// Try to consume one token for the given org. Returns `Ok(())` if allowed,
    /// or `Err(retry_after_secs)` if the bucket is exhausted.
    fn try_acquire(&self, org_id: &str) -> Result<(), u64> {
        let now = Instant::now();
        let rpm = self.org_rpm(org_id);
        let refill_rate = rpm / 60.0; // tokens per second

        let mut entry = self
            .buckets
            .entry(org_id.to_owned())
            .or_insert_with(|| TokenBucket {
                tokens: rpm,
                last_refill: now,
            });

        let bucket = entry.value_mut();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * refill_rate).min(rpm);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            // Calculate how long until one token is available
            let deficit = 1.0 - bucket.tokens;
            let retry_after = Duration::from_secs_f64((deficit / refill_rate).max(0.0)).as_secs();
            Err(retry_after.max(1))
        }
    }
}

/// Axum middleware that enforces per-org rate limits.
///
/// Requires `Claims` to be present in request extensions (i.e., auth middleware
/// must run before this layer).
pub async fn rate_limit_middleware(request: Request, next: Next) -> Response {
    let limiter = request.extensions().get::<RateLimiter>().cloned();

    let claims = request.extensions().get::<Claims>().cloned();

    match (limiter, claims) {
        (Some(limiter), Some(claims)) => match limiter.try_acquire(&claims.org_id) {
            Ok(()) => next.run(request).await,
            Err(retry_after) => {
                tracing::warn!(
                    org_id = %claims.org_id,
                    retry_after = retry_after,
                    "rate limit exceeded"
                );
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("Retry-After", retry_after.to_string())],
                    "rate limit exceeded",
                )
                    .into_response()
            }
        },
        _ => {
            // No limiter or no claims — pass through (auth middleware will reject if needed)
            next.run(request).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_requests_within_limit() {
        let limiter = RateLimiter {
            buckets: Arc::new(DashMap::new()),
            org_rpm: Arc::new(DashMap::new()),
            rpm: 10.0,
        };

        for _ in 0..10 {
            assert!(limiter.try_acquire("org_1").is_ok());
        }
    }

    #[test]
    fn rejects_when_exhausted() {
        let limiter = RateLimiter {
            buckets: Arc::new(DashMap::new()),
            org_rpm: Arc::new(DashMap::new()),
            rpm: 2.0,
        };

        assert!(limiter.try_acquire("org_1").is_ok());
        assert!(limiter.try_acquire("org_1").is_ok());
        assert!(limiter.try_acquire("org_1").is_err());
    }

    #[test]
    fn isolates_orgs() {
        let limiter = RateLimiter {
            buckets: Arc::new(DashMap::new()),
            org_rpm: Arc::new(DashMap::new()),
            rpm: 1.0,
        };

        assert!(limiter.try_acquire("org_a").is_ok());
        assert!(limiter.try_acquire("org_b").is_ok());
        assert!(limiter.try_acquire("org_a").is_err());
    }

    #[test]
    fn a_per_org_ceiling_overrides_the_process_default_and_clears_back_to_it() {
        let limiter = RateLimiter {
            buckets: Arc::new(DashMap::new()),
            org_rpm: Arc::new(DashMap::new()),
            rpm: 100.0,
        };
        assert!((limiter.org_rpm("org") - 100.0).abs() < f64::EPSILON);

        limiter.set_org_rpm("org", Some(10.0));
        assert!((limiter.org_rpm("org") - 10.0).abs() < f64::EPSILON);
        // Another tenant is unaffected.
        assert!((limiter.org_rpm("other") - 100.0).abs() < f64::EPSILON);

        // 0 / negative clears rather than pinning the org at zero, which would
        // lock it out entirely.
        limiter.set_org_rpm("org", None);
        assert!((limiter.org_rpm("org") - 100.0).abs() < f64::EPSILON);
        limiter.set_org_rpm("org", Some(0.0));
        assert!((limiter.org_rpm("org") - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn lowering_a_ceiling_bites_immediately_rather_than_after_the_old_bucket_drains() {
        let limiter = RateLimiter {
            buckets: Arc::new(DashMap::new()),
            org_rpm: Arc::new(DashMap::new()),
            rpm: 100.0,
        };
        // Spend from the generous default bucket.
        for _ in 0..5 {
            limiter.try_acquire("org").expect("within the default");
        }
        // Tighten to 1 rpm. The stale bucket is dropped, so the new ceiling
        // applies now — an operator lowering a limit expects it to take effect.
        limiter.set_org_rpm("org", Some(1.0));
        limiter
            .try_acquire("org")
            .expect("first token under the new ceiling");
        assert!(
            limiter.try_acquire("org").is_err(),
            "a 1-rpm org must not get a second immediate token"
        );
    }
}
