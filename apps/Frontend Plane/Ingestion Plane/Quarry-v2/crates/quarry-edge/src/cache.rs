//! Page-level Redis cache keyed by org and fingerprint.
//!
//! Stores `NormalizedOutput` JSON at `quarry:page:{org_id}:{fingerprint}` with a
//! TTL. Lookups are tried on the scrape path when the client supplies
//! `prev_fingerprint`; on success the cached value is returned and the fetch
//! is skipped entirely. On miss the fresh result is written back keyed by the
//! newly computed fingerprint.
//!
//! INNH-7 adds a second key space on the same connection, read by `max_age`:
//! `quarry:scrape:{org_id}:{blake3(canonical url, options key)}` holds a
//! [`CachedScrape`] (the output plus the run's warnings, never page text; the
//! text is rebuilt from the stored artifacts on a hit). It is keyed by what the
//! caller asked for, not by the fetched body, so one URL under two option sets
//! is two entries. See `scrape_cache.rs` for who may read and write it.
//!
//! The org segment is a hard tenant boundary. A fingerprint is a hash of the
//! raw page body, so anyone can compute it for a public page; without the org
//! in the key, a caller could send another org's fingerprint as
//! `prev_fingerprint` and read that org's cached output (final URL, title,
//! artifact refs). An empty org id is never a key: reads miss and writes are
//! dropped.

use std::time::Duration;

use quarry_core::output::NormalizedOutput;
use quarry_core::scrape::ScrapeWarning;
use redis::{aio::ConnectionManager, AsyncCommands};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

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

    /// `quarry:page:{org_id}:{fingerprint}`. `None` for an empty org id, which
    /// must never address the cache.
    fn key(org_id: &str, fingerprint: &str) -> Option<String> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return None;
        }
        Some(format!("quarry:page:{org_id}:{fingerprint}"))
    }

    /// Returns `Some(out)` on cache hit, `None` on miss or any recoverable
    /// backend/deserialization error. Cache failures must never fail the
    /// request path.
    pub async fn get(&self, org_id: &str, fingerprint: &str) -> Option<NormalizedOutput> {
        let key = Self::key(org_id, fingerprint)?;
        let mut conn = self.conn.clone();
        let raw: Option<String> = match conn.get(key).await {
            Ok(raw) => raw,
            Err(_) => {
                crate::metrics::record_cache("page", "error");
                return None;
            }
        };
        let Some(raw) = raw else {
            crate::metrics::record_cache("page", "miss");
            return None;
        };
        match serde_json::from_str::<NormalizedOutput>(&raw) {
            Ok(out) => {
                crate::metrics::record_cache("page", "hit");
                Some(out)
            }
            Err(err) => {
                crate::metrics::record_cache("page", "error");
                // zdr-log-ok: the page cache is never read for a ZDR request (every caller gates on the ZDR guard), so only non-ZDR work reaches this line
                tracing::warn!(error = %err, fingerprint, "page cache: deserialize failed");
                None
            }
        }
    }

    /// Writes JSON-encoded `NormalizedOutput` under the computed fingerprint
    /// with the configured default TTL.
    pub async fn put(
        &self,
        org_id: &str,
        fingerprint: &str,
        out: &NormalizedOutput,
    ) -> redis::RedisResult<()> {
        self.put_with_ttl(org_id, fingerprint, out, self.ttl_secs)
            .await
    }

    /// Like [`put`] but with a per-call TTL override (in seconds). A `ttl_secs`
    /// of zero is clamped to 1 to satisfy Redis `SETEX` semantics.
    pub async fn put_with_ttl(
        &self,
        org_id: &str,
        fingerprint: &str,
        out: &NormalizedOutput,
        ttl_secs: u64,
    ) -> redis::RedisResult<()> {
        let Some(key) = Self::key(org_id, fingerprint) else {
            // No org, no cache entry: dropping the write is the safe outcome.
            return Ok(());
        };
        let mut conn = self.conn.clone();
        let encoded = serde_json::to_string(out).map_err(|e| {
            redis::RedisError::from((
                redis::ErrorKind::IoError,
                "serialize NormalizedOutput",
                e.to_string(),
            ))
        })?;
        conn.set_ex::<_, _, ()>(key, encoded, ttl_secs.max(1)).await
    }
}

/// What the URL result cache stores for one scrape: the output (with its artifact
/// references and its ORIGINAL `fetched_at`) and the warnings the run produced, so a
/// hit says what the first answer said. No inline text: a hit rebuilds that from the
/// stored artifacts, so the page text is not copied into Redis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedScrape {
    pub output: NormalizedOutput,
    #[serde(default)]
    pub warnings: Vec<ScrapeWarning>,
}

impl PageCache {
    /// `quarry:scrape:{org_id}:{blake3(canonical_url, options_key)}`. `None` for an empty
    /// org id (which must never address the cache) and for a URL with no canonical form.
    /// `options_key` is `ResolvedScrapeOptions::canonical_options_key`; it is part of the
    /// key, so named formats, selectors and an extractor each get their own entry, and
    /// `max_age` (which is not part of it) never rotates a key. The URL is reduced
    /// with the conservative canonical form plus tracking-parameter removal: a fragment,
    /// a default port, the case of the host and `utm_*`/`gclid`-style parameters do not
    /// make a second entry; a trailing slash and the query order do.
    pub fn result_key(org_id: &str, url: &str, options_key: &str) -> Option<String> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return None;
        }
        let canonical = quarry_transform::canonical_url::canonicalize(
            url,
            quarry_transform::canonical_url::CanonicalOptions {
                strip_tracking_params: true,
                ..Default::default()
            },
        )
        .ok()?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(canonical.as_bytes());
        hasher.update(b"\x1f");
        hasher.update(options_key.as_bytes());
        Some(format!("quarry:scrape:{org_id}:{}", hasher.finalize().to_hex()))
    }

    /// `Some(entry)` on a hit, `None` on a miss or any recoverable error. The caller
    /// still checks the entry's age against the request's `max_age`.
    pub async fn get_result(&self, key: &str) -> Option<CachedScrape> {
        let mut conn = self.conn.clone();
        let raw: Option<String> = match conn.get(key).await {
            Ok(raw) => raw,
            Err(_) => {
                crate::metrics::record_cache("scrape", "error");
                return None;
            }
        };
        let Some(raw) = raw else {
            crate::metrics::record_cache("scrape", "miss");
            return None;
        };
        match serde_json::from_str::<CachedScrape>(&raw) {
            Ok(entry) => {
                crate::metrics::record_cache("scrape", "hit");
                Some(entry)
            }
            Err(err) => {
                crate::metrics::record_cache("scrape", "error");
                // zdr-log-ok: the URL result cache is never read for a ZDR request (scrape_cache::eligibility refuses it before a key exists); the key is not logged
                tracing::warn!(error = %err, "scrape result cache: deserialize failed");
                None
            }
        }
    }

    /// Stores `entry` under `key` for `ttl_secs` (clamped to at least 1 second).
    pub async fn put_result(
        &self,
        key: &str,
        entry: &CachedScrape,
        ttl_secs: u64,
    ) -> redis::RedisResult<()> {
        let mut conn = self.conn.clone();
        let encoded = serde_json::to_string(entry).map_err(|e| {
            redis::RedisError::from((
                redis::ErrorKind::IoError,
                "serialize CachedScrape",
                e.to_string(),
            ))
        })?;
        conn.set_ex::<_, _, ()>(key, encoded, ttl_secs.max(1)).await
    }
}

/// Derives a stable cache key from a URL, a set of vary headers, and a
/// JS-required flag.  The key is a hex-encoded BLAKE3 digest so it is safe
/// to use directly as a Redis key segment.
///
/// Vary headers are sorted before hashing so that insertion order does not
/// affect the result.
#[allow(dead_code)] // scaffolding: wired in follow-up
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

/// Generic org-scoped JSON cache over the shared Redis connection, used for
/// `/v1/search` (and answer) responses. Separate key space + TTL policy from
/// [`PageCache`] (scrape bodies). Every operation is best-effort — a cache
/// failure must never fail the request path.
#[derive(Clone)]
pub struct SearchCache {
    conn: ConnectionManager,
}

impl SearchCache {
    pub fn new(conn: ConnectionManager) -> Self {
        Self { conn }
    }

    /// Org-scoped key: `quarry:search:{org}:{blake3(query \u{1f} params)}`.
    /// The `org` segment is a hard tenant-isolation boundary — one org can
    /// never read another's cached results. `params` must encode every field
    /// that changes the *result set* (limit, locale, topic, recency,
    /// exact_match, include_answer) but NOT pure post-processing
    /// (highlight/facets/format), which are re-applied per request so those
    /// variants share a cache entry.
    pub fn key(org_id: &str, query: &str, params: &str) -> String {
        let mut h = blake3::Hasher::new();
        h.update(query.as_bytes());
        h.update(b"\x1f");
        h.update(params.as_bytes());
        format!("quarry:search:{org_id}:{}", h.finalize().to_hex())
    }

    /// `Some(value)` on hit; `None` on miss or any recoverable error.
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let mut conn = self.conn.clone();
        let raw: Option<String> = match conn.get(key).await {
            Ok(raw) => raw,
            Err(_) => {
                crate::metrics::record_cache("search", "error");
                return None;
            }
        };
        let Some(raw) = raw else {
            crate::metrics::record_cache("search", "miss");
            return None;
        };
        match serde_json::from_str::<T>(&raw) {
            Ok(v) => {
                crate::metrics::record_cache("search", "hit");
                Some(v)
            }
            Err(err) => {
                crate::metrics::record_cache("search", "error");
                // zdr-log-ok: the search cache is never read for a ZDR request (the caller skips the lookup when zdr is set), so only non-ZDR work reaches this line
                tracing::warn!(error = %err, key, "search cache: deserialize failed");
                None
            }
        }
    }

    /// Store a JSON value under `key` with a per-call TTL (clamped ≥1s).
    pub async fn put_with_ttl<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_secs: u64,
    ) -> redis::RedisResult<()> {
        let mut conn = self.conn.clone();
        let encoded = serde_json::to_string(value).map_err(|e| {
            redis::RedisError::from((
                redis::ErrorKind::IoError,
                "serialize search cache value",
                e.to_string(),
            ))
        })?;
        conn.set_ex::<_, _, ()>(key, encoded, ttl_secs.max(1)).await
    }
}

/// Intent-driven TTL (seconds) for the search/answer cache. Fresh /
/// time-sensitive queries expire fast; general informational queries use the
/// operator-configured default. Mirrors common CDN/search practice (news →
/// minutes, general → ~5–15 min).
pub fn search_ttl_secs(topic: Option<&str>, time_range: Option<&str>, default_ttl: u64) -> u64 {
    match topic.map(|t| t.trim().to_ascii_lowercase()).as_deref() {
        Some("news") | Some("finance") => 120, // breaking content — 2 min
        _ if time_range.is_some() => 300,      // any recency-scoped query — 5 min
        _ => default_ttl.max(60),              // general — operator default (≥60s)
    }
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

#[cfg(test)]
mod page_cache_tests {
    use super::PageCache;

    #[test]
    fn page_cache_key_includes_the_org() {
        // One fingerprint, two orgs: two keys. A fingerprint alone must never
        // address another org's entry.
        let a = PageCache::key("org_a", "fp1").expect("org a key");
        let b = PageCache::key("org_b", "fp1").expect("org b key");
        assert_ne!(a, b);
        assert_eq!(a, "quarry:page:org_a:fp1");
        assert_eq!(b, "quarry:page:org_b:fp1");
    }

    #[test]
    fn page_cache_key_refuses_an_empty_org() {
        assert!(PageCache::key("", "fp1").is_none());
        assert!(PageCache::key("   ", "fp1").is_none());
    }
}

#[cfg(test)]
mod result_key_tests {
    use super::PageCache;

    const OPTS: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn the_key_is_org_url_and_options() {
        let key = PageCache::result_key("org_a", "https://example.com/a", OPTS).expect("key");
        assert!(key.starts_with("quarry:scrape:org_a:"), "{key}");
        assert_eq!(key.len(), "quarry:scrape:org_a:".len() + 64);
        // Stable.
        assert_eq!(Some(key.clone()), PageCache::result_key("org_a", "https://example.com/a", OPTS));
        // A second org, a second URL and a second option set are three other keys.
        assert_ne!(Some(key.clone()), PageCache::result_key("org_b", "https://example.com/a", OPTS));
        assert_ne!(Some(key.clone()), PageCache::result_key("org_a", "https://example.com/b", OPTS));
        assert_ne!(Some(key), PageCache::result_key("org_a", "https://example.com/a", "other"));
    }

    #[test]
    fn an_empty_org_is_never_a_key() {
        assert!(PageCache::result_key("", "https://example.com/a", OPTS).is_none());
        assert!(PageCache::result_key("  ", "https://example.com/a", OPTS).is_none());
    }

    #[test]
    fn a_url_with_no_canonical_form_is_never_a_key() {
        for url in ["not a url", "ftp://example.com/a", "https://user:pw@example.com/a"] {
            assert!(PageCache::result_key("org_a", url, OPTS).is_none(), "{url}");
        }
    }

    #[test]
    fn spellings_of_one_page_share_a_key_and_distinct_pages_do_not() {
        let key = |url: &str| PageCache::result_key("org_a", url, OPTS);
        let base = key("https://example.com/a?x=1").expect("key");
        // A fragment, the case of the host, a default port and tracking parameters
        // do not change which page it is.
        assert_eq!(Some(base.clone()), key("https://example.com/a?x=1#section"));
        assert_eq!(Some(base.clone()), key("https://EXAMPLE.com:443/a?x=1"));
        assert_eq!(Some(base.clone()), key("https://example.com/a?x=1&utm_source=news&gclid=abc"));
        // A trailing slash, another query value and another scheme are other pages.
        assert_ne!(key("https://example.com/a").unwrap(), key("https://example.com/a/").unwrap());
        assert_ne!(Some(base.clone()), key("https://example.com/a?x=2"));
        assert_ne!(Some(base), key("http://example.com/a?x=1"));
    }
}

#[cfg(test)]
mod search_cache_tests {
    use super::{search_ttl_secs, SearchCache};

    #[test]
    fn key_is_org_scoped() {
        // Same query + params, different orgs → different keys (tenant isolation).
        let a = SearchCache::key("org_a", "rust ownership", "limit=10");
        let b = SearchCache::key("org_b", "rust ownership", "limit=10");
        assert_ne!(a, b);
        assert!(a.starts_with("quarry:search:org_a:"));
        assert!(b.starts_with("quarry:search:org_b:"));
    }

    #[test]
    fn key_stable_and_param_sensitive() {
        let base = SearchCache::key("o", "q", "limit=10|answer=true");
        assert_eq!(base, SearchCache::key("o", "q", "limit=10|answer=true"));
        assert_ne!(base, SearchCache::key("o", "q", "limit=20|answer=true"));
        assert_ne!(base, SearchCache::key("o", "q2", "limit=10|answer=true"));
    }

    #[test]
    fn ttl_news_short_recency_medium_general_default() {
        assert_eq!(search_ttl_secs(Some("news"), None, 900), 120);
        assert_eq!(search_ttl_secs(Some("finance"), None, 900), 120);
        assert_eq!(search_ttl_secs(None, Some("day"), 900), 300);
        assert_eq!(search_ttl_secs(None, None, 900), 900);
        assert_eq!(search_ttl_secs(None, None, 0), 60); // clamp floor
    }
}
