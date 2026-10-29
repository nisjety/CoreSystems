//! Page-level Redis cache keyed by fingerprint.
//!
//! Stores `NormalizedOutput` JSON at `quarry:page:{fingerprint}` with a TTL.
//! Lookups are tried on the scrape path when the client supplies
//! `prev_fingerprint`; on success the cached value is returned and the fetch
//! is skipped entirely. On miss the fresh result is written back keyed by the
//! newly computed fingerprint.

use std::time::Duration;

use quarry_core::output::NormalizedOutput;
use redis::{aio::ConnectionManager, AsyncCommands};

pub const DEFAULT_TTL_SECS: u64 = 3600;

#[derive(Clone)]
pub struct PageCache {
    conn: ConnectionManager,
    ttl_secs: u64,
}

impl PageCache {
    pub fn new(conn: ConnectionManager, ttl: Duration) -> Self {
        Self {
            conn,
            ttl_secs: ttl.as_secs().max(1),
        }
    }

    fn key(fingerprint: &str) -> String {
        format!("quarry:page:{fingerprint}")
    }

    /// Returns `Some(out)` on cache hit, `None` on miss or any recoverable
    /// backend/deserialization error. Cache failures must never fail the
    /// request path.
    pub async fn get(&self, fingerprint: &str) -> Option<NormalizedOutput> {
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn.get(Self::key(fingerprint)).await.ok()?;
        let raw = raw?;
        match serde_json::from_str::<NormalizedOutput>(&raw) {
            Ok(out) => Some(out),
            Err(err) => {
                tracing::warn!(error = %err, fingerprint, "page cache: deserialize failed");
                None
            }
        }
    }

    /// Writes JSON-encoded `NormalizedOutput` under the computed fingerprint
    /// with the configured default TTL.
    pub async fn put(&self, fingerprint: &str, out: &NormalizedOutput) -> redis::RedisResult<()> {
        self.put_with_ttl(fingerprint, out, self.ttl_secs).await
    }

    /// Like [`put`] but with a per-call TTL override (in seconds). A `ttl_secs`
    /// of zero is clamped to 1 to satisfy Redis `SETEX` semantics.
    pub async fn put_with_ttl(
        &self,
        fingerprint: &str,
        out: &NormalizedOutput,
        ttl_secs: u64,
    ) -> redis::RedisResult<()> {
        let mut conn = self.conn.clone();
        let encoded = serde_json::to_string(out).map_err(|e| {
            redis::RedisError::from((
                redis::ErrorKind::IoError,
                "serialize NormalizedOutput",
                e.to_string(),
            ))
        })?;
        conn.set_ex::<_, _, ()>(Self::key(fingerprint), encoded, ttl_secs.max(1))
            .await
    }
}

/// Derives a stable cache key from a URL, a set of vary headers, and a
/// JS-required flag.  The key is a hex-encoded BLAKE3 digest so it is safe
/// to use directly as a Redis key segment.
///
/// Vary headers are sorted before hashing so that insertion order does not
/// affect the result.
pub fn fingerprint(url: &str, vary_headers: &[(&str, &str)], js_required: bool) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(url.as_bytes());
    let mut sorted = vary_headers.to_vec();
    sorted.sort_unstable();
    for (k, v) in &sorted {
        hasher.update(k.as_bytes());
        hasher.update(b":");
        hasher.update(v.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(&[js_required as u8]);
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod fingerprint_tests {
    use super::fingerprint;

    #[test]
    fn stable_across_calls() {
        let a = fingerprint("https://example.com", &[("Accept", "text/html")], false);
        let b = fingerprint("https://example.com", &[("Accept", "text/html")], false);
        assert_eq!(a, b);
    }

    #[test]
    fn header_order_independent() {
        let a = fingerprint("https://x.com", &[("A", "1"), ("B", "2")], true);
        let b = fingerprint("https://x.com", &[("B", "2"), ("A", "1")], true);
        assert_eq!(a, b);
    }

    #[test]
    fn js_flag_changes_key() {
        let a = fingerprint("https://x.com", &[], false);
        let b = fingerprint("https://x.com", &[], true);
        assert_ne!(a, b);
    }

    #[test]
    fn different_urls_differ() {
        let a = fingerprint("https://a.com", &[], false);
        let b = fingerprint("https://b.com", &[], false);
        assert_ne!(a, b);
    }
}
