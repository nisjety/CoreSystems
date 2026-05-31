//! Hybrid search provider (OSS-parity P0 1C) — fuses lexical + semantic recall.
//!
//! Wraps an inner lexical `SearchProvider` (Quarry's `TantivyLocalIndex` /
//! SERP fallback chain) with a `VectorIndex` (Data Plane delegation) and
//! RRF-fuses the two. Because it *is* a `SearchProvider`, it slots into
//! `AppState.search` transparently — `/v1/search` and `AnswerPipeline` get
//! hybrid ranking with no further changes.
//!
//! Resilience: the vector leg is best-effort. If Data Plane retrieval errors
//! (or returns nothing), the provider degrades gracefully to lexical-only —
//! a Data Plane outage never takes search down.

use std::sync::Arc;

use async_trait::async_trait;

use quarry_core::error::QuarryResult;

use crate::fusion::{rrf_fuse, RRF_K};
use crate::serp::{SearchOptions, SearchProvider, SearchResult};
use crate::vector_index::VectorIndex;

pub struct HybridSearchProvider {
    lexical: Arc<dyn SearchProvider>,
    vector: Arc<dyn VectorIndex>,
}

impl HybridSearchProvider {
    pub fn new(lexical: Arc<dyn SearchProvider>, vector: Arc<dyn VectorIndex>) -> Self {
        Self { lexical, vector }
    }
}

#[async_trait]
impl SearchProvider for HybridSearchProvider {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        let org = opts.org_id.clone().unwrap_or_default();
        let limit = (opts.limit.max(1)) as usize;

        // Run both legs concurrently.
        let (lex_res, vec_res) =
            tokio::join!(self.lexical.search(query, opts), self.vector.retrieve(&org, query, limit));

        // Lexical is the primary leg — propagate its error.
        let lexical = lex_res?;
        // Vector is best-effort — circuit-break to lexical-only on failure.
        let vector = match vec_res {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "hybrid: vector retrieval failed; lexical-only");
                Vec::new()
            }
        };

        if vector.is_empty() {
            return Ok(lexical);
        }
        Ok(rrf_fuse(&lexical, &vector, RRF_K, limit))
    }

    fn name(&self) -> &str {
        "hybrid"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector_index::VectorHit;
    use quarry_core::error::{ErrorCode, QuarryError};

    struct LexProvider(Vec<SearchResult>);
    #[async_trait]
    impl SearchProvider for LexProvider {
        async fn search(&self, _q: &str, _o: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
            Ok(self.0.clone())
        }
        fn name(&self) -> &str {
            "lex"
        }
    }

    struct VecOk(Vec<VectorHit>);
    #[async_trait]
    impl VectorIndex for VecOk {
        async fn retrieve(&self, _o: &str, _q: &str, _k: usize) -> QuarryResult<Vec<VectorHit>> {
            Ok(self.0.clone())
        }
    }

    struct VecErr;
    #[async_trait]
    impl VectorIndex for VecErr {
        async fn retrieve(&self, _o: &str, _q: &str, _k: usize) -> QuarryResult<Vec<VectorHit>> {
            Err(QuarryError::new(ErrorCode::UpstreamBlocked, "dp down"))
        }
    }

    fn sr(url: &str) -> SearchResult {
        SearchResult { url: url.into(), title: None, snippet: None, rank: 1, provider: "lex".into() }
    }
    fn vh(url: &str) -> VectorHit {
        VectorHit { url: url.into(), score: 0.9, title: None, snippet: None, document_id: None }
    }

    #[tokio::test]
    async fn fuses_lexical_and_vector() {
        let p = HybridSearchProvider::new(
            Arc::new(LexProvider(vec![sr("a"), sr("b")])),
            Arc::new(VecOk(vec![vh("b"), vh("c")])),
        );
        let res = p.search("q", &SearchOptions::default()).await.unwrap();
        // "b" is in both → ranked first; union covers a, b, c.
        assert_eq!(res[0].url, "b");
        assert_eq!(res.len(), 3);
        assert_eq!(res[0].provider, "hybrid");
    }

    #[tokio::test]
    async fn vector_failure_degrades_to_lexical() {
        let p = HybridSearchProvider::new(
            Arc::new(LexProvider(vec![sr("a"), sr("b")])),
            Arc::new(VecErr),
        );
        let res = p.search("q", &SearchOptions::default()).await.unwrap();
        let urls: Vec<&str> = res.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn empty_vector_returns_lexical() {
        let p = HybridSearchProvider::new(
            Arc::new(LexProvider(vec![sr("a")])),
            Arc::new(VecOk(vec![])),
        );
        let res = p.search("q", &SearchOptions::default()).await.unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].url, "a");
    }
}
