//! SERP-backed search (QRY-12).
//!
//! Provider-agnostic search trait + concrete adapters for Brave Search,
//! Serper.dev, and SearXNG. Quarry uses this to seed crawls when a request
//! supplies a query rather than URLs ("scrape pages about Q1 2026 financial
//! results"). Results are ranked and de-duplicated, then handed to the
//! [`crate::CrawlFrontier`] for normal scrape execution.
//!
//! Auth model: providers use API keys passed in a Bearer header (Brave,
//! Serper) or a server URL with no auth (SearXNG). Keys live in the edge
//! config; the runtime never logs them.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResult {
    pub url: String,
    pub title: Option<String>,
    pub snippet: Option<String>,
    /// Provider-supplied rank (1-based). Lower = higher in SERP.
    pub rank: u32,
    /// Provider name ("brave", "serper", "searxng") for telemetry.
    pub provider: String,
    /// Relevance score in `[0,1]` assigned by the semantic reranker. `None`
    /// when the result was not reranked. Omitted from the wire shape when
    /// absent so non-reranked responses stay byte-identical to before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    /// Query-relevant highlight passages (Exa-style) attached by the reranker.
    /// Empty when not reranked; omitted from the wire shape when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub limit: u32,
    pub country: Option<String>,
    pub language: Option<String>,
    pub safe_search: bool,
    /// Topic vertical (Tavily parity): "general" | "news" | "finance".
    /// Providers that support verticals use it; others ignore it.
    pub topic: Option<String>,
    /// Recency window: "day" | "week" | "month" | "year". Mapped to each
    /// provider's freshness parameter when supported.
    pub time_range: Option<String>,
    /// When true the query is treated as an exact phrase (quoted) — lexical
    /// providers do phrase matching; SERP providers honor the quotes.
    pub exact_match: bool,
    /// Tenant scope. When set, providers that index private corpora (e.g.
    /// `TantivyLocalIndex`) MUST restrict results to documents whose
    /// `org_id` matches. Remote SERP providers (Brave/Serper/SearXNG/
    /// Stract) ignore this field — they search the public web and have
    /// no per-tenant concept.
    pub org_id: Option<String>,
    /// Restrict results to these domains (Exa-style `includeDomains`). Applied
    /// as `site:` operators on remote SERP providers. Empty = no restriction.
    pub include_domains: Vec<String>,
    /// Exclude these domains (Exa-style `excludeDomains`). Applied as `-site:`
    /// operators on remote SERP providers. Empty = no exclusion.
    pub exclude_domains: Vec<String>,
    /// Zero Data Retention. When true, the query text itself must not egress
    /// to an external paid SERP SaaS provider (Brave, Serper) — sending it
    /// there is a disclosure `SmartSearchRouter` must refuse regardless of
    /// the operator-wide `zero_saas_search` config toggle. In-infra providers
    /// (Tantivy, Stract, SearXNG, Data Plane) are unaffected. Defaults to
    /// `false`; callers with a real per-request ZDR signal (the edge's
    /// `/v1/search`, `/v1/answer`, `/v1/answer/stream` handlers) must set it
    /// explicitly rather than relying on `..Default::default()`.
    pub zdr: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 10,
            country: None,
            language: None,
            safe_search: true,
            topic: None,
            time_range: None,
            exact_match: false,
            org_id: None,
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
            zdr: false,
        }
    }
}

/// Append Google-style `site:` / `-site:` operators to a query for domain
/// filtering. Brave, Serper, SearXNG and Stract all proxy to engines that
/// honor these operators, so applying them at the query-string level keeps
/// domain filtering provider-agnostic. Multiple includes form an OR-group;
/// excludes are negated. Returns the query unchanged when no filters are set.
///
/// Not applied by `TantivyLocalIndex` (local corpus is already org-scoped and
/// its query parser would treat `site:` as literal tokens).
pub(crate) fn apply_domain_filters(query: &str, opts: &SearchOptions) -> String {
    if opts.include_domains.is_empty() && opts.exclude_domains.is_empty() {
        return query.to_string();
    }
    let mut q = query.trim().to_string();
    let includes: Vec<String> = opts
        .include_domains
        .iter()
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
        .map(|d| format!("site:{d}"))
        .collect();
    match includes.len() {
        0 => {}
        1 => q.push_str(&format!(" {}", includes[0])),
        _ => q.push_str(&format!(" ({})", includes.join(" OR "))),
    }
    for d in &opts.exclude_domains {
        let d = d.trim();
        if !d.is_empty() {
            q.push_str(&format!(" -site:{d}"));
        }
    }
    q.trim().to_string()
}

/// Map our recency bucket (`day|week|month|year`) to Brave's `freshness` param.
fn brave_freshness(time_range: Option<&str>) -> Option<&'static str> {
    match time_range?.trim() {
        "day" => Some("pd"),
        "week" => Some("pw"),
        "month" => Some("pm"),
        "year" => Some("py"),
        _ => None,
    }
}

/// Map our recency bucket to Google/Serper `tbs=qdr:` value.
fn serper_tbs(time_range: Option<&str>) -> Option<&'static str> {
    match time_range?.trim() {
        "day" => Some("qdr:d"),
        "week" => Some("qdr:w"),
        "month" => Some("qdr:m"),
        "year" => Some("qdr:y"),
        _ => None,
    }
}

/// SearXNG accepts `day|week|month|year` directly on `time_range`; validate so
/// we never forward a junk value.
fn searxng_time_range(time_range: Option<&str>) -> Option<&str> {
    let t = time_range?.trim();
    matches!(t, "day" | "week" | "month" | "year").then_some(t)
}

/// Map a topic vertical to a SearXNG category. Only `news` maps cleanly; other
/// topics fall through to SearXNG's default (general) vertical.
fn searxng_category(topic: Option<&str>) -> Option<&'static str> {
    match topic?.trim().to_ascii_lowercase().as_str() {
        "news" => Some("news"),
        _ => None,
    }
}

#[async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>>;
    fn name(&self) -> &str;
}

/// FallbackSearchProvider tries each underlying provider in order, returning
/// results from the first one that succeeds. On retryable errors
/// (`RateLimited`, `Timeout`, `UpstreamBlocked`, `DriverFailed`) it advances
/// to the next provider; on non-retryable errors (`BadRequest`, `Forbidden`)
/// it surfaces the error immediately.
///
/// Superseded in production by [`crate::smart_router::SmartSearchRouter`]
/// (Cycle 19 / cluster #17), which quarry-edge's `main.rs` actually
/// constructs — this type is no longer wired into any binary and is kept
/// for its simpler sequential-fallback semantics and unit tests only. It is
/// also provider-order-agnostic and, unlike `SmartSearchRouter`, has no
/// concept of ZDR or `zero_saas_search`: it will call whatever provider
/// chain it is handed, in the order given, with no compliance gating. Do
/// not wire this into a binary without adding that gating first.
pub struct FallbackSearchProvider {
    providers: Vec<std::sync::Arc<dyn SearchProvider>>,
}

impl FallbackSearchProvider {
    pub fn new(providers: Vec<std::sync::Arc<dyn SearchProvider>>) -> QuarryResult<Self> {
        if providers.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "FallbackSearchProvider requires at least one provider",
            ));
        }
        Ok(Self { providers })
    }
}

#[async_trait]
impl SearchProvider for FallbackSearchProvider {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        let mut last_err: Option<QuarryError> = None;
        for provider in &self.providers {
            match provider.search(query, opts).await {
                Ok(results) => return Ok(results),
                Err(e) if e.code.retryable() => {
                    tracing::warn!(
                        provider = provider.name(),
                        error = %e,
                        "search provider failed; trying next in chain"
                    );
                    last_err = Some(e);
                    continue;
                }
                Err(e) => return Err(e), // non-retryable: surface immediately
            }
        }
        Err(last_err.unwrap_or_else(|| {
            QuarryError::new(ErrorCode::DriverFailed, "all search providers failed")
        }))
    }

    fn name(&self) -> &str {
        "fallback"
    }
}

/// Brave Search adapter. Docs: https://search.brave.com/help/api
pub struct BraveSearch {
    http: Client,
    api_key: String,
    endpoint: String,
}

impl BraveSearch {
    pub fn new(api_key: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("brave: {e}")))?;
        Ok(Self {
            http,
            api_key: api_key.into(),
            endpoint: "https://api.search.brave.com/res/v1/web/search".into(),
        })
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}

#[async_trait]
impl SearchProvider for BraveSearch {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        let effective_query = apply_domain_filters(query, opts);
        let mut req = self
            .http
            .get(&self.endpoint)
            .header("x-subscription-token", &self.api_key)
            .header("accept", "application/json")
            .query(&[("q", effective_query.as_str())])
            .query(&[("count", &opts.limit.to_string())]);
        if let Some(c) = &opts.country {
            req = req.query(&[("country", c.as_str())]);
        }
        if let Some(l) = &opts.language {
            req = req.query(&[("search_lang", l.as_str())]);
        }
        if let Some(freshness) = brave_freshness(opts.time_range.as_deref()) {
            req = req.query(&[("freshness", freshness)]);
        }
        if opts.safe_search {
            req = req.query(&[("safesearch", "moderate")]);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::DriverFailed, format!("brave: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(map_status(status.as_u16(), "brave"));
        }
        let body: BraveResponse = resp
            .json()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::DriverFailed, format!("brave decode: {e}")))?;
        let mut results = Vec::new();
        for (i, r) in body.web.results.into_iter().enumerate() {
            results.push(SearchResult {
                url: r.url,
                title: r.title,
                snippet: r.description,
                rank: (i as u32) + 1,
                provider: "brave".into(),
                ..Default::default()
            });
        }
        Ok(results)
    }

    fn name(&self) -> &str {
        "brave"
    }
}

#[derive(Debug, Deserialize)]
struct BraveResponse {
    web: BraveWeb,
}

#[derive(Debug, Deserialize)]
struct BraveWeb {
    #[serde(default)]
    results: Vec<BraveResult>,
}

#[derive(Debug, Deserialize)]
struct BraveResult {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// Serper.dev adapter (Google SERP proxy).
pub struct SerperSearch {
    http: Client,
    api_key: String,
    endpoint: String,
}

impl SerperSearch {
    pub fn new(api_key: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("serper: {e}")))?;
        Ok(Self {
            http,
            api_key: api_key.into(),
            endpoint: "https://google.serper.dev/search".into(),
        })
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}

#[async_trait]
impl SearchProvider for SerperSearch {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        let effective_query = apply_domain_filters(query, opts);
        let mut body = serde_json::json!({
            "q": effective_query,
            "num": opts.limit,
            "gl": opts.country.clone().unwrap_or_default(),
            "hl": opts.language.clone().unwrap_or_default(),
        });
        if let Some(tbs) = serper_tbs(opts.time_range.as_deref()) {
            body["tbs"] = serde_json::Value::from(tbs);
        }
        let resp = self
            .http
            .post(&self.endpoint)
            .header("x-api-key", &self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::DriverFailed, format!("serper: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(map_status(status.as_u16(), "serper"));
        }

        let response: SerperResponse = resp.json().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, format!("serper decode: {e}"))
        })?;

        let mut results = Vec::new();
        for (i, r) in response.organic.into_iter().enumerate() {
            results.push(SearchResult {
                url: r.link,
                title: r.title,
                snippet: r.snippet,
                rank: (i as u32) + 1,
                provider: "serper".into(),
                ..Default::default()
            });
        }
        Ok(results)
    }

    fn name(&self) -> &str {
        "serper"
    }
}

#[derive(Debug, Deserialize)]
struct SerperResponse {
    #[serde(default)]
    organic: Vec<SerperOrganic>,
}

#[derive(Debug, Deserialize)]
struct SerperOrganic {
    link: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    snippet: Option<String>,
}

/// SearXNG adapter (self-hosted meta-search). No auth; cheap to run.
pub struct SearXNGSearch {
    http: Client,
    base_url: String,
}

impl SearXNGSearch {
    pub fn new(base_url: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("searxng: {e}")))?;
        Ok(Self {
            http,
            base_url: base_url.into(),
        })
    }
}

#[async_trait]
impl SearchProvider for SearXNGSearch {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        let url = format!("{}/search", self.base_url.trim_end_matches('/'));
        let effective_query = apply_domain_filters(query, opts);
        let mut req = self
            .http
            .get(&url)
            .query(&[("q", effective_query.as_str())])
            .query(&[("format", "json")])
            .query(&[("count", &opts.limit.to_string())]);
        if let Some(c) = &opts.country {
            req = req.query(&[("language", c.as_str())]);
        }
        if let Some(tr) = searxng_time_range(opts.time_range.as_deref()) {
            req = req.query(&[("time_range", tr)]);
        }
        if let Some(cat) = searxng_category(opts.topic.as_deref()) {
            req = req.query(&[("categories", cat)]);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::DriverFailed, format!("searxng: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(map_status(status.as_u16(), "searxng"));
        }
        let body: SearXNGResponse = resp.json().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, format!("searxng decode: {e}"))
        })?;
        let mut results = Vec::new();
        for (i, r) in body.results.into_iter().enumerate() {
            results.push(SearchResult {
                url: r.url,
                title: r.title,
                snippet: r.content,
                rank: (i as u32) + 1,
                provider: "searxng".into(),
                ..Default::default()
            });
        }
        Ok(results)
    }

    fn name(&self) -> &str {
        "searxng"
    }
}

#[derive(Debug, Deserialize)]
struct SearXNGResponse {
    #[serde(default)]
    results: Vec<SearXNGResult>,
}

#[derive(Debug, Deserialize)]
struct SearXNGResult {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

// ── Image search ─────────────────────────────────────────────────────────────

/// A single image hit from a SERP image vertical.
///
/// `img_src` is the full-resolution image URL; `thumbnail_src` is the (often
/// proxied) thumbnail; `source_url` is the page the image was found on. The web
/// `SearchResult` has no image fields, so image search returns this dedicated
/// shape rather than overloading the text result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageResult {
    /// Full-resolution image URL.
    pub img_src: String,
    /// Thumbnail URL (may be a SearXNG-proxied path). `None` if the provider
    /// only returned a full-size image.
    pub thumbnail_src: Option<String>,
    /// The page the image was found on (for attribution / click-through).
    pub source_url: String,
    /// Image title / alt text when provided.
    pub title: Option<String>,
}

/// SearXNG image-search adapter (self-hosted meta-search, no auth).
///
/// SearXNG natively supports an image vertical via
/// `GET /search?q=<q>&format=json&categories=images`, returning results with
/// `img_src`, `thumbnail_src`, `url` (source page), and `title`. This is a
/// focused companion to [`SearXNGSearch`] (which serves the general/web
/// vertical) so Quarry can back Verevon's IMAGES tab without disturbing the
/// `SearchProvider` chain.
pub struct SearXNGImages {
    http: Client,
    base_url: String,
}

impl SearXNGImages {
    pub fn new(base_url: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("searxng-images: {e}")))?;
        Ok(Self {
            http,
            base_url: base_url.into(),
        })
    }

    /// Run an image search. `limit` caps the number of returned hits.
    pub async fn search(&self, query: &str, limit: u32) -> QuarryResult<Vec<ImageResult>> {
        let url = format!("{}/search", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .get(&url)
            .query(&[("q", query)])
            .query(&[("format", "json")])
            .query(&[("categories", "images")])
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, format!("searxng-images: {e}"))
            })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(map_status(status.as_u16(), "searxng-images"));
        }
        let body: SearXNGImageResponse = resp.json().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("searxng-images decode: {e}"),
            )
        })?;
        Ok(map_image_results(body, limit))
    }
}

/// Pure mapping from the SearXNG image-search JSON envelope to [`ImageResult`].
/// Skips entries with no usable image URL and caps the result count at `limit`.
/// Extracted so it can be unit-tested without a live SearXNG.
fn map_image_results(body: SearXNGImageResponse, limit: u32) -> Vec<ImageResult> {
    body.results
        .into_iter()
        .filter_map(|r| {
            // Prefer the full image; fall back to the thumbnail so we never
            // surface an entry that can't render anything.
            let img_src = r.img_src.or_else(|| r.thumbnail_src.clone())?;
            Some(ImageResult {
                img_src,
                thumbnail_src: r.thumbnail_src,
                source_url: r.url.unwrap_or_default(),
                title: r.title,
            })
        })
        .take(limit.max(1) as usize)
        .collect()
}

#[derive(Debug, Deserialize)]
struct SearXNGImageResponse {
    #[serde(default)]
    results: Vec<SearXNGImageResult>,
}

#[derive(Debug, Deserialize)]
struct SearXNGImageResult {
    /// Source page the image was found on.
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    img_src: Option<String>,
    #[serde(default)]
    thumbnail_src: Option<String>,
}

/// Stract adapter — self-hosted independent search engine (Rust).
///
/// Unlike SearXNG (which aggregates Google/Bing/DDG behind a proxy),
/// Stract operates its own crawler + index + ranker. Default endpoint is
/// `http://stract:3000/beta/api/search`. No auth required.
///
/// Cycle 19 / gap-quarry cluster #17 — slots into `FallbackSearchProvider`
/// between Tantivy (own corpus) and SearXNG (long-tail aggregator).
pub struct StractSearch {
    http: Client,
    endpoint: String,
}

impl StractSearch {
    pub fn new(base_url: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("stract: {e}")))?;
        let base = base_url.into();
        let endpoint = format!("{}/beta/api/search", base.trim_end_matches('/'));
        Ok(Self { http, endpoint })
    }

    /// Override the full endpoint (used by tests against mock servers).
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}

#[async_trait]
impl SearchProvider for StractSearch {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        let effective_query = apply_domain_filters(query, opts);
        let body = serde_json::json!({
            "query": effective_query,
            "numResults": opts.limit,
            "safeSearch": opts.safe_search,
            // Stract supports a `selectedRegion` field for country bias.
            // Treat our `country` option as best-effort.
            "selectedRegion": opts.country.clone().unwrap_or_default(),
        });
        let resp = self
            .http
            .post(&self.endpoint)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::DriverFailed, format!("stract: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(map_status(status.as_u16(), "stract"));
        }

        let body: StractResponse = resp.json().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, format!("stract decode: {e}"))
        })?;

        let mut results = Vec::with_capacity(body.webpages.len());
        for (i, w) in body.webpages.into_iter().enumerate() {
            results.push(SearchResult {
                url: w.url,
                title: w.title,
                snippet: w.snippet,
                rank: (i as u32) + 1,
                provider: "stract".into(),
                ..Default::default()
            });
        }
        Ok(results)
    }

    fn name(&self) -> &str {
        "stract"
    }
}

#[derive(Debug, Deserialize)]
struct StractResponse {
    #[serde(default)]
    webpages: Vec<StractWebpage>,
}

#[derive(Debug, Deserialize)]
struct StractWebpage {
    url: String,
    #[serde(default)]
    title: Option<String>,
    /// Stract exposes the snippet under `body` in the public API. Some
    /// older builds use `snippet`; accept both via flatten + alias.
    #[serde(default, alias = "body")]
    snippet: Option<String>,
}

fn map_status(status: u16, provider: &str) -> QuarryError {
    let code = match status {
        429 => ErrorCode::RateLimited,
        401 | 403 => ErrorCode::Forbidden,
        404 => ErrorCode::NotFound,
        500..=599 => ErrorCode::DriverFailed,
        _ => ErrorCode::BadRequest,
    };
    QuarryError::new(code, format!("{provider} returned {status}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path as wpath, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn brave_returns_ranked_results() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/v1/web/search"))
            .and(query_param("q", "rust async"))
            .and(header("x-subscription-token", "key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "web": {
                    "results": [
                        {"url": "https://x.com/a", "title": "A", "description": "first"},
                        {"url": "https://x.com/b", "title": "B"},
                    ]
                }
            })))
            .mount(&server)
            .await;

        let provider = BraveSearch::new("key")
            .unwrap()
            .with_endpoint(format!("{}/v1/web/search", server.uri()));
        let results = provider
            .search("rust async", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].rank, 1);
        assert_eq!(results[0].url, "https://x.com/a");
        assert_eq!(results[1].rank, 2);
        assert_eq!(provider.name(), "brave");
    }

    #[tokio::test]
    async fn serper_returns_organic_results() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/search"))
            .and(header("x-api-key", "k"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "organic": [
                    {"link": "https://r.com/a", "title": "A", "snippet": "snip"},
                ]
            })))
            .mount(&server)
            .await;

        let provider = SerperSearch::new("k")
            .unwrap()
            .with_endpoint(format!("{}/search", server.uri()));
        let results = provider
            .search("query", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://r.com/a");
        assert_eq!(results[0].provider, "serper");
    }

    #[tokio::test]
    async fn searxng_no_auth_works() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    {"url": "https://s.com/a", "title": "A", "content": "snip"},
                ]
            })))
            .mount(&server)
            .await;

        let provider = SearXNGSearch::new(server.uri()).unwrap();
        let results = provider
            .search("query", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "searxng");
    }

    #[tokio::test]
    async fn rate_limited_maps_to_typed_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/search"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let provider = SearXNGSearch::new(server.uri()).unwrap();
        let err = provider
            .search("q", &SearchOptions::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
    }

    #[tokio::test]
    async fn searxng_images_maps_fields() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/search"))
            .and(query_param("format", "json"))
            .and(query_param("categories", "images"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    {
                        "url": "https://page.com/a",
                        "title": "Cat",
                        "img_src": "https://cdn.com/cat.jpg",
                        "thumbnail_src": "https://searx/thumb/cat.jpg"
                    },
                    // No img_src — falls back to thumbnail_src.
                    {
                        "url": "https://page.com/b",
                        "thumbnail_src": "https://searx/thumb/dog.jpg"
                    },
                    // No usable image URL — dropped.
                    { "url": "https://page.com/c", "title": "nothing" }
                ]
            })))
            .mount(&server)
            .await;

        let provider = SearXNGImages::new(server.uri()).unwrap();
        let images = provider.search("cats", 24).await.unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(
            images[0],
            ImageResult {
                img_src: "https://cdn.com/cat.jpg".into(),
                thumbnail_src: Some("https://searx/thumb/cat.jpg".into()),
                source_url: "https://page.com/a".into(),
                title: Some("Cat".into()),
            }
        );
        // Fallback: img_src defaults to thumbnail_src when absent.
        assert_eq!(images[1].img_src, "https://searx/thumb/dog.jpg");
        assert_eq!(images[1].source_url, "https://page.com/b");
    }

    #[test]
    fn image_mapping_respects_limit() {
        let body = SearXNGImageResponse {
            results: (0..10)
                .map(|i| SearXNGImageResult {
                    url: Some(format!("https://p/{i}")),
                    title: None,
                    img_src: Some(format!("https://img/{i}.jpg")),
                    thumbnail_src: None,
                })
                .collect(),
        };
        assert_eq!(map_image_results(body, 3).len(), 3);
    }

    #[tokio::test]
    async fn searxng_images_rate_limited_maps_to_typed_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/search"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let provider = SearXNGImages::new(server.uri()).unwrap();
        let err = provider.search("q", 24).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
    }

    #[test]
    fn options_default_is_safe_and_paginated() {
        let opts = SearchOptions::default();
        assert_eq!(opts.limit, 10);
        assert!(opts.safe_search);
        assert!(opts.include_domains.is_empty());
        assert!(opts.exclude_domains.is_empty());
        assert!(!opts.zdr);
    }

    #[test]
    fn domain_filters_noop_when_empty() {
        let opts = SearchOptions::default();
        assert_eq!(apply_domain_filters("rust async", &opts), "rust async");
    }

    #[test]
    fn domain_filters_single_include_appends_site() {
        let opts = SearchOptions {
            include_domains: vec!["docs.rs".into()],
            ..Default::default()
        };
        assert_eq!(apply_domain_filters("tokio", &opts), "tokio site:docs.rs");
    }

    #[test]
    fn domain_filters_multi_include_or_group() {
        let opts = SearchOptions {
            include_domains: vec!["a.com".into(), "b.com".into()],
            ..Default::default()
        };
        assert_eq!(
            apply_domain_filters("q", &opts),
            "q (site:a.com OR site:b.com)"
        );
    }

    #[test]
    fn domain_filters_exclude_negates() {
        let opts = SearchOptions {
            exclude_domains: vec!["spam.com".into(), " ".into()],
            ..Default::default()
        };
        // Blank entries are skipped.
        assert_eq!(apply_domain_filters("q", &opts), "q -site:spam.com");
    }

    #[test]
    fn time_range_mappings_are_per_provider() {
        assert_eq!(brave_freshness(Some("week")), Some("pw"));
        assert_eq!(brave_freshness(Some("nonsense")), None);
        assert_eq!(serper_tbs(Some("day")), Some("qdr:d"));
        assert_eq!(serper_tbs(None), None);
        assert_eq!(searxng_time_range(Some("month")), Some("month"));
        assert_eq!(searxng_time_range(Some("decade")), None);
    }

    #[test]
    fn searxng_category_maps_only_news() {
        assert_eq!(searxng_category(Some("news")), Some("news"));
        assert_eq!(searxng_category(Some("finance")), None);
        assert_eq!(searxng_category(None), None);
    }

    #[tokio::test]
    async fn fallback_returns_first_success() {
        use std::sync::Arc;

        struct Failing;
        #[async_trait]
        impl SearchProvider for Failing {
            async fn search(
                &self,
                _q: &str,
                _o: &SearchOptions,
            ) -> QuarryResult<Vec<SearchResult>> {
                Err(QuarryError::new(ErrorCode::RateLimited, "throttled"))
            }
            fn name(&self) -> &str {
                "failing"
            }
        }
        struct Working;
        #[async_trait]
        impl SearchProvider for Working {
            async fn search(
                &self,
                _q: &str,
                _o: &SearchOptions,
            ) -> QuarryResult<Vec<SearchResult>> {
                Ok(vec![SearchResult {
                    url: "https://x".into(),
                    title: Some("X".into()),
                    snippet: None,
                    rank: 1,
                    provider: "working".into(),
                    ..Default::default()
                }])
            }
            fn name(&self) -> &str {
                "working"
            }
        }

        let chain =
            FallbackSearchProvider::new(vec![Arc::new(Failing), Arc::new(Working)]).unwrap();
        let res = chain.search("q", &SearchOptions::default()).await.unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].provider, "working");
    }

    #[tokio::test]
    async fn fallback_surfaces_non_retryable_error_immediately() {
        use std::sync::Arc;

        struct ForbiddenProvider;
        #[async_trait]
        impl SearchProvider for ForbiddenProvider {
            async fn search(
                &self,
                _q: &str,
                _o: &SearchOptions,
            ) -> QuarryResult<Vec<SearchResult>> {
                Err(QuarryError::new(ErrorCode::Forbidden, "no key"))
            }
            fn name(&self) -> &str {
                "forbidden"
            }
        }
        struct ShouldNotBeCalled;
        #[async_trait]
        impl SearchProvider for ShouldNotBeCalled {
            async fn search(
                &self,
                _q: &str,
                _o: &SearchOptions,
            ) -> QuarryResult<Vec<SearchResult>> {
                panic!("non-retryable error should not advance to next provider");
            }
            fn name(&self) -> &str {
                "panic"
            }
        }

        let chain = FallbackSearchProvider::new(vec![
            Arc::new(ForbiddenProvider),
            Arc::new(ShouldNotBeCalled),
        ])
        .unwrap();
        let err = chain
            .search("q", &SearchOptions::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
    }

    #[tokio::test]
    async fn fallback_returns_last_error_when_all_retry_fail() {
        use std::sync::Arc;

        struct RateLimited(usize);
        #[async_trait]
        impl SearchProvider for RateLimited {
            async fn search(
                &self,
                _q: &str,
                _o: &SearchOptions,
            ) -> QuarryResult<Vec<SearchResult>> {
                Err(QuarryError::new(
                    ErrorCode::RateLimited,
                    format!("provider-{}-rate-limited", self.0),
                ))
            }
            fn name(&self) -> &str {
                "rate"
            }
        }

        let chain = FallbackSearchProvider::new(vec![
            Arc::new(RateLimited(1)),
            Arc::new(RateLimited(2)),
            Arc::new(RateLimited(3)),
        ])
        .unwrap();
        let err = chain
            .search("q", &SearchOptions::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
        assert!(err.message.contains("provider-3"));
    }

    #[test]
    fn fallback_rejects_empty_chain() {
        match FallbackSearchProvider::new(vec![]) {
            Ok(_) => panic!("expected error for empty provider chain"),
            Err(e) => assert_eq!(e.code, ErrorCode::BadRequest),
        }
    }

    #[tokio::test]
    async fn stract_returns_webpages() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/beta/api/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "webpages": [
                    {"url": "https://x.com/a", "title": "A", "snippet": "first"},
                    {"url": "https://x.com/b", "title": "B", "body": "second"},
                ]
            })))
            .mount(&server)
            .await;

        let provider = StractSearch::new(server.uri()).unwrap();
        let results = provider
            .search("rust async", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].provider, "stract");
        assert_eq!(results[0].url, "https://x.com/a");
        assert_eq!(results[0].snippet.as_deref(), Some("first"));
        // Verify `body` field-alias for `snippet` works on second item.
        assert_eq!(results[1].snippet.as_deref(), Some("second"));
        assert_eq!(provider.name(), "stract");
    }

    #[tokio::test]
    async fn stract_rate_limited_maps_to_typed_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/beta/api/search"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let provider = StractSearch::new(server.uri()).unwrap();
        let err = provider
            .search("q", &SearchOptions::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
    }

    #[tokio::test]
    async fn stract_empty_webpages_returns_empty_vec() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/beta/api/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"webpages": []})))
            .mount(&server)
            .await;

        let provider = StractSearch::new(server.uri()).unwrap();
        let results = provider
            .search("nothing", &SearchOptions::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }
}
