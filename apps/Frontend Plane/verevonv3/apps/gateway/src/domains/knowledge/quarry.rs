use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

use crate::{
    cache,
    config::AppState,
    domains::knowledge::shared,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    public_url::normalize_public_http_url,
    upstream::{proxy_bearer_json, proxy_sse_stream},
};

const PREVIEW_MARKDOWN_CHAR_LIMIT: usize = 80_000;

/// Default freshness window for a cached scrape preview, used when the caller
/// sends no `max_age_s`. Matches `cache::FRESH_SECS`, which this replaced for
/// this route so the caller's policy can override it.
const PREVIEW_FRESH_SECS: u64 = 4 * 60 * 60;

pub(super) async fn scrape(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> impl IntoResponse {
    // SSRF guard at the gateway boundary (quarry-edge is reachable on the internal
    // network): validate + normalize the user-supplied URL exactly like the crawl
    // and onboarding paths do, instead of forwarding an arbitrary target verbatim.
    let normalized = body
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "A non-empty URL is required.".to_owned())
        .and_then(normalize_public_http_url);
    let normalized = match normalized {
        Ok(value) => value,
        Err(message) => return (StatusCode::BAD_REQUEST, Json(error("invalid_url", message))),
    };
    body["url"] = Value::String(normalized);

    let cookie = shared::cookie_header(&headers);
    let token = shared::quarry_token(&state, &user, &cookie).await;
    let url = format!("{}/v1/scrape", state.quarry_edge_url);
    proxy_bearer_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        &user.user_id,
    )
    .await
}

pub(super) async fn scrape_preview(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Response {
    let target = body
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    let Some(target) = target else {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_url", "A non-empty URL is required.")),
        )
            .into_response();
    };

    // SSRF guard: validate + normalize before quarry fetches it (matches /scrape,
    // crawl, and the onboarding paths). Rewrite the body so the forwarded scrape
    // request uses the normalized URL too.
    let target = match normalize_public_http_url(&target) {
        Ok(value) => value,
        Err(message) => {
            return (StatusCode::BAD_REQUEST, Json(error("invalid_url", message))).into_response()
        }
    };
    body["url"] = Value::String(target.clone());

    // Cache admission MIRRORS quarry-edge's (`routes.rs` `should_read` /
    // `should_write` / `effective_ttl`) rather than inventing a second policy.
    // It previously invented one, and the gap was a ZDR hole: a `zdr: true`
    // scrape is forwarded verbatim to Quarry, Quarry correctly refuses to cache
    // it — and the BFF then wrote the same rendered page content into
    // Frontend-Plane Redis for 24 hours anyway, breaking the guarantee at the
    // one boundary the ownership matrix says must not hold this. The caller's
    // `cache` policy was ignored the same way: `{mode: "bypass"}` was honoured
    // upstream and silently overridden here.
    let admission = CacheAdmission::resolve(&state, &user, &headers, &body).await;
    let key = cache::cache_key("scrape-preview", &[&target]);
    let mut stale: Option<Value> = None;
    if admission.may_read {
        if let Some(hit) = state
            .cache
            .lookup_within(&key, admission.max_age_secs)
            .await
        {
            return (StatusCode::OK, Json(ok(hit))).into_response();
        }
        // Outside the freshness window a copy is still worth keeping as an
        // outage fallback, exactly as before — but only within the store window
        // the caller's policy allows.
        if let Some(hit) = state.cache.lookup(&key).await {
            stale = Some(hit.data);
        }
    }

    let cookie = shared::cookie_header(&headers);
    let token = shared::quarry_token(&state, &user, &cookie).await;
    let scrape_url = format!("{}/v1/scrape", state.quarry_edge_url);
    let started = Instant::now();
    let (mut status, Json(mut scrape_body)) = proxy_bearer_json(
        &state,
        Method::POST,
        &scrape_url,
        Some(body.clone()),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    let browser_elapsed = started.elapsed();

    // The browser render can fail RECOVERABLY — the driver is unregistered, or a
    // broken headless Chrome surfaces as a 5xx — and a plain static fetch recovers
    // those. But it can also fail UNrecoverably for a preview: a hung render or a
    // bot-walled origin (403/429) that already burned the request budget. Stacking
    // a second ~25s static fetch on top of that is exactly what turned a slow
    // failure into an empty response at the client, so only retry when the first
    // attempt failed FAST and looks transient — never on a block or a stall.
    let retry_worthwhile = (browser_driver_unavailable(&scrape_body) || status.is_server_error())
        && !matches!(status.as_u16(), 403 | 429)
        && browser_elapsed < Duration::from_secs(12);
    if !status.is_success() && retry_worthwhile {
        tracing::warn!(
            target = %target,
            %status,
            "quarry browser render failed fast; retrying scrape preview as a static fetch"
        );
        let fallback_body = without_browser_rendering(&body);
        let (retry_status, Json(retry_body)) = proxy_bearer_json(
            &state,
            Method::POST,
            &scrape_url,
            Some(fallback_body),
            token.as_deref(),
            &user.user_id,
        )
        .await;
        status = retry_status;
        scrape_body = retry_body;
    }

    if !status.is_success() {
        if let Some(data) = stale {
            tracing::warn!(target = %target, "quarry scrape failed; serving stale cached preview");
            return (StatusCode::OK, Json(ok(data))).into_response();
        }
        // A Quarry refusal is final. This is where a commercial stealth-proxy
        // tier (Scrapfly / Bright Data Web Unlocker) used to run in-process,
        // reached precisely BECAUSE quarry-edge had returned 403/429 — so the
        // one signal that meant "policy said no" was the trigger for going
        // around it. It shipped the org's target URL and the page's full
        // content to a third-party processor with no `zdr` bit, no org
        // scoping, no robots check, no usage record and no step receipt, then
        // relabelled the result with a `quarry` key so nothing downstream could
        // tell first-party evidence from a stealth scrape. Removed: Quarry owns
        // web fetch, and a stealth tier — if ever needed — belongs behind
        // `quarry-edge` as another `DriverKind` in its own waterfall.
        //
        // Always return a clear, typed error promptly — never let a slow or
        // bot-walled upstream collapse into an empty response at the client.
        let (code, message) = classify_scrape_failure(status, browser_elapsed);
        tracing::warn!(target = %target, %status, code, "scrape preview unavailable");
        return (status, Json(error(code, message))).into_response();
    }

    let extract_target = preview_url(&target, &scrape_body);
    let extract_markdown = if preview_markdown(&scrape_body).is_some() {
        None
    } else {
        fetch_extract_markdown(&state, &extract_target, token.as_deref(), &user.user_id).await
    };

    let preview = build_scrape_preview(&target, &scrape_body, extract_markdown.as_deref());
    if admission.may_write {
        state.cache.store(&key, &preview).await;
    }
    (StatusCode::OK, Json(ok(preview))).into_response()
}

/// Whether the BFF may read and write its own copy of a page preview, and for
/// how long a cached copy counts as fresh.
///
/// Deliberately shaped like quarry-edge's three admission helpers so the two
/// layers cannot drift: `may_read`/`may_write` correspond to `should_read` /
/// `should_write` (both additionally gated on `!zdr.is_active()`), and
/// `max_age_secs` to `effective_ttl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheAdmission {
    may_read: bool,
    may_write: bool,
    max_age_secs: u64,
}

impl CacheAdmission {
    /// Nothing in, nothing out. Used for a ZDR-posture caller: page content
    /// fetched under ZDR must not touch Frontend-Plane storage in either
    /// direction — not written, and not served from an entry some earlier
    /// non-ZDR request happened to leave behind.
    const DENIED: Self = Self {
        may_read: false,
        may_write: false,
        max_age_secs: 0,
    };

    /// Resolve admission for this request.
    ///
    /// The ZDR posture is derived SERVER-side ([`crate::zdr`]): the `x-zdr`
    /// header, a `zdr: true` body field, or the organisation's standing posture
    /// in org-core. The org check is what makes this hold for a client that
    /// never sets either — and it is skipped when the request already declares
    /// ZDR, since the answer cannot change.
    async fn resolve(
        state: &AppState,
        user: &AuthenticatedUser,
        headers: &HeaderMap,
        body: &Value,
    ) -> Self {
        if crate::zdr::request_zdr(headers, Some(body)) {
            return Self::DENIED;
        }
        if crate::zdr::org_zdr_enabled(state, user).await {
            return Self::DENIED;
        }
        Self::from_policy(body.get("cache"))
    }

    /// Read the caller's `CachePolicy` (quarry-core's `{mode, max_age_s, …}`) out
    /// of the same body the BFF forwards to quarry-edge. An absent or
    /// unrecognised policy keeps the previous defaults, so a client that sends
    /// no policy behaves exactly as before.
    fn from_policy(policy: Option<&Value>) -> Self {
        let mode = policy
            .and_then(|policy| policy.get("mode"))
            .and_then(Value::as_str)
            .map(str::trim);
        let (may_read, may_write) = match mode {
            None => (true, true),
            Some("read_write") => (true, true),
            Some("read_only") => (true, false),
            Some("write_only") => (false, true),
            Some("bypass") => (false, false),
            // An unknown mode is not a licence to cache: quarry-edge would
            // reject the body outright, so the safe local reading is "no".
            Some(_) => (false, false),
        };
        let max_age_secs = policy
            .and_then(|policy| policy.get("max_age_s"))
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
            .unwrap_or(PREVIEW_FRESH_SECS);
        Self {
            may_read,
            may_write,
            max_age_secs,
        }
    }
}

/// Map a failed upstream scrape into a clear, user-facing (code, message). A long
/// elapsed time means a hung render or a bot-wall stall — the common case for
/// JS-heavy, protected retailer pages — while 403/429 is an explicit block.
fn classify_scrape_failure(status: StatusCode, elapsed: Duration) -> (&'static str, String) {
    if matches!(status.as_u16(), 403 | 429) {
        return (
            "site_blocked",
            "Dette nettstedet blokkerer automatisert henting. Prøv en annen kilde, eller be om proxy-tilgang for å hente beskyttede nettsteder.".to_owned(),
        );
    }
    if elapsed >= Duration::from_secs(20) || matches!(status.as_u16(), 408 | 504) {
        return (
            "scrape_timeout",
            "Siden brukte for lang tid på å svare — ofte en tung side eller bot-beskyttelse. Prøv en mer spesifikk underside, eller en annen kilde.".to_owned(),
        );
    }
    (
        "scrape_failed",
        "Kunne ikke hente forhåndsvisning av siden. Sjekk lenken og prøv igjen.".to_owned(),
    )
}

pub(super) async fn start_crawl(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    // Two shapes share this route. A whole-site crawl ({ url, max_pages }) goes
    // to quarry's `/v1/crawl`. A curated subset ({ urls: [...] }) — the pages a
    // user picked from `/crawl/discover` — goes to quarry's durable `/v1/batch`,
    // which ingests exactly those URLs and emits the same run-event stream as a
    // crawl, so live progress reflects only the selected scope.
    let has_urls = body
        .get("urls")
        .and_then(Value::as_array)
        .is_some_and(|urls| !urls.is_empty());
    let (endpoint, request_body, kind) = if has_urls {
        match normalize_batch_body(&body) {
            Ok(batch) => ("/v1/batch", batch, "batch"),
            Err(message) => return invalid_crawl(message),
        }
    } else {
        match normalize_crawl_body(&body) {
            Ok(crawl) => ("/v1/crawl", crawl, "crawl"),
            Err(message) => return invalid_crawl(message),
        }
    };

    let cookie = shared::cookie_header(&headers);
    let token = shared::quarry_token(&state, &user, &cookie).await;
    let url = format!("{}{}", state.quarry_edge_url, endpoint);
    let (status, Json(resp)) = proxy_bearer_json(
        &state,
        Method::POST,
        &url,
        Some(request_body),
        token.as_deref(),
        &user.user_id,
    )
    .await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    // The durable handoff (`/v1/crawl`, `/v1/batch`) returns a control
    // `job_id`; the Temporal `run_id` is only stamped once the job starts
    // running, so it is usually absent here. The event stream must key off
    // `job_id` (control indexes events by both) and point at the edge's
    // SSE-emitting `/v1/jobs/{id}/events` — the old `/runs/{id}/events`
    // path 400s on a job_id and returns JSON the SPA SSE reader can't read,
    // which is why the UI showed 0 pages.
    let job_id = crawl_handoff_job_id(&resp).unwrap_or_else(|| format!("{kind}_{}", user.user_id));
    let run_id = first_string(&resp, &["/run_id", "/runId", "/data/run_id", "/data/runId"]);
    let normalized = json!({
        "id": job_id.clone(),
        "jobId": job_id.clone(),
        "runId": run_id,
        "kind": kind,
        "status": crawl_job_status(&resp).unwrap_or("queued"),
        "acceptedAt": first_string(&resp, &[
            "/accepted_at",
            "/acceptedAt",
            "/data/accepted_at",
            "/data/acceptedAt",
        ]),
        "eventStream": format!("/api/v1/knowledge/jobs/{}/events", job_id),
        "upstream": resp,
    });

    (StatusCode::OK, Json(ok(normalized))).into_response()
}

fn invalid_crawl(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error("invalid_crawl_request", message)),
    )
        .into_response()
}

fn normalize_crawl_body(input: &Value) -> Result<Value, String> {
    let raw_url = input
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "A non-empty URL is required.".to_owned())?;
    let url = normalize_public_http_url(raw_url)?;
    let mut body = json!({ "url": url });

    if let Some(max_pages) = input
        .get("max_pages")
        .or_else(|| input.get("maxPages"))
        .and_then(Value::as_u64)
    {
        if max_pages == 0 {
            return Err("maxPages must be greater than zero.".to_owned());
        }
        body["max_pages"] = json!(max_pages.min(5_000));
    }

    // Phase 6 selective ingest: forward the resolved ingest decision (the SPA
    // maps crawl_ingest_mode auto→true / never→false / prompt→false+stage).
    // Quarry defaults to NEVER when absent, so a missing flag = working-set only.
    if let Some(ingest) = input.get("ingest").and_then(Value::as_bool) {
        body["ingest"] = json!(ingest);
    }

    Ok(body)
}

/// Validate the curated subset a user picked from `/crawl/discover` into a
/// `{ "urls": [...] }` body for quarry's durable `/v1/batch`. Each URL is run
/// through the same SSRF-guarding normalizer as a single crawl, blanks/invalid
/// entries are dropped, duplicates collapse, and the set is capped so one batch
/// stays within quarry's body budget (it documents "a few hundred URLs"). Errors
/// when nothing valid remains — we never hand quarry an empty batch.
fn normalize_batch_body(input: &Value) -> Result<Value, String> {
    const MAX_BATCH_URLS: usize = 200;
    let raw = input
        .get("urls")
        .and_then(Value::as_array)
        .ok_or_else(|| "A non-empty list of pages is required.".to_owned())?;

    let mut seen = std::collections::BTreeSet::new();
    let mut urls = Vec::new();
    for entry in raw {
        let Some(candidate) = entry
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Ok(normalized) = normalize_public_http_url(candidate) else {
            continue;
        };
        if seen.insert(normalized.clone()) {
            urls.push(normalized);
            if urls.len() >= MAX_BATCH_URLS {
                break;
            }
        }
    }

    if urls.is_empty() {
        return Err("Select at least one valid page to crawl.".to_owned());
    }
    let mut body = json!({ "urls": urls });
    // Phase 6 selective ingest: forward the resolved ingest decision (default
    // NEVER at quarry when absent).
    if let Some(ingest) = input.get("ingest").and_then(Value::as_bool) {
        body["ingest"] = json!(ingest);
    }
    Ok(body)
}

/// Build quarry `/v1/map` input from a dashboard discover request. Only the seed
/// `url` is required; `search` relevance-ranks the result, `limit` caps it
/// (quarry allows up to 1000 — we cap at 200 for a usable picker), and
/// `includeSubdomains` widens host scope beyond the seed host.
fn normalize_map_body(input: &Value) -> Result<Value, String> {
    const DEFAULT_LIMIT: u64 = 100;
    const MAX_LIMIT: u64 = 200;
    let raw_url = input
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "A non-empty URL is required.".to_owned())?;
    let url = normalize_public_http_url(raw_url)?;
    let limit = input
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);

    let mut body = json!({ "url": url, "limit": limit });
    if let Some(search) = input
        .get("search")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body["search"] = json!(search);
    }
    let include_subdomains = input
        .get("includeSubdomains")
        .or_else(|| input.get("include_subdomains"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if include_subdomains {
        body["include_subdomains"] = json!(true);
    }
    Ok(body)
}

/// `POST /api/v1/knowledge/crawl/discover` — fast whole-site URL discovery so the
/// dashboard can let a user pick which pages to crawl. Proxies quarry-edge
/// `/v1/map` (read-only; no durable writes) and reshapes the result into
/// `{ url, pages: [{ url, title?, score? }], count }`.
pub(super) async fn discover_crawl_pages(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let map_body = match normalize_map_body(&body) {
        Ok(value) => value,
        Err(message) => {
            return (StatusCode::BAD_REQUEST, Json(error("invalid_url", message))).into_response();
        }
    };
    let cookie = shared::cookie_header(&headers);
    let token = shared::quarry_token(&state, &user, &cookie).await;
    let url = format!("{}/v1/map", state.quarry_edge_url);
    let (status, Json(resp)) = proxy_bearer_json(
        &state,
        Method::POST,
        &url,
        Some(map_body),
        token.as_deref(),
        &user.user_id,
    )
    .await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    (StatusCode::OK, Json(ok(build_discovery(&resp)))).into_response()
}

/// Reshape quarry's `/v1/map` response (`{ url, links: [{url,title?,score?}] }`,
/// possibly under a `data` envelope) into the dashboard discovery payload. Drops
/// links without a usable URL; omits absent title/score rather than emitting null.
fn build_discovery(resp: &Value) -> Value {
    let links = resp
        .pointer("/links")
        .or_else(|| resp.pointer("/data/links"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pages: Vec<Value> = links
        .iter()
        .filter_map(|link| {
            let url = link
                .get("url")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            let mut page = json!({ "url": url });
            if let Some(title) = link
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                page["title"] = json!(title);
            }
            if let Some(score) = link.get("score").and_then(Value::as_f64) {
                page["score"] = json!(score);
            }
            Some(page)
        })
        .collect();

    json!({
        "url": first_string(resp, &["/url", "/data/url"]).unwrap_or_default(),
        "count": pages.len(),
        "pages": pages,
    })
}

async fn fetch_extract_markdown(
    state: &AppState,
    target: &str,
    token: Option<&str>,
    user_id: &str,
) -> Option<String> {
    let url = format!("{}/v1/extract", state.quarry_edge_url);
    let body = json!({
        "urls": [target],
        "max_urls": 1,
    });
    let (status, Json(response)) =
        proxy_bearer_json(state, Method::POST, &url, Some(body), token, user_id).await;
    if !status.is_success() {
        tracing::warn!(%status, target, "quarry extract fallback failed for scrape preview");
        return None;
    }
    extract_markdown_from_response(&response).map(truncate_markdown)
}

fn build_scrape_preview(target: &str, scrape: &Value, extract_markdown: Option<&str>) -> Value {
    let inline_markdown = preview_markdown(scrape);
    let markdown = extract_markdown
        .filter(|value| !value.trim().is_empty())
        .or(inline_markdown)
        .map(truncate_markdown)
        .unwrap_or_default();
    let source = if extract_markdown.is_some_and(|value| !value.trim().is_empty()) {
        "extract"
    } else if inline_markdown.is_some() {
        "inline"
    } else if markdown_artifact_id(scrape).is_some() {
        "artifact"
    } else {
        "empty"
    };

    json!({
        "url": preview_url(target, scrape),
        "title": first_string(scrape, &[
            "/data/metadata/title",
            "/metadata/title",
            "/data/title",
            "/title",
        ]).unwrap_or_default(),
        "description": first_string(scrape, &[
            "/data/metadata/description",
            "/metadata/description",
            "/data/description",
            "/description",
        ]).unwrap_or_default(),
        "markdown": markdown,
        "source": source,
        "quarry": {
            "runId": first_string(scrape, &["/data/run_id", "/run_id"]),
            "fingerprint": first_string(scrape, &["/data/fingerprint", "/fingerprint"]),
            "status": first_u64(scrape, &["/data/status", "/status"]),
            "markdownArtifactId": markdown_artifact_id(scrape),
            "htmlArtifactId": first_string(scrape, &[
                "/data/formats/html/artifact_id",
                "/formats/html/artifact_id",
            ]),
        },
    })
}

fn preview_markdown(scrape: &Value) -> Option<&str> {
    first_string(
        scrape,
        &[
            "/data/markdown",
            "/markdown",
            "/data/content",
            "/content",
            "/data/text",
            "/text",
        ],
    )
}

fn extract_markdown_from_response(response: &Value) -> Option<&str> {
    first_string(
        response,
        &[
            "/results/0/markdown",
            "/data/results/0/markdown",
            "/data/markdown",
            "/markdown",
        ],
    )
}

fn preview_url(target: &str, scrape: &Value) -> String {
    first_string(
        scrape,
        &[
            "/data/url/final_url",
            "/data/url/final",
            "/data/url/requested",
            "/data/metadata/sourceURL",
            "/data/metadata/source_url",
            "/data/metadata/url",
            "/metadata/sourceURL",
            "/metadata/source_url",
            "/metadata/url",
            "/url",
        ],
    )
    .unwrap_or(target)
    .to_owned()
}

fn markdown_artifact_id(scrape: &Value) -> Option<&str> {
    first_string(
        scrape,
        &[
            "/data/formats/markdown/artifact_id",
            "/formats/markdown/artifact_id",
        ],
    )
}

fn first_string<'a>(value: &'a Value, pointers: &[&str]) -> Option<&'a str> {
    pointers
        .iter()
        .filter_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn first_u64(value: &Value, pointers: &[&str]) -> Option<u64> {
    pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_u64))
}

fn truncate_markdown(value: &str) -> String {
    value
        .trim()
        .chars()
        .take(PREVIEW_MARKDOWN_CHAR_LIMIT)
        .collect()
}

fn without_browser_rendering(body: &Value) -> Value {
    let mut fallback = body.clone();
    if let Some(obj) = fallback.as_object_mut() {
        obj.remove("signals");
        obj.remove("render");
        obj.remove("renderHints");
    }
    fallback
}

fn browser_driver_unavailable(value: &Value) -> bool {
    let serialized = value.to_string().to_ascii_lowercase();
    serialized.contains("driver not registered")
        || serialized.contains("browser=driver not registered")
        || serialized.contains("no drivers succeeded in fallback chain")
}

/// Resolve the durable `job_id` from a crawl/batch handoff envelope.
///
/// We must NOT fall back to `run_id` here, because the event stream is keyed
/// off the `job_id` (control indexes events by both, and the `run_id` is
/// usually absent at handoff). We prefer the explicit job-id fields, then the
/// generic `id` field the edge `HandoffAck` envelope uses.
fn crawl_handoff_job_id(value: &Value) -> Option<String> {
    first_string(
        value,
        &[
            "/job_id",
            "/jobId",
            "/data/job_id",
            "/data/jobId",
            "/id",
            "/data/id",
        ],
    )
    .map(str::to_owned)
}

fn crawl_job_status(value: &Value) -> Option<&str> {
    first_string(value, &["/status", "/state", "/data/status", "/data/state"])
}

#[cfg(test)]
// The crawl-jobs route handlers (list_crawl_jobs, crawl_run_events) intentionally
// follow this test module to keep them next to the crawl flow they serve.
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    fn preview_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    async fn state_with_org_zdr(zdr: bool) -> (AppState, wiremock::MockServer) {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org-1",
                "metadata": { "interactiveRetention": { "zdr": zdr } }
            })))
            .mount(&org_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.org_core_url = org_core.uri();
        (state, org_core)
    }

    /// The BI-2 defect verbatim: a `zdr: true` scrape is forwarded to Quarry,
    /// Quarry refuses to cache it — and the BFF wrote the rendered page content
    /// into Frontend-Plane Redis for 24h regardless. Admission must be denied in
    /// BOTH directions, so a ZDR request also cannot be answered from an entry
    /// an earlier non-ZDR request left behind. An explicit `read_write` policy
    /// must not re-open it: the posture is resolved before the policy is read.
    #[tokio::test]
    async fn a_zdr_request_neither_reads_nor_writes_the_local_preview_cache() {
        let (state, _org_core) = state_with_org_zdr(false).await;
        let body = json!({
            "url": "https://example.invalid/",
            "zdr": true,
            "cache": { "mode": "read_write", "max_age_s": 3600 }
        });
        let admission =
            CacheAdmission::resolve(&state, &preview_user(), &HeaderMap::new(), &body).await;
        assert_eq!(admission, CacheAdmission::DENIED);

        // The `x-zdr` header alone does it too, with no `zdr` in the body.
        let mut headers = HeaderMap::new();
        headers.insert("x-zdr", axum::http::HeaderValue::from_static("true"));
        let admission = CacheAdmission::resolve(
            &state,
            &preview_user(),
            &headers,
            &json!({ "url": "https://example.invalid/" }),
        )
        .await;
        assert_eq!(admission, CacheAdmission::DENIED);
    }

    /// Org-wide ZDR is invisible to the request body — a client under a ZDR
    /// workspace sends an ordinary scrape. The posture has to be derived
    /// server-side or the page content lands in Frontend-Plane Redis anyway.
    #[tokio::test]
    async fn an_org_under_zdr_caches_no_page_content_at_all() {
        let (state, _org_core) = state_with_org_zdr(true).await;
        let admission = CacheAdmission::resolve(
            &state,
            &preview_user(),
            &HeaderMap::new(),
            &json!({ "url": "https://example.invalid/" }),
        )
        .await;
        assert_eq!(admission, CacheAdmission::DENIED);
    }

    #[tokio::test]
    async fn an_ordinary_request_in_a_non_zdr_org_still_caches() {
        let (state, _org_core) = state_with_org_zdr(false).await;
        let admission = CacheAdmission::resolve(
            &state,
            &preview_user(),
            &HeaderMap::new(),
            &json!({ "url": "https://example.invalid/" }),
        )
        .await;
        assert!(admission.may_read && admission.may_write);
        assert_eq!(admission.max_age_secs, PREVIEW_FRESH_SECS);
    }

    /// quarry-edge honours `{mode: "bypass"}`; this layer silently ignored it,
    /// so a caller who asked for a live fetch got a preview up to 4h old.
    #[test]
    fn the_callers_cache_mode_is_honoured_the_way_quarry_edge_honours_it() {
        let admission = |mode: &str| CacheAdmission::from_policy(Some(&json!({ "mode": mode })));

        let bypass = admission("bypass");
        assert!(!bypass.may_read && !bypass.may_write);

        let read_only = admission("read_only");
        assert!(read_only.may_read && !read_only.may_write);

        let write_only = admission("write_only");
        assert!(!write_only.may_read && write_only.may_write);

        let read_write = admission("read_write");
        assert!(read_write.may_read && read_write.may_write);

        // An unknown mode must not read as permission. quarry-edge rejects the
        // body outright; the safe local reading is "cache nothing".
        let unknown = admission("write_through");
        assert!(!unknown.may_read && !unknown.may_write);
    }

    #[test]
    fn no_policy_keeps_the_previous_defaults() {
        let default = CacheAdmission::from_policy(None);
        assert!(default.may_read && default.may_write);
        assert_eq!(default.max_age_secs, PREVIEW_FRESH_SECS);
        // An empty policy object is the same as none.
        let empty = CacheAdmission::from_policy(Some(&json!({})));
        assert!(empty.may_read && empty.may_write);
        assert_eq!(empty.max_age_secs, PREVIEW_FRESH_SECS);
    }

    /// `max_age_s` is the caller's freshness window, matching quarry-edge's
    /// `effective_ttl`. A zero/absent value falls back to the default rather
    /// than meaning "instantly stale".
    #[test]
    fn max_age_s_narrows_the_freshness_window() {
        let tight = CacheAdmission::from_policy(Some(&json!({ "max_age_s": 60 })));
        assert_eq!(tight.max_age_secs, 60);
        let zero = CacheAdmission::from_policy(Some(&json!({ "max_age_s": 0 })));
        assert_eq!(zero.max_age_secs, PREVIEW_FRESH_SECS);
    }

    #[test]
    fn build_scrape_preview_uses_extract_markdown_for_artifact_output() {
        let scrape = json!({
            "data": {
                "run_id": "run-1",
                "status": 200,
                "fingerprint": "blake3:abc",
                "url": {
                    "requested": "https://vg.no/",
                    "final_url": "https://www.vg.no/"
                },
                "formats": {
                    "markdown": { "artifact_id": "art-md", "bytes": 1024 }
                },
                "metadata": {
                    "title": "VG",
                    "description": "Nyheter fra Norge og verden."
                }
            }
        });

        let preview = build_scrape_preview("https://vg.no/", &scrape, Some("# VG\n\nSiste nytt."));

        assert_eq!(preview["source"], "extract");
        assert_eq!(preview["markdown"], "# VG\n\nSiste nytt.");
        assert_eq!(preview["url"], "https://www.vg.no/");
        assert_eq!(preview["quarry"]["markdownArtifactId"], "art-md");
    }

    #[test]
    fn build_scrape_preview_preserves_inline_markdown() {
        let scrape = json!({
            "data": {
                "markdown": "# Inline\n\nContent",
                "metadata": { "title": "Inline page" },
                "url": "https://example.com"
            }
        });

        let preview = build_scrape_preview("https://example.com", &scrape, None);

        assert_eq!(preview["source"], "inline");
        assert_eq!(preview["markdown"], "# Inline\n\nContent");
        assert_eq!(preview["title"], "Inline page");
    }

    #[test]
    fn extract_markdown_supports_wrapped_and_direct_payloads() {
        let direct = json!({ "results": [{ "markdown": "# Direct" }] });
        let wrapped = json!({ "data": { "results": [{ "markdown": "# Wrapped" }] } });

        assert_eq!(extract_markdown_from_response(&direct), Some("# Direct"));
        assert_eq!(extract_markdown_from_response(&wrapped), Some("# Wrapped"));
    }

    #[test]
    fn browser_driver_error_is_detected_for_retry() {
        let body = json!({
            "error": {
                "message": "no drivers succeeded in fallback chain: Browser=driver not registered"
            }
        });

        assert!(browser_driver_unavailable(&body));
    }

    #[test]
    fn fallback_body_removes_browser_only_fields() {
        let body = json!({
            "url": "https://vg.no",
            "signals": { "actions": ["render_page"] },
            "render": { "waitForTimeoutMs": 1800 },
            "renderHints": { "waitForTimeoutMs": 1800 }
        });

        let fallback = without_browser_rendering(&body);

        assert_eq!(fallback["url"], "https://vg.no");
        assert!(fallback.get("signals").is_none());
        assert!(fallback.get("render").is_none());
        assert!(fallback.get("renderHints").is_none());
    }

    #[test]
    fn normalize_batch_body_dedups_drops_invalid_and_caps() {
        let input = json!({
            "urls": [
                "https://vg.no/a",
                "https://vg.no/a",
                "   ",
                "not-a-url",
                "ftp://vg.no/x",
                "https://vg.no/b"
            ]
        });
        let body = normalize_batch_body(&input).unwrap();
        let urls = body["urls"].as_array().unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://vg.no/a");
        assert_eq!(urls[1], "https://vg.no/b");
    }

    #[test]
    fn normalize_batch_body_rejects_empty_after_filtering() {
        let input = json!({ "urls": ["", "not-a-url", "http://localhost/secret"] });
        assert!(normalize_batch_body(&input).is_err());
    }

    #[test]
    fn normalize_map_body_defaults_and_passes_through_options() {
        let body = normalize_map_body(&json!({ "url": "https://vg.no" })).unwrap();
        assert_eq!(body["url"], "https://vg.no/");
        assert_eq!(body["limit"], 100);
        assert!(body.get("search").is_none());
        assert!(body.get("include_subdomains").is_none());

        let body = normalize_map_body(&json!({
            "url": "https://vg.no",
            "search": "sport",
            "limit": 5000,
            "includeSubdomains": true
        }))
        .unwrap();
        assert_eq!(body["limit"], 200); // clamped to MAX_LIMIT
        assert_eq!(body["search"], "sport");
        assert_eq!(body["include_subdomains"], true);
    }

    #[test]
    fn build_discovery_reshapes_links_and_skips_urlless() {
        let resp = json!({
            "url": "https://vg.no/",
            "links": [
                { "url": "https://vg.no/a", "title": "A", "score": 0.9 },
                { "url": "  ", "title": "blank" },
                { "title": "no url" },
                { "url": "https://vg.no/b" }
            ]
        });
        let discovery = build_discovery(&resp);
        let pages = discovery["pages"].as_array().unwrap();
        assert_eq!(discovery["count"], 2);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0]["url"], "https://vg.no/a");
        assert_eq!(pages[0]["title"], "A");
        assert_eq!(pages[0]["score"], 0.9);
        assert!(pages[1].get("title").is_none());
    }

    #[test]
    fn build_discovery_reads_data_enveloped_links() {
        let resp =
            json!({ "data": { "url": "https://x.io/", "links": [{ "url": "https://x.io/p" }] } });
        let discovery = build_discovery(&resp);
        assert_eq!(discovery["url"], "https://x.io/");
        assert_eq!(discovery["count"], 1);
        assert_eq!(discovery["pages"][0]["url"], "https://x.io/p");
    }
}

pub(super) async fn list_crawl_jobs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    uri: Uri,
) -> impl IntoResponse {
    let cookie = shared::cookie_header(&headers);
    let token = shared::quarry_token(&state, &user, &cookie).await;
    let url = format!(
        "{}/v1/crawl/jobs{}",
        state.quarry_edge_url,
        shared::qs(&uri)
    );
    proxy_bearer_json(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await
}

pub(super) async fn crawl_run_events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    uri: Uri,
) -> impl IntoResponse {
    let cookie = shared::cookie_header(&headers);
    let token = shared::quarry_token(&state, &user, &cookie).await;
    let url = format!(
        "{}/v1/runs/{}/events{}",
        state.quarry_edge_url,
        urlencoding::encode(&id),
        shared::qs(&uri)
    );
    proxy_sse_stream(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        headers.get("last-event-id").and_then(|v| v.to_str().ok()),
        None,
        false,
    )
    .await
}

/// Crawl 0-pages fix — durable crawl event stream keyed off the handoff
/// `job_id`. The edge emits real `text/event-stream` SSE at
/// `/v1/jobs/{id}/events` (one frame per control event, terminal `done`),
/// so we forward the bytes verbatim. This is what the normalized handoff's
/// `eventStream` now points at, replacing the broken `/runs/{id}/events`
/// hop that 400'd on a job_id and returned un-streamable JSON.
pub(super) async fn crawl_job_events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    uri: Uri,
) -> impl IntoResponse {
    let cookie = shared::cookie_header(&headers);
    let token = shared::quarry_token(&state, &user, &cookie).await;
    let url = format!(
        "{}/v1/jobs/{}/events{}",
        state.quarry_edge_url,
        urlencoding::encode(&id),
        shared::qs(&uri)
    );
    proxy_sse_stream(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        headers.get("last-event-id").and_then(|v| v.to_str().ok()),
        None,
        false,
    )
    .await
}
