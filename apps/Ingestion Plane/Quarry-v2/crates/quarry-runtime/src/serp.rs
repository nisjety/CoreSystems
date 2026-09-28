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

use crate::fusion::RRF_K;
use crate::smart_router::QueryIntent;

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
    /// Upstream engines that returned this URL, as named by the metasearch
    /// aggregator (e.g. `["google cse", "seznam"]`). Empty for single-engine
    /// providers, which have no cross-engine concept.
    ///
    /// The *length* of this vector is the agreement signal — the one quality
    /// measure a metasearch has that a single engine cannot produce — so it is
    /// carried all the way to the wire rather than collapsed into a number
    /// here; a Model Plane consumer already deserializes this field by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub engines: Vec<String>,
    /// Reciprocal Rank Fusion score over the per-engine ranks that produced
    /// this result. `None` when the provider supplied no per-engine rank
    /// vector to fuse (every non-metasearch provider, and older SearXNG
    /// builds that omit `positions`), which is distinct from a fused score
    /// that happened to come out low.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fusion_score: Option<f32>,
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
    /// Per-request permission to dispatch the paid external SERP providers
    /// (Brave, Serper). This is a *permission*, not an override: it is ANDed
    /// with the existing [`Self::zdr`] check and never replaces it, so a ZDR
    /// request refuses paid providers no matter what this says. It is also
    /// independent of the operator-wide `zero_saas_search` toggle, which
    /// gates the same providers one level up.
    ///
    /// Defaults to `false` — absent permission means none. A caller that
    /// wants paid fan-out has to say so on the request.
    pub allow_paid_providers: bool,
    /// Caller-supplied intent HINT. Purely advisory: the router may classify
    /// the query itself and is free to disagree. `None` means "no hint,
    /// classify normally".
    pub intent: Option<QueryIntent>,
}

impl SearchOptions {
    /// Whether paid external SERP providers (Brave, Serper) may be dispatched
    /// for this request: permission granted AND not a ZDR request.
    ///
    /// One helper rather than the conjunction open-coded at each dispatch
    /// site — four hand-written `!zdr && allow_paid` conditions across the
    /// router is exactly how one of them ends up missing the `zdr` half and
    /// leaks a ZDR query to a SaaS provider.
    pub fn paid_allowed(&self) -> bool {
        !self.zdr && self.allow_paid_providers
    }
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
            allow_paid_providers: false,
            intent: None,
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

/// Base language subtag of a BCP-47-ish tag, lowercased: `"nb-NO"` → `"nb"`.
/// Returns `None` for anything that isn't a 2–3 letter language code, so a
/// junk value is dropped rather than forwarded to an upstream that would
/// reject the whole request over it.
fn language_base(language: Option<&str>) -> Option<String> {
    let base = language?
        .trim()
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    ((2..=3).contains(&base.len()) && base.chars().all(|c| c.is_ascii_alphabetic())).then_some(base)
}

/// ISO-3166 alpha-2 region subtag, uppercased: `"no"` → `"NO"`. Anything that
/// isn't two ASCII letters is dropped.
fn region_code(country: Option<&str>) -> Option<String> {
    let c = country?.trim();
    (c.len() == 2 && c.chars().all(|ch| ch.is_ascii_alphabetic())).then(|| c.to_ascii_uppercase())
}

/// Fold a modern language subtag onto the code Brave's `search_lang` and
/// Google/Serper's `hl` actually understand.
///
/// Both predate BCP-47's split of Norwegian into bokmål (`nb`) and nynorsk
/// (`nn`) and only list the macrolanguage `no`. Verevon's callers send
/// `nb`/`nb-NO`, so without this fold a Norwegian query names a language
/// neither engine lists and silently gets the generic global SERP back —
/// which is exactly how the web results ended up thin and off-topic.
fn legacy_language_code(base: &str) -> &str {
    match base {
        "nb" | "nn" => "no",
        other => other,
    }
}

/// Build SearXNG's `language` value from the caller's language + country.
///
/// SearXNG validates this parameter against `^[a-z]{2,3}(-[a-zA-Z]{2})?$` and
/// rejects the request outright on a miss, so a bare country code ("NO") sent
/// here is not merely the wrong field — it 400s the whole search. The
/// language is therefore the only source of the base tag; `country` may only
/// refine it into a locale when the language alone is ambiguous
/// (`"nb"` + `"NO"` → `"nb-NO"`). `None` language → `None`, leaving SearXNG on
/// its own default.
fn searxng_locale(language: Option<&str>, country: Option<&str>) -> Option<String> {
    let base = language_base(language)?;
    // A caller-supplied locale already names its region; `country` only fills
    // the gap, it never overrides what the language tag stated explicitly.
    let explicit = language.and_then(|l| l.trim().split(['-', '_']).nth(1));
    match region_code(explicit).or_else(|| region_code(country)) {
        Some(region) => Some(format!("{base}-{region}")),
        None => Some(base),
    }
}

/// SearXNG's `safesearch` is numeric (0 = off, 1 = moderate, 2 = strict).
/// Our boolean maps onto moderate so all three SERP adapters agree — Brave
/// sends its own "moderate" for the same flag.
fn searxng_safesearch(safe_search: bool) -> &'static str {
    if safe_search {
        "1"
    } else {
        "0"
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
        // Brave wants an alpha-2 country and a bare language code; handing it
        // the caller's raw locale ("nb-NO") is a 422, not a soft ignore, so
        // both are normalized before they go on the wire.
        if let Some(c) = region_code(opts.country.as_deref()) {
            req = req.query(&[("country", c.as_str())]);
        }
        if let Some(l) = language_base(opts.language.as_deref()) {
            req = req.query(&[("search_lang", legacy_language_code(&l))]);
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
        });
        // `gl`/`hl` used to be sent unconditionally with `unwrap_or_default()`,
        // i.e. as empty strings — which Google reads as an explicit "no region,
        // no language" rather than as an absent preference, pinning every
        // request to the generic global SERP. Send them only when the caller
        // asked, and send `hl` as the bare language code Google lists.
        if let Some(gl) = region_code(opts.country.as_deref()) {
            body["gl"] = serde_json::Value::from(gl.to_ascii_lowercase());
        }
        if let Some(hl) = language_base(opts.language.as_deref()) {
            body["hl"] = serde_json::Value::from(legacy_language_code(&hl));
        }
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
        if let Some(locale) = searxng_locale(opts.language.as_deref(), opts.country.as_deref()) {
            req = req.query(&[("language", locale.as_str())]);
        }
        req = req.query(&[("safesearch", searxng_safesearch(opts.safe_search))]);
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
        let mut results = Vec::with_capacity(body.results.len());
        for (i, r) in body.results.into_iter().enumerate() {
            results.push(SearchResult {
                url: r.url,
                title: r.title,
                snippet: r.content,
                // Provisional: overwritten by `rank_by_fusion` below. Assigned
                // here so a response with no `positions` anywhere still lands
                // on the pre-fusion ranks after the (stable, all-ties) sort.
                rank: (i as u32) + 1,
                provider: "searxng".into(),
                fusion_score: rrf_score(&r.positions),
                engines: r.engines,
                ..Default::default()
            });
        }
        Ok(rank_by_fusion(results))
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
    /// Upstream engines that returned this URL.
    #[serde(default)]
    engines: Vec<String>,
    /// Per-engine 1-based rank, index-aligned with `engines`. Absent on older
    /// SearXNG builds, hence the serde default rather than a hard requirement.
    #[serde(default)]
    positions: Vec<u32>,
}

/// Reciprocal Rank Fusion over one result's own per-engine rank vector.
///
/// Each engine that placed the URL at rank `p` contributes `1/(k + p)`, so a
/// URL several engines agree on outranks one a single engine put first. `None`
/// for an empty vector: no per-engine ranks is "nothing to fuse", which is a
/// different statement than "fused and scored zero", and the caller relies on
/// that distinction to leave such results in their original order.
fn rrf_score(positions: &[u32]) -> Option<f32> {
    if positions.is_empty() {
        return None;
    }
    Some(
        positions
            .iter()
            .map(|p| 1.0 / (RRF_K + *p as f32))
            .sum::<f32>(),
    )
}

/// Re-rank a provider's results by their fused per-engine scores.
///
/// SearXNG's emission order is category-grouped presentation order, not score
/// order — an investigation caught a bing rank-1 hit sitting sixth behind a
/// naver rank-3 — so taking `rank = i + 1` from the response as it arrives
/// ranks on SearXNG's page layout instead of on engine consensus. Fusing the
/// `positions` vector recovers the real ordering.
///
/// The sort is STABLE and results with no `positions` compare as zero, so a
/// response where nothing carries per-engine ranks (any other provider, or an
/// older SearXNG) comes back in exactly the order it arrived, with exactly the
/// ranks it would have had before this function existed.
fn rank_by_fusion(mut results: Vec<SearchResult>) -> Vec<SearchResult> {
    results.sort_by(|a, b| {
        b.fusion_score
            .unwrap_or(0.0)
            .total_cmp(&a.fusion_score.unwrap_or(0.0))
    });
    for (i, r) in results.iter_mut().enumerate() {
        r.rank = (i as u32) + 1;
    }
    results
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

// ── Video search ─────────────────────────────────────────────────────────────

/// A single video hit from a SERP video vertical.
///
/// `url` is the watch page; `iframe_src` is the embeddable player URL when the
/// upstream engine exposes one (SearXNG rewrites YouTube/Vimeo to their
/// privacy-preserving embed hosts). `length` and `published_date` stay as the
/// upstream strings — engines disagree on both formats ("3:21" vs "PT3M21S",
/// ISO-8601 vs "2 days ago") and parsing them here would silently drop the
/// ones that do not fit whichever shape we picked.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VideoResult {
    /// Watch page for the video (for attribution / click-through).
    pub url: String,
    /// Video title when provided.
    pub title: Option<String>,
    /// Poster/preview image (may be a SearXNG-proxied path).
    pub thumbnail_src: Option<String>,
    /// Embeddable player URL, when the engine exposes one.
    pub iframe_src: Option<String>,
    /// Uploader / channel name.
    pub author: Option<String>,
    /// Duration as the upstream reported it.
    pub length: Option<String>,
    /// Publication date as the upstream reported it.
    pub published_date: Option<String>,
    /// Description snippet.
    pub content: Option<String>,
}

/// SearXNG video-search adapter (self-hosted meta-search, no auth).
///
/// Same shape as [`SearXNGImages`], differing only in the `categories=videos`
/// parameter and in the per-result fields the video vertical returns. A
/// focused companion to [`SearXNGSearch`] so Quarry can back a VIDEOS tab
/// without disturbing the `SearchProvider` chain.
pub struct SearXNGVideos {
    http: Client,
    base_url: String,
}

impl SearXNGVideos {
    pub fn new(base_url: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("searxng-videos: {e}")))?;
        Ok(Self {
            http,
            base_url: base_url.into(),
        })
    }

    /// Run a video search. `limit` caps the number of returned hits.
    pub async fn search(&self, query: &str, limit: u32) -> QuarryResult<Vec<VideoResult>> {
        let url = format!("{}/search", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .get(&url)
            .query(&[("q", query)])
            .query(&[("format", "json")])
            .query(&[("categories", "videos")])
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, format!("searxng-videos: {e}"))
            })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(map_status(status.as_u16(), "searxng-videos"));
        }
        let body: SearXNGVideoResponse = resp.json().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("searxng-videos decode: {e}"),
            )
        })?;
        Ok(map_video_results(body, limit))
    }
}

/// Pure mapping from the SearXNG video-search JSON envelope to [`VideoResult`].
/// Skips entries with no watch URL and caps the result count at `limit`.
/// Extracted so it can be unit-tested without a live SearXNG.
fn map_video_results(body: SearXNGVideoResponse, limit: u32) -> Vec<VideoResult> {
    body.results
        .into_iter()
        .filter_map(|r| {
            // Unlike an image (which is itself the payload), a video hit is
            // only useful as a link — an entry with no watch page can neither
            // be opened nor attributed, so drop it.
            let url = r.url.filter(|u| !u.trim().is_empty())?;
            Some(VideoResult {
                url,
                title: r.title,
                thumbnail_src: r.thumbnail,
                iframe_src: r.iframe_src,
                author: r.author,
                length: r.length,
                published_date: r.published_date,
                content: r.content,
            })
        })
        .take(limit.max(1) as usize)
        .collect()
}

#[derive(Debug, Deserialize)]
struct SearXNGVideoResponse {
    #[serde(default)]
    results: Vec<SearXNGVideoResult>,
}

#[derive(Debug, Deserialize)]
struct SearXNGVideoResult {
    /// Watch page for the video.
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    content: Option<String>,
    /// The video vertical names its poster `thumbnail`, but engines routed
    /// through the image pipeline emit `img_src` for the same thing; accept
    /// either so a mixed-engine response does not come back half-illustrated.
    #[serde(default, alias = "img_src")]
    thumbnail: Option<String>,
    #[serde(default)]
    iframe_src: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    length: Option<String>,
    #[serde(default, alias = "publishedDate")]
    published_date: Option<String>,
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
    use wiremock::matchers::{
        body_json, header, method, path as wpath, query_param, query_param_is_missing,
    };
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

    #[tokio::test]
    async fn searxng_videos_maps_fields() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/search"))
            .and(query_param("format", "json"))
            .and(query_param("categories", "videos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    {
                        "url": "https://tube.example/watch?v=a",
                        "title": "Rust in 100 seconds",
                        "content": "A short intro.",
                        "thumbnail": "https://searx/thumb/a.jpg",
                        "iframe_src": "https://tube.example/embed/a",
                        "author": "Some Channel",
                        "length": "1:40",
                        "publishedDate": "2026-01-02T00:00:00"
                    },
                    // `img_src` alias for the poster.
                    {
                        "url": "https://tube.example/watch?v=b",
                        "img_src": "https://searx/thumb/b.jpg"
                    },
                    // No watch URL — dropped.
                    { "title": "orphan", "thumbnail": "https://searx/thumb/c.jpg" }
                ]
            })))
            .mount(&server)
            .await;

        let provider = SearXNGVideos::new(server.uri()).unwrap();
        let videos = provider.search("rust", 24).await.unwrap();
        assert_eq!(videos.len(), 2);
        assert_eq!(
            videos[0],
            VideoResult {
                url: "https://tube.example/watch?v=a".into(),
                title: Some("Rust in 100 seconds".into()),
                thumbnail_src: Some("https://searx/thumb/a.jpg".into()),
                iframe_src: Some("https://tube.example/embed/a".into()),
                author: Some("Some Channel".into()),
                length: Some("1:40".into()),
                published_date: Some("2026-01-02T00:00:00".into()),
                content: Some("A short intro.".into()),
            }
        );
        assert_eq!(
            videos[1].thumbnail_src.as_deref(),
            Some("https://searx/thumb/b.jpg")
        );
        assert!(videos[1].iframe_src.is_none());
    }

    #[test]
    fn video_mapping_respects_limit() {
        let body = SearXNGVideoResponse {
            results: (0..10)
                .map(|i| SearXNGVideoResult {
                    url: Some(format!("https://tube/{i}")),
                    title: None,
                    content: None,
                    thumbnail: None,
                    iframe_src: None,
                    author: None,
                    length: None,
                    published_date: None,
                })
                .collect(),
        };
        assert_eq!(map_video_results(body, 3).len(), 3);
    }

    #[tokio::test]
    async fn searxng_videos_rate_limited_maps_to_typed_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/search"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let provider = SearXNGVideos::new(server.uri()).unwrap();
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
        assert!(!opts.allow_paid_providers);
        assert!(opts.intent.is_none());
    }

    #[test]
    fn default_options_deny_paid_providers() {
        assert!(!SearchOptions::default().paid_allowed());
    }

    #[test]
    fn paid_allowed_requires_explicit_permission() {
        let opts = SearchOptions {
            allow_paid_providers: true,
            ..Default::default()
        };
        assert!(opts.paid_allowed());
    }

    #[test]
    fn zdr_overrides_paid_permission() {
        // The permission is ANDed with ZDR, never a substitute for it: a ZDR
        // request must not reach Brave/Serper even when the caller granted
        // paid fan-out.
        let opts = SearchOptions {
            allow_paid_providers: true,
            zdr: true,
            ..Default::default()
        };
        assert!(!opts.paid_allowed());
    }

    #[test]
    fn intent_hint_round_trips_through_options() {
        let opts = SearchOptions {
            intent: Some(QueryIntent::Fresh),
            ..Default::default()
        };
        assert_eq!(opts.intent, Some(QueryIntent::Fresh));
    }

    #[test]
    fn rrf_score_sums_reciprocals_and_rewards_agreement() {
        assert_eq!(rrf_score(&[]), None);
        let single = rrf_score(&[1]).unwrap();
        assert!((single - 1.0 / (RRF_K + 1.0)).abs() < f32::EPSILON);
        // Two engines at rank 3 beat one engine at rank 1 — agreement is the
        // whole point of fusing.
        assert!(rrf_score(&[3, 3]).unwrap() > single);
    }

    fn searxng_hit(url: &str, positions: &[u32]) -> SearchResult {
        SearchResult {
            url: url.into(),
            rank: 0,
            provider: "searxng".into(),
            fusion_score: rrf_score(positions),
            ..Default::default()
        }
    }

    #[test]
    fn fusion_beats_presentation_order() {
        // The observed failure: a bing rank-1 hit emitted behind a naver
        // rank-3 hit because SearXNG groups by category, not by score.
        let emitted = vec![
            searxng_hit("https://naver.example/", &[3]),
            searxng_hit("https://bing.example/", &[1]),
        ];
        let ranked = rank_by_fusion(emitted);
        assert_eq!(ranked[0].url, "https://bing.example/");
        assert_eq!(ranked[0].rank, 1);
        assert_eq!(ranked[1].url, "https://naver.example/");
        assert_eq!(ranked[1].rank, 2);
    }

    #[test]
    fn cross_engine_agreement_outranks_a_single_first_place() {
        let ranked = rank_by_fusion(vec![
            searxng_hit("https://solo.example/", &[1]),
            searxng_hit("https://agreed.example/", &[2, 2, 4]),
        ]);
        assert_eq!(ranked[0].url, "https://agreed.example/");
    }

    #[test]
    fn results_without_positions_keep_original_order_and_ranks() {
        // Other providers, and older SearXNG builds, send no `positions`.
        // Those must come back byte-identical to the pre-fusion behaviour.
        let emitted = vec![
            searxng_hit("https://a.example/", &[]),
            searxng_hit("https://b.example/", &[]),
            searxng_hit("https://c.example/", &[]),
        ];
        let ranked = rank_by_fusion(emitted);
        let urls: Vec<&str> = ranked.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://a.example/",
                "https://b.example/",
                "https://c.example/"
            ]
        );
        assert_eq!(ranked[0].rank, 1);
        assert_eq!(ranked[2].rank, 3);
        assert!(ranked.iter().all(|r| r.fusion_score.is_none()));
    }

    #[test]
    fn equal_fusion_scores_preserve_emission_order() {
        // Stability matters: a tie must not reshuffle the upstream order.
        let ranked = rank_by_fusion(vec![
            searxng_hit("https://first.example/", &[2]),
            searxng_hit("https://second.example/", &[2]),
        ]);
        assert_eq!(ranked[0].url, "https://first.example/");
        assert_eq!(ranked[1].url, "https://second.example/");
    }

    #[tokio::test]
    async fn searxng_ranks_by_fusion_not_emission_order() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    {
                        "url": "https://naver.example/",
                        "title": "N",
                        "engines": ["naver"],
                        "positions": [3]
                    },
                    {
                        "url": "https://bing.example/",
                        "title": "B",
                        "engines": ["bing", "duckduckgo"],
                        "positions": [1, 2]
                    },
                ]
            })))
            .mount(&server)
            .await;

        let provider = SearXNGSearch::new(server.uri()).unwrap();
        let results = provider
            .search("q", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results[0].url, "https://bing.example/");
        assert_eq!(results[0].rank, 1);
        assert_eq!(results[0].engines, vec!["bing", "duckduckgo"]);
        assert!(results[0].fusion_score.unwrap() > results[1].fusion_score.unwrap());
        assert_eq!(results[1].rank, 2);
    }

    #[tokio::test]
    async fn searxng_without_provenance_keeps_emission_order() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    {"url": "https://s.com/a", "title": "A", "content": "snip"},
                    {"url": "https://s.com/b", "title": "B", "content": "snip"},
                ]
            })))
            .mount(&server)
            .await;

        let provider = SearXNGSearch::new(server.uri()).unwrap();
        let results = provider
            .search("q", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results[0].url, "https://s.com/a");
        assert_eq!(results[0].rank, 1);
        assert_eq!(results[1].url, "https://s.com/b");
        assert_eq!(results[1].rank, 2);
        assert!(results[0].engines.is_empty());
        assert!(results[0].fusion_score.is_none());
    }

    #[test]
    fn engines_round_trip_through_the_wire_shape() {
        let result = SearchResult {
            url: "https://x.example/".into(),
            rank: 1,
            provider: "searxng".into(),
            engines: vec!["google cse".into(), "seznam".into()],
            fusion_score: Some(0.25),
            ..Default::default()
        };
        let encoded = serde_json::to_string(&result).unwrap();
        let decoded: SearchResult = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.engines, vec!["google cse", "seznam"]);
        assert_eq!(decoded.fusion_score, Some(0.25));
    }

    #[test]
    fn empty_engines_and_fusion_score_stay_off_the_wire() {
        // Additive and wire-compatible: a result with no provenance must
        // serialize byte-identically to the pre-change shape.
        let encoded = serde_json::to_string(&SearchResult {
            url: "https://x.example/".into(),
            rank: 1,
            provider: "brave".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(!encoded.contains("engines"));
        assert!(!encoded.contains("fusion_score"));
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

    #[test]
    fn language_base_takes_subtag_and_rejects_junk() {
        assert_eq!(language_base(Some("nb-NO")).as_deref(), Some("nb"));
        assert_eq!(language_base(Some("NB_no")).as_deref(), Some("nb"));
        assert_eq!(language_base(Some("en")).as_deref(), Some("en"));
        assert_eq!(language_base(Some("  nn  ")).as_deref(), Some("nn"));
        assert_eq!(language_base(Some("norsk-bokmål")), None);
        assert_eq!(language_base(Some("")), None);
        assert_eq!(language_base(None), None);
    }

    #[test]
    fn region_code_normalizes_alpha2_only() {
        assert_eq!(region_code(Some("no")).as_deref(), Some("NO"));
        assert_eq!(region_code(Some(" NO ")).as_deref(), Some("NO"));
        assert_eq!(region_code(Some("NOR")), None);
        assert_eq!(region_code(Some("47")), None);
        assert_eq!(region_code(None), None);
    }

    #[test]
    fn legacy_language_code_folds_norwegian_to_macrolanguage() {
        assert_eq!(legacy_language_code("nb"), "no");
        assert_eq!(legacy_language_code("nn"), "no");
        assert_eq!(legacy_language_code("en"), "en");
    }

    #[test]
    fn searxng_locale_refines_language_with_country() {
        // Language alone is ambiguous → country supplies the region.
        assert_eq!(
            searxng_locale(Some("nb"), Some("no")).as_deref(),
            Some("nb-NO")
        );
        // An explicit locale wins over a conflicting country.
        assert_eq!(
            searxng_locale(Some("nb-NO"), Some("se")).as_deref(),
            Some("nb-NO")
        );
        // No country → bare language, which SearXNG accepts.
        assert_eq!(searxng_locale(Some("en"), None).as_deref(), Some("en"));
    }

    #[test]
    fn searxng_locale_never_forwards_a_bare_country() {
        // The pre-fix adapter sent `language=NO` here, which SearXNG 400s.
        assert_eq!(searxng_locale(None, Some("NO")), None);
        assert_eq!(searxng_locale(None, None), None);
    }

    #[test]
    fn searxng_safesearch_is_numeric() {
        assert_eq!(searxng_safesearch(true), "1");
        assert_eq!(searxng_safesearch(false), "0");
    }

    #[tokio::test]
    async fn searxng_sends_language_and_safesearch() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/search"))
            .and(query_param("language", "nb-NO"))
            .and(query_param("safesearch", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{"url": "https://s.no/a", "title": "A", "content": "snip"}]
            })))
            .mount(&server)
            .await;

        let provider = SearXNGSearch::new(server.uri()).unwrap();
        let opts = SearchOptions {
            language: Some("nb".into()),
            country: Some("no".into()),
            ..Default::default()
        };
        let results = provider.search("siste nytt", &opts).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn searxng_omits_language_when_only_country_is_set() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/search"))
            .and(query_param_is_missing("language"))
            .and(query_param("safesearch", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
            .mount(&server)
            .await;

        let provider = SearXNGSearch::new(server.uri()).unwrap();
        let opts = SearchOptions {
            country: Some("NO".into()),
            safe_search: false,
            ..Default::default()
        };
        assert!(provider.search("q", &opts).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn brave_normalizes_country_and_search_lang() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wpath("/v1/web/search"))
            .and(query_param("country", "NO"))
            .and(query_param("search_lang", "no"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "web": {"results": [{"url": "https://x.no/a", "title": "A"}]}
            })))
            .mount(&server)
            .await;

        let provider = BraveSearch::new("key")
            .unwrap()
            .with_endpoint(format!("{}/v1/web/search", server.uri()));
        let opts = SearchOptions {
            language: Some("nb-NO".into()),
            country: Some("no".into()),
            ..Default::default()
        };
        assert_eq!(provider.search("q", &opts).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn serper_omits_gl_and_hl_when_unset() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/search"))
            // Exact body — proves the empty-string `gl`/`hl` keys are gone.
            .and(body_json(json!({"q": "query", "num": 10})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"organic": []})))
            .mount(&server)
            .await;

        let provider = SerperSearch::new("k")
            .unwrap()
            .with_endpoint(format!("{}/search", server.uri()));
        assert!(provider
            .search("query", &SearchOptions::default())
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn serper_sends_normalized_gl_and_hl() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/search"))
            .and(body_json(
                json!({"q": "query", "num": 10, "gl": "no", "hl": "no"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"organic": []})))
            .mount(&server)
            .await;

        let provider = SerperSearch::new("k")
            .unwrap()
            .with_endpoint(format!("{}/search", server.uri()));
        let opts = SearchOptions {
            language: Some("nb-NO".into()),
            country: Some("NO".into()),
            ..Default::default()
        };
        assert!(provider.search("query", &opts).await.unwrap().is_empty());
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
