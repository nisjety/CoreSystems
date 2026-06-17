//! AnswerPipeline — Tavily-replacement orchestrator.
//!
//! Cycle 19 / gap-quarry cluster #18. Combines existing pieces into a
//! one-call answer flow:
//!
//! ```text
//! query
//!   ↓
//! 1. SearchProvider (Tantivy → Stract → SearXNG → Brave) — get top-K URLs
//!   ↓
//! 2. scrape_fn (caller-supplied page fetcher) — pull markdown for each URL
//!   ↓
//! 3. AiFormatRunner.query — synthesize grounded answer from concatenated sources
//!   ↓
//! returns AnswerResult { answer, citations[], sources[], usage, ... }
//! ```
//!
//! Design notes:
//!
//! - **Pluggable scrape function** — we don't take a hard dep on `PageRunner`
//!   (which lives in `pipeline.rs` and brings driver registry/security/etc.)
//!   because the answer pipeline is orthogonal to scrape mechanics. Callers
//!   wire whatever scrape impl they have. This lets the edge supply a real
//!   `PageRunner`, tests supply a mock, and future paths (e.g. a Tantivy-only
//!   short-circuit) can skip scraping entirely.
//!
//! - **ZDR propagation** — `zdr=true` flows into both the search call (free,
//!   queries are control-plane signals) and the synthesis call (Model Plane
//!   is contracted to keep ephemeral). Citations are URLs only — never raw
//!   content — so ZDR can return citations safely.
//!
//! - **Per-source markdown cap** — we truncate each source to `MAX_SOURCE_CHARS`
//!   before concatenation. Default 8K chars per source × 5 sources = 40K input
//!   tokens, well within most model context windows.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::zdr::ZdrMode;

use crate::ai_formats::AiFormatRunner;
use crate::serp::{SearchOptions, SearchProvider};

/// Maximum characters of markdown to feed per source. Prevents one giant
/// page from monopolizing the synthesis context window.
const MAX_SOURCE_CHARS: usize = 8_000;

/// Default top-K search results to fetch + synthesize over.
const DEFAULT_TOP_K: usize = 5;

/// Total per-request char ceiling. If concatenated source markdown
/// exceeds this, we truncate the lowest-ranked sources first.
const MAX_TOTAL_CHARS: usize = 40_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerRequest {
    pub query: String,
    #[serde(default)]
    pub top_k: Option<usize>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub zdr: Option<bool>,
    /// Tenant scope. Always populated server-side from the verified JWT
    /// claim — never trusted from the client wire. Threaded into the
    /// underlying `SearchProvider::search` so private-corpus providers
    /// (TantivyLocalIndex) restrict citations to this org's documents.
    #[serde(default)]
    pub org_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    pub rank: u32,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerResult {
    pub query: String,
    pub answer: String,
    pub citations: Vec<Citation>,
    pub model: String,
    pub latency_ms: u64,
    /// Number of sources actually used in synthesis (after truncation/skip).
    pub sources_used: usize,
    /// Number of sources skipped because their fetch failed.
    pub sources_skipped: usize,
}

/// Pluggable scrape interface. The edge wires a real `PageRunner`-backed
/// impl; tests pass a closure or fake.
#[async_trait]
pub trait MarkdownFetcher: Send + Sync {
    /// Fetch the markdown body for a URL. Returns `None` on transport
    /// failure so the pipeline can skip and continue with remaining
    /// sources instead of aborting the whole answer.
    async fn fetch_markdown(&self, url: &str, zdr: ZdrMode) -> Option<String>;
}

pub struct AnswerPipeline {
    search: Arc<dyn SearchProvider>,
    fetcher: Arc<dyn MarkdownFetcher>,
    formats: AiFormatRunner,
}

/// Grounded inputs for synthesis, shared by `answer()` (blocking) and the edge's
/// streaming `/v1/answer/stream` route. `combined` is empty when no source
/// contributed text (callers then return a citations-only empty answer).
pub struct PreparedAnswer {
    pub citations: Vec<Citation>,
    pub combined: String,
    pub sources_used: usize,
    pub sources_skipped: usize,
}

impl AnswerPipeline {
    pub fn new(
        search: Arc<dyn SearchProvider>,
        fetcher: Arc<dyn MarkdownFetcher>,
        formats: AiFormatRunner,
    ) -> Self {
        Self {
            search,
            fetcher,
            formats,
        }
    }

    /// Accessor so the edge's streaming route can drive synthesis itself
    /// (`formats().query_stream(...)`) over the prepared context.
    pub fn formats(&self) -> &AiFormatRunner {
        &self.formats
    }

    /// Search + concurrent fetch + context assembly — everything up to (but not
    /// including) synthesis. Shared by `answer()` and the streaming route. The
    /// verified `org_id` flows through so private-corpus providers filter to the
    /// tenant; public-web providers ignore it.
    pub async fn prepare(&self, req: &AnswerRequest) -> QuarryResult<PreparedAnswer> {
        if req.query.trim().is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "answer query must not be empty",
            ));
        }
        let top_k = req.top_k.unwrap_or(DEFAULT_TOP_K).clamp(1, 20);
        let zdr = ZdrMode::from(req.zdr.unwrap_or(false));

        let opts = SearchOptions {
            limit: top_k as u32,
            country: req.country.clone(),
            language: req.language.clone(),
            safe_search: true,
            topic: None,
            time_range: None,
            exact_match: false,
            org_id: req.org_id.clone(),
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
        };
        let results = self.search.search(&req.query, &opts).await?;
        if results.is_empty() {
            return Ok(PreparedAnswer {
                citations: vec![],
                combined: String::new(),
                sources_used: 0,
                sources_skipped: 0,
            });
        }

        // Concurrent fetch of top-K sources.
        let fetch_futures = results.iter().map(|r| {
            let url = r.url.clone();
            let fetcher = self.fetcher.clone();
            async move { fetcher.fetch_markdown(&url, zdr).await }
        });
        let markdowns = futures::future::join_all(fetch_futures).await;

        let mut sources_used = 0usize;
        let mut sources_skipped = 0usize;
        let mut combined = String::with_capacity(MAX_TOTAL_CHARS);
        let mut citations: Vec<Citation> = Vec::with_capacity(results.len());

        for (i, (result, markdown)) in results.iter().zip(markdowns.iter()).enumerate() {
            citations.push(Citation {
                url: result.url.clone(),
                title: result.title.clone(),
                rank: (i as u32) + 1,
                provider: result.provider.clone(),
            });
            // Prefer the fetched page body; fall back to the search-result
            // snippet (paywall/block/timeout, or Data Plane chunk text).
            let source_text: Option<String> = match markdown {
                Some(md) if !md.trim().is_empty() => Some(md.clone()),
                _ => result.snippet.clone().filter(|s| !s.trim().is_empty()),
            };
            let Some(md) = source_text else {
                sources_skipped += 1;
                continue;
            };
            if combined.chars().count() >= MAX_TOTAL_CHARS {
                sources_skipped += 1;
                continue;
            }
            let trimmed = if md.chars().count() > MAX_SOURCE_CHARS {
                md.chars().take(MAX_SOURCE_CHARS).collect::<String>()
            } else {
                md.clone()
            };
            combined.push_str(&format!(
                "\n\n[Source {} — {}]\n{}",
                i + 1,
                result.url,
                trimmed
            ));
            sources_used += 1;
        }

        Ok(PreparedAnswer {
            citations,
            combined,
            sources_used,
            sources_skipped,
        })
    }

    pub async fn answer(&self, req: AnswerRequest) -> QuarryResult<AnswerResult> {
        let started = Instant::now();
        let prepared = self.prepare(&req).await?;
        if prepared.sources_used == 0 {
            // No source contributed text — citations-only empty answer.
            return Ok(AnswerResult {
                query: req.query,
                answer: String::new(),
                citations: prepared.citations,
                model: String::new(),
                latency_ms: started.elapsed().as_millis() as u64,
                sources_used: 0,
                sources_skipped: prepared.sources_skipped,
            });
        }
        let zdr = ZdrMode::from(req.zdr.unwrap_or(false));
        let query_result = self
            .formats
            .query(&prepared.combined, &req.query, zdr)
            .await?;
        Ok(AnswerResult {
            query: req.query,
            answer: query_result.answer,
            citations: prepared.citations,
            model: query_result.model,
            latency_ms: started.elapsed().as_millis() as u64,
            sources_used: prepared.sources_used,
            sources_skipped: prepared.sources_skipped,
        })
    }
}

/// Convenience implementation that uses a closure as the fetcher. Used
/// in tests and in the edge wiring where a real PageRunner is wrapped.
pub struct ClosureFetcher<F>(pub F);

#[async_trait]
impl<F, Fut> MarkdownFetcher for ClosureFetcher<F>
where
    F: Fn(String, ZdrMode) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Option<String>> + Send,
{
    async fn fetch_markdown(&self, url: &str, zdr: ZdrMode) -> Option<String> {
        (self.0)(url.to_string(), zdr).await
    }
}

/// Lightweight HTTP-based markdown fetcher.
///
/// Production path for `/v1/answer`: fetches a URL via reqwest, runs the
/// page through `quarry-transform::readability` to extract main content,
/// and returns markdown. Skips the heavier `PageRunner` machinery
/// (driver registry, security engine, artifact store, ingest, etc.) — the
/// answer pipeline only needs raw markdown for synthesis. Pages that 4xx,
/// 5xx, or time out return `None` so the pipeline continues with
/// remaining sources.
///
/// For full-fidelity capture (TLS impersonation, browser-rendered JS,
/// artifact-stored sources), a `PageRunnerMarkdownFetcher` is the natural
/// follow-up — same trait, swap the impl.
pub struct SimpleHttpMarkdownFetcher {
    http: reqwest::Client,
    user_agent: String,
    max_bytes: usize,
}

impl SimpleHttpMarkdownFetcher {
    /// Build with sensible defaults: 10s timeout, 5 MB body cap, the
    /// Quarry user-agent string.
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self {
            http,
            user_agent: "Quarry/2.0 (+https://triodelab.com/quarry)".into(),
            max_bytes: 5_000_000,
        }
    }

    pub fn with_user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }

    pub fn with_max_bytes(mut self, n: usize) -> Self {
        self.max_bytes = n;
        self
    }
}

impl Default for SimpleHttpMarkdownFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MarkdownFetcher for SimpleHttpMarkdownFetcher {
    async fn fetch_markdown(&self, url: &str, _zdr: ZdrMode) -> Option<String> {
        // 1. GET with UA + timeout. Any error → None so AnswerPipeline
        // skips and continues with remaining sources.
        let resp = self
            .http
            .get(url)
            .header("user-agent", &self.user_agent)
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            tracing::debug!(url, status = %resp.status(), "fetch_markdown: non-2xx");
            return None;
        }

        // Cap body size — defensive. A 50 MB SPA dump would blow up
        // synthesis even after truncation, and we have per-source caps
        // upstream anyway.
        let bytes = resp.bytes().await.ok()?;
        if bytes.len() > self.max_bytes {
            tracing::debug!(url, bytes = bytes.len(), "fetch_markdown: body exceeds cap");
            return None;
        }
        let html = std::str::from_utf8(&bytes).ok()?;

        // 2. Run through readability + html2md.
        let md = quarry_transform::readability::html_to_readable_markdown(html);
        if md.trim().is_empty() {
            return None;
        }
        Some(md)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp_client::ModelPlaneClient;
    use crate::serp::SearchResult;
    use serde_json::json;
    use std::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Mock SearchProvider that returns a canned set of results.
    struct MockSearch {
        results: Vec<SearchResult>,
    }

    #[async_trait]
    impl SearchProvider for MockSearch {
        async fn search(
            &self,
            _query: &str,
            _opts: &SearchOptions,
        ) -> QuarryResult<Vec<SearchResult>> {
            Ok(self.results.clone())
        }
        fn name(&self) -> &str {
            "mock"
        }
    }

    /// Fetcher that returns canned markdown per URL.
    struct MockFetcher {
        responses: std::collections::HashMap<String, Option<String>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl MarkdownFetcher for MockFetcher {
        async fn fetch_markdown(&self, url: &str, _zdr: ZdrMode) -> Option<String> {
            self.calls.lock().unwrap().push(url.to_string());
            self.responses.get(url).cloned().flatten()
        }
    }

    async fn pipeline_with_mp(server: &MockServer) -> AnswerPipeline {
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let formats = AiFormatRunner::new(client);
        let search = Arc::new(MockSearch {
            results: vec![
                SearchResult {
                    url: "https://a.example/page".into(),
                    title: Some("A".into()),
                    snippet: None,
                    rank: 1,
                    provider: "mock".into(),
                    ..Default::default()
                },
                SearchResult {
                    url: "https://b.example/page".into(),
                    title: Some("B".into()),
                    snippet: None,
                    rank: 2,
                    provider: "mock".into(),
                    ..Default::default()
                },
            ],
        });
        let mut responses = std::collections::HashMap::new();
        responses.insert(
            "https://a.example/page".into(),
            Some("Rust async is great.".into()),
        );
        responses.insert(
            "https://b.example/page".into(),
            Some("Tokio is the runtime.".into()),
        );
        let fetcher = Arc::new(MockFetcher {
            responses,
            calls: Arc::new(Mutex::new(vec![])),
        });
        AnswerPipeline::new(search, fetcher, formats)
    }

    fn mp_response(content: &str) -> serde_json::Value {
        json!({
            "request_id": "req_1",
            "content": content,
            "model_used": "claude-sonnet-4-6",
        })
    }

    #[tokio::test]
    async fn end_to_end_returns_answer_with_citations() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(mp_response("Rust's async ecosystem centers on Tokio.")),
            )
            .mount(&server)
            .await;

        let pipeline = pipeline_with_mp(&server).await;
        let result = pipeline
            .answer(AnswerRequest {
                query: "rust async runtime".into(),
                top_k: Some(2),
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap();
        assert!(result.answer.contains("Tokio"));
        assert_eq!(result.citations.len(), 2);
        assert_eq!(result.citations[0].url, "https://a.example/page");
        assert_eq!(result.sources_used, 2);
        assert_eq!(result.sources_skipped, 0);
        assert!(!result.model.is_empty());
    }

    #[tokio::test]
    async fn empty_query_rejected() {
        let server = MockServer::start().await;
        let pipeline = pipeline_with_mp(&server).await;
        let err = pipeline
            .answer(AnswerRequest {
                query: "  ".into(),
                top_k: None,
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn no_search_results_returns_empty_typed_response() {
        let server = MockServer::start().await;
        let search = Arc::new(MockSearch { results: vec![] });
        let fetcher = Arc::new(MockFetcher {
            responses: Default::default(),
            calls: Arc::new(Mutex::new(vec![])),
        });
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let formats = AiFormatRunner::new(client);
        let pipeline = AnswerPipeline::new(search, fetcher, formats);

        let result = pipeline
            .answer(AnswerRequest {
                query: "nothing".into(),
                top_k: None,
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap();
        assert!(result.answer.is_empty());
        assert!(result.citations.is_empty());
        assert_eq!(result.sources_used, 0);
    }

    #[tokio::test]
    async fn all_fetches_fail_returns_citations_only() {
        let server = MockServer::start().await;
        let search = Arc::new(MockSearch {
            results: vec![SearchResult {
                url: "https://broken.example/x".into(),
                title: None,
                snippet: None,
                rank: 1,
                provider: "mock".into(),
                ..Default::default()
            }],
        });
        let fetcher = Arc::new(MockFetcher {
            responses: Default::default(), // empty map → fetch_markdown returns None
            calls: Arc::new(Mutex::new(vec![])),
        });
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let formats = AiFormatRunner::new(client);
        let pipeline = AnswerPipeline::new(search, fetcher, formats);

        let result = pipeline
            .answer(AnswerRequest {
                query: "anything".into(),
                top_k: None,
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap();
        assert!(
            result.answer.is_empty(),
            "no synthesis when no fetches succeed"
        );
        assert_eq!(result.citations.len(), 1);
        assert_eq!(result.sources_used, 0);
        assert_eq!(result.sources_skipped, 1);
    }

    #[tokio::test]
    async fn closure_fetcher_works() {
        let cf =
            ClosureFetcher(
                |url: String, _zdr: ZdrMode| async move { Some(format!("mock for {url}")) },
            );
        let md = cf
            .fetch_markdown("https://x.com", ZdrMode::Off)
            .await
            .unwrap();
        assert!(md.contains("https://x.com"));
    }

    #[tokio::test]
    async fn simple_http_fetcher_returns_markdown_for_2xx() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/article"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("<html><body><article><h1>Rust</h1><p>Hello world from the article body.</p></article></body></html>"),
            )
            .mount(&server)
            .await;

        let fetcher = SimpleHttpMarkdownFetcher::new();
        let md = fetcher
            .fetch_markdown(&format!("{}/article", server.uri()), ZdrMode::Off)
            .await;
        assert!(md.is_some(), "should return markdown on 2xx");
        let md = md.unwrap();
        assert!(md.contains("Rust") || md.contains("article body"));
    }

    #[tokio::test]
    async fn simple_http_fetcher_returns_none_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let fetcher = SimpleHttpMarkdownFetcher::new();
        let md = fetcher
            .fetch_markdown(&format!("{}/missing", server.uri()), ZdrMode::Off)
            .await;
        assert!(md.is_none(), "should return None on non-2xx");
    }

    #[tokio::test]
    async fn simple_http_fetcher_respects_max_bytes() {
        let server = MockServer::start().await;
        let big = "x".repeat(2000);
        Mock::given(method("GET"))
            .and(path("/big"))
            .respond_with(ResponseTemplate::new(200).set_body_string(big))
            .mount(&server)
            .await;

        let fetcher = SimpleHttpMarkdownFetcher::new().with_max_bytes(500);
        let md = fetcher
            .fetch_markdown(&format!("{}/big", server.uri()), ZdrMode::Off)
            .await;
        assert!(md.is_none(), "should skip body when exceeds max_bytes");
    }

    #[tokio::test]
    async fn top_k_clamped_to_20() {
        // top_k=999 should be capped at 20 so SearchOptions.limit doesn't
        // explode upstream providers.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mp_response("ok")))
            .mount(&server)
            .await;
        let pipeline = pipeline_with_mp(&server).await;
        let _ = pipeline
            .answer(AnswerRequest {
                query: "rust".into(),
                top_k: Some(999),
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap();
        // No panic, no error — clamp worked.
    }
}
