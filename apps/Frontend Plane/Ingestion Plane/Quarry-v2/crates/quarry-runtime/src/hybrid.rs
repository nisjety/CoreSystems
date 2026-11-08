//! Hybrid search provider: fuses lexical and semantic recall.
//!
//! Wraps an inner lexical `SearchProvider` (Quarry's `TantivyLocalIndex` / SERP
//! fallback chain) with a `VectorIndex` (Data Plane delegation) and RRF-fuses
//! the two. Because it *is* a `SearchProvider`, it slots into `AppState.search`
//! transparently: `/v1/search` and `AnswerPipeline` get hybrid ranking with no
//! further changes.
//!
//! Resilience: the vector leg is best-effort and bounded. It may be skipped by
//! posture, it may fail, or it may run past its deadline. In each case the
//! lexical results are returned as they are, and the response is never held
//! longer than the deadline by the Data Plane.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use quarry_core::error::QuarryResult;
use quarry_core::zdr::ZdrMode;

use crate::fusion::{rrf_fuse, RRF_K};
use crate::serp::{SearchOptions, SearchProvider, SearchResult};
use crate::vector_index::{VectorIndex, VectorOutcome, VectorQuery, VECTOR_LEG_TIMEOUT};

pub struct HybridSearchProvider {
    lexical: Arc<dyn SearchProvider>,
    vector: Arc<dyn VectorIndex>,
    /// The most time the vector leg may hold the response. Past it, the leg is
    /// dropped and the lexical results stand.
    vector_timeout: Duration,
}

impl HybridSearchProvider {
    pub fn new(lexical: Arc<dyn SearchProvider>, vector: Arc<dyn VectorIndex>) -> Self {
        Self {
            lexical,
            vector,
            vector_timeout: VECTOR_LEG_TIMEOUT,
        }
    }

    pub fn with_vector_timeout(mut self, timeout: Duration) -> Self {
        self.vector_timeout = timeout;
        self
    }
}

#[async_trait]
impl SearchProvider for HybridSearchProvider {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        let org = opts.org_id.as_deref().unwrap_or("");
        let limit = (opts.limit.max(1)) as usize;
        let request = VectorQuery {
            org_id: org,
            query,
            zdr: ZdrMode::from(opts.zdr),
            limit,
        };

        // Both legs run concurrently. The vector leg is bounded, so the Data
        // Plane can delay this response by at most the deadline.
        let (lexical, vector) = tokio::join!(
            self.lexical.search(query, opts),
            tokio::time::timeout(self.vector_timeout, self.vector.retrieve(&request))
        );

        // Lexical is the primary leg: its error is the search's error.
        let lexical = lexical?;
        let hits = match vector {
            Ok(Ok(VectorOutcome::Hits(hits))) => hits,
            Ok(Ok(VectorOutcome::Skipped(reason))) => {
                tracing::debug!(
                    marker = reason.marker(),
                    "hybrid: vector leg did not run; lexical-only"
                );
                Vec::new()
            }
            // The index has already logged the failure at the level its class
            // deserves (an error for auth and contract breaks, a warning for outages).
            Ok(Err(error)) => {
                tracing::debug!(
                    error_class = error.code.wire_name(),
                    "hybrid: vector leg failed; lexical-only"
                );
                Vec::new()
            }
            Err(_elapsed) => {
                tracing::warn!(
                    deadline = ?self.vector_timeout,
                    "hybrid: vector leg exceeded its deadline; lexical-only"
                );
                Vec::new()
            }
        };

        if hits.is_empty() {
            return Ok(lexical);
        }
        Ok(rrf_fuse(&lexical, &hits, RRF_K, limit))
    }

    fn name(&self) -> &str {
        "hybrid"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serp::SearXNGSearch;
    use crate::vector_index::{SkipReason, VectorHit};
    use quarry_core::error::{ErrorCode, QuarryError};
    use serde_json::json;
    use std::sync::Mutex;
    use std::time::Instant;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
        async fn retrieve(&self, _q: &VectorQuery<'_>) -> QuarryResult<VectorOutcome> {
            Ok(VectorOutcome::Hits(self.0.clone()))
        }
    }

    struct VecErr;
    #[async_trait]
    impl VectorIndex for VecErr {
        async fn retrieve(&self, _q: &VectorQuery<'_>) -> QuarryResult<VectorOutcome> {
            Err(QuarryError::new(ErrorCode::UpstreamBlocked, "dp down"))
        }
    }

    struct VecSkip(SkipReason);
    #[async_trait]
    impl VectorIndex for VecSkip {
        async fn retrieve(&self, _q: &VectorQuery<'_>) -> QuarryResult<VectorOutcome> {
            Ok(VectorOutcome::Skipped(self.0))
        }
    }

    /// Records the (org, zdr, limit) of every call it receives.
    #[derive(Default)]
    struct VecRecord(Mutex<Vec<(String, ZdrMode, usize)>>);
    #[async_trait]
    impl VectorIndex for VecRecord {
        async fn retrieve(&self, q: &VectorQuery<'_>) -> QuarryResult<VectorOutcome> {
            self.0
                .lock()
                .unwrap()
                .push((q.org_id.to_owned(), q.zdr, q.limit));
            Ok(VectorOutcome::Hits(Vec::new()))
        }
    }

    /// Answers only after `delay`.
    struct VecSlow(Duration);
    #[async_trait]
    impl VectorIndex for VecSlow {
        async fn retrieve(&self, _q: &VectorQuery<'_>) -> QuarryResult<VectorOutcome> {
            tokio::time::sleep(self.0).await;
            Ok(VectorOutcome::Hits(vec![vh("https://late.test/")]))
        }
    }

    fn sr(url: &str) -> SearchResult {
        SearchResult {
            url: url.into(),
            title: None,
            snippet: None,
            rank: 1,
            provider: "lex".into(),
            ..Default::default()
        }
    }
    fn vh(url: &str) -> VectorHit {
        VectorHit {
            url: url.into(),
            title: None,
            snippet: Some(format!("snip-{url}")),
            document_id: None,
        }
    }

    fn opts(org: &str, zdr: bool, limit: u32) -> SearchOptions {
        SearchOptions {
            org_id: Some(org.into()),
            zdr,
            limit,
            ..Default::default()
        }
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

    #[tokio::test]
    async fn a_skipped_leg_returns_the_lexical_results_untouched() {
        let p = HybridSearchProvider::new(
            Arc::new(LexProvider(vec![sr("a"), sr("b")])),
            Arc::new(VecSkip(SkipReason::Zdr)),
        );
        let res = p.search("q", &opts("org_a", true, 10)).await.unwrap();
        let urls: Vec<&str> = res.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, vec!["a", "b"]);
        assert_eq!(res[0].provider, "lex");
    }

    #[tokio::test]
    async fn the_leg_receives_the_runs_org_zdr_posture_and_limit() {
        let recorder = Arc::new(VecRecord::default());
        let p = HybridSearchProvider::new(Arc::new(LexProvider(vec![sr("a")])), recorder.clone());
        p.search("q", &opts("org_a", true, 5)).await.unwrap();
        p.search("q", &opts("org_b", false, 7)).await.unwrap();
        let calls = recorder.0.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![
                ("org_a".to_owned(), ZdrMode::On, 5),
                ("org_b".to_owned(), ZdrMode::Off, 7),
            ]
        );
    }

    #[tokio::test]
    async fn a_leg_past_its_deadline_is_cut_and_the_lexical_results_survive() {
        let p = HybridSearchProvider::new(
            Arc::new(LexProvider(vec![sr("a")])),
            Arc::new(VecSlow(Duration::from_secs(5))),
        )
        .with_vector_timeout(Duration::from_millis(100));
        let started = Instant::now();
        let res = p.search("q", &SearchOptions::default()).await.unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the deadline bounds the response: {:?}",
            started.elapsed()
        );
        let urls: Vec<&str> = res.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, vec!["a"]);
    }

    #[tokio::test]
    async fn fused_results_carry_each_page_once_under_its_real_url() {
        let p = HybridSearchProvider::new(
            Arc::new(LexProvider(vec![
                sr("https://x.test/a"),
                sr("https://x.test/b"),
            ])),
            Arc::new(VecOk(vec![vh("https://x.test/b"), vh("https://x.test/c")])),
        );
        let res = p.search("q", &SearchOptions::default()).await.unwrap();
        let mut urls: Vec<&str> = res.iter().map(|r| r.url.as_str()).collect();
        urls.sort_unstable();
        assert_eq!(
            urls,
            vec!["https://x.test/a", "https://x.test/b", "https://x.test/c"],
            "a page found by both legs appears once"
        );
    }

    /// A SearXNG answer of the shape the adapter reads: two metasearch rows, each
    /// with the engines that returned it and their per-engine positions.
    fn searxng_answer() -> serde_json::Value {
        json!({
            "results": [
                {
                    "url": "https://docs.example.test/pricing/?utm_source=bing",
                    "title": "Pricing",
                    "content": "Plans and what they cost.",
                    "engines": ["bing", "duckduckgo"],
                    "positions": [1, 2]
                },
                {
                    "url": "https://guides.example.test/cleaning",
                    "title": "Cleaning guide",
                    "content": "How often to clean the belt.",
                    "engines": ["google cse"],
                    "positions": [3]
                }
            ]
        })
    }

    #[tokio::test]
    async fn serp_engines_survive_a_vector_leg_that_names_the_same_page() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(searxng_answer()))
            .mount(&server)
            .await;
        let p = HybridSearchProvider::new(
            Arc::new(SearXNGSearch::new(server.uri()).unwrap()),
            Arc::new(VecOk(vec![
                vh("http://docs.example.test/pricing"),
                vh("https://stored.example.test/doc-7"),
            ])),
        );
        let res = p
            .search("pricing", &opts("org_a", false, 10))
            .await
            .unwrap();

        let urls: Vec<&str> = res.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://docs.example.test/pricing/?utm_source=bing",
                "https://guides.example.test/cleaning",
                "https://stored.example.test/doc-7",
            ],
            "the stored and the SERP spelling of the pricing page are one result"
        );
        assert_eq!(res[0].engines, vec!["bing", "duckduckgo"]);
        let expected = 1.0_f32 / 61.0 + 1.0_f32 / 62.0;
        let fused = res[0]
            .fusion_score
            .expect("the SERP fusion score is carried");
        assert!((fused - expected).abs() < 1e-6, "fusion_score {fused}");
        assert_eq!(res[1].engines, vec!["google cse"]);
        assert!(res[2].engines.is_empty(), "the Data Plane is not an engine");
        assert_eq!(res[2].fusion_score, None);
    }

    #[tokio::test]
    async fn serp_engines_survive_when_the_vector_leg_is_empty() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(searxng_answer()))
            .mount(&server)
            .await;
        let p = HybridSearchProvider::new(
            Arc::new(SearXNGSearch::new(server.uri()).unwrap()),
            Arc::new(VecOk(vec![])),
        );
        let res = p
            .search("pricing", &opts("org_a", false, 10))
            .await
            .unwrap();
        assert_eq!(res[0].engines, vec!["bing", "duckduckgo"]);
        assert_eq!(res[1].engines, vec!["google cse"]);
    }
}
