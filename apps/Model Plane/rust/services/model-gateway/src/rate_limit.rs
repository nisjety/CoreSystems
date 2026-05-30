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
        }
    }

    /// Try to consume one token for the given org. Returns `Ok(())` if allowed,
    /// or `Err(retry_after_secs)` if the bucket is exhausted.
    fn try_acquire(&self, org_id: &str) -> Result<(), u64> {
        let now = Instant::now();
        let refill_rate = self.rpm / 60.0; // tokens per second

        let mut entry = self
            .buckets
            .entry(org_id.to_owned())
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
            rpm: 1.0,
        };

        assert!(limiter.try_acquire("org_a").is_ok());
        assert!(limiter.try_acquire("org_b").is_ok());
        assert!(limiter.try_acquire("org_a").is_err());
    }
}
