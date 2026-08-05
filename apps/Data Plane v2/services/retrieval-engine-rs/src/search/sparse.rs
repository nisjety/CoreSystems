use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Map, Value};
use sqlx::PgPool;

use crate::pipeline::types::ScoredCandidate;

#[async_trait]
pub trait SparseSearchBackend: Send + Sync {
    fn name(&self) -> &'static str;

    async fn search(
        &self,
        query: &str,
        org_id: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>>;
}

pub type DynSparseSearchBackend = Arc<dyn SparseSearchBackend>;

pub struct PostgresSparseBackend {
    pool: PgPool,
}

impl PostgresSparseBackend {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SparseSearchBackend for PostgresSparseBackend {
    fn name(&self) -> &'static str {
        "postgres"
    }

    async fn search(
        &self,
        query: &str,
        org_id: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        bm25_search(&self.pool, query, org_id, top_k).await
    }
}

pub struct QuickwitSparseBackend {
    client: Client,
    base_url: String,
    index_id: String,
}

impl QuickwitSparseBackend {
    pub fn new(base_url: impl Into<String>, index_id: impl Into<String>, timeout_ms: u64) -> Self {
        let timeout = Duration::from_millis(timeout_ms.max(100));
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            index_id: index_id.into(),
        }
    }
}

#[async_trait]
impl SparseSearchBackend for QuickwitSparseBackend {
    fn name(&self) -> &'static str {
        "quickwit"
    }

    #[tracing::instrument(
        name = "quickwit.sparse_search",
        skip(self, query),
        fields(org_id = org_id, top_k = top_k)
    )]
    async fn search(
        &self,
        query: &str,
        org_id: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        if query.trim().is_empty() || top_k == 0 {
            return Ok(Vec::new());
        }

        let quickwit_query = build_quickwit_query(org_id, query);
        let url = format!("{}/api/v1/{}/search", self.base_url, self.index_id);
        let body = serde_json::json!({
            "query": quickwit_query,
            "max_hits": top_k
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        let parsed: QuickwitSearchResponse = response.json().await?;

        Ok(parsed
            .hits
            .into_iter()
            .enumerate()
            .filter_map(|(idx, hit)| hit.into_candidate(top_k, idx))
            .collect())
    }
}

pub struct FallbackSparseBackend {
    primary: DynSparseSearchBackend,
    fallback: DynSparseSearchBackend,
}

impl FallbackSparseBackend {
    pub fn new(primary: DynSparseSearchBackend, fallback: DynSparseSearchBackend) -> Self {
        Self { primary, fallback }
    }
}

#[async_trait]
impl SparseSearchBackend for FallbackSparseBackend {
    fn name(&self) -> &'static str {
        "quickwit-with-postgres-fallback"
    }

    async fn search(
        &self,
        query: &str,
        org_id: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        match self.primary.search(query, org_id, top_k).await {
            Ok(candidates) => Ok(candidates),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    primary = self.primary.name(),
                    fallback = self.fallback.name(),
                    "sparse search backend failed; falling back"
                );
                self.fallback.search(query, org_id, top_k).await
            }
        }
    }
}

#[tracing::instrument(
    name = "postgres.bm25_search",
    skip(pool, query),
    fields(
        otel.kind = "client",
        db.system = "postgresql",
        org_id = org_id,
        top_k = top_k,
    ),
)]
pub async fn bm25_search(
    pool: &PgPool,
    query: &str,
    org_id: &str,
    top_k: usize,
) -> anyhow::Result<Vec<ScoredCandidate>> {
    // §16.3.3 — uses the precomputed `content_tsv` generated column (GIN
    // indexed) instead of re-tokenizing per query. The query planner picks
    // the GIN index over the previous expression index automatically.
    let rows = sqlx::query_as::<_, BM25Row>(
        r#"
        SELECT
            knowledge_id,
            document_id,
            text,
            ts_rank_cd(content_tsv, plainto_tsquery('simple', $1)) AS rank_score,
            chunk_index,
            metadata
        FROM knowledge_units
        WHERE org_id = $2
          AND content_tsv @@ plainto_tsquery('simple', $1)
          -- Phase 4 read-your-writes: the sparse (FTS) arm needs no vectors, so
          -- surface just-chunked content immediately (status 'pending') instead
          -- of waiting for the async embed. Exclude only 'failed'. The ownership
          -- post-filter still gates by viewer; the dense arm joins once embedded
          -- and RRF fusion dedups by knowledge_id (no double-count).
          AND embedding_status <> 'failed'
        ORDER BY rank_score DESC
        LIMIT $3
        "#,
    )
    .bind(query)
    .bind(org_id)
    .bind(top_k as i64)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ScoredCandidate {
            knowledge_id: r.knowledge_id,
            document_id: r.document_id,
            text: r.text,
            dense_score: 0.0,
            sparse_score: r.rank_score,
            rerank_score: 0.0,
            final_score: r.rank_score,
            chunk_index: r.chunk_index,
            metadata: std::collections::HashMap::new(),
        })
        .collect())
}

#[derive(sqlx::FromRow)]
struct BM25Row {
    knowledge_id: String,
    document_id: String,
    text: String,
    rank_score: f32,
    chunk_index: i32,
    #[allow(dead_code)]
    metadata: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct QuickwitSearchResponse {
    #[serde(default)]
    hits: Vec<QuickwitHit>,
}

#[derive(Debug, Deserialize)]
struct QuickwitHit {
    #[serde(default)]
    score: Option<f32>,
    #[serde(default)]
    json: Option<Value>,
    #[serde(default)]
    doc: Option<Value>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl QuickwitHit {
    fn into_candidate(self, top_k: usize, idx: usize) -> Option<ScoredCandidate> {
        let doc = if let Some(json) = self.json {
            json
        } else if let Some(doc) = self.doc {
            doc
        } else if !self.extra.is_empty() {
            Value::Object(self.extra)
        } else {
            return None;
        };

        let knowledge_id = doc.get("knowledge_id")?.as_str()?.to_string();
        let document_id = doc.get("document_id")?.as_str()?.to_string();
        let text = doc
            .get("body")
            .or_else(|| doc.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let chunk_index = doc
            .get("chunk_index")
            .and_then(Value::as_i64)
            .unwrap_or_default() as i32;
        let sparse_score = self
            .score
            .unwrap_or_else(|| (top_k.saturating_sub(idx)) as f32);

        Some(ScoredCandidate {
            knowledge_id,
            document_id,
            text,
            dense_score: 0.0,
            sparse_score,
            rerank_score: 0.0,
            final_score: sparse_score,
            chunk_index,
            metadata: std::collections::HashMap::new(),
        })
    }
}

fn build_quickwit_query(org_id: &str, query: &str) -> String {
    let cleaned_terms = sanitize_query(query);
    let org_filter = format!(
        "org_id:{} AND entity_type:{}",
        quote_term(org_id),
        quote_term("knowledge_unit")
    );
    if cleaned_terms.is_empty() {
        org_filter
    } else {
        format!("{org_filter} AND ({cleaned_terms})")
    }
}

fn sanitize_query(query: &str) -> String {
    query
        .split_whitespace()
        .filter_map(|term| {
            let cleaned: String = term
                .chars()
                .filter(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.'))
                .collect();
            (!cleaned.is_empty()).then_some(cleaned)
        })
        .take(32)
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_term(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StaticBackend {
        name: &'static str,
        error: Option<&'static str>,
        candidates: Vec<ScoredCandidate>,
        calls: AtomicUsize,
    }

    impl StaticBackend {
        fn ok(name: &'static str, candidates: Vec<ScoredCandidate>) -> Self {
            Self {
                name,
                error: None,
                candidates,
                calls: AtomicUsize::new(0),
            }
        }

        fn fail(name: &'static str, error: &'static str) -> Self {
            Self {
                name,
                error: Some(error),
                candidates: Vec::new(),
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl SparseSearchBackend for StaticBackend {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn search(
            &self,
            _query: &str,
            _org_id: &str,
            _top_k: usize,
        ) -> anyhow::Result<Vec<ScoredCandidate>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.error {
                anyhow::bail!(error);
            }
            Ok(self.candidates.clone())
        }
    }

    fn candidate(id: &str) -> ScoredCandidate {
        ScoredCandidate {
            knowledge_id: id.to_string(),
            document_id: format!("doc-{id}"),
            text: format!("text-{id}"),
            dense_score: 0.0,
            sparse_score: 1.0,
            rerank_score: 0.0,
            final_score: 1.0,
            chunk_index: 0,
            metadata: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn quickwit_query_scopes_to_org_and_knowledge_units() {
        let query = build_quickwit_query("org-1", "alpha beta site:ignored ../bad");

        assert!(query.contains("org_id:\"org-1\""));
        assert!(query.contains("entity_type:\"knowledge_unit\""));
        assert!(query.contains("alpha beta"));
        assert!(!query.contains("site:ignored"));
        assert!(!query.contains("../bad"));
    }

    #[tokio::test]
    async fn fallback_backend_uses_postgres_when_quickwit_fails() {
        let primary = Arc::new(StaticBackend::fail("quickwit", "connection refused"));
        let fallback = Arc::new(StaticBackend::ok("postgres", vec![candidate("k1")]));
        let backend = FallbackSparseBackend::new(primary.clone(), fallback.clone());

        let results = backend.search("alpha", "org-1", 10).await.unwrap();

        assert_eq!(backend.name(), "quickwit-with-postgres-fallback");
        assert_eq!(primary.calls(), 1);
        assert_eq!(fallback.calls(), 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].knowledge_id, "k1");
    }

    #[tokio::test]
    async fn fallback_backend_does_not_call_postgres_when_quickwit_succeeds() {
        let primary = Arc::new(StaticBackend::ok("quickwit", vec![candidate("k2")]));
        let fallback = Arc::new(StaticBackend::ok("postgres", vec![candidate("k3")]));
        let backend = FallbackSparseBackend::new(primary.clone(), fallback.clone());

        let results = backend.search("alpha", "org-1", 10).await.unwrap();

        assert_eq!(primary.calls(), 1);
        assert_eq!(fallback.calls(), 0);
        assert_eq!(results[0].knowledge_id, "k2");
    }

    #[test]
    fn quickwit_hit_parses_flat_hit_shape() {
        let hit: QuickwitHit = serde_json::from_value(serde_json::json!({
            "score": 7.5,
            "knowledge_id": "knowledge-1",
            "document_id": "document-1",
            "body": "body text",
            "chunk_index": 4
        }))
        .unwrap();

        let candidate = hit.into_candidate(10, 0).unwrap();

        assert_eq!(candidate.knowledge_id, "knowledge-1");
        assert_eq!(candidate.document_id, "document-1");
        assert_eq!(candidate.text, "body text");
        assert_eq!(candidate.chunk_index, 4);
        assert_eq!(candidate.sparse_score, 7.5);
    }

    #[test]
    fn quickwit_hit_parses_nested_json_hit_shape() {
        let hit: QuickwitHit = serde_json::from_value(serde_json::json!({
            "json": {
                "knowledge_id": "knowledge-2",
                "document_id": "document-2",
                "text": "nested text"
            }
        }))
        .unwrap();

        let candidate = hit.into_candidate(5, 1).unwrap();

        assert_eq!(candidate.knowledge_id, "knowledge-2");
        assert_eq!(candidate.document_id, "document-2");
        assert_eq!(candidate.text, "nested text");
        assert_eq!(candidate.sparse_score, 4.0);
    }
}
