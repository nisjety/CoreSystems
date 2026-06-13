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

    /// §16.2.2 — retrieval cache keys now include the org_version. A
    /// document mutation bumps the version → all prior keys for that org
    /// become unreachable instantly, eliminating the up-to-5-min staleness
    /// window of pure TTL invalidation.
    pub async fn get_retrieval(
        &self,
        org_id: &str,
        org_version: i64,
        cache_key: &str,
    ) -> Option<String> {
        let key = format!("{RETRIEVAL_PREFIX}{org_id}:v{org_version}:{cache_key}");
        self.conn.clone().get(&key).await.ok()?
    }

    pub async fn set_retrieval(&self, org_id: &str, org_version: i64, cache_key: &str, json: &str) {
        let key = format!("{RETRIEVAL_PREFIX}{org_id}:v{org_version}:{cache_key}");
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
