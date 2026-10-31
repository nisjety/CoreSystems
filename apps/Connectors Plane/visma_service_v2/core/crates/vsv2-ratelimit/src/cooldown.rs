use chrono::{DateTime, Utc};
use deadpool_redis::{redis::AsyncCommands, Pool};
use tracing::warn;
use vsv2_common::{Error, Result, Source};

/// Shared Redis cooldown — set when upstream returns 429 / Retry-After.
/// Key: `vsv2:rate-limit:{source}:cooldown`. Value: ISO-8601 UTC.
#[derive(Debug, Clone)]
pub struct Cooldown {
    pub source: Source,
    pub until: DateTime<Utc>,
}

impl Cooldown {
    pub fn redis_key(source: Source) -> String {
        format!("vsv2:rate-limit:{source}:cooldown")
    }

    /// Check Redis for an active cooldown. Returns `Ok(None)` when free,
    /// `Ok(Some(until))` when blocked.
    pub async fn check(pool: &Pool, source: Source) -> Result<Option<DateTime<Utc>>> {
        let mut conn = pool
            .get()
            .await
            .map_err(|e| Error::Other(format!("redis: {e}")))?;
        let raw: Option<String> = conn
            .get(Self::redis_key(source))
            .await
            .map_err(|e| Error::Other(format!("redis get: {e}")))?;

        let Some(s) = raw else { return Ok(None) };
        let parsed = DateTime::parse_from_rfc3339(&s)
            .map_err(|e| Error::Other(format!("parse cooldown ts: {e}")))?
            .with_timezone(&Utc);
        if parsed > Utc::now() {
            Ok(Some(parsed))
        } else {
            Ok(None)
        }
    }

    /// Set a cooldown until the given timestamp.
    pub async fn set(pool: &Pool, source: Source, until: DateTime<Utc>) -> Result<()> {
        let ttl = (until - Utc::now()).num_seconds().max(1) as u64;
        let mut conn = pool
            .get()
            .await
            .map_err(|e| Error::Other(format!("redis: {e}")))?;
        let _: () = conn
            .set_ex(Self::redis_key(source), until.to_rfc3339(), ttl)
            .await
            .map_err(|e| Error::Other(format!("redis setex: {e}")))?;
        warn!(source = %source, until = %until, "cooldown engaged");
        Ok(())
    }
}
