use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

use crate::{
    cache_admission::{variant_digest, CacheAdmission},
    config::AppState,
    domains::browser::{
        is_valid_artifact_id, scrape_artifact_content_type, BrowserArtifactAbsence, CallerClass,
        MAX_BROWSER_ARTIFACT_BYTES,
    },
    domains::knowledge::shared,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    public_url::normalize_public_http_url,
    upstream::{proxy_bearer_json, proxy_bearer_json_with_timeout, proxy_sse_stream},
};

const PREVIEW_MARKDOWN_CHAR_LIMIT: usize = 80_000;

/// WS13.5 and INNH-1: the ceiling for a scrape that drives the browser (actions, a
/// screenshot, a PDF) or asks a model about the page (the AI formats). The edge's
/// route allows 125 s for both, so this client waits a little longer and the edge
/// answers with its own typed timeout first. Every other scrape keeps the standard
/// client's 25 s.
const SCRAPE_BROWSER_TIMEOUT: Duration = Duration::from_secs(135);

/// Whether a scrape body asks for work that outlasts the standard 25 s: a non-empty
/// `actions` list, an object-form format (a capture or an AI format), or a
/// `screenshot`, `pdf` or `summary` string format.
fn scrape_needs_browser_budget(body: &Value) -> bool {
    let has_actions = body
        .get("actions")
        .and_then(Value::as_array)
        .is_some_and(|actions| !actions.is_empty());
    let has_capture_format = body
        .get("formats")
        .and_then(Value::as_array)
        .is_some_and(|formats| {
            formats.iter().any(|format| match format {
                Value::Object(_) => true,
                Value::String(name) => matches!(name.as_str(), "screenshot" | "pdf" | "summary"),
                _ => false,
            })
        });
    has_actions || has_capture_format
}

/// WS13.5: stamp who proposed this scrape, from the caller the gateway verified.
/// The edge reads `initiated_by` as a tighten-only hint: `"agent"` makes it refuse
/// `actions`, so a Model-authored list relayed under a person's bearer is refused as a
/// service principal's would be. A verified service principal is always the agent,
/// whatever the body says; a person's session keeps a stated `"agent"` (it can only
/// tighten) and nothing else: any other client value is dropped, never forwarded.
fn stamp_scrape_origin(body: &mut Value, caller: CallerClass) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    let stated_agent = object.get("initiated_by").and_then(Value::as_str) == Some("agent");
    if caller == CallerClass::ServicePrincipal || stated_agent {
        object.insert("initiated_by".to_owned(), json!("agent"));
    } else {
        object.remove("initiated_by");
    }
}

pub(super) async fn scrape(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    caller: Option<Extension<CallerClass>>,
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
    stamp_scrape_origin(&mut body, caller.map(|Extension(class)| class).unwrap_or_default());

    let cookie = shared::cookie_header(&headers);
    let token = shared::quarry_token(&state, &user, &cookie).await;
    let url = format!("{}/v1/scrape", state.quarry_edge_url);
    if scrape_needs_browser_budget(&body) {
        return proxy_bearer_json_with_timeout(
            &state,
            Method::POST,
            &url,
            Some(body),
            token.as_deref(),
            &user.user_id,
            SCRAPE_BROWSER_TIMEOUT,
        )
        .await;
    }
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
    // WS13.7: the cached preview is text only (no actions, viewport or capture
    // formats), so the cache key and the browser-failure retry never see them.
    strip_preview_only_fields(&mut body);
    // Ask for the text inline. Without `formats` quarry-edge answers a
    // non-ZDR scrape with an artifact reference, which costs a second
    // `/v1/extract` call below to read back; a caller that sent its own
    // `formats` keeps them. Set before the variant digest so the cache key
    // describes the request that is actually forwarded.
    if body.get("formats").is_none() {
        body["formats"] = json!(["markdown"]);
    }

    // Cache admission MIRRORS quarry-edge's (`routes.rs` `should_read` /
    // `should_write` / `effective_ttl`) rather than inventing a second policy.
    // It previously invented one, and the gap was a ZDR hole: a `zdr: true`
    // scrape is forwarded verbatim to Quarry, Quarry correctly refuses to cache
    // it — and the BFF then wrote the same rendered page content into
    // Frontend-Plane Redis for 24 hours anyway, breaking the guarantee at the
    // one boundary the ownership matrix says must not hold this. The caller's
    // `cache` policy was ignored the same way: `{mode: "bypass"}` was honoured
    // upstream and silently overridden here.
    //
    // The entry is keyed by ORG and by the request variant (formats, render
    // hints, waits...), and a request that carries headers, actions, a profile
    // or a session is never cached at all: its response depends on state the
    // key does not capture.
    let admission = CacheAdmission::resolve(&state, &user, &headers, &body, true).await;
    admission.record_bypass("scrape-preview");
    let variant = variant_digest(&body);
    let key = admission.key("scrape-preview", &[&target, &variant]);
    let mut stale: Option<Value> = None;
    if let (Some(key), Some(scope), true) = (key.as_deref(), admission.scope(), admission.may_read)
    {
        if let Some(hit) = state
            .cache
            .lookup_within_scoped(key, scope, admission.max_age_secs)
            .await
        {
            crate::observability::record_result_cache("scrape-preview", "hit");
            return (StatusCode::OK, Json(ok(hit))).into_response();
        }
        crate::observability::record_result_cache("scrape-preview", "miss");
        // Outside the freshness window a copy is still worth keeping as an
        // outage fallback, exactly as before — but only within the store window
        // the caller's policy allows.
        if let Some(hit) = state.cache.lookup_scoped(key, scope).await {
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
            tracing::warn!("quarry scrape failed; serving stale cached preview");
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
        let (code, message) = classify_scrape_failure(status, browser_elapsed, &scrape_body);
        tracing::warn!(%status, code, "scrape preview unavailable");
        return scrape_failure_response(status, code, message, &scrape_body);
    }

    let extract_target = preview_url(&target, &scrape_body);
    let extract_markdown = if preview_markdown(&scrape_body).is_some() {
        None
    } else {
        fetch_extract_markdown(&state, &extract_target, token.as_deref(), &user.user_id).await
    };

    let preview = build_scrape_preview(&target, &scrape_body, extract_markdown.as_deref());
    if let (Some(key), Some(scope), true) = (key.as_deref(), admission.scope(), admission.may_write)
    {
        state.cache.store_scoped(key, scope, &preview).await;
        crate::observability::record_result_cache("scrape-preview", "store");
    }
    (StatusCode::OK, Json(ok(preview))).into_response()
}

/// `GET /api/v1/knowledge/scrape-artifacts/{artifact_id}` (WS13.7): one artifact of
/// a scrape capture (a screenshot or a PDF) as bytes the SPA can show or download.
///
/// Quarry's `GET /v1/artifacts/{id}` answers only for the caller's own org, so the
/// org-scoped token is the isolation boundary here. A capture belongs to an org, not
/// to a browser run, so the browser route's per-run gate does not apply. Quarry's
/// refusals are normalised: a missing or aged-out artifact (and another org's id) is
/// one 404, and a ZDR org gets the `withheld` shape. Only the image types and
/// `application/pdf` are served, with no-store caching.
pub(super) async fn scrape_artifact(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(artifact_id): Path<String>,
) -> Response {
    if !is_valid_artifact_id(&artifact_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_scrape_artifact",
                "The requested scrape artifact id is invalid.",
            )),
        )
            .into_response();
    }
    // Zero Data Retention: Quarry writes no artifact for a ZDR org, so there is
    // nothing to fetch. The posture is read before anything is sent upstream, and
    // an unknown posture counts as ZDR (`crate::zdr::org_zdr_enabled`).
    if crate::zdr::org_zdr_enabled(&state, &user).await {
        return BrowserArtifactAbsence::Withheld.into_response();
    }

    let cookie = shared::cookie_header(&headers);
    let token = shared::quarry_token(&state, &user, &cookie).await;
    let url = format!(
        "{}/v1/artifacts/{}",
        state.quarry_edge_url,
        urlencoding::encode(&artifact_id)
    );
    let mut req =
        crate::request_id::apply(state.client.get(&url)).header("x-user-id", &user.user_id);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let upstream = match req.send().await {
        Ok(upstream) => upstream,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(crate::envelope::upstream_unavailable()),
            )
                .into_response()
        }
    };
    let status = upstream.status();
    if matches!(status.as_u16(), 404 | 410) {
        return scrape_artifact_not_found();
    }
    if !status.is_success() {
        return scrape_artifact_failed();
    }
    if upstream
        .content_length()
        .is_some_and(|length| length > MAX_BROWSER_ARTIFACT_BYTES)
    {
        return scrape_artifact_too_large();
    }
    let upstream_content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let Some(content_type) = scrape_artifact_content_type(upstream_content_type.as_deref()) else {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(error(
                "scrape_artifact_unsupported",
                "This artifact type cannot be shown or downloaded.",
            )),
        )
            .into_response();
    };
    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return scrape_artifact_failed(),
    };
    if bytes.len() as u64 > MAX_BROWSER_ARTIFACT_BYTES {
        return scrape_artifact_too_large();
    }

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    if content_type == "application/pdf" {
        // A PDF is downloaded, never rendered inline by the SPA, and a browser that
        // opens it anyway runs it under the sandbox policy.
        response = response
            .header(
                header::CONTENT_DISPOSITION,
                r#"attachment; filename="page.pdf""#,
            )
            .header(header::CONTENT_SECURITY_POLICY, "sandbox");
    }
    response
        .body(Body::from(bytes))
        .unwrap_or_else(|_| scrape_artifact_failed())
}

fn scrape_artifact_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "scrape_artifact_not_found",
            "The scrape artifact is no longer available.",
        )),
    )
        .into_response()
}

fn scrape_artifact_failed() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(error(
            "scrape_artifact_failed",
            "The scrape artifact could not be returned.",
        )),
    )
        .into_response()
}

fn scrape_artifact_too_large() -> Response {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(error(
            "scrape_artifact_too_large",
            "The scrape artifact is too large to show.",
        )),
    )
        .into_response()
}

/// Keys a cached scrape preview never forwards (WS13.7). Browser actions, the
/// viewport and the capture options make the response depend on per-request browser
/// state, and the browser-failure retry in `scrape_preview` would replay them. The
/// captures are served by `/v1/knowledge/scrape`, which is not cached.
const PREVIEW_DROPPED_KEYS: [&str; 4] = ["actions", "actionsOnError", "viewport", "capture"];

/// The `formats` entries that request a capture. A non-string format (an options
/// object such as `{"type":"pdf"}`) is dropped as well.
const PREVIEW_DROPPED_FORMATS: [&str; 2] = ["screenshot", "pdf"];

/// Remove the preview-only-excluded fields from a scrape-preview body. A `formats`
/// list left empty is removed, so the default `["markdown"]` applies again.
fn strip_preview_only_fields(body: &mut Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    for key in PREVIEW_DROPPED_KEYS {
        object.remove(key);
    }
    let formats_empty = match object.get_mut("formats").and_then(Value::as_array_mut) {
        Some(formats) => {
            formats.retain(|format| {
                format
                    .as_str()
                    .is_some_and(|name| !PREVIEW_DROPPED_FORMATS.contains(&name))
            });
            formats.is_empty()
        }
        None => false,
    };
    if formats_empty {
        object.remove("formats");
    }
}

/// `details.block_reason` values Quarry sends on a scrape's `403
/// CHALLENGE_DETECTED` (the closed `BlockReason` enum in `quarry-core`). The
/// gateway reads the field and nothing else of the upstream body: a value that is
/// not in this list is treated as if the field were absent, so a new or garbled
/// reason falls back to the bare-403 class instead of being echoed.
const SCRAPE_BLOCK_REASONS: [&str; 4] = [
    "captcha_widget",
    "waf_interstitial",
    "unexpected_bot_error",
    "challenge_loop",
];

/// The typed block reason Quarry attached to a refused scrape, as one of this
/// file's own static strings. `None` unless the upstream error is a
/// `CHALLENGE_DETECTED` carrying a known `details.block_reason`. The returned
/// value is never upstream text.
fn scrape_block_reason(upstream: &Value) -> Option<&'static str> {
    if upstream.pointer("/error/code").and_then(Value::as_str) != Some("CHALLENGE_DETECTED") {
        return None;
    }
    let reason = upstream
        .pointer("/error/details/block_reason")?
        .as_str()?;
    SCRAPE_BLOCK_REASONS
        .iter()
        .copied()
        .find(|known| *known == reason)
}

/// Map a failed upstream scrape into a clear, user-facing (code, message). Only
/// a Quarry-supplied strong `block_reason` means the site put a bot wall in
/// front of the page (`site_blocked`); a bare 403 is a refusal we cannot
/// explain (`scrape_refused`: login, geo, policy, a plain "no"), and a 429 is
/// the site or Quarry asking us to slow down (`scrape_rate_limited`). A long
/// elapsed time means a hung render or a stall. None of the messages offers a
/// proxy, and none carries the host, the page text or the upstream message.
fn classify_scrape_failure(
    status: StatusCode,
    elapsed: Duration,
    upstream: &Value,
) -> (&'static str, String) {
    // W3-E3.8.5: a DRIVER_FAILED naming dns, connect or tls (sent only with
    // QUARRY_EDGE__FETCH_SUB_CODES on) has a code of its own, ahead of the elapsed rule.
    if let Some((code, message)) = shared::fetch_sub_code_refusal(upstream) {
        return (code, message.to_owned());
    }
    if status == StatusCode::FORBIDDEN && scrape_block_reason(upstream).is_some() {
        return (
            "site_blocked",
            "Dette nettstedet stopper automatisert henting med en bot-sperre. Prøv en annen kilde, eller åpne siden selv i nettleseren.".to_owned(),
        );
    }
    if status == StatusCode::FORBIDDEN {
        return (
            "scrape_refused",
            "Nettstedet avviste forespørselen om å hente siden. Siden kan kreve innlogging eller være stengt for automatisk henting. Prøv en annen kilde.".to_owned(),
        );
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return (
            "scrape_rate_limited",
            "Nettstedet mottar for mange forespørsler akkurat nå. Vent litt og prøv igjen.".to_owned(),
        );
    }
    if status.as_u16() == 415 {
        return (
            "unsupported_media",
            "Dette innholdet kan ikke vises som forhåndsvisning (for eksempel et bilde, et arkiv eller en annen filtype uten tekst). Prøv en lenke til en nettside eller et dokument.".to_owned(),
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

/// Seconds Quarry asked the caller to wait on a scrape's 429, bounded to
/// 1..=3600. quarry-edge answers a `/v1/scrape` `RATE_LIMITED` with the flat
/// `ApiError` (`retry_after_seconds` at the top of the body), other 429s with
/// the canonical envelope (`error.details.retry_after_seconds`); both are read,
/// the envelope first. `None` for a missing, non-numeric or out-of-range value.
fn scrape_retry_after_seconds(upstream: &Value) -> Option<u64> {
    upstream
        .pointer("/error/details/retry_after_seconds")
        .and_then(Value::as_u64)
        .filter(|seconds| (1..=3600).contains(seconds))
        .or_else(|| {
            upstream
                .pointer("/retry_after_seconds")
                .and_then(Value::as_u64)
                .filter(|seconds| (1..=3600).contains(seconds))
        })
}

/// The refusal the caller sees for a failed scrape preview: the typed gateway
/// code and a fixed message, plus the two typed hints the SPA has copy for. The
/// block reason is one of this file's static strings (`scrape_block_reason`);
/// the retry hint is a number in 1..=3600 seconds, read from either shape
/// Quarry answers a scrape's 429 with (`scrape_retry_after_seconds`). Nothing
/// else of the upstream body is forwarded.
fn scrape_failure_response(
    status: StatusCode,
    code: &'static str,
    message: String,
    upstream: &Value,
) -> Response {
    let mut body = error(code, message);
    if code == "site_blocked" {
        if let Some(reason) = scrape_block_reason(upstream) {
            body["error"]["blockReason"] = json!(reason);
        }
    }
    if code == "scrape_rate_limited" {
        if let Some(seconds) = scrape_retry_after_seconds(upstream) {
            body["error"]["retryAfterSeconds"] = json!(seconds);
            return (
                status,
                [(axum::http::header::RETRY_AFTER, seconds.to_string())],
                Json(body),
            )
                .into_response();
        }
    }
    (status, Json(body)).into_response()
}

/// The `org_backlog_full` refusal from Quarry's error envelope as a gateway
/// `crawl_backlog_full` 429 with `Retry-After`. Only the numbers survive
/// (limit, queued, seconds); nothing else from the upstream body is echoed.
/// `None` for any other 429.
fn crawl_backlog_full_response(resp: &Value) -> Option<Response> {
    let details = resp.pointer("/error/details")?;
    if details.get("reason")?.as_str()? != "org_backlog_full" {
        return None;
    }
    let count = |key: &str| {
        details
            .get(key)
            .and_then(Value::as_u64)
            .filter(|n| *n <= 1_000_000)
    };
    let retry_after = count("retry_after_seconds")
        .filter(|seconds| (1..=3600).contains(seconds))
        .unwrap_or(30);
    let mut body = error(
        "crawl_backlog_full",
        "Too many crawls are waiting for this organization. Try again shortly.",
    );
    body["error"]["retryAfterSeconds"] = json!(retry_after);
    if let Some(limit) = count("limit") {
        body["error"]["limit"] = json!(limit);
    }
    if let Some(queued) = count("queued") {
        body["error"]["queued"] = json!(queued);
    }
    Some(
        (
            StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, retry_after.to_string())],
            Json(body),
        )
            .into_response(),
    )
}

/// The organization's crawl or batch allowance is spent: Quarry's
/// `BUDGET_EXHAUSTED` 402 with `details.dimension = org_allowance` as a gateway
/// `crawl_allowance_exhausted` 402. Only the metric (one of two known names) and a
/// sane retry hint survive; nothing else from the upstream body is echoed.
/// `None` for any other 402.
fn crawl_allowance_exhausted_response(resp: &Value) -> Option<Response> {
    let details = resp.pointer("/error/details")?;
    if details.get("dimension")?.as_str()? != "org_allowance" {
        return None;
    }
    let mut body = error(
        "crawl_allowance_exhausted",
        "This organization has used up its crawl allowance.",
    );
    if let Some(metric) = details
        .get("metric")
        .and_then(Value::as_str)
        .filter(|metric| matches!(*metric, "quarry.crawl.page" | "quarry.batch.url"))
    {
        body["error"]["metric"] = json!(metric);
    }
    if let Some(seconds) = details
        .get("retry_after_seconds")
        .and_then(Value::as_u64)
        .filter(|seconds| (1..=2_592_000).contains(seconds))
    {
        body["error"]["retryAfterSeconds"] = json!(seconds);
    }
    Some((StatusCode::PAYMENT_REQUIRED, Json(body)).into_response())
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
        // The organization's own crawl queue is full: say so in a code the app
        // has copy for, with when to try again, instead of passing the
        // upstream envelope through.
        if status == StatusCode::TOO_MANY_REQUESTS {
            if let Some(response) = crawl_backlog_full_response(&resp) {
                return response;
            }
        }
        // The organization's configured allowance is spent (WS6.D): the same, with the
        // metric that ran out and no other upstream detail.
        if status == StatusCode::PAYMENT_REQUIRED {
            if let Some(response) = crawl_allowance_exhausted_response(&resp) {
                return response;
            }
        }
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
        // The bounds the crawl will actually run with and why they differ from
        // the request (an absent or zero page limit takes the default; a value
        // above the ceiling is clamped). Surfaced so a clamp is never silent.
        "limits": resp.pointer("/data/limits").cloned().unwrap_or(Value::Null),
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

/// Who proposed a crawl. The UI may set every scope switch the page offers. A Model
/// proposal (the `knowledge.crawl_site` action) may not set `allowExternalLinks`, and
/// no robots key is forwarded from either surface (WS14.S6; the robots override is WS14.S7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    Ui,
    Model,
}

/// The caps the edge enforces again (CRAWL_SCOPE_ROBOTS.md section 12). Gateway and edge
/// share these numbers; a test in each tree pins them.
pub(crate) const CRAWL_MAX_DEPTH: u64 = 10;
pub(crate) const CRAWL_MAX_PATTERNS: usize = 100;
pub(crate) const CRAWL_MAX_PATTERN_CHARS: usize = 512;
pub(crate) const CRAWL_MAX_PATTERN_TOTAL_CHARS: usize = 8_192;

pub(crate) fn normalize_crawl_body(input: &Value) -> Result<Value, String> {
    normalize_crawl_body_for(input, Surface::Ui)
}

pub(crate) fn normalize_crawl_body_for(input: &Value, surface: Surface) -> Result<Value, String> {
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
    // WS14.S3: the two scope switches, camel or snake case. Absent means the edge default (false).
    if let Some(allow) = input.get("allowSubdomains").or_else(|| input.get("allow_subdomains")).and_then(Value::as_bool) {
        body["allow_subdomains"] = json!(allow);
    }
    // The external-links switch is UI-only: a Model proposal never carries it.
    if surface == Surface::Ui {
        if let Some(allow) = input.get("allowExternalLinks").or_else(|| input.get("allow_external_links")).and_then(Value::as_bool) {
            body["allow_external_links"] = json!(allow);
        }
    }
    // WS14.S6: the depth bound (zero takes the default; the ceiling is 10) and the pattern
    // lists, capped as the edge caps them.
    if let Some(depth) = input.get("maxDepth").or_else(|| input.get("max_depth")).and_then(Value::as_u64) {
        if depth > 0 {
            body["max_depth"] = json!(depth.min(CRAWL_MAX_DEPTH));
        }
    }
    let include = pattern_list(input, "includePaths", "include_paths")?;
    let exclude = pattern_list(input, "excludePaths", "exclude_paths")?;
    if include.len() + exclude.len() > CRAWL_MAX_PATTERNS {
        return Err(format!(
            "include and exclude patterns together are limited to {CRAWL_MAX_PATTERNS}."
        ));
    }
    let total: usize = include.iter().chain(exclude.iter()).map(|p| p.chars().count()).sum();
    if total > CRAWL_MAX_PATTERN_TOTAL_CHARS {
        return Err(format!(
            "patterns are limited to {CRAWL_MAX_PATTERN_TOTAL_CHARS} characters in total."
        ));
    }
    if !include.is_empty() {
        body["include_paths"] = json!(include);
    }
    if !exclude.is_empty() {
        body["exclude_paths"] = json!(exclude);
    }
    // INNH-6: sitemap discovery, and the cleaning options each page is scraped with.
    // Absent or null means the edge default.
    if let Some(sitemap) = input.get("sitemap").filter(|value| !value.is_null()) {
        let mode = sitemap
            .as_str()
            .filter(|mode| matches!(*mode, "skip" | "include" | "only"));
        let Some(mode) = mode else {
            return Err("sitemap must be skip, include or only.".to_owned());
        };
        body["sitemap"] = json!(mode);
    }
    if let Some(options) = input
        .get("scrape_options")
        .or_else(|| input.get("scrapeOptions"))
        .filter(|value| !value.is_null())
    {
        let options = scrape_options_body(options)?;
        if !options.is_empty() {
            body["scrape_options"] = Value::Object(options);
        }
    }

    Ok(body)
}

/// The formats a crawl page can return (`scrape_options.formats`).
const CRAWL_PAGE_FORMATS: [&str; 5] = ["markdown", "html", "rawHtml", "links", "metadata"];

/// The per-page cleaning options of a crawl (INNH-6), in snake case from either spelling.
/// Only the keys Quarry applies to each page pass. Anything else is refused by name, so a
/// crawl cannot carry a capture, an action or an AI format through `scrape_options`.
fn scrape_options_body(input: &Value) -> Result<serde_json::Map<String, Value>, String> {
    let object = input
        .as_object()
        .ok_or_else(|| "scrape_options must be an object.".to_owned())?;
    let mut out = serde_json::Map::new();
    for (key, value) in object {
        let field = match key.as_str() {
            "formats" => "formats",
            "extractor" => "extractor",
            "onlyMainContent" | "only_main_content" => "only_main_content",
            "includeTags" | "include_tags" => "include_tags",
            "excludeTags" | "exclude_tags" => "exclude_tags",
            "removeBase64Images" | "remove_base64_images" => "remove_base64_images",
            "timeoutMs" | "timeout_ms" => "timeout_ms",
            other => return Err(format!("scrape_options.{other} is not supported here.")),
        };
        let checked = match field {
            "formats" => {
                let formats = value
                    .as_array()
                    .ok_or_else(|| "scrape_options.formats must be a list.".to_owned())?;
                let mut kept = Vec::with_capacity(formats.len());
                for format in formats {
                    let name = format
                        .as_str()
                        .filter(|name| CRAWL_PAGE_FORMATS.contains(name))
                        .ok_or_else(|| {
                            format!("scrape_options.formats has an unsupported format: {format}.")
                        })?;
                    kept.push(json!(name));
                }
                Value::Array(kept)
            }
            "extractor" => {
                let name = value
                    .as_str()
                    .filter(|name| matches!(*name, "readability" | "clean_full" | "raw"))
                    .ok_or_else(|| {
                        "scrape_options.extractor must be readability, clean_full or raw.".to_owned()
                    })?;
                json!(name)
            }
            "only_main_content" | "remove_base64_images" => json!(value.as_bool().ok_or_else(|| {
                format!("scrape_options.{field} must be true or false.")
            })?),
            "include_tags" | "exclude_tags" => selector_list(value, field)?,
            _ => json!(value
                .as_u64()
                .filter(|ms| (1_000..=55_000).contains(ms))
                .ok_or_else(|| {
                    "scrape_options.timeout_ms must be between 1000 and 55000.".to_owned()
                })?),
        };
        out.insert(field.to_owned(), checked);
    }
    Ok(out)
}

/// A CSS selector list of a crawl page's cleaning options: at most 32 entries, each
/// non-blank and at most 256 characters.
fn selector_list(value: &Value, field: &str) -> Result<Value, String> {
    let items = value
        .as_array()
        .ok_or_else(|| format!("scrape_options.{field} must be a list of selectors."))?;
    if items.len() > 32 {
        return Err(format!("scrape_options.{field} is limited to 32 selectors."));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let selector = item
            .as_str()
            .map(str::trim)
            .filter(|selector| !selector.is_empty())
            .ok_or_else(|| format!("scrape_options.{field} entries must be non-blank strings."))?;
        if selector.chars().count() > 256 {
            return Err(format!(
                "scrape_options.{field} entries are limited to 256 characters."
            ));
        }
        out.push(json!(selector));
    }
    Ok(Value::Array(out))
}

/// Validate the curated subset a user picked from `/crawl/discover` into a
/// `{ "urls": [...] }` body for quarry's durable `/v1/batch`. Each URL is run
/// through the same SSRF-guarding normalizer as a single crawl, blanks/invalid
/// entries are dropped, duplicates collapse, and the set is capped so one batch
/// stays within quarry's body budget (it documents "a few hundred URLs"). Errors
/// when nothing valid remains — we never hand quarry an empty batch.
/// A list of crawl patterns from either spelling. Each entry is kept as sent, but may
/// not be blank or longer than the per-pattern cap.
fn pattern_list(input: &Value, camel: &str, snake: &str) -> Result<Vec<String>, String> {
    let Some(raw) = input.get(camel).or_else(|| input.get(snake)) else {
        return Ok(Vec::new());
    };
    if raw.is_null() {
        return Ok(Vec::new());
    }
    let items = raw
        .as_array()
        .ok_or_else(|| format!("{snake} must be a list of patterns."))?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let text = item
            .as_str()
            .ok_or_else(|| format!("{snake} entries must be strings."))?;
        if text.trim().is_empty() {
            return Err(format!("{snake} entries must not be empty."));
        }
        if text.chars().count() > CRAWL_MAX_PATTERN_CHARS {
            return Err(format!(
                "{snake} entries are limited to {CRAWL_MAX_PATTERN_CHARS} characters."
            ));
        }
        out.push(text.to_owned());
    }
    Ok(out)
}

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
        tracing::warn!(%status, "quarry extract fallback failed for scrape preview");
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

    /// WS13.5: only work that drives the browser gets the long ceiling.
    #[test]
    fn scrape_actions_timeout_budget_applies_to_browser_work_only() {
        let long = |body: Value| scrape_needs_browser_budget(&body);
        assert!(long(json!({ "url": "https://e.test/", "actions": [{ "type": "click" }] })));
        assert!(long(json!({ "formats": ["markdown", "screenshot"] })));
        assert!(long(json!({ "formats": ["pdf"] })));
        assert!(long(json!({ "formats": [{ "type": "screenshot", "fullPage": true }] })));
        assert!(!long(json!({ "url": "https://e.test/" })));
        assert!(!long(json!({ "url": "https://e.test/", "actions": [] })));
        assert!(!long(json!({ "formats": ["markdown", "html"] })));
        // INNH-1: the AI formats are model calls, so they share the long ceiling.
        assert!(long(json!({ "formats": ["markdown", "summary"] })));
        assert!(long(json!({ "formats": ["markdown", { "type": "json", "schema": { "type": "object" } }] })));
        assert!(long(json!({ "formats": [{ "type": "highlights", "query": "price" }] })));
        // The edge's route allows 125 s; this client must wait longer than that.
        assert!(SCRAPE_BROWSER_TIMEOUT > Duration::from_secs(125));
    }

    /// INNH-1: the AI format objects reach the edge exactly as the caller sent them,
    /// and an AI scrape runs under the long ceiling. The edge's answer (the inline
    /// `json`, `summary` and `ai` report) comes back unchanged too.
    #[tokio::test]
    async fn ai_format_objects_pass_through_unchanged_and_get_the_long_ceiling() {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/scrape"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(1500))
                    .set_body_json(json!({
                        "data": {
                            "status": 200,
                            "markdown": "# Price 2 190 NOK",
                            "json": { "title": "Ridge 40" },
                            "summary": "A gravel bike.",
                            "ai": {
                                "json": { "status": "ok", "untrusted_source": true, "schema_valid": true, "repair": "none" },
                                "total": { "calls": 1, "input_tokens": 10, "output_tokens": 4 }
                            }
                        }
                    })),
            )
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        state.client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let format_objects = json!([
            "markdown",
            { "type": "json", "schema": { "type": "object", "required": ["title"], "properties": { "title": { "type": "string" } } } },
            { "type": "summary", "max_chars": 300 },
        ]);
        let response = scrape(
            State(state),
            Extension(preview_user()),
            None,
            HeaderMap::new(),
            Json(json!({ "url": "https://example.com/p", "formats": format_objects.clone() })),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK, "an AI scrape outlasts the 1 s client default");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let answer: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(answer["data"]["json"]["title"], "Ridge 40");
        assert_eq!(answer["data"]["ai"]["total"]["calls"], 1);

        let received = quarry.received_requests().await.unwrap();
        let forwarded: Value = received
            .iter()
            .find(|request| request.url.path() == "/v1/scrape")
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .expect("the scrape reached the edge");
        assert_eq!(forwarded["formats"], format_objects, "the AI format objects are forwarded unchanged");
        assert!(forwarded.get("initiated_by").is_none(), "a person's scrape carries no origin stamp");
    }

    /// WS13.5: the origin stamp is tighten-only and comes from the verified caller.
    #[test]
    fn model_originated_scrape_carries_initiated_by_agent() {
        // A verified service principal is the agent whatever the body says.
        for stated in [None, Some("human"), Some("agent"), Some("anything")] {
            let mut body = json!({ "url": "https://e.test/", "actions": [{ "type": "wait" }] });
            if let Some(stated) = stated {
                body["initiated_by"] = json!(stated);
            }
            stamp_scrape_origin(&mut body, CallerClass::ServicePrincipal);
            assert_eq!(body["initiated_by"], "agent", "stated {stated:?}");
        }
        // A person's own call carries no hint, and a client-chosen value is dropped.
        let mut person = json!({ "url": "https://e.test/" });
        stamp_scrape_origin(&mut person, CallerClass::User);
        assert!(person.get("initiated_by").is_none());
        let mut forged = json!({ "url": "https://e.test/", "initiated_by": "human" });
        stamp_scrape_origin(&mut forged, CallerClass::User);
        assert!(forged.get("initiated_by").is_none(), "only the enum is forwarded");
        // A person may label their own request as the agent's: it can only tighten.
        let mut stated = json!({ "url": "https://e.test/", "initiated_by": "agent" });
        stamp_scrape_origin(&mut stated, CallerClass::User);
        assert_eq!(stated["initiated_by"], "agent");
    }

    /// WS13.5: end to end through the handler. The upstream is slower than a 1 s client
    /// default: a plain scrape times out (502), the same scrape with actions waits it out
    /// under the 125 s ceiling, and the forwarded body carries the stamp for a service
    /// principal only.
    #[tokio::test]
    async fn scrape_actions_timeout_is_per_request_and_the_stamp_reaches_the_edge() {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/scrape"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(1500))
                    .set_body_json(json!({ "data": { "status": 200 } })),
            )
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        state.client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let call = |body: Value, caller: Option<CallerClass>| {
            let state = state.clone();
            async move {
                scrape(
                    State(state),
                    Extension(preview_user()),
                    caller.map(Extension),
                    HeaderMap::new(),
                    Json(body),
                )
                .await
                .into_response()
                .status()
            }
        };

        let plain = call(json!({ "url": "https://example.com/" }), None).await;
        assert_eq!(plain, StatusCode::BAD_GATEWAY, "the client default still applies to a plain scrape");

        let with_actions = call(
            json!({ "url": "https://example.com/", "actions": [{ "type": "wait", "milliseconds": 100 }] }),
            Some(CallerClass::ServicePrincipal),
        )
        .await;
        assert_eq!(with_actions, StatusCode::OK, "an actions scrape runs under the long ceiling");

        let received = quarry.received_requests().await.unwrap();
        let bodies: Vec<Value> = received
            .iter()
            .filter(|request| request.url.path() == "/v1/scrape")
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect();
        let actions_body = bodies
            .iter()
            .find(|body| body.get("actions").is_some())
            .expect("the actions request reached the edge");
        assert_eq!(actions_body["initiated_by"], "agent");
        let plain_body = bodies
            .iter()
            .find(|body| body.get("actions").is_none())
            .expect("the plain request reached the edge");
        assert!(plain_body.get("initiated_by").is_none(), "a direct call carries no stamp");
    }

    /// WS14.S3: the scope switches are forwarded in snake case from either spelling, and
    /// are absent when the caller sends neither.
    #[test]
    fn crawl_body_forwards_scope_switches_in_both_spellings() {
        let camel = normalize_crawl_body(&json!({
            "url": "https://example.com/",
            "allowSubdomains": true,
            "allowExternalLinks": true,
        }))
        .unwrap();
        assert_eq!(camel["allow_subdomains"], json!(true));
        assert_eq!(camel["allow_external_links"], json!(true));

        let snake = normalize_crawl_body(&json!({
            "url": "https://example.com/",
            "allow_subdomains": false,
        }))
        .unwrap();
        assert_eq!(snake["allow_subdomains"], json!(false));
        assert!(snake.get("allow_external_links").is_none());
    }

    /// WS14.S6: a Model proposal carries neither the external-links switch nor any robots key.
    #[test]
    fn model_surface_drops_external_and_robots_keys() {
        let body = normalize_crawl_body_for(
            &json!({
                "url": "https://example.com/",
                "allowExternalLinks": true,
                "allow_external_links": true,
                "ignoreRobots": true,
                "ignore_robots_txt": true,
                "robots_mode": "ignore",
                "allowSubdomains": true,
            }),
            Surface::Model,
        )
        .unwrap();
        assert!(body.get("allow_external_links").is_none());
        assert!(body.as_object().unwrap().keys().all(|key| !key.contains("robot")));
        assert_eq!(body["allow_subdomains"], json!(true));
    }

    #[test]
    fn ui_surface_forwards_scope_fields() {
        let body = normalize_crawl_body_for(
            &json!({
                "url": "https://example.com/",
                "maxDepth": 99,
                "includePaths": ["/docs/*"],
                "excludePaths": ["re:^/blog/"],
                "allowExternalLinks": true,
            }),
            Surface::Ui,
        )
        .unwrap();
        assert_eq!(body["max_depth"], json!(CRAWL_MAX_DEPTH));
        assert_eq!(body["include_paths"], json!(["/docs/*"]));
        assert_eq!(body["exclude_paths"], json!(["re:^/blog/"]));
        assert_eq!(body["allow_external_links"], json!(true));
    }

    /// The action path keeps the SSRF guard: a private-address seed is refused before the call.
    #[test]
    fn crawl_site_action_goes_through_the_ssrf_normalizer() {
        assert!(normalize_crawl_body_for(&json!({ "url": "http://127.0.0.1/" }), Surface::Model).is_err());
        assert!(normalize_crawl_body_for(&json!({ "url": "http://localhost/" }), Surface::Model).is_err());
    }

    /// INNH-6: the sitemap mode and the per-page cleaning options reach the edge in snake
    /// case, from either spelling. An empty options object is left out.
    #[test]
    fn crawl_body_forwards_sitemap_and_page_options() {
        let body = normalize_crawl_body(&json!({
            "url": "https://example.com/",
            "sitemap": "include",
            "scrapeOptions": {
                "onlyMainContent": false,
                "includeTags": ["main", " article "],
                "excludeTags": [".cookie-banner"],
                "removeBase64Images": true,
                "timeoutMs": 20000,
                "extractor": "clean_full",
                "formats": ["markdown", "links"],
            },
        }))
        .unwrap();
        assert_eq!(body["sitemap"], json!("include"));
        assert_eq!(body["scrape_options"]["only_main_content"], json!(false));
        assert_eq!(body["scrape_options"]["include_tags"], json!(["main", "article"]));
        assert_eq!(body["scrape_options"]["exclude_tags"], json!([".cookie-banner"]));
        assert_eq!(body["scrape_options"]["remove_base64_images"], json!(true));
        assert_eq!(body["scrape_options"]["timeout_ms"], json!(20_000));
        assert_eq!(body["scrape_options"]["extractor"], json!("clean_full"));
        assert_eq!(body["scrape_options"]["formats"], json!(["markdown", "links"]));

        let plain = normalize_crawl_body(&json!({
            "url": "https://example.com/",
            "sitemap": "skip",
            "scrape_options": {},
        }))
        .unwrap();
        assert_eq!(plain["sitemap"], json!("skip"));
        assert!(plain.get("scrape_options").is_none());

        let absent = normalize_crawl_body(&json!({ "url": "https://example.com/", "sitemap": null })).unwrap();
        assert!(absent.get("sitemap").is_none());
    }

    /// INNH-6: every sitemap value and page option Quarry cannot apply is refused, by name.
    #[test]
    fn crawl_body_refuses_sitemap_and_page_options_it_cannot_apply() {
        let with = |key: &str, value: Value| {
            let mut input = json!({ "url": "https://example.com/" });
            input[key] = value;
            normalize_crawl_body(&input)
        };
        assert_eq!(
            with("sitemap", json!("all")).unwrap_err(),
            "sitemap must be skip, include or only."
        );
        assert_eq!(
            with("scrape_options", json!("main")).unwrap_err(),
            "scrape_options must be an object."
        );
        assert_eq!(
            with("scrape_options", json!({ "actions": [] })).unwrap_err(),
            "scrape_options.actions is not supported here."
        );
        assert!(with("scrape_options", json!({ "formats": ["screenshot"] })).unwrap_err().contains("screenshot"));
        assert!(with("scrape_options", json!({ "formats": "markdown" })).is_err());
        assert!(with("scrape_options", json!({ "include_tags": vec!["div"; 33] })).is_err());
        assert!(with("scrape_options", json!({ "exclude_tags": [" "] })).is_err());
        assert!(with("scrape_options", json!({ "include_tags": ["x".repeat(257)] })).is_err());
        assert!(with("scrape_options", json!({ "extractor": "magic" })).is_err());
        assert!(with("scrape_options", json!({ "timeout_ms": 999 })).is_err());
        assert!(with("scrape_options", json!({ "timeout_ms": 55_001 })).is_err());
        assert!(with("scrape_options", json!({ "only_main_content": "yes" })).is_err());
    }

    /// INNH-7: `max_age` and `cache` are not gateway fields. The scrape body passes through,
    /// so the edge receives the freshness window and the cache policy as the caller sent them.
    #[tokio::test]
    async fn scrape_forwards_max_age_and_cache_unchanged() {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "status": 200, "markdown": "# A", "effective_options": { "cache_state": "miss" } }
            })))
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        let response = scrape(
            State(state),
            Extension(preview_user()),
            None,
            HeaderMap::new(),
            Json(json!({
                "url": "https://example.com/p",
                "max_age": 120_000,
                "cache": { "max_age_s": 60 },
            })),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let received = quarry.received_requests().await.unwrap();
        let forwarded: Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(forwarded["max_age"], json!(120_000));
        assert_eq!(forwarded["cache"]["max_age_s"], json!(60));
        assert!(forwarded["cache"].get("mode").is_none());
    }

    #[test]
    fn max_pages_is_clamped_on_the_action_path() {
        let body = normalize_crawl_body_for(
            &json!({ "url": "https://example.com/", "maxPages": 999_999 }),
            Surface::Model,
        )
        .unwrap();
        assert_eq!(body["max_pages"], json!(5_000));
    }

    #[test]
    fn pattern_caps_match_the_edge_boundaries() {
        let at_cap: Vec<String> = (0..CRAWL_MAX_PATTERNS).map(|i| format!("/p{i}")).collect();
        assert!(normalize_crawl_body_for(&json!({ "url": "https://example.com/", "includePaths": at_cap }), Surface::Ui).is_ok());
        let over: Vec<String> = (0..=CRAWL_MAX_PATTERNS).map(|i| format!("/p{i}")).collect();
        assert!(normalize_crawl_body_for(&json!({ "url": "https://example.com/", "includePaths": over }), Surface::Ui).is_err());
        let long = "a".repeat(CRAWL_MAX_PATTERN_CHARS + 1);
        assert!(normalize_crawl_body_for(&json!({ "url": "https://example.com/", "includePaths": [long] }), Surface::Ui).is_err());
    }

    #[tokio::test]
    async fn start_crawl_maps_backlog_429() {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/crawl"))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                "request_id": "req_edge",
                "success": false,
                "error": {
                    "code": "RATE_LIMITED",
                    "message": "internal text that must not be echoed",
                    "details": {
                        "reason": "org_backlog_full", "limit": 200, "queued": 200,
                        "retry_after_seconds": 45, "internal": "postgres://leak"
                    }
                }
            })))
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        let user = AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        };

        let response = start_crawl(
            State(state),
            Extension(user),
            HeaderMap::new(),
            Json(json!({ "url": "https://example.com/" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get("retry-after").unwrap(), "45");
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "crawl_backlog_full");
        assert_eq!(body["error"]["retryAfterSeconds"], 45);
        assert_eq!(body["error"]["limit"], 200);
        assert_eq!(body["error"]["queued"], 200);
        let rendered = body.to_string();
        assert!(
            !rendered.contains("internal text") && !rendered.contains("postgres://leak"),
            "{rendered}"
        );
    }

    #[tokio::test]
    async fn start_crawl_maps_org_allowance_402() {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/crawl"))
            .respond_with(ResponseTemplate::new(402).set_body_json(json!({
                "request_id": "req_edge",
                "success": false,
                "error": {
                    "code": "BUDGET_EXHAUSTED",
                    "message": "internal text that must not be echoed",
                    "details": {
                        "dimension": "org_allowance", "metric": "quarry.crawl.page",
                        "limit_source": "entitlement", "retry_after_seconds": 3600,
                        "internal": "postgres://leak"
                    }
                }
            })))
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        let user = AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        };

        let response = start_crawl(
            State(state),
            Extension(user),
            HeaderMap::new(),
            Json(json!({ "url": "https://example.com/" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "crawl_allowance_exhausted");
        assert_eq!(body["error"]["metric"], "quarry.crawl.page");
        assert_eq!(body["error"]["retryAfterSeconds"], 3600);
        let rendered = body.to_string();
        assert!(
            !rendered.contains("internal text") && !rendered.contains("postgres://leak"),
            "{rendered}"
        );
    }

    #[test]
    fn only_the_org_allowance_402_is_remapped() {
        // Another budget dimension, a missing detail or a non-envelope body stays as it came.
        assert!(crawl_allowance_exhausted_response(
            &json!({ "error": { "details": { "dimension": "runtime" } } })
        )
        .is_none());
        assert!(crawl_allowance_exhausted_response(&json!({ "error": "pay up" })).is_none());
        assert!(crawl_allowance_exhausted_response(&json!({})).is_none());
        // An unknown metric and an absurd retry hint are dropped, not echoed.
        let response = crawl_allowance_exhausted_response(&json!({
            "error": { "details": {
                "dimension": "org_allowance", "metric": "postgres://leak", "retry_after_seconds": 99999999999u64
            } }
        }))
        .expect("typed");
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
    }

    #[test]
    fn only_the_backlog_429_is_remapped() {
        assert!(crawl_backlog_full_response(
            &json!({ "error": { "details": { "reason": "other" } } })
        )
        .is_none());
        assert!(crawl_backlog_full_response(&json!({ "error": "slow down" })).is_none());
        assert!(crawl_backlog_full_response(&json!({})).is_none());
        // An absent or absurd retry hint takes the default; nothing negative or huge is echoed.
        let response = crawl_backlog_full_response(&json!({
            "error": { "details": { "reason": "org_backlog_full", "retry_after_seconds": 99999999 } }
        }))
        .expect("typed");
        assert_eq!(response.headers().get("retry-after").unwrap(), "30");
    }

    #[tokio::test]
    async fn start_crawl_exposes_upstream_limits() {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/crawl"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "job_id": "job_1",
                    "accepted_at": "2026-01-01T00:00:00Z",
                    "limits": {
                        "max_pages": { "requested": 0, "effective": 1000, "clamped_by": ["default"] },
                        "max_depth": { "requested": null, "effective": 5, "clamped_by": ["default"] }
                    }
                }
            })))
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        let user = AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        };

        let response = start_crawl(
            State(state),
            Extension(user),
            HeaderMap::new(),
            Json(json!({ "url": "https://example.com/" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let limits = &value["data"]["limits"];
        assert_eq!(limits["max_pages"]["effective"], json!(1000));
        assert_eq!(limits["max_pages"]["clamped_by"], json!(["default"]));
        assert_eq!(limits["max_depth"]["effective"], json!(5));
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

    fn preview_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    async fn preview_body(response: Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// The preview asks for `formats: ["markdown"]`, so quarry-edge inlines the
    /// text and the second `/v1/extract` call has nothing left to do. The mock
    /// mounts no `/v1/extract`: a request to it would 404 and show up below.
    #[tokio::test]
    async fn preview_makes_one_quarry_call_when_markdown_is_inline() {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "status": 200,
                    "markdown": "# Inline\n\nContent",
                    "metadata": { "title": "Inline page", "content_kind": "html" },
                    "formats": { "markdown": { "artifact_id": "art-md", "bytes": 17 } }
                }
            })))
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();

        let response = scrape_preview(
            State(state),
            Extension(preview_user()),
            HeaderMap::new(),
            Json(json!({ "url": "https://example.com/a" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = preview_body(response).await;
        assert_eq!(value["data"]["source"], "inline");
        assert_eq!(value["data"]["markdown"], "# Inline\n\nContent");

        let requests = quarry.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1, "one quarry call per preview");
        assert_eq!(requests[0].url.path(), "/v1/scrape");
        let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(sent["formats"], json!(["markdown"]));
    }

    /// A caller that names its own formats keeps them.
    #[tokio::test]
    async fn preview_keeps_the_formats_the_caller_sent() {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "status": 200, "markdown": "# Hi" }
            })))
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();

        let response = scrape_preview(
            State(state),
            Extension(preview_user()),
            HeaderMap::new(),
            Json(json!({ "url": "https://example.com/a", "formats": ["markdown", "html"] })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let requests = quarry.received_requests().await.unwrap();
        let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(sent["formats"], json!(["markdown", "html"]));
    }

    #[test]
    fn a_415_is_classified_as_unsupported_media() {
        let (code, message) = classify_scrape_failure(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Duration::ZERO,
            &Value::Null,
        );
        assert_eq!(code, "unsupported_media");
        assert!(!message.is_empty());
        // A static sentence: no host, page text or upstream message in it.
        assert_no_proxy_or_leak(&message);
        // Even when the upstream body carries a message of its own, the
        // classifier does not echo it.
        let (_, with_body) = classify_scrape_failure(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Duration::ZERO,
            &json!({ "error": { "code": "UNSUPPORTED_MEDIA", "message": "image/png secret.example" } }),
        );
        assert_eq!(with_body, message);
        assert!(!with_body.contains("image/png"));
        // The neighbours are unchanged.
        assert_eq!(
            classify_scrape_failure(StatusCode::FORBIDDEN, Duration::ZERO, &Value::Null).0,
            "scrape_refused"
        );
        assert_eq!(
            classify_scrape_failure(StatusCode::BAD_GATEWAY, Duration::ZERO, &Value::Null).0,
            "scrape_failed"
        );
    }

    /// W3-E3.8.5: the fetch reasons Quarry names on a DRIVER_FAILED get their own code, also
    /// after a slow chain; any other reason keeps the status classification.
    #[test]
    fn a_driver_failed_fetch_reason_has_its_own_scrape_code() {
        let body = |reason: &str| {
            json!({ "error": { "code": "DRIVER_FAILED", "message": "dns error for secret.example", "details": { "reason": reason } } })
        };
        for (reason, want) in [
            ("dns", "scrape_host_not_found"),
            ("connect", "scrape_unreachable"),
            ("tls", "scrape_tls_failed"),
        ] {
            let (code, message) =
                classify_scrape_failure(StatusCode::BAD_GATEWAY, Duration::from_secs(25), &body(reason));
            assert_eq!(code, want, "{reason}");
            assert!(!message.contains("secret.example"), "{reason}: no upstream text");
        }
        assert_eq!(
            classify_scrape_failure(StatusCode::BAD_GATEWAY, Duration::ZERO, &body("all_drivers_failed")).0,
            "scrape_failed"
        );
        assert_eq!(
            classify_scrape_failure(StatusCode::BAD_GATEWAY, Duration::from_secs(25), &body("all_drivers_failed")).0,
            "scrape_timeout"
        );
    }

    /// End to end through the handler: quarry-edge's typed 415 reaches the
    /// caller as the same status with the typed code.
    #[tokio::test]
    async fn preview_of_an_unsupported_media_type_is_a_typed_415() {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(415).set_body_json(json!({
                "error": { "code": "UNSUPPORTED_MEDIA", "message": "image/png" }
            })))
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();

        let response = scrape_preview(
            State(state),
            Extension(preview_user()),
            HeaderMap::new(),
            Json(json!({ "url": "https://example.com/logo.png" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let value = preview_body(response).await;
        assert_eq!(value["error"]["code"], "unsupported_media");
        let message = value["error"]["message"].as_str().unwrap();
        assert_no_proxy_or_leak(message);
        assert!(
            !message.contains("image/png"),
            "the upstream message is not echoed: {message}"
        );
        assert_eq!(
            quarry.received_requests().await.unwrap().len(),
            1,
            "a typed refusal is final: no static retry"
        );
    }

    /// The formats default is set before the variant digest, so the cache key
    /// describes the request that is forwarded: a preview with no `formats`
    /// and one that names `["markdown"]` share ONE entry (the second is a
    /// hit, quarry is asked once), and `["html"]` is a different request with
    /// its own entry. Moving the default below the digest makes the first two
    /// differ, which fails the key and call counts below.
    #[tokio::test]
    async fn preview_cache_key_describes_the_forwarded_formats() {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org-1",
                "metadata": { "interactiveRetention": { "zdr": false } }
            })))
            .mount(&org_core)
            .await;
        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "status": 200, "markdown": "# Hi" }
            })))
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.org_core_url = org_core.uri();
        state.quarry_edge_url = quarry.uri();
        state.cache = crate::cache::ResultCache::in_memory();
        let mut user = preview_user();
        user.authorized_membership = Some(crate::middleware::AuthorizedMembership {
            organization_id: "org-1".to_owned(),
            role: "member".to_owned(),
        });

        let preview_keys = |state: &AppState| -> Vec<String> {
            state
                .cache
                .mem_keys()
                .into_iter()
                .filter(|key| key.contains("scrape-preview"))
                .collect()
        };
        let run = |body: Value| {
            let state = state.clone();
            let user = user.clone();
            async move {
                let response =
                    scrape_preview(State(state), Extension(user), HeaderMap::new(), Json(body))
                        .await;
                assert_eq!(response.status(), StatusCode::OK);
            }
        };

        run(json!({ "url": "https://example.com/a" })).await;
        let first = preview_keys(&state);
        assert_eq!(first.len(), 1, "the first preview is stored: {first:?}");
        assert_eq!(quarry.received_requests().await.unwrap().len(), 1);

        run(json!({ "url": "https://example.com/a", "formats": ["markdown"] })).await;
        assert_eq!(
            preview_keys(&state),
            first,
            "no formats and [markdown] are the same forwarded request: one key"
        );
        assert_eq!(
            quarry.received_requests().await.unwrap().len(),
            1,
            "the second preview is a cache hit"
        );

        run(json!({ "url": "https://example.com/a", "formats": ["html"] })).await;
        assert_eq!(
            preview_keys(&state).len(),
            2,
            "[html] is a different request with its own entry"
        );
        assert_eq!(quarry.received_requests().await.unwrap().len(), 2);
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

    // ---- scrape refusals: classify_scrape_failure reads details.block_reason (WS3.J) ----

    fn challenge_body(reason: Value) -> Value {
        json!({
            "success": false,
            "error": {
                "code": "CHALLENGE_DETECTED",
                "message": "blocked https://secret.example/private?q=1 <html>wall</html>",
                "retryable": false,
                "details": { "reason": reason.clone(), "block_reason": reason, "recovery": "not_applicable", "next": "other_source" }
            }
        })
    }

    fn assert_no_proxy_or_leak(message: &str) {
        let lowered = message.to_lowercase();
        assert!(!message.is_empty());
        assert!(!lowered.contains("proxy"), "no proxy suggestion: {message}");
        assert!(!lowered.contains("secret.example") && !lowered.contains("<html"), "no host or page text: {message}");
    }

    #[test]
    fn a_strong_block_reason_from_quarry_is_site_blocked() {
        for reason in ["captcha_widget", "waf_interstitial", "unexpected_bot_error", "challenge_loop"] {
            let (code, message) = classify_scrape_failure(
                StatusCode::FORBIDDEN,
                Duration::ZERO,
                &challenge_body(json!(reason)),
            );
            assert_eq!(code, "site_blocked", "{reason}");
            assert_no_proxy_or_leak(&message);
        }
    }

    #[test]
    fn a_bare_403_is_scrape_refused_not_site_blocked() {
        let bodies = [
            Value::Null,
            json!({}),
            json!({ "error": { "code": "FORBIDDEN", "message": "body_too_large" } }),
            json!({ "error": { "code": "SECURITY_BLOCKED", "message": "ssrf secret.example" } }),
            // a CHALLENGE_DETECTED that names no reason is not a verdict
            json!({ "error": { "code": "CHALLENGE_DETECTED", "details": {} } }),
        ];
        for body in bodies {
            let (code, message) = classify_scrape_failure(StatusCode::FORBIDDEN, Duration::ZERO, &body);
            assert_eq!(code, "scrape_refused", "{body}");
            assert_no_proxy_or_leak(&message);
        }
        // Slow or not, a refusal stays a refusal.
        assert_eq!(
            classify_scrape_failure(StatusCode::FORBIDDEN, Duration::from_secs(30), &Value::Null).0,
            "scrape_refused"
        );
    }

    #[test]
    fn an_unknown_or_malformed_block_reason_falls_back_to_scrape_refused() {
        let reasons = [
            json!("made_up_reason"),
            json!("https://secret.example/wall"),
            json!("WAF_INTERSTITIAL"),
            json!(""),
            json!(7),
            json!(null),
            json!(["captcha_widget"]),
        ];
        for reason in reasons {
            let body = challenge_body(reason.clone());
            let (code, message) = classify_scrape_failure(StatusCode::FORBIDDEN, Duration::ZERO, &body);
            assert_eq!(code, "scrape_refused", "{reason}");
            assert_no_proxy_or_leak(&message);
            assert!(scrape_block_reason(&body).is_none(), "{reason}");
        }
    }

    #[test]
    fn a_block_reason_counts_only_beside_the_challenge_code_and_a_403() {
        let other_code = json!({ "error": { "code": "RATE_LIMITED", "details": { "block_reason": "captcha_widget" } } });
        assert_eq!(
            classify_scrape_failure(StatusCode::FORBIDDEN, Duration::ZERO, &other_code).0,
            "scrape_refused"
        );
        // Only the `block_reason` field is read, not the agent path's `reason`.
        let reason_only = json!({ "error": { "code": "CHALLENGE_DETECTED", "details": { "reason": "captcha_widget" } } });
        assert_eq!(
            classify_scrape_failure(StatusCode::FORBIDDEN, Duration::ZERO, &reason_only).0,
            "scrape_refused"
        );
        // On any other status the reason does not turn a failure into a bot wall.
        let wall = challenge_body(json!("captcha_widget"));
        assert_eq!(classify_scrape_failure(StatusCode::TOO_MANY_REQUESTS, Duration::ZERO, &wall).0, "scrape_rate_limited");
        assert_eq!(classify_scrape_failure(StatusCode::BAD_GATEWAY, Duration::ZERO, &wall).0, "scrape_failed");
        assert_eq!(classify_scrape_failure(StatusCode::GATEWAY_TIMEOUT, Duration::ZERO, &wall).0, "scrape_timeout");
    }

    #[test]
    fn a_429_is_scrape_rate_limited_and_the_rest_keep_their_codes() {
        let (code, message) =
            classify_scrape_failure(StatusCode::TOO_MANY_REQUESTS, Duration::ZERO, &Value::Null);
        assert_eq!(code, "scrape_rate_limited");
        assert_no_proxy_or_leak(&message);
        assert_eq!(classify_scrape_failure(StatusCode::GATEWAY_TIMEOUT, Duration::ZERO, &Value::Null).0, "scrape_timeout");
        assert_eq!(classify_scrape_failure(StatusCode::BAD_GATEWAY, Duration::from_secs(21), &Value::Null).0, "scrape_timeout");
        assert_eq!(classify_scrape_failure(StatusCode::BAD_GATEWAY, Duration::ZERO, &Value::Null).0, "scrape_failed");
    }

    fn refusal_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    /// Run `scrape_preview` against a mock Quarry that answers `status` and
    /// `upstream`; returns the gateway's status, `Retry-After` and body text.
    async fn refused_preview(status: u16, upstream: Value) -> (StatusCode, Option<String>, String, usize) {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(status).set_body_json(upstream))
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        let response = scrape_preview(
            State(state),
            Extension(refusal_user()),
            HeaderMap::new(),
            Json(json!({ "url": "https://example.com/a" })),
        )
        .await;
        let status = response.status();
        let retry_after = response
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let calls = quarry.received_requests().await.unwrap().len();
        (status, retry_after, String::from_utf8(bytes.to_vec()).unwrap(), calls)
    }

    #[tokio::test]
    async fn a_scrape_the_site_walled_reaches_the_caller_as_site_blocked_with_the_typed_reason_only() {
        let (status, retry_after, text, calls) =
            refused_preview(403, challenge_body(json!("waf_interstitial"))).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(retry_after.is_none());
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["error"]["code"], "site_blocked");
        assert_eq!(body["error"]["blockReason"], "waf_interstitial");
        assert_no_proxy_or_leak(body["error"]["message"].as_str().unwrap());
        assert!(!text.contains("secret.example") && !text.contains("CHALLENGE_DETECTED") && !text.contains("<html"), "{text}");
        assert_eq!(calls, 1, "a refusal is final: no static retry");
    }

    #[tokio::test]
    async fn a_bare_403_scrape_reaches_the_caller_as_scrape_refused_without_a_block_reason() {
        let (status, _, text, calls) = refused_preview(
            403,
            json!({ "error": { "code": "FORBIDDEN", "message": "denied by https://secret.example" } }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["error"]["code"], "scrape_refused");
        assert!(body["error"].get("blockReason").is_none());
        assert!(!text.contains("secret.example"), "{text}");
        assert_eq!(calls, 1);

        // A made-up reason is dropped, not echoed.
        let (_, _, text, _) = refused_preview(403, challenge_body(json!("https://secret.example/x"))).await;
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["error"]["code"], "scrape_refused");
        assert!(body["error"].get("blockReason").is_none());
        assert!(!text.contains("secret.example"), "{text}");
    }

    #[tokio::test]
    async fn a_429_scrape_carries_retry_after_only_when_quarry_sent_a_sane_number() {
        let upstream = |seconds: Value| json!({
            "error": { "code": "RATE_LIMITED", "message": "secret.example", "details": { "reason": "org_render_cap", "retry_after_seconds": seconds } }
        });
        let (status, retry_after, text, _) = refused_preview(429, upstream(json!(17))).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(retry_after.as_deref(), Some("17"));
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["error"]["code"], "scrape_rate_limited");
        assert_eq!(body["error"]["retryAfterSeconds"], 17);
        assert!(!text.contains("secret.example") && !text.contains("org_render_cap"), "{text}");

        for bad in [json!(0), json!(99999), json!("soon"), json!(null)] {
            let (_, retry_after, text, _) = refused_preview(429, upstream(bad.clone())).await;
            assert!(retry_after.is_none(), "{bad}");
            let body: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(body["error"]["code"], "scrape_rate_limited");
            assert!(body["error"].get("retryAfterSeconds").is_none(), "{bad}");
        }
    }

    // The body quarry-edge really sends for a /v1/scrape RATE_LIMITED
    // (`ApiError::rate_limited`): flat, `error` is a string, the hint is top-level.
    #[tokio::test]
    async fn a_429_scrape_in_quarry_edges_flat_shape_carries_its_retry_hint() {
        let flat = |seconds: Value| json!({
            "error": "rate limited on secret.example",
            "code": "RATE_LIMITED",
            "hint": "reduce request rate or retry after the indicated window",
            "window": "1m",
            "retry_after_seconds": seconds,
            "next_actions": ["retry after 60 seconds", "reduce concurrent request rate"],
        });
        let (status, retry_after, text, calls) = refused_preview(429, flat(json!(60))).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(retry_after.as_deref(), Some("60"));
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["error"]["code"], "scrape_rate_limited");
        assert_eq!(body["error"]["retryAfterSeconds"], 60);
        assert!(
            !text.contains("secret.example") && !text.contains("next_actions") && !text.contains("window"),
            "{text}"
        );
        assert_eq!(calls, 1);

        for bad in [json!(0), json!(3601), json!("soon"), json!(null)] {
            let (_, retry_after, text, _) = refused_preview(429, flat(bad.clone())).await;
            assert!(retry_after.is_none(), "{bad}");
            let body: Value = serde_json::from_str(&text).unwrap();
            assert!(body["error"].get("retryAfterSeconds").is_none(), "{bad}");
        }
        // The envelope wins when both are present.
        let both = json!({
            "error": { "code": "RATE_LIMITED", "details": { "retry_after_seconds": 5 } },
            "retry_after_seconds": 60,
        });
        assert_eq!(scrape_retry_after_seconds(&both), Some(5));
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
        None,
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
        None,
    )
    .await
}

// ---- Job lifecycle and control proxy (WS15.L5) ---------------------------------
//
// The six routes below carry a person's (or a Model proposal's) read and control
// calls for one crawl to quarry-edge's `/v1/crawl/{id}` family. They use the same
// session-bound `aud=quarry` token as every other Quarry leg, so the organization
// comes from the token's signed claims; the only scoping header sent is the
// `x-user-id` the proxy helper derives from the session. No browser header and no
// browser query parameter other than the two paging numbers below is forwarded, so
// a caller cannot name another organization or user. The edge keeps the control
// verbs behind `QUARRY_EDGE__JOB_CONTROL`; while that is off it answers a typed
// 501 and this proxy returns it unchanged.

/// A job id is `job_<ulid>`: ASCII letters, digits, `_` and `-` only. Anything else
/// (a `.`, which could climb a path segment, a `/`, `%`, a space) is refused before
/// an upstream call is built.
fn job_id_is_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn invalid_job_id() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error("invalid_job_id", "The job id is not valid.")),
    )
        .into_response()
}

/// The query of the errors read: `after_seq` pages by event sequence and `limit`
/// caps the page. Both must be plain digits; every other parameter is dropped.
fn errors_query(uri: &Uri) -> String {
    let mut kept: Vec<String> = Vec::new();
    for pair in uri.query().unwrap_or_default().split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if matches!(key, "after_seq" | "limit")
            && !value.is_empty()
            && value.len() <= 12
            && value.bytes().all(|b| b.is_ascii_digit())
            && !kept.iter().any(|k| k.starts_with(&format!("{key}=")))
        {
            kept.push(format!("{key}={value}"));
        }
    }
    if kept.is_empty() {
        String::new()
    } else {
        format!("?{}", kept.join("&"))
    }
}

/// One call to the edge's crawl family, answered as the edge answered it.
async fn crawl_edge_call(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    path: String,
) -> Response {
    let cookie = shared::cookie_header(headers);
    let token = shared::quarry_token(state, user, &cookie).await;
    let url = format!("{}{}", state.quarry_edge_url, path);
    let (status, Json(body)) =
        proxy_bearer_json(state, method, &url, None, token.as_deref(), &user.user_id).await;
    (status, Json(body)).into_response()
}

/// `GET /api/v1/knowledge/crawl/{id}`: the status of one crawl.
pub(crate) async fn crawl_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !job_id_is_valid(&id) {
        return invalid_job_id();
    }
    let path = format!("/v1/crawl/{}", urlencoding::encode(&id));
    crawl_edge_call(&state, &user, &headers, Method::GET, path).await
}

/// `GET /api/v1/knowledge/crawl/{id}/errors`: the pages of the crawl that failed.
pub(crate) async fn crawl_errors(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    uri: Uri,
) -> Response {
    if !job_id_is_valid(&id) {
        return invalid_job_id();
    }
    let path = format!(
        "/v1/crawl/{}/errors{}",
        urlencoding::encode(&id),
        errors_query(&uri)
    );
    crawl_edge_call(&state, &user, &headers, Method::GET, path).await
}

/// `GET /api/v1/knowledge/crawl/ongoing`: the organization's crawls still in flight.
pub(crate) async fn crawl_ongoing(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    crawl_edge_call(
        &state,
        &user,
        &headers,
        Method::GET,
        "/v1/crawl/ongoing".to_owned(),
    )
    .await
}

/// `DELETE /api/v1/knowledge/crawl/{id}`: stop a crawl for good.
pub(crate) async fn crawl_cancel(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !job_id_is_valid(&id) {
        return invalid_job_id();
    }
    let path = format!("/v1/crawl/{}", urlencoding::encode(&id));
    crawl_edge_call(&state, &user, &headers, Method::DELETE, path).await
}

/// `POST /api/v1/knowledge/crawl/{id}/pause`.
pub(crate) async fn crawl_pause(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !job_id_is_valid(&id) {
        return invalid_job_id();
    }
    let path = format!("/v1/crawl/{}/pause", urlencoding::encode(&id));
    crawl_edge_call(&state, &user, &headers, Method::POST, path).await
}

/// `POST /api/v1/knowledge/crawl/{id}/resume`.
pub(crate) async fn crawl_resume(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !job_id_is_valid(&id) {
        return invalid_job_id();
    }
    let path = format!("/v1/crawl/{}/resume", urlencoding::encode(&id));
    crawl_edge_call(&state, &user, &headers, Method::POST, path).await
}

/// WS15.L3: the knowledge job-event proxy forwards `follow=true` in the query and
/// the caller's `Last-Event-ID` header to quarry-edge, and returns the upstream
/// SSE bytes unchanged.
#[cfg(test)]
mod job_event_tests {
    use super::*;
    use wiremock::matchers::{
        header as wm_header, method as wm_method, path as wm_path, query_param,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const JOB: &str = "job_01J9SSSSSSSSSSSSSSSSSSSSSS";

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    #[tokio::test]
    async fn crawl_job_events_forwards_follow_and_last_event_id() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("/v1/jobs/{JOB}/events")))
            .and(query_param("follow", "true"))
            .and(wm_header("last-event-id", "7"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string("id: 8\nevent: page_fetched\ndata: {}\n\ndata: done\n\n"),
            )
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();

        let uri: Uri = format!("/api/v1/knowledge/jobs/{JOB}/events?follow=true")
            .parse()
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", "7".parse().unwrap());
        let response = crawl_job_events(
            State(state),
            Extension(user()),
            headers,
            Path(JOB.to_owned()),
            uri,
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body = String::from_utf8_lossy(&bytes);
        assert!(body.contains("id: 8"), "{body}");
        assert!(body.contains("data: done"), "{body}");
    }
}

#[cfg(test)]
mod scrape_artifact_tests {
    use super::*;
    use axum::body::to_bytes;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn caller() -> AuthenticatedUser {
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

    /// State whose org-core answers the org's ZDR posture and whose Quarry is the
    /// given mock. The posture lookup is mocked because an unknown posture counts
    /// as ZDR (fail closed).
    async fn state_for(quarry: &MockServer, zdr: bool) -> (AppState, MockServer) {
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
        state.quarry_edge_url = quarry.uri();
        (state, org_core)
    }

    async fn body_of(response: Response) -> (StatusCode, HeaderMap, Value) {
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let body = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
        (status, headers, body)
    }

    #[tokio::test]
    async fn scrape_artifact_serves_png_with_nosniff() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/artifacts/art_01"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/png")
                    .set_body_bytes(vec![0x89, b'P', b'N', b'G']),
            )
            .mount(&quarry)
            .await;
        let (state, _org_core) = state_for(&quarry, false).await;

        let response = scrape_artifact(
            State(state),
            Extension(caller()),
            HeaderMap::new(),
            Path("art_01".to_owned()),
        )
        .await;
        let (status, headers, _) = body_of(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("content-type").unwrap(), "image/png");
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(headers.get("cache-control").unwrap(), "private, no-store");
        assert!(headers.get("content-disposition").is_none());
    }

    #[tokio::test]
    async fn scrape_artifact_serves_pdf_as_attachment_with_csp_sandbox() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/artifacts/art_02"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/pdf")
                    .set_body_bytes(b"%PDF-1.7\n".to_vec()),
            )
            .mount(&quarry)
            .await;
        let (state, _org_core) = state_for(&quarry, false).await;

        let response = scrape_artifact(
            State(state),
            Extension(caller()),
            HeaderMap::new(),
            Path("art_02".to_owned()),
        )
        .await;
        let (status, headers, _) = body_of(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("content-type").unwrap(), "application/pdf");
        assert_eq!(
            headers.get("content-disposition").unwrap(),
            r#"attachment; filename="page.pdf""#
        );
        assert_eq!(headers.get("content-security-policy").unwrap(), "sandbox");
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
    }

    #[tokio::test]
    async fn scrape_artifact_rejects_unknown_content_type_with_415() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/artifacts/art_03"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html")
                    .set_body_bytes(b"<script>x</script>".to_vec()),
            )
            .mount(&quarry)
            .await;
        let (state, _org_core) = state_for(&quarry, false).await;

        let response = scrape_artifact(
            State(state),
            Extension(caller()),
            HeaderMap::new(),
            Path("art_03".to_owned()),
        )
        .await;
        let (status, _, body) = body_of(response).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(body["error"]["code"], "scrape_artifact_unsupported");
    }

    #[tokio::test]
    async fn scrape_artifact_never_falls_back_to_image_png() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/artifacts/art_04"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/octet-stream")
                    .set_body_bytes(b"\x89PNG\r\n\x1a\n".to_vec()),
            )
            .mount(&quarry)
            .await;
        let (state, _org_core) = state_for(&quarry, false).await;

        let response = scrape_artifact(
            State(state),
            Extension(caller()),
            HeaderMap::new(),
            Path("art_04".to_owned()),
        )
        .await;
        let (status, headers, _) = body_of(response).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        // The refusal is a JSON error, never the PNG bytes under an image type.
        assert!(!headers.get("content-type").is_some_and(|value| value == "image/png"));
    }

    #[tokio::test]
    async fn foreign_org_artifact_is_normalised_to_not_found() {
        // Quarry binds the artifact to the caller's org and answers 404 for another
        // tenant's id; the gateway returns the same not-found shape as a missing one.
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/artifacts/art_05"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": { "code": "NOT_FOUND", "message": "internal org-2 detail" }
            })))
            .mount(&quarry)
            .await;
        let (state, _org_core) = state_for(&quarry, false).await;

        let response = scrape_artifact(
            State(state),
            Extension(caller()),
            HeaderMap::new(),
            Path("art_05".to_owned()),
        )
        .await;
        let (status, _, body) = body_of(response).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "scrape_artifact_not_found");
        assert!(!body.to_string().contains("org-2"));
    }

    #[tokio::test]
    async fn zdr_org_gets_withheld() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/artifacts/art_06"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&quarry)
            .await;
        let (state, _org_core) = state_for(&quarry, true).await;

        let response = scrape_artifact(
            State(state),
            Extension(caller()),
            HeaderMap::new(),
            Path("art_06".to_owned()),
        )
        .await;
        let (status, _, body) = body_of(response).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "browser_artifact_withheld");
        quarry.verify().await;
    }

    #[tokio::test]
    async fn invalid_artifact_id_is_400() {
        let quarry = MockServer::start().await;
        let (state, _org_core) = state_for(&quarry, false).await;

        let response = scrape_artifact(
            State(state),
            Extension(caller()),
            HeaderMap::new(),
            Path("../etc/passwd".to_owned()),
        )
        .await;
        let (status, _, body) = body_of(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_scrape_artifact");
    }

    #[test]
    fn scrape_preview_strips_actions_and_capture_keys_before_cache_and_retry() {
        let mut body = json!({
            "url": "https://example.com/",
            "formats": ["markdown", "screenshot", { "type": "pdf" }],
            "actions": [{ "type": "click" }],
            "actionsOnError": true,
            "viewport": { "width": 1280, "height": 720 },
            "capture": { "screenshot": true }
        });
        strip_preview_only_fields(&mut body);
        assert!(body.get("actions").is_none());
        assert!(body.get("actionsOnError").is_none());
        assert!(body.get("viewport").is_none());
        assert!(body.get("capture").is_none());
        assert_eq!(body["formats"], json!(["markdown"]));

        // The retry body is built from the stripped body, so it cannot replay them.
        let retry = without_browser_rendering(&body);
        assert!(retry.get("actions").is_none());
        assert!(retry.get("viewport").is_none());
    }

    #[test]
    fn scrape_preview_cache_key_is_unchanged_for_plain_bodies() {
        let original = json!({ "url": "https://example.com/", "formats": ["markdown"] });
        let mut body = original.clone();
        strip_preview_only_fields(&mut body);
        assert_eq!(body, original);
        assert_eq!(variant_digest(&body), variant_digest(&original));
    }

    #[test]
    fn scrape_preview_drops_formats_list_that_only_held_captures() {
        let mut body = json!({ "url": "https://example.com/", "formats": ["pdf"] });
        strip_preview_only_fields(&mut body);
        assert!(body.get("formats").is_none());
    }
}

/// WS15.L5: the crawl status, errors, ongoing and control proxy. Each handler reaches
/// the edge's `/v1/crawl` family with the session user, forwards nothing the browser
/// sent, and returns the edge's answer (a 501 included) unchanged.
#[cfg(test)]
mod job_control_tests {
    use super::*;
    use wiremock::matchers::{header as wm_header, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const JOB: &str = "job_01J9SSSSSSSSSSSSSSSSSSSSSS";

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    fn state_for(quarry: &MockServer) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        state
    }

    async fn body_json(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[test]
    fn a_job_id_is_letters_digits_underscore_and_dash_only() {
        assert!(job_id_is_valid(JOB));
        assert!(job_id_is_valid("run-1_A"));
        for bad in ["", "..", "a.b", "a/b", "a%2Fb", "a b", "a?x=1", "a:b", "ö"] {
            assert!(!job_id_is_valid(bad), "{bad:?}");
        }
        assert!(job_id_is_valid(&"a".repeat(128)));
        assert!(!job_id_is_valid(&"a".repeat(129)));
    }

    #[test]
    fn the_errors_query_keeps_only_two_numeric_paging_parameters() {
        let q = |s: &str| errors_query(&s.parse::<Uri>().unwrap());
        assert_eq!(q("/x"), "");
        assert_eq!(q("/x?after_seq=5&limit=10"), "?after_seq=5&limit=10");
        assert_eq!(q("/x?limit=10&org_id=org-2&after_seq=5"), "?limit=10&after_seq=5");
        assert_eq!(q("/x?org_id=org-2&user_id=u"), "");
        assert_eq!(q("/x?limit=abc"), "");
        assert_eq!(q("/x?limit=-1"), "");
        assert_eq!(q("/x?limit="), "");
        assert_eq!(q("/x?limit=1&limit=2"), "?limit=1");
        assert_eq!(q("/x?limit=1234567890123"), "");
    }

    #[tokio::test]
    async fn status_reads_the_crawl_with_the_session_user_and_returns_the_edge_answer() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("/v1/crawl/{JOB}")))
            .and(wm_header("x-user-id", "user-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "job_id": JOB, "kind": "crawl", "status": "running", "phase": "fetching" }
            })))
            .expect(1)
            .mount(&quarry)
            .await;

        let response = crawl_status(
            State(state_for(&quarry)),
            Extension(user()),
            HeaderMap::new(),
            Path(JOB.to_owned()),
        )
        .await;
        let (status, body) = body_json(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["phase"], "fetching");
    }

    #[tokio::test]
    async fn no_browser_header_or_foreign_scope_reaches_the_edge() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("/v1/crawl/{JOB}/errors")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "items": [] } })))
            .mount(&quarry)
            .await;

        let mut headers = HeaderMap::new();
        headers.insert("x-user-id", "attacker".parse().unwrap());
        headers.insert("x-org-id", "org-2".parse().unwrap());
        headers.insert("x-organization-id", "org-2".parse().unwrap());
        headers.insert("authorization", "Bearer forged".parse().unwrap());
        let uri: Uri = format!("/api/v1/knowledge/crawl/{JOB}/errors?after_seq=7&limit=20&org_id=org-2")
            .parse()
            .unwrap();
        let response = crawl_errors(
            State(state_for(&quarry)),
            Extension(user()),
            headers,
            Path(JOB.to_owned()),
            uri,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let seen = quarry.received_requests().await.unwrap();
        assert_eq!(seen.len(), 1);
        let request = &seen[0];
        assert_eq!(request.url.query(), Some("after_seq=7&limit=20"));
        assert_eq!(request.headers.get("x-user-id").unwrap(), "user-1");
        assert!(request.headers.get("x-org-id").is_none());
        assert!(request.headers.get("x-organization-id").is_none());
        // The forged bearer is not the one sent: with no minted token there is none.
        assert_ne!(
            request.headers.get("authorization").map(|v| v.to_str().unwrap_or("")),
            Some("Bearer forged")
        );
    }

    #[tokio::test]
    async fn ongoing_reads_the_static_route_of_the_edge() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/crawl/ongoing"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "items": [] } })))
            .expect(1)
            .mount(&quarry)
            .await;
        let response =
            crawl_ongoing(State(state_for(&quarry)), Extension(user()), HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn pause_resume_and_cancel_use_the_edge_verbs() {
        let quarry = MockServer::start().await;
        let accepted = ResponseTemplate::new(200).set_body_json(json!({
            "data": { "job_id": JOB, "phase": "pausing", "control": { "desired": "pause", "pending": true } }
        }));
        for (verb, route) in [
            ("POST", format!("/v1/crawl/{JOB}/pause")),
            ("POST", format!("/v1/crawl/{JOB}/resume")),
            ("DELETE", format!("/v1/crawl/{JOB}")),
        ] {
            Mock::given(wm_method(verb))
                .and(wm_path(route))
                .and(wm_header("x-user-id", "user-1"))
                .respond_with(accepted.clone())
                .expect(1)
                .mount(&quarry)
                .await;
        }
        let state = state_for(&quarry);

        let paused = crawl_pause(
            State(state.clone()),
            Extension(user()),
            HeaderMap::new(),
            Path(JOB.to_owned()),
        )
        .await;
        let resumed = crawl_resume(
            State(state.clone()),
            Extension(user()),
            HeaderMap::new(),
            Path(JOB.to_owned()),
        )
        .await;
        let cancelled = crawl_cancel(
            State(state),
            Extension(user()),
            HeaderMap::new(),
            Path(JOB.to_owned()),
        )
        .await;
        for response in [paused, resumed, cancelled] {
            let (status, body) = body_json(response).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["data"]["control"]["pending"], true);
        }
    }

    #[tokio::test]
    async fn the_edges_typed_501_while_job_control_is_off_reaches_the_caller() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path(format!("/v1/crawl/{JOB}/pause")))
            .respond_with(ResponseTemplate::new(501).set_body_json(json!({
                "ok": false,
                "error": { "code": "UNSUPPORTED", "message": "job control is not enabled" }
            })))
            .mount(&quarry)
            .await;
        let response = crawl_pause(
            State(state_for(&quarry)),
            Extension(user()),
            HeaderMap::new(),
            Path(JOB.to_owned()),
        )
        .await;
        let (status, body) = body_json(response).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["error"]["code"], "UNSUPPORTED");
    }

    #[tokio::test]
    async fn an_unsafe_id_is_refused_before_any_upstream_call() {
        let quarry = MockServer::start().await;
        let state = state_for(&quarry);
        for bad in ["..", "a.b", "a/b", "", "x y"] {
            let responses = vec![
                crawl_status(
                    State(state.clone()),
                    Extension(user()),
                    HeaderMap::new(),
                    Path(bad.to_owned()),
                )
                .await,
                crawl_errors(
                    State(state.clone()),
                    Extension(user()),
                    HeaderMap::new(),
                    Path(bad.to_owned()),
                    "/x".parse().unwrap(),
                )
                .await,
                crawl_cancel(
                    State(state.clone()),
                    Extension(user()),
                    HeaderMap::new(),
                    Path(bad.to_owned()),
                )
                .await,
                crawl_pause(
                    State(state.clone()),
                    Extension(user()),
                    HeaderMap::new(),
                    Path(bad.to_owned()),
                )
                .await,
                crawl_resume(
                    State(state.clone()),
                    Extension(user()),
                    HeaderMap::new(),
                    Path(bad.to_owned()),
                )
                .await,
            ];
            for response in responses {
                let (status, body) = body_json(response).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}");
                assert_eq!(body["error"]["code"], "invalid_job_id");
            }
        }
        assert!(quarry.received_requests().await.unwrap().is_empty());
    }
}
