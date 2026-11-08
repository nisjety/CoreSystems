//! INNH-6a: sitemap discovery for a crawl, `POST /v1/internal/crawl/sitemap`.
//!
//! The orchestrator (Go) cannot fetch a sitemap safely, so it asks the edge, which
//! has the guarded driver chain, the robots gate and the crawl scope. The route is
//! internal: the runtime service token plus the org binding (`x-quarry-run-sig`
//! over the org id and the seed URL, the binding `/v1/internal/run_page` uses).
//! Served only with `QUARRY_EDGE__CRAWL_SITEMAP` on (default off: 501).
//!
//! What it does, in order:
//!
//! 1. The sitemap files: the `Sitemap:` lines of the seed origin's robots.txt, else
//!    `/sitemap.xml`. At most [`MAX_ROOTS`] roots.
//! 2. Every sitemap file URL must pass the checks of any fetch the edge makes for a
//!    caller: the web policy, the static SSRF heuristics, the security preflight and
//!    the public-address DNS guard, and its host must be inside the crawl's hosts
//!    (the seed hosts, with subdomains when the scope allows them). A cross-host
//!    `Sitemap:` line is refused, never fetched.
//! 3. A body is read up to [`MAX_COMPRESSED_BYTES`], inflated up to the parser's own
//!    cap if it is gzip, and parsed. An index is followed to at most [`MAX_DEPTH`]
//!    levels with at most [`MAX_CHILDREN`] children per index, at most [`MAX_FILES`]
//!    files in all, inside [`BUDGET`].
//! 4. Every `<loc>` is canonicalised and de-duplicated, then goes through the crawl's
//!    own scope (host, section, include and exclude patterns, deny extensions) and,
//!    when the robots gate enforces, robots.txt. The scope runs in enforce mode here
//!    whatever the crawl's mode is: a sitemap is attacker-controlled input, and a
//!    URL the scope refuses is never seeded. Each page is checked again by
//!    `run_page` when it runs.
//! 5. At most `max_urls` URLs come back. Nothing is stored.
//!
//! The response carries counts only besides the URLs: `discovered`, files fetched,
//! and what was skipped and why. No new event type: the orchestrator folds the counts
//! into `run_completed`.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use url::Url;

use quarry_core::crawl_scope::{CrawlContext, ScopeMode, MAX_LINKS_EVALUATED};
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_runtime::crawl_filter::{host_matches, CompiledScope, RobotsLookup, RobotsPeek};
use quarry_runtime::driver::FetchHints;
use quarry_transform::canonical_url::{canonicalize, CanonicalOptions};

use crate::state::AppState;

/// Sitemap file URLs taken from robots.txt (or the one fallback).
pub const MAX_ROOTS: usize = 5;
/// Children followed from one sitemap index.
pub const MAX_CHILDREN: usize = 5;
/// Index nesting: roots are level 0, so levels 0, 1 and 2 are read.
pub const MAX_DEPTH: u32 = 2;
/// Sitemap files fetched for one discovery, roots and children together.
pub const MAX_FILES: usize = 20;
/// `<loc>` entries read in all, before scope. A sitemap file holds at most 50 000.
pub const MAX_LOCS: usize = 100_000;
/// The compressed (as sent) size of one sitemap file.
pub const MAX_COMPRESSED_BYTES: usize = 10 * 1024 * 1024;
/// The most URLs one call returns, whatever `max_urls` asks.
pub const MAX_URLS_CEILING: usize = 100_000;
/// The time one discovery may take, between its fetches.
pub const BUDGET: Duration = Duration::from_secs(30);
/// The time one sitemap fetch may take.
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
/// Distinct origins whose robots.txt is read for the filter.
const MAX_ROBOTS_ORIGINS: usize = 20;

// -- wire types --------------------------------------------------------------

/// The body the orchestrator sends.
#[derive(Debug, Deserialize)]
pub struct InternalSitemapRequest {
    /// The crawl's seed URL.
    pub url: String,
    #[serde(default)]
    pub org_id: Option<String>,
    /// The crawl's scope context, as `run_page` takes it (`crawl_scope::CrawlContext`).
    #[serde(default)]
    pub crawl: Option<serde_json::Value>,
    /// The most URLs to return (the crawl's page bound). Absent: 1000.
    #[serde(default)]
    pub max_urls: Option<u32>,
    /// Crawl jobs have no ZDR path; `true` is refused.
    #[serde(default)]
    pub zdr: Option<bool>,
}

/// What was left out, by reason. Counts only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SitemapSkipped {
    /// Refused by the crawl scope (host, section, patterns, extension).
    pub out_of_scope: u32,
    /// Refused by robots.txt (only when the gate enforces).
    pub robots_disallowed: u32,
    /// Listed twice (after canonicalisation and normalisation).
    pub duplicate: u32,
    /// Allowed, but past `max_urls`.
    pub cap: u32,
    /// Not an absolute http or https URL, too long, or with credentials.
    pub invalid: u32,
    /// Sitemap files not fetched: another host, the web policy, the security checks.
    pub sitemaps_refused: u32,
    /// Sitemap files that did not answer 2xx, timed out, or could not be read.
    pub sitemaps_unavailable: u32,
}

/// The discovery result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SitemapResponse {
    /// Normalised URLs the crawl may seed, in sitemap order, at most `max_urls`.
    pub urls: Vec<String>,
    /// `<loc>` entries read.
    pub discovered: u32,
    /// Sitemap files fetched and parsed.
    pub sitemaps_fetched: u32,
    /// Of those, files reached through an index.
    pub nested_fetched: u32,
    pub skipped: SitemapSkipped,
    /// A bound stopped the discovery early (files, entries or time).
    pub truncated: bool,
}

// -- fetching ----------------------------------------------------------------

/// The outcome of one sitemap fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// A 2xx body, within the compressed cap.
    Body(Vec<u8>),
    /// Not fetched: refused by a check (policy, security, host).
    Refused,
    /// Fetched or tried, and unusable: non-2xx, timeout, transport error, too large.
    Unavailable,
}

/// Fetches one sitemap file. The production implementation is [`GuardedFetcher`].
#[async_trait::async_trait]
pub trait SitemapFetch: Send + Sync {
    async fn fetch(&self, url: &Url) -> Fetched;
}

/// Fetches through the edge's guarded driver chain, with every check an edge fetch has.
struct GuardedFetcher<'a> {
    state: &'a AppState,
    decision: crate::policy_baseline::Decision,
    security: std::sync::Arc<dyn quarry_security::SecurityEngine>,
    org_id: String,
    hosts: HostScope,
}

#[async_trait::async_trait]
impl SitemapFetch for GuardedFetcher<'_> {
    async fn fetch(&self, url: &Url) -> Fetched {
        if !self.hosts.allows(url) {
            return Fetched::Refused;
        }
        if matches!(
            quarry_security::heur::check(url).decision,
            quarry_security::Decision::Block
        ) {
            return Fetched::Refused;
        }
        if crate::web_policy_edge::gate_url(&self.decision, url.as_str()).is_err() {
            return Fetched::Refused;
        }
        if self.security.preflight(url).await.decision == quarry_security::Decision::Block {
            return Fetched::Refused;
        }
        let resolved_target = if self.security.allow_private_hosts() {
            None
        } else {
            match quarry_runtime::dns_guard::resolve_public_url(url).await {
                Ok(target) => Some(target),
                Err(_) => return Fetched::Refused,
            }
        };
        let hints = FetchHints {
            org_id: self.org_id.clone(),
            resolved_target,
            max_body_bytes: Some(MAX_COMPRESSED_BYTES),
            truncate_at_cap: false,
            ..FetchHints::default()
        };
        match tokio::time::timeout(
            FETCH_TIMEOUT,
            self.state.driver.fetch_conditional(url, &hints),
        )
        .await
        {
            Ok(Ok(response)) if (200..300).contains(&response.status) => {
                // A redirect can land on another host; the body of a host outside the
                // crawl is not read as this crawl's sitemap.
                if !self.hosts.allows(&response.final_url) {
                    return Fetched::Refused;
                }
                Fetched::Body(response.body)
            }
            _ => Fetched::Unavailable,
        }
    }
}

/// The hosts a sitemap file may come from: the crawl's hosts, never an external one.
#[derive(Debug, Clone)]
pub struct HostScope {
    roots: Vec<String>,
    allow_subdomains: bool,
}

impl HostScope {
    /// The seed hosts of the crawl, else the seed URL's own host.
    pub fn new(ctx: &CrawlContext, seed: &Url) -> Self {
        let mut roots = ctx.seed_hosts.clone();
        if let Some(host) = seed.host_str() {
            if !roots.iter().any(|root| host_matches(root, host, false)) {
                roots.push(host.to_owned());
            }
        }
        Self {
            roots,
            allow_subdomains: ctx.scope.allow_subdomains,
        }
    }

    /// Whether `url` is an http(s) URL on one of the crawl's hosts.
    pub fn allows(&self, url: &Url) -> bool {
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        let Some(host) = url.host_str() else {
            return false;
        };
        self.roots
            .iter()
            .any(|root| host_matches(root, host, self.allow_subdomains))
    }
}

// -- phase 1: read the sitemap files -----------------------------------------

/// The bounds of one discovery. [`Limits::default`] is the route's.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_files: usize,
    pub max_children: usize,
    pub max_depth: u32,
    pub max_locs: usize,
    pub budget: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files: MAX_FILES,
            max_children: MAX_CHILDREN,
            max_depth: MAX_DEPTH,
            max_locs: MAX_LOCS,
            budget: BUDGET,
        }
    }
}

/// What reading the files produced.
#[derive(Debug, Default)]
pub struct Collected {
    pub locs: Vec<String>,
    pub files_fetched: u32,
    pub nested_fetched: u32,
    pub refused: u32,
    pub unavailable: u32,
    pub truncated: bool,
}

/// Read the sitemap files breadth first from `roots`. A file that cannot be read is
/// counted and skipped; the discovery goes on with the others.
pub async fn collect(fetcher: &dyn SitemapFetch, roots: Vec<Url>, limits: &Limits) -> Collected {
    let started = Instant::now();
    let mut out = Collected::default();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(Url, u32)> = roots.into_iter().map(|url| (url, 0)).collect();
    while let Some((url, depth)) = queue.pop_front() {
        if !seen.insert(url.as_str().to_owned()) {
            continue;
        }
        if out.files_fetched as usize >= limits.max_files || started.elapsed() >= limits.budget {
            out.truncated = true;
            break;
        }
        let body = match fetcher.fetch(&url).await {
            Fetched::Body(body) => body,
            Fetched::Refused => {
                out.refused += 1;
                continue;
            }
            Fetched::Unavailable => {
                out.unavailable += 1;
                continue;
            }
        };
        let sitemap = match quarry_transform::sitemap::parse_body(&body) {
            Ok(sitemap) => sitemap,
            Err(_) => {
                out.unavailable += 1;
                continue;
            }
        };
        out.files_fetched += 1;
        if depth > 0 {
            out.nested_fetched += 1;
        }
        if depth < limits.max_depth {
            for child in sitemap.nested.iter().take(limits.max_children) {
                let child = xml_unescape(child);
                if let Ok(next) = Url::parse(&child).or_else(|_| url.join(&child)) {
                    queue.push_back((next, depth + 1));
                }
            }
        }
        for entry in sitemap.urls {
            if out.locs.len() >= limits.max_locs {
                out.truncated = true;
                break;
            }
            out.locs.push(xml_unescape(&entry.loc));
        }
    }
    out
}

/// The text of an XML node: a CDATA wrapper removed, the five predefined entities
/// decoded (`&amp;` last, so `&amp;lt;` is `&lt;` and not `<`).
pub fn xml_unescape(text: &str) -> String {
    let trimmed = text.trim();
    // CDATA is literal: no entity inside it is decoded.
    if let Some(cdata) = trimmed
        .strip_prefix("<![CDATA[")
        .and_then(|rest| rest.strip_suffix("]]>"))
    {
        return cdata.trim().to_owned();
    }
    trimmed
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

// -- phase 2: the scope ------------------------------------------------------

/// The URLs the crawl may seed, and why the others were left out.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub urls: Vec<String>,
    pub skipped: SitemapSkipped,
}

/// Canonicalise, de-duplicate and scope `locs`, keeping at most `max_urls`. The page
/// the scope evaluates them against is the seed, so a relative section rule
/// (`crawl_entire_domain: false`) is the seed's directory.
pub fn select(
    scope: &CompiledScope,
    seed: &Url,
    locs: &[String],
    lookup: Option<RobotsLookup<'_>>,
    max_urls: usize,
) -> Selection {
    let mut skipped = SitemapSkipped::default();
    let mut seen: HashSet<String> = HashSet::new();
    let mut hrefs: Vec<String> = Vec::with_capacity(locs.len());
    for loc in locs {
        match canonicalize(loc.trim(), CanonicalOptions::default()) {
            Err(_) => skipped.invalid += 1,
            Ok(canonical) => {
                if seen.insert(canonical.clone()) {
                    hrefs.push(canonical);
                } else {
                    skipped.duplicate += 1;
                }
            }
        }
    }
    let mut urls: Vec<String> = Vec::new();
    for chunk in hrefs.chunks(MAX_LINKS_EVALUATED) {
        let outcome = scope.evaluate(seed, chunk, lookup);
        // WS14.S4, D2-091: the robots refusals of this chunk are counted only when a lookup is
        // given, and the handler gives one only when the gate enforces (`robots_enforced`).
        if lookup.is_some() {
            crate::metrics::record_robots_link_refusals(&outcome.report);
        }
        for (code, count) in &outcome.report.denials.counts {
            match code.as_str() {
                "robots_disallowed" | "robots_unreachable" => skipped.robots_disallowed += count,
                "duplicate" => skipped.duplicate += count,
                _ => skipped.out_of_scope += count,
            }
        }
        for link in outcome.links {
            if urls.len() < max_urls {
                urls.push(link);
            } else {
                skipped.cap += 1;
            }
        }
    }
    Selection { urls, skipped }
}

/// The scope a sitemap is selected with: the crawl's own, forced to enforce, depth 0
/// and no depth bound (sitemap URLs are seeded one level below the seed by the
/// orchestrator, which owns the depth).
pub fn selection_context(mut ctx: CrawlContext) -> CrawlContext {
    ctx.scope_mode = ScopeMode::Enforce;
    ctx.kind = quarry_core::crawl_scope::CrawlKind::Crawl;
    ctx.depth = 0;
    ctx.max_depth = 0;
    ctx
}

// -- the route ---------------------------------------------------------------

type RouteError = (StatusCode, Json<Envelope<()>>);

fn refuse(request_id: &str, error: QuarryError) -> RouteError {
    (
        StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(Envelope::<()>::err(request_id, error)),
    )
}

/// `POST /v1/internal/crawl/sitemap`. See the module documentation.
pub async fn internal_sitemap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<InternalSitemapRequest>,
) -> Result<Json<SitemapResponse>, RouteError> {
    let request_id = crate::request_id::current();
    crate::routes::verify_runtime_token(&headers).map_err(|e| refuse(&request_id, e))?;
    if !crate::crawl_policy::CrawlFeatures::current().sitemap {
        return Err(refuse(
            &request_id,
            QuarryError::new(
                ErrorCode::Unsupported,
                "sitemap discovery is not enabled on this edge (QUARRY_EDGE__CRAWL_SITEMAP)",
            ),
        ));
    }
    let org_id = req.org_id.clone().unwrap_or_default();
    if org_id.trim().is_empty() {
        return Err(refuse(
            &request_id,
            QuarryError::new(
                ErrorCode::BadRequest,
                "sitemap discovery requires org_id in the body",
            ),
        ));
    }
    // The same org binding run_page has: the bearer authenticates the caller, the
    // signature binds the tenant, so a leaked bearer cannot be replayed for another org.
    // Fail closed when the edge has no secret to verify with.
    match state.internal_signer.as_ref() {
        Some(signer) => {
            let sig = headers
                .get("x-quarry-run-sig")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("");
            if !signer.verify_run_binding(&org_id, &req.url, sig) {
                tracing::warn!(
                    had_sig = !sig.is_empty(),
                    "sitemap discovery HMAC org-binding failed"
                );
                return Err(refuse(
                    &request_id,
                    QuarryError::new(
                        ErrorCode::Unauthorized,
                        "sitemap discovery org binding invalid",
                    ),
                ));
            }
        }
        None => {
            return Err(refuse(
                &request_id,
                QuarryError::new(
                    ErrorCode::Unsupported,
                    "sitemap discovery org binding not configured (set QUARRY_EDGE__INTERNAL_SECRET)",
                ),
            ));
        }
    }
    if req.zdr == Some(true) {
        return Err(refuse(
            &request_id,
            QuarryError::new(
                ErrorCode::BadRequest,
                "sitemap discovery has no zero data retention path; crawl jobs are refused under ZDR",
            ),
        ));
    }
    let seed: Url = req
        .url
        .parse()
        .ok()
        .filter(|url: &Url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        .ok_or_else(|| {
            refuse(
                &request_id,
                QuarryError::new(ErrorCode::BadRequest, "url must be an absolute http(s) URL"),
            )
        })?;

    // The crawl's scope, required: without it there is nothing to scope the URLs with.
    let ctx = match req.crawl.clone().filter(|value| !value.is_null()) {
        Some(raw) => CrawlContext::from_json(raw).map_err(|errors| {
            invalid_scope(
                &request_id,
                errors.iter().map(|e| (e.field.clone(), e.message.clone())),
            )
        })?,
        None => {
            return Err(refuse(
                &request_id,
                QuarryError::new(
                    ErrorCode::BadRequest,
                    "sitemap discovery requires the crawl scope (crawl)",
                ),
            ));
        }
    };
    let ctx = selection_context(ctx);
    let scope = CompiledScope::compile(&ctx).map_err(|errors| {
        invalid_scope(
            &request_id,
            errors.iter().map(|e| (e.field.clone(), e.message.clone())),
        )
    })?;
    let max_urls = (req.max_urls.unwrap_or(1000) as usize).clamp(1, MAX_URLS_CEILING);

    // The same decision and gate as run_page: the autonomous principal, the baseline.
    let decision = crate::web_policy_edge::decide_for_principal(
        &state.policy,
        quarry_core::web_policy::Principal::Autonomous,
        false,
        None,
    )
    .map_err(|error| crate::web_policy_edge::envelope_error(&request_id, error))?;
    crate::web_policy_edge::gate_url(&decision, &req.url)
        .map_err(|error| crate::web_policy_edge::envelope_error(&request_id, error))?;

    let hosts = HostScope::new(&ctx, &seed);
    let fetcher = GuardedFetcher {
        state: &state,
        security: crate::web_policy_edge::security_engine(
            &state.security,
            &state.policy,
            &decision,
        ),
        decision,
        org_id: org_id.clone(),
        hosts: hosts.clone(),
    };

    // Step 1: the sitemap files. robots.txt names them; without a robots gate the file is
    // read through the same guarded fetcher.
    let roots = sitemap_roots(&state, &fetcher, &seed, &org_id, &hosts).await;
    let mut response = SitemapResponse::default();
    let collected = collect(&fetcher, roots.urls, &Limits::default()).await;
    response.skipped.sitemaps_refused = collected.refused + roots.refused;
    response.skipped.sitemaps_unavailable = collected.unavailable;
    response.sitemaps_fetched = collected.files_fetched;
    response.nested_fetched = collected.nested_fetched;
    response.discovered = u32::try_from(collected.locs.len()).unwrap_or(u32::MAX);
    response.truncated = collected.truncated;

    // Step 4: robots for the URLs, only when the gate enforces for this crawl. The rules of
    // each origin are read first (one fetch each, then cached), because the scope's own
    // robots lookup answers from the cache and never from the network.
    let robots_enforced = state.robots.as_ref().filter(|gate| {
        scope.respects_robots() && gate.mode() == quarry_runtime::RobotsEnforceMode::Enforce
    });
    if let Some(gate) = robots_enforced {
        let mut origins: Vec<Url> = Vec::new();
        for loc in &collected.locs {
            if let Ok(url) = Url::parse(loc.trim()) {
                if hosts.allows(&url)
                    && !origins.iter().any(|o| o.origin() == url.origin())
                    && origins.len() < MAX_ROBOTS_ORIGINS
                {
                    origins.push(url);
                }
            }
        }
        for origin in &origins {
            let _ = gate.rules_for_origin(origin, &org_id, false).await;
        }
    }
    let peek = |link: &Url| robots_enforced.map_or(RobotsPeek::Unknown, |gate| gate.peek(link));
    let lookup: Option<RobotsLookup<'_>> = robots_enforced.map(|_| &peek as RobotsLookup<'_>);
    let selection = select(&scope, &seed, &collected.locs, lookup, max_urls);
    response.urls = selection.urls;
    response.skipped.out_of_scope = selection.skipped.out_of_scope;
    response.skipped.robots_disallowed = selection.skipped.robots_disallowed;
    response.skipped.duplicate = selection.skipped.duplicate;
    response.skipped.cap = selection.skipped.cap;
    response.skipped.invalid = selection.skipped.invalid;

    tracing::info!(
        urls = response.urls.len(),
        discovered = response.discovered,
        files = response.sitemaps_fetched,
        truncated = response.truncated,
        "crawl sitemap discovery"
    );
    Ok(Json(response))
}

fn invalid_scope(request_id: &str, errors: impl Iterator<Item = (String, String)>) -> RouteError {
    let fields: Vec<serde_json::Value> = errors
        .map(|(path, message)| serde_json::json!({ "path": path, "code": "invalid", "message": message }))
        .collect();
    refuse(
        request_id,
        QuarryError::new(ErrorCode::BadRequest, "the crawl scope is invalid")
            .with_details(serde_json::json!({ "fields": fields })),
    )
}

struct Roots {
    urls: Vec<Url>,
    refused: u32,
}

/// The sitemap files of the seed's origin: the `Sitemap:` lines of its robots.txt that
/// are on the crawl's hosts, else `/sitemap.xml`.
async fn sitemap_roots(
    state: &AppState,
    fetcher: &dyn SitemapFetch,
    seed: &Url,
    org_id: &str,
    hosts: &HostScope,
) -> Roots {
    let mut lines: Vec<String> = Vec::new();
    let mut from_gate = false;
    if let Some(gate) = state.robots.as_ref() {
        if let Some(state) = gate.rules_for_origin(seed, org_id, false).await {
            from_gate = true;
            if let quarry_runtime::RobotsState::Rules(rules) = state {
                lines = rules.sitemaps.clone();
            }
        }
    }
    if !from_gate {
        if let Ok(robots_url) = seed.join("/robots.txt") {
            if let Fetched::Body(body) = fetcher.fetch(&robots_url).await {
                lines = quarry_transform::sitemap::sitemap_urls_from_robots(
                    &String::from_utf8_lossy(&body),
                );
            }
        }
    }
    let mut roots = Roots {
        urls: Vec::new(),
        refused: 0,
    };
    for line in lines {
        let Ok(url) = Url::parse(line.trim()) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") {
            continue;
        }
        if !hosts.allows(&url) {
            // A `Sitemap:` line may name any URL; one outside the crawl's hosts is never fetched.
            roots.refused += 1;
            continue;
        }
        if !roots.urls.iter().any(|known| known == &url) && roots.urls.len() < MAX_ROOTS {
            roots.urls.push(url);
        }
    }
    if roots.urls.is_empty() {
        if let Ok(fallback) = seed.join("/sitemap.xml") {
            roots.urls.push(fallback);
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A sitemap source from memory; records the URLs asked for.
    #[derive(Default)]
    struct Fake {
        files: HashMap<String, Fetched>,
        asked: Mutex<Vec<String>>,
    }

    impl Fake {
        fn with(mut self, url: &str, body: &str) -> Self {
            self.files
                .insert(url.to_owned(), Fetched::Body(body.as_bytes().to_vec()));
            self
        }
        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl SitemapFetch for Fake {
        async fn fetch(&self, url: &Url) -> Fetched {
            self.asked.lock().unwrap().push(url.to_string());
            self.files
                .get(url.as_str())
                .cloned()
                .unwrap_or(Fetched::Unavailable)
        }
    }

    fn urlset(locs: &[&str]) -> String {
        let entries: String = locs
            .iter()
            .map(|l| format!("<url><loc>{l}</loc></url>"))
            .collect();
        format!("<?xml version=\"1.0\"?><urlset>{entries}</urlset>")
    }

    fn index(locs: &[&str]) -> String {
        let entries: String = locs
            .iter()
            .map(|l| format!("<sitemap><loc>{l}</loc></sitemap>"))
            .collect();
        format!("<?xml version=\"1.0\"?><sitemapindex>{entries}</sitemapindex>")
    }

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn scope_for(ctx: CrawlContext) -> CompiledScope {
        CompiledScope::compile(&selection_context(ctx)).expect("scope compiles")
    }

    #[test]
    fn xml_entities_and_cdata_are_decoded() {
        assert_eq!(
            xml_unescape(" https://e.test/a?x=1&amp;y=2 "),
            "https://e.test/a?x=1&y=2"
        );
        assert_eq!(
            xml_unescape("<![CDATA[https://e.test/a?x=1&y=2]]>"),
            "https://e.test/a?x=1&y=2"
        );
        assert_eq!(
            xml_unescape("a&amp;lt;b"),
            "a&lt;b",
            "&amp; is decoded last"
        );
        assert_eq!(xml_unescape("&quot;q&apos;"), "\"q'");
    }

    #[tokio::test]
    async fn an_index_is_followed_to_the_depth_and_child_bounds() {
        let fake = Fake::default()
            .with(
                "https://e.test/idx.xml",
                &index(&["https://e.test/c1.xml", "https://e.test/c2.xml"]),
            )
            .with("https://e.test/c1.xml", &index(&["https://e.test/g1.xml"]))
            .with("https://e.test/c2.xml", &urlset(&["https://e.test/a"]))
            .with("https://e.test/g1.xml", &urlset(&["https://e.test/b"]));
        let out = collect(
            &fake,
            vec![url("https://e.test/idx.xml")],
            &Limits::default(),
        )
        .await;
        assert_eq!(
            out.locs,
            vec!["https://e.test/a".to_owned(), "https://e.test/b".to_owned()]
        );
        assert_eq!(out.files_fetched, 4);
        assert_eq!(out.nested_fetched, 3);
        assert!(!out.truncated);

        // Make g1 an index: its child is at level 3 and is not followed.
        let deeper = Fake::default()
            .with("https://e.test/idx.xml", &index(&["https://e.test/c1.xml"]))
            .with("https://e.test/c1.xml", &index(&["https://e.test/g1.xml"]))
            .with("https://e.test/g1.xml", &index(&["https://e.test/g2.xml"]))
            .with("https://e.test/g2.xml", &urlset(&["https://e.test/never"]));
        let out = collect(
            &deeper,
            vec![url("https://e.test/idx.xml")],
            &Limits::default(),
        )
        .await;
        assert!(out.locs.is_empty());
        assert!(
            !deeper.asked().contains(&"https://e.test/g2.xml".to_owned()),
            "{:?}",
            deeper.asked()
        );
    }

    #[tokio::test]
    async fn an_index_follows_at_most_five_children() {
        let children: Vec<String> = (0..8).map(|i| format!("https://e.test/c{i}.xml")).collect();
        let refs: Vec<&str> = children.iter().map(String::as_str).collect();
        let mut fake = Fake::default().with("https://e.test/idx.xml", &index(&refs));
        for (i, child) in children.iter().enumerate() {
            fake = fake.with(child, &urlset(&[&format!("https://e.test/p{i}")]));
        }
        let out = collect(
            &fake,
            vec![url("https://e.test/idx.xml")],
            &Limits::default(),
        )
        .await;
        assert_eq!(out.locs.len(), 5);
        assert_eq!(out.files_fetched, 6);
    }

    #[tokio::test]
    async fn the_file_bound_and_the_entry_bound_truncate() {
        let fake = Fake::default()
            .with(
                "https://e.test/idx.xml",
                &index(&["https://e.test/a.xml", "https://e.test/b.xml"]),
            )
            .with(
                "https://e.test/a.xml",
                &urlset(&["https://e.test/1", "https://e.test/2", "https://e.test/3"]),
            )
            .with("https://e.test/b.xml", &urlset(&["https://e.test/4"]));
        let limits = Limits {
            max_files: 2,
            ..Limits::default()
        };
        let out = collect(&fake, vec![url("https://e.test/idx.xml")], &limits).await;
        assert_eq!(out.files_fetched, 2);
        assert!(out.truncated);

        let limits = Limits {
            max_locs: 2,
            ..Limits::default()
        };
        let out = collect(&fake, vec![url("https://e.test/idx.xml")], &limits).await;
        assert_eq!(out.locs.len(), 2);
        assert!(out.truncated);
    }

    #[tokio::test]
    async fn a_repeated_sitemap_is_read_once_and_a_bad_body_is_counted() {
        let fake = Fake::default()
            .with(
                "https://e.test/idx.xml",
                &index(&["https://e.test/idx.xml", "https://e.test/a.xml"]),
            )
            .with("https://e.test/a.xml", &urlset(&["https://e.test/1"]));
        let out = collect(
            &fake,
            vec![url("https://e.test/idx.xml")],
            &Limits::default(),
        )
        .await;
        assert_eq!(out.files_fetched, 2);
        assert_eq!(out.locs, vec!["https://e.test/1".to_owned()]);

        let mut gone = Fake::default();
        gone.files
            .insert("https://e.test/idx.xml".to_owned(), Fetched::Refused);
        gone.files.insert(
            "https://e.test/b.xml".to_owned(),
            Fetched::Body(vec![0x1f, 0x8b, 0, 0]),
        );
        let out = collect(
            &gone,
            vec![
                url("https://e.test/idx.xml"),
                url("https://e.test/b.xml"),
                url("https://e.test/c.xml"),
            ],
            &Limits::default(),
        )
        .await;
        assert_eq!((out.refused, out.unavailable, out.files_fetched), (1, 2, 0));
    }

    #[test]
    fn selection_scopes_dedups_canonicalises_and_caps() {
        let scope = scope_for(CrawlContext::default());
        let seed = url("https://e.test/");
        let locs: Vec<String> = [
            "https://e.test/docs/a",
            "https://E.TEST/docs/a#frag",
            "https://e.test:443/docs/a",
            "https://e.test/docs/b",
            "https://other.test/x",
            "https://e.test/logo.png",
            "not a url",
            "ftp://e.test/f",
            "https://e.test/docs/c",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let all = select(&scope, &seed, &locs, None, 100);
        assert_eq!(
            all.urls,
            vec![
                "https://e.test/docs/a",
                "https://e.test/docs/b",
                "https://e.test/docs/c"
            ]
        );
        assert_eq!(all.skipped.duplicate, 2);
        assert_eq!(all.skipped.invalid, 2, "the text and the ftp URL");
        assert_eq!(
            all.skipped.out_of_scope, 2,
            "another host and a denied extension"
        );
        assert_eq!(all.skipped.cap, 0);

        let capped = select(&scope, &seed, &locs, None, 2);
        assert_eq!(capped.urls.len(), 2);
        assert_eq!(capped.skipped.cap, 1);
    }

    #[test]
    fn selection_applies_include_and_exclude_patterns() {
        let mut ctx = CrawlContext::default();
        ctx.scope.include_paths = vec!["/docs/*".to_owned()];
        ctx.scope.exclude_paths = vec!["/docs/draft*".to_owned()];
        let scope = scope_for(ctx);
        let locs: Vec<String> = [
            "https://e.test/docs/a",
            "https://e.test/docs/draft-1",
            "https://e.test/blog/x",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let out = select(&scope, &url("https://e.test/"), &locs, None, 10);
        assert_eq!(out.urls, vec!["https://e.test/docs/a"]);
        assert_eq!(out.skipped.out_of_scope, 2);
    }

    #[test]
    fn selection_enforces_the_scope_even_when_the_crawl_only_observes() {
        let ctx = CrawlContext {
            scope_mode: ScopeMode::Observe,
            ..CrawlContext::default()
        };
        let scope = scope_for(ctx);
        let locs = vec![
            "https://e.test/a".to_owned(),
            "https://elsewhere.test/b".to_owned(),
        ];
        let out = select(&scope, &url("https://e.test/"), &locs, None, 10);
        assert_eq!(out.urls, vec!["https://e.test/a"]);
    }

    #[test]
    fn selection_drops_what_robots_disallows_when_a_lookup_is_given() {
        let scope = scope_for(CrawlContext::default());
        let locs = vec![
            "https://e.test/ok".to_owned(),
            "https://e.test/private/x".to_owned(),
        ];
        let deny_private = |link: &Url| {
            if link.path().starts_with("/private/") {
                RobotsPeek::Disallowed {
                    user_agent: "quarrybot".into(),
                    path: link.path().into(),
                }
            } else {
                RobotsPeek::Allowed
            }
        };
        let out = select(
            &scope,
            &url("https://e.test/"),
            &locs,
            Some(&deny_private),
            10,
        );
        assert_eq!(out.urls, vec!["https://e.test/ok"]);
        assert_eq!(out.skipped.robots_disallowed, 1);
        let none = select(&scope, &url("https://e.test/"), &locs, None, 10);
        assert_eq!(none.urls.len(), 2, "no lookup: robots is not consulted");
    }

    #[test]
    fn a_section_scope_keeps_only_the_seeds_directory() {
        let mut ctx = CrawlContext::default();
        ctx.scope.crawl_entire_domain = false;
        ctx.seed_url = Some("https://e.test/docs/".to_owned());
        let scope = scope_for(ctx);
        let locs = vec![
            "https://e.test/docs/a".to_owned(),
            "https://e.test/blog/b".to_owned(),
        ];
        let out = select(&scope, &url("https://e.test/docs/"), &locs, None, 10);
        assert_eq!(out.urls, vec!["https://e.test/docs/a"]);
    }

    #[test]
    fn robots_refusals_are_counted_only_when_a_lookup_is_given() {
        let scope = scope_for(CrawlContext::default());
        let seed = url("https://e.test/");
        let locs = vec![
            "https://e.test/ok".to_owned(),
            "https://e.test/private/x".to_owned(),
        ];
        let deny_private = |link: &Url| {
            if link.path().starts_with("/private/") {
                RobotsPeek::Disallowed {
                    user_agent: "quarrybot".into(),
                    path: link.path().into(),
                }
            } else {
                RobotsPeek::Allowed
            }
        };
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        // No lookup: the crawl does not consult robots, so nothing is refused or counted.
        let unchecked =
            metrics::with_local_recorder(&recorder, || select(&scope, &seed, &locs, None, 10));
        assert_eq!(unchecked.urls.len(), 2);
        assert!(
            !handle
                .render()
                .contains("quarry_robots_link_refusals_total"),
            "{}",
            handle.render()
        );
        // A lookup means the gate enforces: the refused link is counted once.
        let enforced = metrics::with_local_recorder(&recorder, || {
            select(&scope, &seed, &locs, Some(&deny_private), 10)
        });
        assert_eq!(enforced.urls, vec!["https://e.test/ok"]);
        assert!(
            handle
                .render()
                .contains(r#"quarry_robots_link_refusals_total{outcome="denied"} 1"#),
            "{}",
            handle.render()
        );
    }

    #[test]
    fn sitemap_hosts_are_the_crawl_hosts_only() {
        let mut ctx = CrawlContext::default();
        ctx.seed_hosts = vec!["e.test".to_owned()];
        let hosts = HostScope::new(&ctx, &url("https://e.test/"));
        assert!(hosts.allows(&url("https://e.test/sitemap.xml")));
        assert!(
            hosts.allows(&url("https://www.e.test/sitemap.xml")),
            "www is the same host"
        );
        assert!(
            !hosts.allows(&url("https://cdn.e.test/sitemap.xml")),
            "a subdomain needs allow_subdomains"
        );
        assert!(!hosts.allows(&url("https://evil.test/sitemap.xml")));
        assert!(!hosts.allows(&url("http://169.254.169.254/latest/meta-data")));
        assert!(!hosts.allows(&url("ftp://e.test/sitemap.xml")));
        ctx.scope.allow_subdomains = true;
        assert!(HostScope::new(&ctx, &url("https://e.test/"))
            .allows(&url("https://cdn.e.test/sitemap.xml")));
        // External links being allowed does not widen where a sitemap may come from.
        ctx.scope.allow_external_links = true;
        assert!(!HostScope::new(&ctx, &url("https://e.test/"))
            .allows(&url("https://evil.test/sitemap.xml")));
    }
}
