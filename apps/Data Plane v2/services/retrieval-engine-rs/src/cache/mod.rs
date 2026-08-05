#![allow(dead_code, unused_imports)]
use std::time::Duration;

use redis::aio::ConnectionManager;
use redis::AsyncCommands;

pub mod invalidator;
pub mod org_version;
pub mod semantic;

const EMBED_PREFIX: &str = "dpv2:embed:";
const RETRIEVAL_PREFIX: &str = "dpv2:ret:";
const EMBED_TTL: u64 = 3600;
const RETRIEVAL_TTL: u64 = 300;

/// Derives the authorization scope that partitions the retrieval cache.
///
/// Two callers may share a cache entry only if they would pass the *same*
/// ownership post-filter. That is determined by the viewer identity plus the
/// exact set of documents specifically granted to them, so both go into the
/// token.
///
/// - `None` viewer → `"org-shared"`. Sound because with no viewer the pipeline
///   applies no per-user ownership filter at all: the result is the org-visible
///   set, identical for every such caller.
/// - `Some(viewer)` → viewer id plus a hash of their **sorted** grant set.
///   Sorting matters: `user-core` returns grants in no guaranteed order, and an
///   unsorted token would fragment the cache for one user (a miss, not a leak —
///   but it would quietly make the cache useless).
///
/// A grant change alters the hash, so newly granted or revoked access is never
/// served from a pre-change entry.
pub fn viewer_scope_token(viewer: Option<&str>, granted_docs: &[String]) -> String {
    let Some(viewer) = viewer.map(str::trim).filter(|v| !v.is_empty()) else {
        return "org-shared".to_string();
    };
    let mut grants: Vec<&str> = granted_docs.iter().map(String::as_str).collect();
    grants.sort_unstable();
    grants.dedup();

    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    viewer.hash(&mut hasher);
    for g in &grants {
        // Domain separator: without it, grants ["ab","c"] and ["a","bc"] would
        // hash identically and two different grant sets could share an entry.
        hasher.write_u8(0);
        g.hash(&mut hasher);
    }
    format!("{viewer}:{:x}", hasher.finish())
}

#[derive(Clone)]
pub struct CacheLayer {
    conn: ConnectionManager,
}

impl CacheLayer {
    pub async fn connect(cache_url: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(cache_url)?;
        let conn = ConnectionManager::new(client).await?;
        tracing::info!("redis-compatible cache connected");
        Ok(Self { conn })
    }

    /// Wave-3.1 §16.2.8 — model_version is part of the key so a deployment
    /// that swaps the embedding route/model doesn't serve stale vectors for
    /// the hour the old TTL is still alive. Pass the active embedding route
    /// namespace (e.g. "model_plane:azure_openai:text-embedding-3-large").
    pub async fn get_embedding(&self, model_version: &str, text_hash: &str) -> Option<Vec<f32>> {
        let key = format!("{EMBED_PREFIX}{model_version}:{text_hash}");
        let data: Option<Vec<u8>> = self.conn.clone().get(&key).await.ok()?;
        data.map(|bytes| {
            bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect()
        })
    }

    pub async fn set_embedding(&self, model_version: &str, text_hash: &str, embedding: &[f32]) {
        let key = format!("{EMBED_PREFIX}{model_version}:{text_hash}");
        let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        let _: Result<(), _> = self.conn.clone().set_ex(&key, bytes, EMBED_TTL).await;
    }

    /// §16.2.2 — retrieval cache keys include the org_version. A document
    /// mutation bumps the version → all prior keys for that org become
    /// unreachable instantly, eliminating the up-to-5-min staleness window of
    /// pure TTL invalidation.
    ///
    /// # Why `scope` is mandatory (plan P1-6)
    ///
    /// Retrieval results are **viewer-dependent**: the orchestrator's step-6
    /// ownership gate keeps a document only if
    /// `owner_id = viewer OR visibility = 'org' OR it is in the viewer's grants`.
    /// Two users in the same org issuing the same query at the same
    /// `org_version` legitimately get *different* result sets.
    ///
    /// Keying on `org_id` alone — as this tier originally did — would therefore
    /// serve one user's private and specifically-granted documents to another
    /// user in the same organisation. That is the same class of defect as the
    /// org-IDOR this plane has already had to fix once.
    ///
    /// So `scope` is a required parameter and an **empty scope fails closed**:
    /// a miss on read, a no-op on write. It is deliberately impossible to use
    /// this cache without deciding whose results are being cached. Derive the
    /// value with [`viewer_scope_token`]; the sibling semantic cache enforces
    /// the same rule via `semantic_cache_require_scope`.
    pub async fn get_retrieval(
        &self,
        org_id: &str,
        org_version: i64,
        scope: &str,
        cache_key: &str,
    ) -> Option<String> {
        if scope.trim().is_empty() {
            return None;
        }
        let key = format!("{RETRIEVAL_PREFIX}{org_id}:v{org_version}:s{scope}:{cache_key}");
        self.conn.clone().get(&key).await.ok()?
    }

    /// Stores a retrieval result. See [`Self::get_retrieval`] for why `scope` is
    /// required; an empty scope is silently not cached rather than cached
    /// unsafely.
    pub async fn set_retrieval(
        &self,
        org_id: &str,
        org_version: i64,
        scope: &str,
        cache_key: &str,
        json: &str,
    ) {
        if scope.trim().is_empty() {
            return;
        }
        let key = format!("{RETRIEVAL_PREFIX}{org_id}:v{org_version}:s{scope}:{cache_key}");
        let _: Result<(), _> = self.conn.clone().set_ex(&key, json, RETRIEVAL_TTL).await;
    }

    pub async fn health_check(&self) -> bool {
        let result: Result<String, _> =
            redis::cmd("PING").query_async(&mut self.conn.clone()).await;
        result.is_ok()
    }

    /// Invalidate all retrieval-result cache entries for an org_id by deleting
    /// keys matching `dpv2:ret:{org_id}:*`. Uses SCAN to avoid blocking the
    /// Redis-compatible cache backend.
    pub async fn invalidate_org_retrieval(&self, org_id: &str) -> usize {
        let pattern = format!("{RETRIEVAL_PREFIX}{org_id}:*");
        let mut conn = self.conn.clone();
        let mut deleted = 0usize;
        let mut cursor: u64 = 0;

        loop {
            let scan: Result<(u64, Vec<String>), _> = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut conn)
                .await;

            let (next_cursor, keys) = match scan {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(?e, pattern, "cache invalidation SCAN failed");
                    return deleted;
                }
            };

            if !keys.is_empty() {
                let n: Result<usize, _> = redis::cmd("DEL").arg(&keys).query_async(&mut conn).await;
                if let Ok(n) = n {
                    deleted += n;
                }
            }

            if next_cursor == 0 {
                break;
            }
            cursor = next_cursor;
        }
        tracing::info!(org_id, deleted, "retrieval cache invalidated");
        deleted
    }
}

pub fn hash_text(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_viewer_is_org_shared() {
        // With no viewer the pipeline applies no ownership filter, so every such
        // caller sees the identical org-visible set and may share one entry.
        assert_eq!(viewer_scope_token(None, &[]), "org-shared");
        assert_eq!(viewer_scope_token(Some("   "), &[]), "org-shared");
        assert_eq!(
            viewer_scope_token(None, &["doc-1".to_string()]),
            "org-shared",
            "grants are irrelevant when no ownership filter runs"
        );
    }

    #[test]
    fn different_viewers_never_share_a_scope() {
        // The leak this guards: same org, same org_version, same query, but
        // different viewers must not collide.
        assert_ne!(
            viewer_scope_token(Some("user-a"), &[]),
            viewer_scope_token(Some("user-b"), &[])
        );
        assert_ne!(
            viewer_scope_token(Some("user-a"), &[]),
            viewer_scope_token(None, &[]),
            "a viewer must not share the org-shared bucket"
        );
    }

    #[test]
    fn grant_set_changes_the_scope_but_order_does_not() {
        let a = viewer_scope_token(Some("u"), &["d1".to_string(), "d2".to_string()]);
        let reordered = viewer_scope_token(Some("u"), &["d2".to_string(), "d1".to_string()]);
        assert_eq!(a, reordered, "user-core returns grants in no stable order");
        let dup = viewer_scope_token(
            Some("u"),
            &["d2".to_string(), "d1".to_string(), "d2".to_string()],
        );
        assert_eq!(a, dup, "duplicate grants must not fragment the cache");

        // Gaining or losing access must invalidate: a pre-change entry must not
        // be reachable after the grant set changes.
        assert_ne!(a, viewer_scope_token(Some("u"), &["d1".to_string()]));
        assert_ne!(
            a,
            viewer_scope_token(Some("u"), &["d1".to_string(), "d2".to_string(), "d3".to_string()])
        );
    }

    #[test]
    fn grant_boundaries_cannot_be_confused() {
        // Without a domain separator, ["ab","c"] and ["a","bc"] would hash the
        // same and two different grant sets could share an entry.
        assert_ne!(
            viewer_scope_token(Some("u"), &["ab".to_string(), "c".to_string()]),
            viewer_scope_token(Some("u"), &["a".to_string(), "bc".to_string()])
        );
    }
}
