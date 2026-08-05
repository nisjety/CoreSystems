//! /v1/search — SERP-backed discovery (QRY-12) + Tavily-parity params (P0 1A).
//!
//! Wraps the runtime's `SearchProvider` trait. The edge route normalizes
//! request shape, enforces ZDR (search queries are control-plane signals
//! and never persisted, so ZDR=on is fine), and returns ranked URLs ready
//! for crawl seeding via `/v1/crawl` or one-shot scrape.
//!
//! Tavily-parity options (for Verevon's search/answer UX):
//!   * `topic` (general|news|finance), `time_range`/`days` recency window,
//!   * `exact_match` (quoted-phrase), `chunks_per_source`,
//!   * `include_answer` (fuse `/v1/answer`), `format=context` (token-bounded
//!     RAG context string).
//!
//! When no provider is configured, returns 501 Unsupported with a hint.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};

use quarry_core::zdr::ZdrMode;
use quarry_runtime::answer::{AnswerRequest, Citation, MarkdownFetcher, SimpleHttpMarkdownFetcher};
use quarry_runtime::mp_client::{ModelPlaneClient, ModelPlaneInvokeRequest};
use quarry_runtime::serp::{ImageResult, SearXNGImages, SearchOptions, SearchResult};

use crate::state::AppState;

/// Char budget for `format=context` (~2k tokens at ~4 chars/token).
const MAX_CONTEXT_CHARS: usize = 8_000;
/// Cap on sources scraped when `include_answer` is set.
const ANSWER_TOP_K: usize = 5;
/// Max chars of a fetched page used as the find-similar query seed. The whole
/// page would bloat the embedding call; the lead is the strongest signal.
const SIMILAR_SEED_MAX_CHARS: usize = 2_000;

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    /// Zero Data Retention: bypass caches and durable content-bearing events.
    #[serde(default)]
    pub zdr: bool,
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
    /// Restrict results to these domains (Exa-style `includeDomains`). Applied
    /// as `site:` operators across the SERP providers.
    #[serde(default)]
    pub include_domains: Vec<String>,
    /// Exclude these domains (Exa-style `excludeDomains`). Applied as `-site:`
    /// operators across the SERP providers.
    #[serde(default)]
    pub exclude_domains: Vec<String>,
    /// Optional client-supplied request ID for tracing.
    #[serde(default)]
    #[allow(dead_code)] // scaffolding: wired in follow-up
    pub request_id: Option<String>,
}

fn default_safe_search() -> bool {
    true
}

const fn effective_zdr(requested: bool) -> bool {
    requested
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

/// Default TTL (seconds) for the general search/answer cache bucket. News /
/// recency-scoped queries override this downward — see `cache::search_ttl_secs`.
const SEARCH_CACHE_DEFAULT_TTL_SECS: u64 = 900;

/// Cached upstream of a search: provider results + any synthesized answer.
/// Post-processing (facets/highlight/context) is re-applied per request, and
/// billing/events still fire on cache hits — only the expensive provider + LLM
/// calls are skipped. Keyed org-scoped so one tenant never reads another's.
#[derive(Serialize, Deserialize)]
struct CachedSearch {
    provider: String,
    results: Vec<SearchResult>,
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    citations: Option<Vec<Citation>>,
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

/// Reduce host crowding ("result diversity"): keep at most `MAX_PER_HOST`
/// results from any single host in the primary block; results beyond that cap
/// from a dominant host are demoted to the tail (still returned, never dropped).
/// Stable — intra-host order is preserved, and results with no parseable host
/// are never capped. Applied outermost (after merge/rerank) so one domain can't
/// monopolise the visible results. Pure → unit-tested.
pub(crate) fn diversify_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    use std::collections::HashMap;
    const MAX_PER_HOST: usize = 3;
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut kept: Vec<SearchResult> = Vec::with_capacity(results.len());
    let mut deferred: Vec<SearchResult> = Vec::new();
    for result in results {
        let host = url::Url::parse(&result.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned));
        match host {
            Some(h) => {
                let count = counts.entry(h).or_insert(0);
                if *count < MAX_PER_HOST {
                    *count += 1;
                    kept.push(result);
                } else {
                    deferred.push(result);
                }
            }
            None => kept.push(result),
        }
    }
    kept.extend(deferred);
    kept
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
    let mut v: Vec<FacetCount> = counts
        .into_iter()
        .map(|(value, count)| FacetCount { value, count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.value.cmp(&b.value)));
    v
}

/// Wrap each query term (≥2 chars) in `<mark>…</mark>` within `text`,
/// case-insensitively, longest-term-first to reduce nesting. ASCII fast-path
/// keeps byte indices aligned; non-ASCII falls back to a case-sensitive
/// replace to stay UTF-8-boundary-safe.
pub(crate) fn highlight_terms(text: &str, query: &str) -> String {
    let q = query.trim().trim_matches('"');
    let mut terms: Vec<&str> = q
        .split_whitespace()
        .filter(|t| t.chars().count() >= 2)
        .collect();
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
                hint: Some(
                    "set BRAVE_SEARCH_KEY, SERPER_KEY, or SEARXNG_URL in edge config".into(),
                ),
            }),
        )
            .into_response();
    };

    let effective_query = apply_exact_match(&req.query, req.exact_match);
    let zdr = effective_zdr(req.zdr);
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
        include_domains: req.include_domains.clone(),
        exclude_domains: req.exclude_domains.clone(),
    };

    // ── Cache lookup (org-scoped, intent-driven TTL) ─────────────────────────
    // Key encodes only result-affecting params; highlight/facets/format are
    // re-applied per request so those variants share one entry. The org_id
    // segment is a hard tenant-isolation boundary. Billing + events still fire
    // on a hit (below) — only the expensive provider + synthesis are skipped.
    let params_sig = format!(
        "l={}|c={:?}|lg={:?}|s={}|t={:?}|tr={:?}|x={}|a={}|inc={:?}|exc={:?}",
        opts.limit,
        opts.country,
        opts.language,
        opts.safe_search,
        req.topic,
        opts.time_range,
        req.exact_match,
        req.include_answer,
        opts.include_domains,
        opts.exclude_domains,
    );
    let cache_key = crate::cache::SearchCache::key(&claims.org_id, &effective_query, &params_sig);
    let scache = state.redis.clone().map(crate::cache::SearchCache::new);
    let from_cache: Option<CachedSearch> = if zdr {
        None
    } else {
        match &scache {
            Some(c) => c.get(&cache_key).await,
            None => None,
        }
    };

    let (results, answer, citations, provider_name, cache_hit) = match from_cache {
        Some(c) => (c.results, c.answer, c.citations, c.provider, true),
        None => match provider.search(&effective_query, &opts).await {
            Ok(results) => {
                let provider_name = provider.name().to_string();

                // include_answer — best-effort synthesis; never fail search on it.
                let (answer, citations) = if req.include_answer {
                    match state.answer_pipeline.as_ref() {
                        Some(p) => {
                            let areq = AnswerRequest {
                                query: req.query.clone(),
                                top_k: Some(ANSWER_TOP_K),
                                country: req.country.clone(),
                                language: req.language.clone(),
                                zdr: Some(zdr),
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

                // Write-back (best-effort). Cache failures never fail the request.
                if !zdr {
                    if let Some(c) = &scache {
                        let ttl = crate::cache::search_ttl_secs(
                            req.topic.as_deref(),
                            opts.time_range.as_deref(),
                            SEARCH_CACHE_DEFAULT_TTL_SECS,
                        );
                        let payload = CachedSearch {
                            provider: provider_name.clone(),
                            results: results.clone(),
                            answer: answer.clone(),
                            citations: citations.clone(),
                        };
                        if let Err(e) = c.put_with_ttl(&cache_key, &payload, ttl).await {
                            tracing::warn!(error = %e, "search cache: put failed");
                        }
                    }
                }

                (results, answer, citations, provider_name, false)
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
                return (
                    status,
                    Json(ErrorBody {
                        error: e.message,
                        code: format!("{:?}", e.code).to_uppercase(),
                        hint: None,
                    }),
                )
                    .into_response();
            }
        },
    };

    // Result diversity: demote host crowding so one domain can't monopolise the
    // visible results (applied outermost — after the provider merge + any rerank).
    let results = diversify_results(results);
    let count = results.len();

    // format=context — token-bounded RAG context for Verevon.
    let context = match req.format.as_deref() {
        Some("context") => Some(build_context(
            &results,
            req.chunks_per_source,
            MAX_CONTEXT_CHARS,
        )),
        _ => None,
    };

    // Cycle 19 / cluster #19: emit SearchIssued for autocomplete-core.
    let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
    let run_id_str = run_id.to_string();
    let idem = format!("search:{}", run_id);
    if !zdr {
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
                    "cache_hit": cache_hit,
                }),
                idem,
            )
            .await;
    }

    // P3 / billing — one unit per query (cache hits included: the query still
    // happened; only upstream compute was saved).
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
                "cache_hit": cache_hit,
                "zdr": zdr,
            }),
        ))
        .await;

    // 2E — facets computed from clean results (before highlight markup).
    let facets = if req.facets {
        Some(compute_host_facets(&results))
    } else {
        None
    };
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

// ── /v1/search/similar — find-similar (Exa-style) ────────────────────────────

/// Request body for `POST /v1/search/similar`. Supply `text` (a passage to find
/// neighbours for) and/or `url` (its page content is fetched and used as the
/// seed). When both are present, `text` wins.
#[derive(Debug, Deserialize)]
pub struct SimilarRequest {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    /// Max neighbours to return. Defaults to 10, capped at 50.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Truncate a seed to the embedding budget on a char boundary.
fn truncate_seed(seed: &str) -> String {
    let seed = seed.trim();
    match seed.char_indices().nth(SIMILAR_SEED_MAX_CHARS) {
        Some((idx, _)) => seed[..idx].to_string(),
        None => seed.to_string(),
    }
}

/// `POST /v1/search/similar` — semantic neighbours of a passage or URL.
///
/// Delegates to the Data Plane vector index (Qdrant). Org-scoped via the JWT.
/// Returns 501 when no Data Plane is configured. Results reuse `SearchResponse`
/// (`provider: "similar"`, each hit's `score` set from the vector distance).
pub async fn similar(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<SimilarRequest>,
) -> impl IntoResponse {
    let text = req.text.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let url = req.url.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if text.is_none() && url.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "provide `text` or `url`".into(),
                code: "BAD_REQUEST".into(),
                hint: None,
            }),
        )
            .into_response();
    }

    let Some(vector) = state.vector_index.as_ref() else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(ErrorBody {
                error: "find-similar requires a Data Plane vector index".into(),
                code: "UNSUPPORTED".into(),
                hint: Some(
                    "set DATA_PLANE_URL (QUARRY_EDGE__DATA_PLANE_URL) in edge config".into(),
                ),
            }),
        )
            .into_response();
    };

    // Resolve the seed: explicit text wins; otherwise fetch the URL's content
    // (best-effort — fall back to the URL string itself so the call still runs).
    let (seed, label) = match (text, url) {
        (Some(t), _) => (truncate_seed(t), "text".to_string()),
        (None, Some(u)) => {
            let fetched = SimpleHttpMarkdownFetcher::new()
                .fetch_markdown(u, ZdrMode::from(false))
                .await
                .map(|md| truncate_seed(&md))
                .filter(|s| !s.is_empty());
            (fetched.unwrap_or_else(|| u.to_string()), u.to_string())
        }
        (None, None) => unreachable!("validated above"),
    };

    let limit = req.limit.unwrap_or(10).clamp(1, 50) as usize;
    let hits = match vector.retrieve(&claims.org_id, &seed, limit).await {
        Ok(h) => h,
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
            return (
                status,
                Json(ErrorBody {
                    error: e.message,
                    code: format!("{:?}", e.code).to_uppercase(),
                    hint: None,
                }),
            )
                .into_response();
        }
    };

    let results: Vec<SearchResult> = hits
        .into_iter()
        .enumerate()
        .map(|(i, h)| SearchResult {
            url: h.url,
            title: h.title,
            snippet: h.snippet,
            rank: (i as u32) + 1,
            provider: "similar".into(),
            score: Some(h.score),
            ..Default::default()
        })
        .collect();
    let count = results.len();

    // Meter one unit (same as a search query) so find-similar is billed.
    let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
    state
        .usage
        .meter(quarry_runtime::UsageEvent::new(
            run_id.to_string(),
            claims.org_id.clone(),
            quarry_runtime::usage_metrics::SEARCH_QUERY,
            1.0,
            serde_json::json!({
                "user_id": claims.user_id,
                "kind": "similar",
                "result_count": count,
            }),
        ))
        .await;

    (
        StatusCode::OK,
        Json(SearchResponse {
            query: label,
            provider: "similar".into(),
            results,
            count,
            answer: None,
            citations: None,
            context: None,
            facets: None,
        }),
    )
        .into_response()
}

// ── /v1/search/suggest — did-you-mean + related searches (Google-style) ───────

/// Request for `POST /v1/search/suggest`. The SPA fires this in parallel with
/// the main search so suggestions never add latency to results.
#[derive(Debug, Deserialize)]
pub struct SuggestRequest {
    pub query: String,
}

/// `corrected_query` is set only when the query likely has a typo; `related`
/// are follow-up searches. Both omitted/empty when the Model Plane is absent or
/// fails — the route always answers 200 so the UI degrades silently.
#[derive(Debug, Serialize, Default)]
pub struct EntityFact {
    pub label: String,
    pub value: String,
}

/// A compact knowledge-panel for the query's primary entity (Google-style),
/// text-only (no image source). Present only when the query is about one
/// specific entity.
#[derive(Debug, Serialize)]
pub struct EntityPanel {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    pub facts: Vec<EntityFact>,
}

#[derive(Debug, Serialize, Default)]
pub struct SuggestResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corrected_query: Option<String>,
    pub related_queries: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityPanel>,
}

#[derive(Debug, Deserialize, Default)]
struct RawFact {
    #[serde(default)]
    label: String,
    #[serde(default)]
    value: String,
}

#[derive(Debug, Deserialize, Default)]
struct RawEntity {
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    facts: Vec<RawFact>,
}

#[derive(Debug, Deserialize, Default)]
struct RawSuggestions {
    #[serde(default)]
    corrected_query: Option<String>,
    #[serde(default, alias = "related")]
    related_queries: Vec<String>,
    #[serde(default)]
    entity: Option<RawEntity>,
}

/// `POST /v1/search/suggest` — Model-Plane-backed spelling correction + related
/// queries. Degrade-safe: returns an empty `200` when no Model Plane is wired or
/// the call/parse fails, so the search UX never breaks on suggestions.
pub async fn suggest(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<SuggestRequest>,
) -> impl IntoResponse {
    let query = req.query.trim();
    let Some(mp_url) = state.model_plane_url.as_deref().filter(|s| !s.is_empty()) else {
        return (StatusCode::OK, Json(SuggestResponse::default()));
    };
    if query.is_empty() {
        return (StatusCode::OK, Json(SuggestResponse::default()));
    }
    let suggestions = fetch_suggestions(
        mp_url,
        state.service_token_provider.clone(),
        state.model_plane_token.as_deref(),
        &claims.org_id,
        query,
    )
    .await;
    (StatusCode::OK, Json(suggestions))
}

async fn fetch_suggestions(
    mp_url: &str,
    token_provider: Option<quarry_runtime::service_tokens::SharedServiceTokenProvider>,
    dev_token: Option<&str>,
    org_id: &str,
    query: &str,
) -> SuggestResponse {
    let Ok(mut client) = ModelPlaneClient::new(mp_url) else {
        return SuggestResponse::default();
    };
    if let Some(provider) = token_provider {
        client = client.with_token_provider(provider);
    } else if let Some(t) = dev_token.filter(|t| !t.is_empty()) {
        client = client.with_bearer_token(t);
    }
    let prompt = format!(
        "You are a search assistant. The user searched: \"{query}\".\n\
Return ONLY a JSON object: {{\"corrected_query\": <string or null>, \"related_queries\": [<3-5 strings>], \"entity\": <object or null>}}.\n\
corrected_query: the corrected spelling ONLY if the query likely contains a typo or misspelling; otherwise null (never paraphrase an already-correct query).\n\
related_queries: 3-5 distinct, useful follow-up searches a user might try next. No prose, no numbering.\n\
entity: a compact knowledge panel for the query's PRIMARY entity (one specific person, place, organization, product, or concept) as {{\"name\": string, \"kind\": short type label, \"summary\": 1-2 factual sentences, \"facts\": [{{\"label\": string, \"value\": string}}] with 2-5 key facts}}. Use null when the query is not about one specific entity (e.g. a how-to, comparison, or generic search). Only include facts you are confident are correct."
    );
    let req = ModelPlaneInvokeRequest {
        content: prompt,
        model: None,
        session_key: None,
        thread_id: None,
    };
    match client.invoke_for_org(org_id, &req).await {
        Ok(resp) => parse_suggestions(&resp.content, query),
        Err(e) => {
            tracing::warn!(error = %e, "suggest: model plane failed; returning no suggestions");
            SuggestResponse::default()
        }
    }
}

/// Parse the model's JSON reply into a clean [`SuggestResponse`]: drop a
/// correction equal to the original query, trim/dedupe related queries, cap at
/// 5. Unparseable replies yield empty (degrade-safe). Pure → unit-tested.
fn parse_suggestions(body: &str, query: &str) -> SuggestResponse {
    let stripped = strip_json_fences(body);
    let raw: RawSuggestions = serde_json::from_str(stripped).unwrap_or_default();
    let corrected = raw
        .corrected_query
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case(query));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let related: Vec<String> = raw
        .related_queries
        .into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| {
            !s.is_empty() && !s.eq_ignore_ascii_case(query) && seen.insert(s.to_lowercase())
        })
        .take(5)
        .collect();
    let entity = raw.entity.and_then(|e| {
        let name = e.name.trim().to_owned();
        if name.is_empty() {
            return None;
        }
        let facts: Vec<EntityFact> = e
            .facts
            .into_iter()
            .filter_map(|f| {
                let label = f.label.trim().to_owned();
                let value = f.value.trim().to_owned();
                (!label.is_empty() && !value.is_empty()).then_some(EntityFact { label, value })
            })
            .take(5)
            .collect();
        Some(EntityPanel {
            name,
            kind: e.kind.trim().to_owned(),
            summary: e.summary.trim().to_owned(),
            facts,
        })
    });
    SuggestResponse {
        corrected_query: corrected,
        related_queries: related,
        entity,
    }
}

fn strip_json_fences(body: &str) -> &str {
    let trimmed = body.trim();
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|rest| rest.trim().trim_end_matches("```").trim())
        .unwrap_or(trimmed);
    inner
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
            ..Default::default()
        }
    }

    #[test]
    fn truncate_seed_caps_on_char_boundary() {
        let short = "hello world";
        assert_eq!(truncate_seed(short), "hello world");
        let long: String = "é".repeat(SIMILAR_SEED_MAX_CHARS + 100);
        let out = truncate_seed(&long);
        // Capped at the char budget, and still valid UTF-8 (no mid-char split).
        assert_eq!(out.chars().count(), SIMILAR_SEED_MAX_CHARS);
    }

    #[test]
    fn parse_suggestions_drops_echo_and_dedupes() {
        let body = "```json\n{\"corrected_query\":\"rust async\",\"related_queries\":[\"rust async\",\"Rust Async\",\"tokio runtime\",\"async await rust\"]}\n```";
        let out = parse_suggestions(body, "rust asnyc");
        assert_eq!(out.corrected_query.as_deref(), Some("rust async"));
        // "Rust Async" dedupes against "rust async"; cap respected.
        assert_eq!(
            out.related_queries,
            vec![
                "rust async".to_string(),
                "tokio runtime".to_string(),
                "async await rust".to_string(),
            ]
        );
    }

    #[test]
    fn parse_suggestions_drops_correction_equal_to_query() {
        let out = parse_suggestions(
            "{\"corrected_query\":\"rust\",\"related_queries\":[]}",
            "RUST",
        );
        assert!(out.corrected_query.is_none());
    }

    #[test]
    fn parse_suggestions_garbage_is_empty() {
        let out = parse_suggestions("not json at all", "q");
        assert!(out.corrected_query.is_none());
        assert!(out.related_queries.is_empty());
        assert!(out.entity.is_none());
    }

    #[test]
    fn parse_suggestions_builds_entity_panel() {
        let body = r#"{"corrected_query":null,"related_queries":[],"entity":{"name":"Ada Lovelace","kind":"Person","summary":"19th-century mathematician.","facts":[{"label":"Born","value":"1815"},{"label":"","value":"drop"},{"label":"Known for","value":"first algorithm"}]}}"#;
        let entity = parse_suggestions(body, "ada lovelace")
            .entity
            .expect("entity present");
        assert_eq!(entity.name, "Ada Lovelace");
        assert_eq!(entity.kind, "Person");
        // The blank-label fact is dropped; valid ones survive.
        assert_eq!(entity.facts.len(), 2);
        assert_eq!(entity.facts[0].label, "Born");
    }

    #[test]
    fn parse_suggestions_drops_nameless_entity() {
        let body = r#"{"entity":{"name":"  ","summary":"x","facts":[]}}"#;
        assert!(parse_suggestions(body, "q").entity.is_none());
    }

    #[test]
    fn diversify_caps_host_crowding_and_preserves_order() {
        let mk = |url: &str| SearchResult {
            url: url.into(),
            provider: "x".into(),
            ..Default::default()
        };
        let out = diversify_results(vec![
            mk("https://a.com/1"),
            mk("https://a.com/2"),
            mk("https://a.com/3"),
            mk("https://a.com/4"),
            mk("https://b.com/1"),
            mk("https://a.com/5"),
        ]);
        let urls: Vec<&str> = out.iter().map(|r| r.url.as_str()).collect();
        // a.com capped at 3 in the primary block; b.com stays; over-cap a.com trails.
        assert_eq!(
            urls,
            vec![
                "https://a.com/1",
                "https://a.com/2",
                "https://a.com/3",
                "https://b.com/1",
                "https://a.com/4",
                "https://a.com/5",
            ]
        );
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
        assert_eq!(
            derive_time_range(Some("week"), Some(365)),
            Some("week".into())
        );
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
        assert_eq!(
            derive_time_range(Some("garbage"), Some(3)),
            Some("week".into())
        );
    }

    #[test]
    fn context_builds_numbered_blocks() {
        let results = vec![
            r("https://a.com/1", "A", "alpha"),
            r("https://b.com/2", "B", "beta"),
        ];
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
        assert_eq!(
            f[0],
            FacetCount {
                value: "a.com".into(),
                count: 2
            }
        );
        assert_eq!(
            f[1],
            FacetCount {
                value: "b.com".into(),
                count: 1
            }
        );
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
        let p: Arc<dyn SearchProvider> = Arc::new(FakeProvider {
            results: vec![r("https://x.com/a", "A", "s")],
        });
        assert_eq!(
            p.search("q", &SearchOptions::default())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn rate_limited_yields_typed_error() {
        let p: Arc<dyn SearchProvider> = Arc::new(ErrProvider);
        let err = p.search("q", &SearchOptions::default()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
    }
}
