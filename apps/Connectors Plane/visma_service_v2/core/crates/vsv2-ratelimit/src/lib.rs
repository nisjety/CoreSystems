//! `vsv2-ratelimit` — per-source token bucket + Redis cooldown.

use std::sync::Arc;

use deadpool_redis::Pool;
use tokio::sync::Mutex;
use tracing::{debug, info};
use vsv2_common::{Error, Result, Source};

pub mod bucket;
pub mod cooldown;

pub use bucket::TokenBucket;
pub use cooldown::Cooldown;

#[derive(Debug, Clone)]
pub struct Limiter {
    pub source: Source,
    pub per_minute: u32,
    pub per_day: u32,
    bucket_minute: Arc<TokenBucket>,
    bucket_day: Arc<TokenBucket>,
    local_cooldown_until: Arc<Mutex<Option<chrono::DateTime<chrono::Utc>>>>,
    redis: Option<Pool>,
}

impl Limiter {
    pub fn new(source: Source, per_minute: u32, per_day: u32) -> Self {
        Self {
            source,
            per_minute,
            per_day,
            bucket_minute: Arc::new(TokenBucket::new(per_minute, per_minute as f64 / 60.0)),
            bucket_day: Arc::new(TokenBucket::new(per_day, per_day as f64 / 86_400.0)),
            local_cooldown_until: Arc::new(Mutex::new(None)),
            redis: None,
        }
    }

    pub fn with_redis(mut self, pool: Pool) -> Self {
        self.redis = Some(pool);
        self
    }

    /// Block until a slot opens. Honours both buckets and any active Redis cooldown.
    pub async fn acquire(&self) -> Result<()> {
        let local_cooldown = { *self.local_cooldown_until.lock().await };
        if let Some(until) = local_cooldown {
            if until > chrono::Utc::now() {
                return Err(Error::RateLimited {
                    kind: self.source.as_str(),
                    until,
                });
            }
        }

        if let Some(pool) = &self.redis {
            if let Some(until) = Cooldown::check(pool, self.source).await? {
                return Err(Error::RateLimited {
                    kind: self.source.as_str(),
                    until,
                });
            }
        }
        debug!(source = %self.source, "ratelimit: acquiring");
        self.bucket_minute.acquire().await;
        self.bucket_day.acquire().await;
        Ok(())
    }

    /// Engage local and Redis cooldowns until a known reset time.
    pub async fn engage_cooldown_until(&self, until: chrono::DateTime<chrono::Utc>) -> Result<()> {
        {
            let mut guard = self.local_cooldown_until.lock().await;
            *guard = Some(until);
        }
        if let Some(pool) = &self.redis {
            Cooldown::set(pool, self.source, until).await?;
        }
        Ok(())
    }

    /// Observe upstream rate-limit headers and dynamically pause when the
    /// server reports no quota remaining before the reset point.
    pub async fn observe_rate_limit(
        &self,
        limit: Option<u32>,
        remaining: Option<u32>,
        reset: Option<i64>,
        policy: Option<&str>,
    ) -> Result<()> {
        if limit.is_none() && remaining.is_none() && reset.is_none() {
            return Ok(());
        }

        info!(
            source = %self.source,
            ?limit,
            ?remaining,
            ?reset,
            ?policy,
            "ratelimit: upstream headers"
        );

        let Some(0) = remaining else {
            return Ok(());
        };
        let until = reset
            .and_then(reset_to_until)
            .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::seconds(60));
        self.engage_cooldown_until(until).await
    }
}

fn reset_to_until(reset: i64) -> Option<chrono::DateTime<chrono::Utc>> {
    if reset <= 0 {
        return None;
    }
    if reset > 1_000_000_000 {
        chrono::DateTime::from_timestamp(reset, 0)
    } else {
        Some(chrono::Utc::now() + chrono::Duration::seconds(reset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn observe_rate_limit_remaining_zero_engages_local_cooldown() {
        let limiter = Limiter::new(Source::ErpRest, 60, 5_000);
        limiter
            .observe_rate_limit(Some(60), Some(0), Some(60), Some("test"))
            .await
            .expect("observe headers");

        let err = limiter.acquire().await.unwrap_err();
        assert!(matches!(err, Error::RateLimited { .. }));
    }

    #[tokio::test]
    async fn observe_rate_limit_with_remaining_quota_does_not_block() {
        let limiter = Limiter::new(Source::ErpRest, 60, 5_000);
        limiter
            .observe_rate_limit(Some(60), Some(10), Some(60), Some("test"))
            .await
            .expect("observe headers");

        limiter.acquire().await.expect("quota remains available");
    }
}
