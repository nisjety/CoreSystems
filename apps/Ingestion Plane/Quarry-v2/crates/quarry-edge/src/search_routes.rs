//! /v1/search — SERP-backed discovery (QRY-12) + Tavily-parity params (P0 1A).
//!
//! Wraps the runtime's `SearchProvider` trait. The edge route normalizes
//! request shape, enforces ZDR (search queries are control-plane signals
//! and never persisted, so ZDR=on is fine), and returns ranked URLs ready
//! for crawl seeding via `/v1/crawl` or one-shot scrape.
//!
//! Tavily-parity options (for Velion's search/answer UX):
//!   * `topic` (general|news|finance), `time_range`/`days` recency window,
//!   * `exact_match` (quoted-phrase), `chunks_per_source`,
//!   * `include_answer` (fuse `/v1/answer`), `format=context` (token-bounded
//!     RAG context string).
//!
//! When no provider is configured, returns 501 Unsupported with a hint.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};

use quarry_runtime::answer::{AnswerRequest, Citation};
use quarry_runtime::serp::{ImageResult, SearchOptions, SearchResult, SearXNGImages};

use crate::state::AppState;

/// Char budget for `format=context` (~2k tokens at ~4 chars/token).
const MAX_CONTEXT_CHARS: usize = 8_000;
/// Cap on sources scraped when `include_answer` is set.
const ANSWER_TOP_K: usize = 5;

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default = "default_safe_search")]
    pub safe_search: bool,
    /// Topic vertical: "general" | "news" | "finance".
    #[serde(default)]
    pub topic: Option<String>,
    /// Recency window: "day" | "week" | "month" | "year".
    #[serde(default)]
    pub time_range: Option<String>,
    /// Recency in days — convenience alias; converted to a `time_range` bucket.
    #[serde(default)]
    pub days: Option<u32>,
    /// Treat the query as an exact phrase (quoted).
    #[serde(default)]
    pub exact_match: bool,
    /// Max snippets retained per source host when building context.
    #[serde(default)]
    pub chunks_per_source: Option<u32>,
    /// Also synthesize an answer (fuses `/v1/answer`); requires answer pipeline.
    #[serde(default)]
    pub include_answer: bool,
    /// Output format: "results" (default) or "context" (RAG context string).
    #[serde(default)]
    pub format: Option<String>,
    /// Wrap query terms in `<mark>…</mark>` within result titles/snippets.
    #[serde(default)]
    pub highlight: bool,
    /// Include result aggregations (facet counts by host).
    #[serde(default)]
    pub facets: bool,
    /// Optional client-supplied request ID for tracing.
    #[serde(default)]
    #[allow(dead_code)] // scaffolding: wired in follow-up
    pub request_id: Option<String>,
}

fn default_safe_search() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub provider: String,
    pub results: Vec<SearchResult>,
    pub count: usize,
    /// Present when `include_answer=true` and synthesis succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citations: Option<Vec<Citation>>,
    /// Present when `format=context` — token-bounded RAG context string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Present when `facets=true` — result aggregations by host (desc count).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facets: Option<Vec<FacetCount>>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FacetCount {
    pub value: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

// ── pure helpers (unit-tested) ──────────────────────────────────────────────

/// Wrap the query in double-quotes for exact-phrase matching, unless it is
/// already a single quoted phrase. Lexical providers do phrase matching; SERP
/// providers (Brave/Serper/SearXNG) honor quotes natively.
pub(crate) fn apply_exact_match(query: &str, exact: bool) -> String {
    let q = query.trim();
    if !exact || (q.starts_with('"') && q.ends_with('"') && q.len() >= 2) {
        return q.to_string();
    }
    format!("\"{}\"", q.replace('"', ""))
}

/// Resolve the effective recency window from an explicit `time_range` or a
/// `days` count. Explicit `time_range` wins; otherwise `days` is bucketed.
pub(crate) fn derive_time_range(time_range: Option<&str>, days: Option<u32>) -> Option<String> {
    if let Some(tr) = time_range {
        let tr = tr.trim().to_lowercase();
        if matches!(tr.as_str(), "day" | "week" | "month" | "year") {
            return Some(tr);
        }
    }
    match days {
        Some(d) if d <= 1 => Some("day".into()),
        Some(d) if d <= 7 => Some("week".into()),
        Some(d) if d <= 31 => Some("month".into()),
        Some(_) => Some("year".into()),
        None => None,
    }
}

/// Build a token-bounded RAG context string from ranked results. Caps snippets
/// per source host at `chunks_per_source` (when set) and stops at `max_chars`.
pub(crate) fn build_context(
    results: &[SearchResult],
    chunks_per_source: Option<u32>,
    max_chars: usize,
) -> String {
    use std::collections::HashMap;
    let per_host_cap = chunks_per_source.map(|c| c as usize);
    let mut per_host: HashMap<String, usize> = HashMap::new();
    let mut out = String::new();
    let mut n = 0usize;
    for r in results {
        let host = url::Url::parse(&r.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        if let Some(cap) = per_host_cap {
            let c = per_host.entry(host.clone()).or_insert(0);
            if *c >= cap {
                continue;
            }
            *c += 1;
        }
        n += 1;
        let title = r.title.as_deref().unwrap_or("");
        let snippet = r.snippet.as_deref().unwrap_or("");
        let block = format!("[{n}] {title}\n{snippet}\n({})\n\n", r.url);
        if out.len() + block.len() > max_chars {
            break;
        }
        out.push_str(&block);
    }
    out.trim_end().to_string()
}

/// Aggregate results by host into descending-count facets (ties broken by host
/// name). Provider-agnostic — works on any `SearchResult` set.
pub(crate) fn compute_host_facets(results: &[SearchResult]) -> Vec<FacetCount> {
    use std::collections::HashMap;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for r in results {
        if let Some(h) = url::Url::parse(&r.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
        {
            *counts.entry(h).or_insert(0) += 1;
        }
    }
    let mut v: Vec<FacetCount> =
        counts.into_iter().map(|(value, count)| FacetCount { value, count }).collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.value.cmp(&b.value)));
    v
}

/// Wrap each query term (≥2 chars) in `<mark>…</mark>` within `text`,
/// case-insensitively, longest-term-first to reduce nesting. ASCII fast-path
/// keeps byte indices aligned; non-ASCII falls back to a case-sensitive
/// replace to stay UTF-8-boundary-safe.
pub(crate) fn highlight_terms(text: &str, query: &str) -> String {
    let q = query.trim().trim_matches('"');
    let mut terms: Vec<&str> = q.split_whitespace().filter(|t| t.chars().count() >= 2).collect();
    terms.sort_by_key(|t| std::cmp::Reverse(t.len()));
    let mut out = text.to_string();
    for term in terms {
        out = wrap_ci(&out, term);
    }
    out
}

fn wrap_ci(text: &str, term: &str) -> String {
    if !text.is_ascii() || !term.is_ascii() {
        return text.replace(term, &format!("<mark>{term}</mark>"));
    }
    let lower = text.to_ascii_lowercase();
    let lt = term.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len() + 13);
    let (mut last, mut from) = (0usize, 0usize);
    while let Some(rel) = lower[from..].find(&lt) {
        let start = from + rel;
        let end = start + lt.len();
        result.push_str(&text[last..start]);
        result.push_str("<mark>");
        result.push_str(&text[start..end]);
        result.push_str("</mark>");
        last = end;
        from = end;
    }
    result.push_str(&text[last..]);
    result
}

// ── handler ─────────────────────────────────────────────────────────────────

pub async fn search(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<SearchRequest>,
) -> impl IntoResponse {
    if req.query.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "query must not be empty".into(),
                code: "BAD_REQUEST".into(),
                hint: None,
            }),
        )
            .into_response();
    }

    let Some(provider) = &state.search else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(ErrorBody {
                error: "no SERP provider configured".into(),
                code: "UNSUPPORTED".into(),
                hint: Some("set BRAVE_SEARCH_KEY, SERPER_KEY, or SEARXNG_URL in edge config".into()),
            }),
        )
            .into_response();
    };

    let effective_query = apply_exact_match(&req.query, req.exact_match);
    let opts = SearchOptions {
        limit: req.limit.unwrap_or(10).min(50),
        country: req.country.clone(),
        language: req.language.clone(),
        safe_search: req.safe_search,
        topic: req.topic.clone(),
        time_range: derive_time_range(req.time_range.as_deref(), req.days),
        exact_match: req.exact_match,
        // Tenant isolation: thread the verified JWT org_id so private-corpus
        // providers (TantivyLocalIndex) restrict to this org.
        org_id: Some(claims.org_id.clone()),
    };

    match provider.search(&effective_query, &opts).await {
        Ok(results) => {
            let count = results.len();
            let provider_name = provider.name().to_string();

            // format=context — token-bounded RAG context for Velion.
            let context = match req.format.as_deref() {
                Some("context") => {
                    Some(build_context(&results, req.chunks_per_source, MAX_CONTEXT_CHARS))
                }
                _ => None,
            };

            // include_answer — best-effort synthesis; never fail search on it.
            let (answer, citations) = if req.include_answer {
                match state.answer_pipeline.as_ref() {
                    Some(p) => {
                        let areq = AnswerRequest {
                            query: req.query.clone(),
                            top_k: Some(ANSWER_TOP_K),
                            country: req.country.clone(),
                            language: req.language.clone(),
                            zdr: None,
                            org_id: Some(claims.org_id.clone()),
                        };
                        match p.answer(areq).await {
                            Ok(ar) => (Some(ar.answer), Some(ar.citations)),
                            Err(e) => {
                                tracing::warn!(error = %e, "include_answer synthesis failed");
                                (None, None)
                            }
                        }
                    }
                    None => (None, None),
                }
            } else {
                (None, None)
            };

            // Cycle 19 / cluster #19: emit SearchIssued for autocomplete-core.
            let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
            let run_id_str = run_id.to_string();
            let idem = format!("search:{}", run_id);
            state
                .event_sink
                .emit(
                    run_id,
                    quarry_core::event::EventType::SearchIssued,
                    serde_json::json!({
                        "query": req.query,
                        "provider": provider_name,
                        "result_count": count,
                        "limit": opts.limit,
                        "topic": req.topic,
                        "exact_match": req.exact_match,
                        "org_id": claims.org_id,
                        "user_id": claims.user_id,
                    }),
                    idem,
                )
                .await;

            // P3 / billing — one unit per query.
            state
                .usage
                .meter(quarry_runtime::UsageEvent::new(
                    run_id_str,
                    claims.org_id.clone(),
                    quarry_runtime::usage_metrics::SEARCH_QUERY,
                    1.0,
                    serde_json::json!({
                        "user_id": claims.user_id,
                        "result_count": count,
                        "provider": provider_name,
                        "query_chars": req.query.chars().count(),
                        "include_answer": req.include_answer,
                        "format": req.format,
                    }),
                ))
                .await;

            // 2E — facets computed from clean results (before highlight markup).
            let facets = if req.facets { Some(compute_host_facets(&results)) } else { None };
            // 2E — highlight query terms in titles/snippets when requested.
            let results = if req.highlight {
                results
                    .into_iter()
                    .map(|mut r| {
                        if let Some(t) = r.title.take() {
                            r.title = Some(highlight_terms(&t, &req.query));
                        }
                        if let Some(s) = r.snippet.take() {
                            r.snippet = Some(highlight_terms(&s, &req.query));
                        }
                        r
                    })
                    .collect()
            } else {
                results
            };

            (
                StatusCode::OK,
                Json(SearchResponse {
                    query: req.query,
                    provider: provider_name,
                    results,
                    count,
                    answer,
                    citations,
                    context,
                    facets,
                }),
            )
                .into_response()
        }
        Err(e) => {
            let status = match e.code.http_status() {
                400 => StatusCode::BAD_REQUEST,
                401 => StatusCode::UNAUTHORIZED,
                403 => StatusCode::FORBIDDEN,
                429 => StatusCode::TOO_MANY_REQUESTS,
                502 => StatusCode::BAD_GATEWAY,
                504 => StatusCode::GATEWAY_TIMEOUT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            // 3D — structured rate-limit envelope (Tavily-style) on 429.
            if status == StatusCode::TOO_MANY_REQUESTS {
                return (
                    status,
                    Json(crate::api_error::ApiError::rate_limited(e.message, 60)),
                )
                    .into_response();
            }
            (
                status,
                Json(ErrorBody {
                    error: e.message,
                    code: format!("{:?}", e.code).to_uppercase(),
                    hint: None,
                }),
            )
                .into_response()
        }
    }
}

// ── /v1/search/images — IMAGES vertical ──────────────────────────────────────

/// Request body for `POST /v1/search/images`. Deliberately minimal: the web
/// `SmartSearchRouter` has no image concept, so this path talks to SearXNG's
/// image vertical directly. Same Bearer auth as `/v1/search`.
#[derive(Debug, Deserialize)]
pub struct ImageSearchRequest {
    pub query: String,
    /// Max images to return. Defaults to 24, capped at 50.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ImageSearchResponse {
    pub query: String,
    pub provider: String,
    pub images: Vec<ImageResult>,
}

/// `POST /v1/search/images` — focused SearXNG image search.
///
/// Reuses the configured `searxng_url`. When SearXNG isn't configured the
/// route returns 501 with a hint (mirrors `/v1/search`). The response is
/// `{ images: [{ img_src, thumbnail_src, source_url, title }], query,
/// provider: "searxng" }`.
pub async fn images(
    State(state): State<AppState>,
    Extension(_claims): Extension<crate::auth::Claims>,
    Json(req): Json<ImageSearchRequest>,
) -> impl IntoResponse {
    if req.query.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "query must not be empty".into(),
                code: "BAD_REQUEST".into(),
                hint: None,
            }),
        )
            .into_response();
    }

    let Some(searxng_url) = state.searxng_url.as_deref().filter(|s| !s.is_empty()) else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(ErrorBody {
                error: "image search requires a SearXNG provider".into(),
                code: "UNSUPPORTED".into(),
                hint: Some("set SEARXNG_URL (QUARRY_EDGE__SEARXNG_URL) in edge config".into()),
            }),
        )
            .into_response();
    };

    let provider = match SearXNGImages::new(searxng_url) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: e.message,
                    code: "INTERNAL".into(),
                    hint: None,
                }),
            )
                .into_response();
        }
    };

    let limit = req.limit.unwrap_or(24).clamp(1, 50);
    match provider.search(req.query.trim(), limit).await {
        Ok(images) => (
            StatusCode::OK,
            Json(ImageSearchResponse {
                query: req.query,
                provider: "searxng".into(),
                images,
            }),
        )
            .into_response(),
        Err(e) => {
            let status = match e.code.http_status() {
                400 => StatusCode::BAD_REQUEST,
                401 => StatusCode::UNAUTHORIZED,
                403 => StatusCode::FORBIDDEN,
                429 => StatusCode::TOO_MANY_REQUESTS,
                502 => StatusCode::BAD_GATEWAY,
                504 => StatusCode::GATEWAY_TIMEOUT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            if status == StatusCode::TOO_MANY_REQUESTS {
                return (
                    status,
                    Json(crate::api_error::ApiError::rate_limited(e.message, 60)),
                )
                    .into_response();
            }
            (
                status,
                Json(ErrorBody {
                    error: e.message,
                    code: format!("{:?}", e.code).to_uppercase(),
                    hint: None,
                }),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
    use quarry_runtime::serp::{SearchOptions, SearchProvider, SearchResult};
    use std::sync::Arc;

    fn r(url: &str, title: &str, snippet: &str) -> SearchResult {
        SearchResult {
            url: url.into(),
            title: Some(title.into()),
            snippet: Some(snippet.into()),
            rank: 1,
            provider: "fake".into(),
        }
    }

    #[test]
    fn exact_match_quotes_unquoted() {
        assert_eq!(apply_exact_match("john smith", true), "\"john smith\"");
    }

    #[test]
    fn exact_match_idempotent_on_quoted() {
        assert_eq!(apply_exact_match("\"john smith\"", true), "\"john smith\"");
    }

    #[test]
    fn exact_match_off_passthrough() {
        assert_eq!(apply_exact_match("john smith", false), "john smith");
    }

    #[test]
    fn time_range_explicit_wins() {
        assert_eq!(derive_time_range(Some("week"), Some(365)), Some("week".into()));
    }

    #[test]
    fn time_range_from_days_buckets() {
        assert_eq!(derive_time_range(None, Some(1)), Some("day".into()));
        assert_eq!(derive_time_range(None, Some(5)), Some("week".into()));
        assert_eq!(derive_time_range(None, Some(20)), Some("month".into()));
        assert_eq!(derive_time_range(None, Some(400)), Some("year".into()));
        assert_eq!(derive_time_range(None, None), None);
    }

    #[test]
    fn time_range_invalid_string_falls_to_days() {
        assert_eq!(derive_time_range(Some("garbage"), Some(3)), Some("week".into()));
    }

    #[test]
    fn context_builds_numbered_blocks() {
        let results = vec![r("https://a.com/1", "A", "alpha"), r("https://b.com/2", "B", "beta")];
        let ctx = build_context(&results, None, MAX_CONTEXT_CHARS);
        assert!(ctx.contains("[1] A"));
        assert!(ctx.contains("[2] B"));
        assert!(ctx.contains("(https://a.com/1)"));
    }

    #[test]
    fn context_respects_per_source_cap() {
        let results = vec![
            r("https://a.com/1", "A1", "x"),
            r("https://a.com/2", "A2", "y"),
            r("https://a.com/3", "A3", "z"),
        ];
        let ctx = build_context(&results, Some(2), MAX_CONTEXT_CHARS);
        assert!(ctx.contains("A1") && ctx.contains("A2"));
        assert!(!ctx.contains("A3"));
    }

    #[test]
    fn context_respects_char_budget() {
        let results = vec![r("https://a.com/1", "A", &"x".repeat(100))];
        let ctx = build_context(&results, None, 20);
        assert!(ctx.len() <= 20);
    }

    #[test]
    fn host_facets_count_and_sort() {
        let results = vec![
            r("https://a.com/1", "", ""),
            r("https://a.com/2", "", ""),
            r("https://b.com/1", "", ""),
        ];
        let f = compute_host_facets(&results);
        assert_eq!(f[0], FacetCount { value: "a.com".into(), count: 2 });
        assert_eq!(f[1], FacetCount { value: "b.com".into(), count: 1 });
    }

    #[test]
    fn highlight_wraps_terms_case_insensitive() {
        let out = highlight_terms("The Rust Language is rust-y", "rust");
        assert_eq!(out.matches("<mark>").count(), 2);
        assert!(out.contains("<mark>Rust</mark>"));
        assert!(out.contains("<mark>rust</mark>"));
    }

    #[test]
    fn highlight_skips_short_and_handles_quotes() {
        // single-char terms (<2) skipped; quoted phrase unwrapped to tokens.
        let out = highlight_terms("a big cat", "\"a c\"");
        assert!(!out.contains("<mark>")); // "a" and "c" are <2 chars
    }

    struct FakeProvider {
        results: Vec<SearchResult>,
    }

    #[async_trait]
    impl SearchProvider for FakeProvider {
        async fn search(&self, _q: &str, _o: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
            Ok(self.results.clone())
        }
        fn name(&self) -> &str {
            "fake"
        }
    }

    struct ErrProvider;

    #[async_trait]
    impl SearchProvider for ErrProvider {
        async fn search(&self, _q: &str, _o: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
            Err(QuarryError::new(ErrorCode::RateLimited, "throttled"))
        }
        fn name(&self) -> &str {
            "err"
        }
    }

    #[tokio::test]
    async fn fake_provider_returns_results() {
        let p: Arc<dyn SearchProvider> =
            Arc::new(FakeProvider { results: vec![r("https://x.com/a", "A", "s")] });
        assert_eq!(p.search("q", &SearchOptions::default()).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn rate_limited_yields_typed_error() {
        let p: Arc<dyn SearchProvider> = Arc::new(ErrProvider);
        let err = p.search("q", &SearchOptions::default()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
    }
}
