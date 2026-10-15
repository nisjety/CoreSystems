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
use sha2::{Digest, Sha256};

/// Env lever: set to `off` (or `0`/`false`) to disable the two CONTENT caches
/// (search results, scrape previews) without touching the identity and posture
/// caches that share this Redis. Read once at connect. Default on.
const CONTENT_CACHE_ENV: &str = "GATEWAY_RESULT_CACHE_CONTENT";

/// Within this age a cached entry is "fresh" — return it and skip the upstream.
const FRESH_SECS: u64 = 4 * 60 * 60; // 4 hours
/// Entries are kept this long so a stale copy can be served if the upstream is
/// down (between FRESH_SECS and STORE_SECS we prefer the upstream but fall back).
const STORE_SECS: u64 = 24 * 60 * 60; // 24 hours

#[derive(Clone)]
pub(crate) struct ResultCache {
    conn: Option<redis::aio::ConnectionManager>,
    /// False when `GATEWAY_RESULT_CACHE_CONTENT=off`: content caches bypass.
    content_enabled: bool,
    /// In-process backend used only by tests, so handler tests can exercise the
    /// real cache paths without a Redis.
    #[cfg(test)]
    mem: Option<std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>>,
}

/// A cache hit. `fresh` is false once past FRESH_SECS — the caller should try the
/// upstream first and only fall back to `data` if that fails.
pub(crate) struct Cached {
    pub(crate) fresh: bool,
    pub(crate) data: Value,
}

impl ResultCache {
    pub(crate) fn disabled() -> Self {
        Self {
            conn: None,
            content_enabled: true,
            #[cfg(test)]
            mem: None,
        }
    }

    /// A working in-process cache for tests.
    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        Self {
            conn: None,
            content_enabled: true,
            mem: Some(Default::default()),
        }
    }

    /// Whether the content caches (search results, scrape previews) may be used
    /// at all. The identity and posture caches are not affected.
    pub(crate) fn content_enabled(&self) -> bool {
        self.content_enabled
    }

    async fn get_raw(&self, key: &str) -> Option<String> {
        #[cfg(test)]
        if let Some(mem) = &self.mem {
            return mem.lock().ok()?.get(key).cloned();
        }
        let mut conn = self.conn.clone()?;
        conn.get::<_, Option<String>>(key).await.ok().flatten()
    }

    async fn set_raw(&self, key: &str, payload: String, ttl_secs: u64) {
        #[cfg(test)]
        if let Some(mem) = &self.mem {
            if let Ok(mut map) = mem.lock() {
                map.insert(key.to_owned(), payload);
            }
            return;
        }
        let Some(mut conn) = self.conn.clone() else {
            return;
        };
        let _: Result<(), redis::RedisError> = conn.set_ex(key, payload, ttl_secs).await;
    }

    async fn del_raw(&self, key: &str) {
        #[cfg(test)]
        if let Some(mem) = &self.mem {
            if let Ok(mut map) = mem.lock() {
                map.remove(key);
            }
            return;
        }
        let Some(mut conn) = self.conn.clone() else {
            return;
        };
        let _: Result<(), redis::RedisError> = conn.del(key).await;
    }

    /// Connect to the cache. Any failure (missing URL, bad URL, unreachable
    /// server) yields a disabled cache rather than an error — caching is an
    /// optimization, never a hard dependency.
    pub(crate) async fn connect(url: Option<&str>) -> Self {
        let Some(url) = url.map(str::trim).filter(|value| !value.is_empty()) else {
            return Self::disabled();
        };
        let content_enabled = !matches!(
            std::env::var(CONTENT_CACHE_ENV)
                .ok()
                .as_deref()
                .map(|value| value.trim().to_ascii_lowercase())
                .as_deref(),
            Some("off" | "0" | "false")
        );
        match redis::Client::open(url) {
            Ok(client) => {
                // 3-second timeout so a missing/unreachable cache never blocks gateway startup.
                let connect_fut = redis::aio::ConnectionManager::new(client);
                match tokio::time::timeout(std::time::Duration::from_secs(3), connect_fut).await {
                    Ok(Ok(conn)) => {
                        tracing::info!(
                            content_cache = content_enabled,
                            "gateway result cache connected"
                        );
                        Self {
                            conn: Some(conn),
                            content_enabled,
                            #[cfg(test)]
                            mem: None,
                        }
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

    /// Look up a cached entry only if it is younger than `max_age_secs`. Unlike
    /// [`Self::lookup`] there is no stale-fallback window — for short-lived caches
    /// (e.g. per-user session context) where a stale value must not be served.
    /// Returns the stored `data` on a fresh hit, else `None` (miss / too old / any
    /// cache error). Degrade-safe: a disabled cache always yields `None`.
    pub(crate) async fn lookup_within(&self, key: &str, max_age_secs: u64) -> Option<Value> {
        let raw = self.get_raw(key).await;
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
        let payload = json!({ "cached_at": now_secs(), "data": data }).to_string();
        self.set_raw(key, payload, ttl_secs).await;
    }

    /// Scoped lookup for CONTENT entries. The envelope carries the org scope it
    /// was written under; an entry whose scope tag differs from `scope` (or has
    /// none, as every pre-scoping entry does) is a MISS, never a hit. The key
    /// already contains the scope, so a mismatch means a collision or a tampered
    /// entry: either way it must not be served.
    pub(crate) async fn lookup_scoped(&self, key: &str, scope: &str) -> Option<Cached> {
        let raw = self.get_raw(key).await;
        let entry: Value = serde_json::from_str(&raw?).ok()?;
        if entry.get("scope").and_then(Value::as_str) != Some(scope) {
            return None;
        }
        let data = entry.get("data").cloned()?;
        let cached_at = entry.get("cached_at").and_then(Value::as_u64).unwrap_or(0);
        Some(Cached {
            fresh: now_secs().saturating_sub(cached_at) < FRESH_SECS,
            data,
        })
    }

    /// Scoped lookup with a caller-owned freshness window; see
    /// [`Self::lookup_scoped`] for the scope rule.
    pub(crate) async fn lookup_within_scoped(
        &self,
        key: &str,
        scope: &str,
        max_age_secs: u64,
    ) -> Option<Value> {
        let raw = self.get_raw(key).await;
        let entry: Value = serde_json::from_str(&raw?).ok()?;
        if entry.get("scope").and_then(Value::as_str) != Some(scope) {
            return None;
        }
        let cached_at = entry.get("cached_at").and_then(Value::as_u64).unwrap_or(0);
        if now_secs().saturating_sub(cached_at) >= max_age_secs {
            return None;
        }
        entry.get("data").cloned()
    }

    /// Store a CONTENT entry tagged with its org scope (24h store window).
    pub(crate) async fn store_scoped(&self, key: &str, scope: &str, data: &Value) {
        let payload = json!({ "cached_at": now_secs(), "scope": scope, "data": data }).to_string();
        self.set_raw(key, payload, STORE_SECS).await;
    }

    /// Delete a cached entry. Failures are ignored (degrade-safe).
    pub(crate) async fn delete(&self, key: &str) {
        self.del_raw(key).await;
    }

    /// Every key held by the in-process test backend.
    #[cfg(test)]
    pub(crate) fn mem_keys(&self) -> Vec<String> {
        self.mem
            .as_ref()
            .and_then(|mem| mem.lock().ok().map(|map| map.keys().cloned().collect()))
            .unwrap_or_default()
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
    // Documented for the GDPR-erasure roster caller described above; not yet
    // wired up anywhere in this crate, so clippy sees it as unreachable.
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

/// Org-scoped key for a CONTENT entry:
/// `verevon:gw:{namespace}:o:{org_id}:{sha256_hex(len-prefixed parts)}`.
///
/// The org id is a hard tenant boundary. The caller must have validated it
/// (see `cache_admission::valid_org_scope`): it is interpolated into the key
/// and must not contain separators or glob characters. Parts are length-prefixed
/// before hashing so `["a", "bc"]` and `["ab", "c"]` cannot collide.
pub(crate) fn scoped_key(namespace: &str, org_id: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!(
        "verevon:gw:{namespace}:o:{org_id}:{}",
        hex::encode(hasher.finalize())
    )
}

/// Build a stable cache key from a namespace + parts. Deterministic across
/// process restarts (std's DefaultHasher uses fixed keys), so a restarted gateway
/// still hits entries written by its predecessor.
///
/// NOT tenant-scoped: for identity and posture caches only. Content caches use
/// [`scoped_key`].
pub(crate) fn cache_key(namespace: &str, parts: &[&str]) -> String {
    let mut hasher = DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
        0u8.hash(&mut hasher); // delimiter so ["a","b"] != ["ab"]
    }
    format!("verevon:gw:{namespace}:{:x}", hasher.finish())
}
