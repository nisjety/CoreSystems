//! `/v1/map` — bounded public URL discovery (cluster #/v1/map, OSS-parity P0 1B).
//!
//! Recombines existing primitives — `transform::sitemap`, `transform::robots`,
//! `transform::links` — plus the runtime `LexicalRanker` for a partial site map,
//! optionally ranked by relevance to a `search` term. It is not a site audit.
//!
//! Read-only: no durable writes, so ZDR=on is always fine. Org-scoped only for
//! usage metering + event emission; the discovery itself touches no tenant data.

use std::collections::BTreeSet;
use std::time::Duration;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};
use url::Url;

use quarry_runtime::crawl_ranker::{CrawlRanker, LexicalRanker};

use crate::api_error::ApiError;
use crate::state::AppState;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1000;
const MAP_UA: &str = quarry_runtime::fetch::DISCOVERY_USER_AGENT;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_CANDIDATES: usize = 2000;
const MAX_NESTED: usize = 20;
const MAX_SITEMAP_DEPTH: usize = 4;
const MAX_REDIRECTS: usize = 3;
const MAP_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Debug, Deserialize)]
pub struct MapRequest {
    /// Site URL to map (any page on the origin works; the origin is derived).
    pub url: String,
    /// Optional relevance term; when set, results are ranked by it.
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    /// Include subdomains of the seed host in scope (default: same host only).
    #[serde(default)]
    pub include_subdomains: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MapLink {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct MapResponse {
    pub url: String,
    pub links: Vec<MapLink>,
    pub count: usize,
    /// True when results were relevance-ranked against `search`.
    pub ranked: bool,
    pub coverage: MapCoverage,
}

#[derive(Debug, Default, Serialize)]
pub struct MapCoverage {
    pub sitemap_files: usize,
    pub sitemap_valid: usize,
    pub sitemap_partial: bool,
    pub candidates: usize,
    pub robots_filtered: usize,
    pub truncated: bool,
}

// OSS-parity 3D — shared structured error envelope (extends the old
// `{error, code, hint}` shape with optional rate-limit fields). All map
// error paths serialize through this type so clients parse one shape.
type ErrorBody = ApiError;

fn bad_request(message: impl Into<String>) -> ErrorBody {
    ApiError::new("BAD_REQUEST", message)
}

fn forbidden(message: impl Into<String>) -> ErrorBody {
    ApiError::new("FORBIDDEN", message)
}

fn upstream_empty(message: impl Into<String>, hint: impl Into<String>) -> ErrorBody {
    ApiError::new("UPSTREAM_EMPTY", message).with_hint(hint)
}

// ── pure helpers (unit-tested; no network) ──────────────────────────────────

/// True when `host` is in scope relative to `seed_host`. Same host always
/// passes; subdomains pass only when `include_subdomains` is set.
pub(crate) fn host_in_scope(seed_host: &str, host: &str, include_subdomains: bool) -> bool {
    if host == seed_host {
        return true;
    }
    include_subdomains && host.ends_with(&format!(".{seed_host}"))
}

/// Merge sitemap + page-link URLs, drop anything out of host scope, dedup
/// (stable by first-seen via sorted set), and return the candidate URL list.
pub(crate) fn merge_and_scope(
    seed_host: &str,
    include_subdomains: bool,
    candidates: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for raw in candidates.into_iter().take(MAX_CANDIDATES) {
        let Ok(u) = Url::parse(&raw) else { continue };
        if !public_discovery_url(&u) {
            continue;
        }
        let Some(host) = u.host_str() else { continue };
        if !host_in_scope(seed_host, host, include_subdomains) {
            continue;
        }
        let norm = u.as_str().trim_end_matches('#').to_string();
        if seen.insert(norm.clone()) {
            out.push(norm);
        }
    }
    out
}

/// Discovery is intentionally limited to public HTTPS documents. Encoded paths
/// are excluded rather than guessing how an upstream router decodes them.
pub(crate) fn public_discovery_url(url: &Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().contains('%')
    {
        return false;
    }
    !url.path().split('/').any(|part| {
        matches!(
            part.to_ascii_lowercase().as_str(),
            "admin"
                | "administrator"
                | "wp-admin"
                | "wp-login.php"
                | "login"
                | "signin"
                | "sign-in"
                | "logout"
                | "account"
                | "dashboard"
                | "auth"
                | "oauth"
                | "callback"
                | "api"
                | "checkout"
                | "payment"
                | "reset-password"
        )
    })
}

fn nested_in_scope(base: &Url, candidate: &Url) -> bool {
    public_discovery_url(candidate) && candidate.origin() == base.origin()
}

pub(crate) fn map_failure(error: quarry_core::error::QuarryError) -> axum::response::Response {
    use quarry_core::error::ErrorCode;
    if error.code == ErrorCode::RateLimited {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ApiError::rate_limited(
                "discovery transport is rate limited",
                60,
            )),
        )
            .into_response();
    }
    let (status, code, message) = match error.code {
        ErrorCode::Timeout => (
            StatusCode::GATEWAY_TIMEOUT,
            "TIMEOUT",
            "discovery exceeded its time budget",
        ),
        ErrorCode::BadRequest => (
            StatusCode::BAD_GATEWAY,
            "UPSTREAM_LIMIT",
            "discovery source exceeds the bounded response limit",
        ),
        ErrorCode::Unsupported => (
            StatusCode::SERVICE_UNAVAILABLE,
            "UNSUPPORTED",
            "bounded discovery transport is unavailable",
        ),
        _ => (
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "discovery source blocked by transport policy",
        ),
    };
    (status, Json(ApiError::new(code, message))).into_response()
}

fn clamp_limit(limit: Option<u32>) -> usize {
    limit
        .map(|l| l as usize)
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT)
}

struct DiscoveryText {
    body: String,
    url: Url,
}

fn redirect_in_scope(seed: &Url, target: &Url) -> bool {
    if !public_discovery_url(target)
        || seed.port_or_known_default() != target.port_or_known_default()
    {
        return false;
    }
    let (Some(seed_host), Some(target_host)) = (seed.host_str(), target.host_str()) else {
        return false;
    };
    let root = seed_host.strip_prefix("www.").unwrap_or(seed_host);
    target_host == root || target_host == format!("www.{root}")
}

fn redirect_blocked() -> quarry_core::error::QuarryError {
    quarry_core::error::QuarryError::new(
        quarry_core::error::ErrorCode::SecurityBlocked,
        "discovery redirect requires security approval",
    )
}

pub(crate) fn check_robots(
    response: &quarry_runtime::fetch::FetchResponse,
    path: &str,
) -> Result<(), quarry_core::error::QuarryError> {
    if response.status == 429 {
        return Err(quarry_core::error::QuarryError::new(
            quarry_core::error::ErrorCode::RateLimited,
            "robots policy is rate limited",
        ));
    }
    if response.status >= 500
        || matches!(response.status, 401 | 403)
        || ((200..300).contains(&response.status)
            && !quarry_transform::robots::RobotsTxt::parse(&String::from_utf8_lossy(
                &response.body,
            ))
            .is_allowed(MAP_UA, path))
    {
        return Err(redirect_blocked());
    }
    Ok(())
}

async fn fetch_text(
    state: &AppState,
    url: &Url,
) -> Result<Option<DiscoveryText>, quarry_core::error::QuarryError> {
    Ok(fetch_document(state, url)
        .await?
        .filter(|response| (200..300).contains(&response.status))
        .map(|response| DiscoveryText {
            body: String::from_utf8_lossy(&response.body).into_owned(),
            url: response.final_url,
        }))
}

/// Credentialless, pinned, bounded public document transport shared by map
/// and audit capture. No artifacts, browser fallback, cookies or ingestion.
pub(crate) async fn fetch_document(
    state: &AppState,
    url: &Url,
) -> Result<Option<quarry_runtime::fetch::FetchResponse>, quarry_core::error::QuarryError> {
    fetch_document_traced(state, url, &mut Vec::new()).await
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RedirectHop { pub url: String, pub status: u16, pub target: String }

pub(crate) async fn fetch_document_traced(
    state: &AppState,
    url: &Url,
    redirects: &mut Vec<RedirectHop>,
) -> Result<Option<quarry_runtime::fetch::FetchResponse>, quarry_core::error::QuarryError> {
    let mut current = url.clone();
    let mut visited = BTreeSet::new();
    for hop in 0..=MAX_REDIRECTS {
        if !visited.insert(current.as_str().to_owned())
            || quarry_security::heur::check(&current).decision == quarry_security::Decision::Block
            || state.security.preflight(&current).await.decision != quarry_security::Decision::Allow
        {
            return Err(quarry_core::error::QuarryError::new(
                quarry_core::error::ErrorCode::SecurityBlocked,
                "discovery source requires security approval",
            ));
        }
        match state
            .driver
            .fetch_discovery_hop(&current, MAX_BODY_BYTES)
            .await
        {
            Ok(resp) if matches!(resp.status, 301 | 302 | 303 | 307 | 308) => {
                if resp.final_url != current || hop == MAX_REDIRECTS {
                    return Err(redirect_blocked());
                }
                let locations = resp
                    .headers
                    .iter()
                    .filter(|(name, _)| name.eq_ignore_ascii_case("location"))
                    .map(|(_, value)| value)
                    .collect::<Vec<_>>();
                let [location] = locations.as_slice() else {
                    return Err(redirect_blocked());
                };
                if location.len() > 4096 {
                    return Err(redirect_blocked());
                }
                let target = current.join(location).map_err(|_| redirect_blocked())?;
                if !redirect_in_scope(url, &target)
                    || (url.path() == "/robots.txt" && target.path() != "/robots.txt")
                {
                    return Err(redirect_blocked());
                }
                // A canonical host does not inherit another origin's robots rules.
                // Robots itself is fetched through this same bounded hop policy.
                if target.path() != "/robots.txt" {
                    let robots_url = target.join("/robots.txt").map_err(|_| redirect_blocked())?;
                    let robots = Box::pin(fetch_document(state, &robots_url))
                        .await?
                        .ok_or_else(redirect_blocked)?;
                    if robots.final_url.origin() != target.origin() {
                        return Err(redirect_blocked());
                    }
                    check_robots(&robots, target.path())?;
                }
                redirects.push(RedirectHop { url: current.to_string(), status: resp.status, target: target.to_string() });
                current = target;
            }
            Ok(resp) => {
                if resp.final_url != current {
                    return Err(redirect_blocked());
                }
                // Defence in depth for other bounded driver implementations.
                if resp.body.len() > MAX_BODY_BYTES {
                    return Err(quarry_core::error::QuarryError::new(
                        quarry_core::error::ErrorCode::BadRequest,
                        "discovery body limit exceeded",
                    ));
                }
                if resp.status == 429 {
                    return Err(quarry_core::error::QuarryError::new(
                        quarry_core::error::ErrorCode::RateLimited,
                        "discovery source is rate limited",
                    ));
                }
                return Ok(Some(resp));
            }
            // Transport-level rate limit from the driver — surface as a typed
            // error so the handler can answer with the structured 429 envelope.
            Err(e)
                if matches!(
                    e.code,
                    quarry_core::error::ErrorCode::RateLimited
                        | quarry_core::error::ErrorCode::Timeout
                        | quarry_core::error::ErrorCode::BadRequest
                        | quarry_core::error::ErrorCode::Unsupported
                        | quarry_core::error::ErrorCode::SecurityBlocked
                        | quarry_core::error::ErrorCode::Forbidden
                ) =>
            {
                return Err(e)
            }
            _ => return Ok(None),
        }
    }
    Err(redirect_blocked())
}

// ── handler ─────────────────────────────────────────────────────────────────

pub async fn map(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<MapRequest>,
) -> impl IntoResponse {
    map_with_budget(state, claims, req, MAP_TIMEOUT).await
}

async fn map_with_budget(
    state: AppState,
    claims: crate::auth::Claims,
    req: MapRequest,
    budget: Duration,
) -> axum::response::Response {
    match tokio::time::timeout(budget, map_inner(state, claims, req)).await {
        Ok(response) => response,
        Err(_) => map_failure(quarry_core::error::QuarryError::new(
            quarry_core::error::ErrorCode::Timeout,
            "discovery deadline",
        )),
    }
}

#[cfg(test)]
#[path = "map_security_tests.rs"]
mod security_tests;

async fn map_inner(
    state: AppState,
    claims: crate::auth::Claims,
    req: MapRequest,
) -> axum::response::Response {
    if req.url.len() > 4096 || req.search.as_ref().is_some_and(|search| search.len() > 512) {
        return (
            StatusCode::BAD_REQUEST,
            Json(bad_request(
                "discovery URL or search exceeds its input limit",
            )),
        )
            .into_response();
    }
    let Ok(mut base) = Url::parse(req.url.trim()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(bad_request("url must be a valid absolute http(s) URL")),
        )
            .into_response();
    };
    let Some(mut seed_host) = base.host_str().map(str::to_owned) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(bad_request("url has no host")),
        )
            .into_response();
    };
    if !public_discovery_url(&base) {
        return (StatusCode::BAD_REQUEST, Json(bad_request("url must be a public HTTPS document without credentials, query, fragment, or sensitive path"))).into_response();
    }

    // SSRF guard: scheme/userinfo/host heuristics before any fetch.
    if matches!(
        quarry_security::heur::check(&base).decision,
        quarry_security::Decision::Block
    ) {
        return (
            StatusCode::FORBIDDEN,
            Json(forbidden("url blocked by security policy")),
        )
            .into_response();
    }

    let mut origin = base.origin().ascii_serialization();

    // Best-effort robots filtering is not a compliance assertion. Missing or
    // failed robots may be ignored, but bounds/security/rate failures are fatal.
    let mut robots = match Url::parse(&format!("{origin}/robots.txt")) {
        Ok(u) => match fetch_text(&state, &u).await {
            Ok(t) => t.map(|document| {
                // Preserve the requested path, adopting only the validated
                // canonical origin. Never rewrite discovered evidence URLs.
                base.set_host(document.url.host_str())
                    .expect("validated redirect host");
                origin = base.origin().ascii_serialization();
                seed_host = base.host_str().unwrap().to_owned();
                quarry_transform::robots::RobotsTxt::parse(&document.body)
            }),
            Err(e) => return map_failure(e),
        },
        Err(_) => None,
    };

    // Traverse same-origin sitemap indexes with cycle, depth, byte, time and
    // document budgets. All requests use the shared credentialless SSRF guard.
    let mut coverage = MapCoverage::default();
    let mut candidates: Vec<String> = Vec::new();
    let mut sitemap_queue = std::collections::VecDeque::new();
    let mut sitemap_seen = BTreeSet::new();
    if let Ok(url) = base.join("/sitemap.xml") { sitemap_queue.push_back((url, 0)); }
    if let Some(rt) = &robots {
        for url in rt.sitemaps.iter().filter_map(|v| Url::parse(v).ok()).filter(|u| nested_in_scope(&base, u)).take(MAX_NESTED) { sitemap_queue.push_back((url, 0)); }
    }
    let sitemap_deadline = tokio::time::Instant::now() + Duration::from_secs(18);
    while let Some((url, depth)) = sitemap_queue.pop_front() {
        if !nested_in_scope(&base, &url) || !sitemap_seen.insert(url.to_string()) { continue; }
        if coverage.sitemap_files >= MAX_NESTED || candidates.len() >= MAX_CANDIDATES || tokio::time::Instant::now() >= sitemap_deadline { coverage.sitemap_partial = true; break; }
        if robots.as_ref().is_some_and(|rt| !rt.is_allowed(MAP_UA, url.path())) { coverage.sitemap_partial = true; continue; }
        coverage.sitemap_files += 1;
        let remaining = sitemap_deadline.saturating_duration_since(tokio::time::Instant::now()).min(Duration::from_secs(4));
        let xml = match tokio::time::timeout(remaining, fetch_text(&state, &url)).await {
            Ok(Ok(Some(document))) => document,
            Ok(Ok(None)) => continue,
            Ok(Err(error)) => return map_failure(error),
            Err(_) => { coverage.sitemap_partial = true; continue; }
        };
        let sm = quarry_transform::sitemap::parse(&xml.body);
        if !sm.valid { coverage.sitemap_partial = true; continue; }
        coverage.sitemap_valid += 1;
        coverage.sitemap_partial |= sm.truncated;
        candidates.extend(sm.urls.into_iter().take(MAX_CANDIDATES.saturating_sub(candidates.len())).map(|e| e.loc));
        for nested in sm.nested.into_iter().filter_map(|raw| Url::parse(&raw).ok()).filter(|u| nested_in_scope(&base,u)) {
            if depth < MAX_SITEMAP_DEPTH && sitemap_queue.len() < MAX_CANDIDATES { sitemap_queue.push_back((nested, depth+1)); }
            else { coverage.sitemap_partial = true; }
        }
    }
    if !sitemap_queue.is_empty() { coverage.sitemap_partial = true; }

    // Page links + titles.
    let mut titles: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let base_html = if robots.as_ref().is_some_and(|rt| !rt.is_allowed(MAP_UA, base.path())) { None } else {match fetch_text(&state, &base).await {
        Ok(html) => html,
        Err(e) => return map_failure(e),
    }};
    if let Some(html) = base_html {
        if html.url.origin() != base.origin() {
            base = html.url.clone();
            seed_host = base.host_str().unwrap().to_owned();
            let robots_url = base.join("/robots.txt").expect("valid origin");
            robots = match fetch_text(&state, &robots_url).await {
                Ok(document) => document
                    .map(|document| quarry_transform::robots::RobotsTxt::parse(&document.body)),
                Err(error) => return map_failure(error),
            };
        }
        for link in quarry_transform::links::extract(&html.body, &base)
            .into_iter()
            .take(MAX_CANDIDATES.saturating_sub(candidates.len()))
        {
            if let Some(t) = &link.text {
                titles.entry(link.href.clone()).or_insert_with(|| t.clone());
            }
            candidates.push(link.href);
        }
    }

    if candidates.is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(upstream_empty(
                "could not fetch sitemap or page to discover URLs",
                "the origin returned no sitemap.xml and the page had no links",
            )),
        )
            .into_response();
    }

    // Scope + dedup, then robots allow-filter.
    let mut urls = merge_and_scope(&seed_host, req.include_subdomains, candidates);
    urls.retain(|raw| {
        Url::parse(raw).is_ok_and(|url| {
            url.port_or_known_default() == base.port_or_known_default()
                && (url.origin() == base.origin() || req.include_subdomains)
        })
    });
    let before_robots = urls.len();
    if let Some(rt) = &robots {
        urls.retain(|u| {
            Url::parse(u)
                .ok()
                .map(|p| rt.is_allowed(MAP_UA, p.path()))
                .unwrap_or(true)
        });
    }

    coverage.robots_filtered = before_robots.saturating_sub(urls.len());
    coverage.candidates = urls.len();
    let limit = clamp_limit(req.limit);
    coverage.truncated = urls.len() > limit || candidates_exhausted(&coverage);

    // Optional relevance ranking.
    let (links, ranked) = match &req.search {
        Some(term) if !term.trim().is_empty() => {
            let ranker = LexicalRanker;
            match ranker.rank(term, &urls).await {
                Ok(mut ranked_urls) => {
                    ranked_urls.sort_by(|a, b| {
                        b.score
                            .partial_cmp(&a.score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    let links = ranked_urls
                        .into_iter()
                        .take(limit)
                        .map(|r| MapLink {
                            title: titles.get(&r.url).cloned(),
                            url: r.url,
                            score: Some(r.score),
                        })
                        .collect::<Vec<_>>();
                    (links, true)
                }
                Err(_) => (to_links(urls, &titles, limit), false),
            }
        }
        _ => (to_links(urls, &titles, limit), false),
    };

    let count = links.len();

    // Usage metering — one unit per map call.
    let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
    state
        .usage
        .meter(quarry_runtime::UsageEvent::new(
            run_id.to_string(),
            claims.org_id.clone(),
            quarry_runtime::usage_metrics::SEARCH_QUERY,
            1.0,
            serde_json::json!({
                "op": "map",
                "user_id": claims.user_id,
                "host": seed_host,
                "result_count": count,
                "ranked": ranked,
            }),
        ))
        .await;

    (
        StatusCode::OK,
        Json(MapResponse {
            url: req.url,
            links,
            count,
            ranked,
            coverage,
        }),
    )
        .into_response()
}

fn candidates_exhausted(coverage: &MapCoverage) -> bool { coverage.sitemap_partial || coverage.candidates >= MAX_CANDIDATES }

fn to_links(
    urls: Vec<String>,
    titles: &std::collections::HashMap<String, String>,
    limit: usize,
) -> Vec<MapLink> {
    urls.into_iter()
        .take(limit)
        .map(|u| MapLink {
            title: titles.get(&u).cloned(),
            url: u,
            score: None,
        })
        .collect()
}

#[cfg(test)]
#[path = "map_routes_tests.rs"]
mod tests;
