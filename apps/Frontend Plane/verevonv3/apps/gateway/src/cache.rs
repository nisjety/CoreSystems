//! Optional Dragonfly/Redis-backed result cache for expensive upstream calls
//! (web search, page scrapes). Keyed by query/URL with a 4h freshness window and
//! a 24h stale-fallback window so repeat queries skip quarry-edge entirely and
//! results can still be served if quarry is briefly unavailable.
//!
//! Entirely degrade-safe: if `GATEWAY_CACHE_REDIS_URL` is unset or the server is
//! unreachable, every operation is a silent no-op and the gateway behaves exactly
//! as it did before (just without caching). The cache must never break a request.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::{SystemTime, UNIX_EPOCH},
};

use redis::AsyncCommands;
use serde_json::{json, Value};

/// Within this age a cached entry is "fresh" — return it and skip the upstream.
const FRESH_SECS: u64 = 4 * 60 * 60; // 4 hours
/// Entries are kept this long so a stale copy can be served if the upstream is
/// down (between FRESH_SECS and STORE_SECS we prefer the upstream but fall back).
const STORE_SECS: u64 = 24 * 60 * 60; // 24 hours

#[derive(Clone)]
pub(crate) struct ResultCache {
    conn: Option<redis::aio::ConnectionManager>,
}

/// A cache hit. `fresh` is false once past FRESH_SECS — the caller should try the
/// upstream first and only fall back to `data` if that fails.
pub(crate) struct Cached {
    pub(crate) fresh: bool,
    pub(crate) data: Value,
}

impl ResultCache {
    pub(crate) fn disabled() -> Self {
        Self { conn: None }
    }

    /// Connect to the cache. Any failure (missing URL, bad URL, unreachable
    /// server) yields a disabled cache rather than an error — caching is an
    /// optimization, never a hard dependency.
    pub(crate) async fn connect(url: Option<&str>) -> Self {
        let Some(url) = url.map(str::trim).filter(|value| !value.is_empty()) else {
            return Self::disabled();
        };
        match redis::Client::open(url) {
            Ok(client) => {
                // 3-second timeout so a missing/unreachable cache never blocks gateway startup.
                let connect_fut = redis::aio::ConnectionManager::new(client);
                match tokio::time::timeout(std::time::Duration::from_secs(3), connect_fut).await {
                    Ok(Ok(conn)) => {
                        tracing::info!("gateway result cache connected");
                        Self { conn: Some(conn) }
                    }
                    Ok(Err(error)) => {
                        tracing::warn!(%error, "gateway result cache unreachable; running without cache");
                        Self::disabled()
                    }
                    Err(_elapsed) => {
                        tracing::warn!(
                            "gateway result cache connect timed out (3s); running without cache"
                        );
                        Self::disabled()
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, "invalid GATEWAY_CACHE_REDIS_URL; running without cache");
                Self::disabled()
            }
        }
    }

    /// Look up a cached entry. Returns None on a miss or any cache error.
    pub(crate) async fn lookup(&self, key: &str) -> Option<Cached> {
        let mut conn = self.conn.clone()?;
        let raw: Option<String> = conn.get::<_, Option<String>>(key).await.ok().flatten();
        let entry: Value = serde_json::from_str(&raw?).ok()?;
        let data = entry.get("data").cloned()?;
        let cached_at = entry.get("cached_at").and_then(Value::as_u64).unwrap_or(0);
        let age = now_secs().saturating_sub(cached_at);
        Some(Cached {
            fresh: age < FRESH_SECS,
            data,
        })
    }

    /// Look up a cached entry only if it is younger than `max_age_secs`. Unlike
    /// [`Self::lookup`] there is no stale-fallback window — for short-lived caches
    /// (e.g. per-user session context) where a stale value must not be served.
    /// Returns the stored `data` on a fresh hit, else `None` (miss / too old / any
    /// cache error). Degrade-safe: a disabled cache always yields `None`.
    pub(crate) async fn lookup_within(&self, key: &str, max_age_secs: u64) -> Option<Value> {
        let mut conn = self.conn.clone()?;
        let raw: Option<String> = conn.get::<_, Option<String>>(key).await.ok().flatten();
        let entry: Value = serde_json::from_str(&raw?).ok()?;
        let cached_at = entry.get("cached_at").and_then(Value::as_u64).unwrap_or(0);
        if now_secs().saturating_sub(cached_at) >= max_age_secs {
            return None;
        }
        entry.get("data").cloned()
    }

    /// Store a fresh entry. Failures are ignored (degrade-safe).
    pub(crate) async fn store(&self, key: &str, data: &Value) {
        self.store_for_secs(key, data, STORE_SECS).await;
    }

    /// Store a fresh entry with a caller-owned TTL. Failures are ignored
    /// (degrade-safe). The payload shape matches [`Self::lookup`] so callers can
    /// still use `lookup_within` with their own freshness window.
    pub(crate) async fn store_for_secs(&self, key: &str, data: &Value, ttl_secs: u64) {
        let Some(mut conn) = self.conn.clone() else {
            return;
        };
        let payload = json!({ "cached_at": now_secs(), "data": data }).to_string();
        let _: Result<(), redis::RedisError> = conn.set_ex(key, payload, ttl_secs).await;
    }

    /// Delete a cached entry. Failures are ignored (degrade-safe).
    pub(crate) async fn delete(&self, key: &str) {
        let Some(mut conn) = self.conn.clone() else {
            return;
        };
        let _: Result<(), redis::RedisError> = conn.del(key).await;
    }

    /// Add `member` to the Redis SET at `key` and (re)arm its TTL.
    ///
    /// A real `SADD` rather than a read-modify-write of a JSON array, because
    /// the only caller is a membership ROSTER used by GDPR erasure: two users
    /// in one org writing concurrently would lose an update under
    /// read-modify-write, and a lost roster entry is a subject whose data the
    /// erasure fan-out then silently fails to reach. `SADD` is atomic and
    /// idempotent, so concurrent writers and redelivery are both non-events.
    ///
    /// The TTL is refreshed on every add so an active roster never expires out
    /// from under the data it indexes.
    pub(crate) async fn set_add(&self, key: &str, member: &str, ttl_secs: u64) {
        let Some(mut conn) = self.conn.clone() else {
            return;
        };
        let added: Result<i64, redis::RedisError> = conn.sadd(key, member).await;
        if let Err(error) = added {
            tracing::debug!(%error, "roster sadd failed");
            return;
        }
        let _: Result<bool, redis::RedisError> = conn.expire(key, ttl_secs as i64).await;
    }

    /// Read every member of the Redis SET at `key`. An empty vector on a miss,
    /// a disabled cache, or any error — callers must treat "empty" as "nothing
    /// known here", never as proof of absence.
    pub(crate) async fn set_members(&self, key: &str) -> Vec<String> {
        let Some(mut conn) = self.conn.clone() else {
            return Vec::new();
        };
        conn.smembers::<_, Vec<String>>(key)
            .await
            .unwrap_or_else(|error| {
                tracing::debug!(%error, "roster smembers failed");
                Vec::new()
            })
    }

    /// Hand out a clone of the underlying connection manager, if the cache is
    /// connected. `ConnectionManager` is cheaply clonable (it shares one
    /// multiplexed connection), so other subsystems — e.g. the distributed
    /// [`crate::rate_limit::RateLimiter`] — can reuse the same Dragonfly link
    /// instead of opening a second one. `None` when the cache is disabled, so
    /// callers degrade exactly as the cache itself does.
    pub(crate) fn connection(&self) -> Option<redis::aio::ConnectionManager> {
        self.conn.clone()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Build a stable cache key from a namespace + parts. Deterministic across
/// process restarts (std's DefaultHasher uses fixed keys), so a restarted gateway
/// still hits entries written by its predecessor.
pub(crate) fn cache_key(namespace: &str, parts: &[&str]) -> String {
    let mut hasher = DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
        0u8.hash(&mut hasher); // delimiter so ["a","b"] != ["ab"]
    }
    format!("verevon:gw:{namespace}:{:x}", hasher.finish())
}
