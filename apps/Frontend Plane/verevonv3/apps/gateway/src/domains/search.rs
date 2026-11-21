//! Unified search domain — the home dashboard "Søk" surface.
//!
//! All web search infrastructure lives inside quarry-edge (its SmartSearchRouter
//! fans out across local Tantivy → SearXNG → Brave, and the AnswerPipeline is the
//! Tavily-replacement synthesizer). This domain is a thin, normalizing proxy:
//!
//! - `web`           → quarry-edge `/v1/search` (keyword) or `/v1/scrape` (URL input)
//! - `images`        → quarry-edge `/v1/search/images`
//! - `videos`        → quarry-edge `/v1/search/videos` (embeddable iframe results)
//! - `answer/stream` → quarry-edge `/v1/answer/stream` (SSE: citations/delta/done)
//! - `suggestions`   → autocomplete-core `/v1/suggestions` (degrades to empty)
//!
//! Responses are wrapped in the gateway `{data}` envelope; the SPA's `requestJson`
//! unwraps it, yielding the same shapes verevonv2 returned.

use std::{collections::HashMap, time::Duration};

use axum::{
    extract::{Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    audience_tokens::get_audience_token,
    cache_admission::CacheAdmission,
    config::AppState,
    domains::web_domain_policy as domain_policy,
    envelope::{error, ok},
    middleware::{require_session, AuthenticatedUser},
    rate_limit::{with_rate_class, RateClass},
    upstream::proxy_sse_stream,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    let routes = Router::new()
        .route("/api/v1/search/web", post(search_web))
        .route("/api/v1/search/similar", post(search_similar))
        .route("/api/v1/search/suggest", post(search_suggest))
        .route("/api/v1/search/images", post(search_images))
        .route("/api/v1/search/videos", post(search_videos))
        .route("/api/v1/search/suggestions", get(search_suggestions))
        .route("/api/v1/search/answer/stream", post(search_answer_stream));
    // Search and scrape share one per-organization bucket (see
    // `rate_limit::RateClass`).
    with_rate_class(routes, &state.rate_limiter, RateClass::SearchScrape)
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// `POST /api/v1/search/web` — keyword search, or a single-page fetch when the
/// query is a URL / bare domain. Mirrors verevonv2's `search/web` route.
pub(crate) async fn search_web(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let query = body
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if query.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_query", "A non-empty query is required.")),
        );
    }

    let limit = body
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(8)
        .clamp(1, 50);
    let public_only = body
        .get("publicOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_answer = !public_only
        && body
            .get("includeAnswer")
            .or_else(|| body.get("include_answer"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
    // Exa-style filters (accepted in camelCase or snake_case) → forwarded to
    // quarry-edge's SmartSearchRouter, which maps them to each provider's
    // native params (topic→category, time_range→freshness) and `site:`
    // operators (domains). Empty/absent filters are simply omitted.
    let topic = body
        .get("topic")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let time_range = body
        .get("timeRange")
        .or_else(|| body.get("time_range"))
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let days = body.get("days").and_then(Value::as_u64);
    let exact_match = body
        .get("exactMatch")
        .or_else(|| body.get("exact_match"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // Two-letter market (`NO`) and language (`no`) hints; quarry-edge applies them
    // to the provider query. Blank values are dropped, never forwarded.
    let country = body
        .get("country")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| s.len() == 2 && s.chars().all(|c| c.is_ascii_alphabetic()));
    let language = body
        .get("language")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| (2..=3).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphabetic()));
    let include_domains = string_array(
        body.get("includeDomains")
            .or_else(|| body.get("include_domains")),
    );
    let exclude_domains = string_array(
        body.get("excludeDomains")
            .or_else(|| body.get("exclude_domains")),
    );

    let token = quarry_token(&state, &user, &headers).await;

    // Cache admission and the ZDR posture, resolved once and server-side: the
    // request flag, the live org membership and org-core's standing posture.
    // The cached search body is built by quarry-edge from the caller's
    // ORG-SCOPED private corpus, so entries are keyed by org and a ZDR caller
    // gets no cache in either direction. When ZDR is established it is also
    // forwarded to quarry-edge, which can only raise it.
    let admission = CacheAdmission::resolve(&state, &user, &headers, &body, false).await;
    admission.record_bypass("search-web");
    let zdr = admission.zdr_established();

    // URL / bare-domain path → scrape the page so the searchbar can "open" links.
    if !public_only && is_url_input(&query) {
        let target = normalize_url(&query);
        // O-23: a URL or bare-domain query is a single-page fetch, so the org's rules judge its host before
        // any upstream call. Off (the default), nothing is asked.
        if let Some(refusal) = refuse_blocked_target(&state, &user, &headers, &target).await {
            return refusal;
        }
        let url = format!("{}/v1/scrape", state.quarry_edge_url);
        let mut browser_payload = json!({
            "url": target,
            "formats": ["markdown"],
            "signals": {
                "actions": ["render_page"],
                "screenshot": false,
                "pdf": false,
                "prior_block_signals": 0,
                "profile_required": false,
                "url_type": "Default",
            },
            "render": {
                "waitForTimeoutMs": 1800,
            },
        });
        let mut static_payload = json!({
            "url": target,
            "formats": ["markdown"],
        });
        if zdr {
            browser_payload["zdr"] = json!(true);
            static_payload["zdr"] = json!(true);
        }
        let data = match post_quarry(
            &state,
            &url,
            token.as_deref(),
            &user.user_id,
            &browser_payload,
            Duration::from_secs(20),
        )
        .await
        {
            Ok(data) => data,
            // Browser render unavailable/broken (driver missing, or a present-but-
            // broken headless Chrome that crashes/drops mid-render → 5xx) → fall
            // back to a static fetch rather than failing the searchbar fetch.
            Err((status, Json(err_body)))
                if browser_driver_unavailable(&err_body) || status.is_server_error() =>
            {
                tracing::warn!(
                    %status,
                    "quarry browser render failed; retrying URL search fetch as a static fetch"
                );
                match post_quarry(
                    &state,
                    &url,
                    token.as_deref(),
                    &user.user_id,
                    &static_payload,
                    Duration::from_secs(20),
                )
                .await
                {
                    Ok(data) => data,
                    Err(envelope) => return envelope,
                }
            }
            Err(envelope) => return envelope,
        };

        let metadata = data.pointer("/data/metadata");
        let title = metadata
            .and_then(|m| m.get("title"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(target.as_str());
        let description = metadata
            .and_then(|m| m.get("description"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty());
        let excerpt: String = data
            .pointer("/data/markdown")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .take(600)
            .collect();
        return (
            StatusCode::OK,
            Json(ok(json!({
                "mode": "fetch",
                "url": target,
                "title": title,
                "description": description,
                "excerpt": if excerpt.trim().is_empty() { Value::Null } else { Value::String(excerpt) },
            }))),
        );
    }

    // Keyword path → SmartSearchRouter inside quarry-edge, cached 4h so repeat
    // queries skip quarry entirely and survive brief quarry outages.
    // Filters participate in the cache key — a filtered and unfiltered query
    // for the same terms must not collide on one entry.
    let filters_sig = format!(
        "{}|{}|{}|{}|{}|{}",
        topic.as_deref().unwrap_or(""),
        time_range.as_deref().unwrap_or(""),
        days.map(|d| d.to_string()).unwrap_or_default(),
        exact_match,
        include_domains.join(","),
        exclude_domains.join(","),
    );
    let filters_sig = format!(
        "{filters_sig}|{}|{}",
        country.as_deref().unwrap_or(""),
        language.as_deref().unwrap_or(""),
    );
    let key = admission.key(
        if public_only {
            "search-web-public"
        } else {
            "search-web"
        },
        &[
            &query,
            &limit.to_string(),
            &include_answer.to_string(),
            &filters_sig,
        ],
    );
    let mut stale: Option<Value> = None;
    if let (Some(key), Some(scope), true) = (key.as_deref(), admission.scope(), admission.may_read)
    {
        match state.cache.lookup_scoped(key, scope).await {
            Some(hit) if hit.fresh => {
                crate::observability::record_result_cache("search-web", "hit");
                return serve_search(&state, &user, &headers, hit.data).await;
            }
            Some(hit) => stale = Some(hit.data),
            None => crate::observability::record_result_cache("search-web", "miss"),
        }
    }

    let url = format!("{}/v1/search", state.quarry_edge_url);
    let mut payload = json!({
        "query": query,
        "limit": limit,
        "include_answer": include_answer,
        "safe_search": true,
    });
    if public_only {
        // Quarry's live SERP path skips the tenant's private index.
        payload["intent"] = json!("fresh");
    }
    if let Some(t) = &topic {
        payload["topic"] = json!(t);
    }
    if let Some(tr) = &time_range {
        payload["time_range"] = json!(tr);
    }
    if let Some(d) = days {
        payload["days"] = json!(d);
    }
    if exact_match {
        payload["exact_match"] = json!(true);
    }
    if let Some(country) = &country {
        payload["country"] = json!(country);
    }
    if let Some(language) = &language {
        payload["language"] = json!(language);
    }
    if !include_domains.is_empty() {
        payload["include_domains"] = json!(include_domains);
    }
    if !exclude_domains.is_empty() {
        payload["exclude_domains"] = json!(exclude_domains);
    }
    if zdr {
        payload["zdr"] = json!(true);
    }
    match post_quarry(
        &state,
        &url,
        token.as_deref(),
        &user.user_id,
        &payload,
        Duration::from_secs(25),
    )
    .await
    {
        Ok(data) => {
            let cached_payload = if public_only {
                let results = data.get("results").and_then(Value::as_array).into_iter().flatten()
                    .filter(|hit| {
                        matches!(
                            hit.get("provider").and_then(Value::as_str),
                            Some("searxng" | "stract" | "brave" | "serper")
                        )
                    })
                    .collect::<Vec<_>>();
                json!({ "mode": "search", "results": results })
            } else {
                let results = data.get("results").cloned().unwrap_or_else(|| json!([]));
                let answer = data
                    .get("answer")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .map(Value::from)
                    .unwrap_or(Value::Null);
                let citations = data.get("citations").cloned().unwrap_or_else(|| json!([]));
                // INNH-8: why there is or is not an answer (ok, no_sources, skipped_zdr, unavailable).
                let answer_status = data.get("answer_status").cloned().unwrap_or(Value::Null);
                let answer_reason = data.get("answer_reason").cloned().unwrap_or(Value::Null);
                json!({
                    "mode": "search",
                    "results": results,
                    "answer": answer,
                    "citations": citations,
                    "answerStatus": answer_status,
                    "answerReason": answer_reason,
                })
            };
            if let (Some(key), Some(scope), true) =
                (key.as_deref(), admission.scope(), admission.may_write)
            {
                state.cache.store_scoped(key, scope, &cached_payload).await;
                crate::observability::record_result_cache("search-web", "store");
            }
            // The stored payload stays whole: the org's rules are applied as it is served, so a rule changed
            // later binds the next read of the same entry.
            serve_search(&state, &user, &headers, cached_payload).await
        }
        Err(envelope) => match stale {
            Some(data) => {
                crate::observability::record_result_cache("search-web", "stale_hit");
                serve_search(&state, &user, &headers, data).await
            }
            None => envelope,
        },
    }
}

/// `POST /api/v1/search/similar` → quarry-edge `/v1/search/similar` (Exa-style
/// find-similar). Body: `{ url?, text?, limit? }` — at least one of `url`/`text`.
/// Returns `{ mode: "similar", results: [...] }`. Not cached (results are
/// seeded by the caller's text/URL, so hit-rate is low).
async fn search_similar(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let url_in = body
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let text_in = body
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if url_in.is_none() && text_in.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_input", "Provide `url` or `text`.")),
        );
    }
    let limit = body
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .clamp(1, 50);
    // O-23: a seed URL is a page this search reads, so the org's rules judge its host first.
    if let Some(seed) = url_in {
        if let Some(refusal) = refuse_blocked_target(&state, &user, &headers, seed).await {
            return refusal;
        }
    }
    let token = quarry_token(&state, &user, &headers).await;
    let url = format!("{}/v1/search/similar", state.quarry_edge_url);
    let mut payload = json!({ "limit": limit });
    if let Some(u) = url_in {
        payload["url"] = json!(u);
    }
    if let Some(t) = text_in {
        payload["text"] = json!(t);
    }
    match post_quarry(
        &state,
        &url,
        token.as_deref(),
        &user.user_id,
        &payload,
        Duration::from_secs(25),
    )
    .await
    {
        Ok(data) => {
            let results = data.get("results").cloned().unwrap_or_else(|| json!([]));
            let payload = json!({ "mode": "similar", "results": results });
            serve_hits(&state, &user, &headers, payload, &[("results", "url")]).await
        }
        Err(envelope) => envelope,
    }
}

/// `POST /api/v1/search/images` → quarry-edge `/v1/search/images`, sanitized.
async fn search_images(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let query = body
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if query.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_query", "A non-empty query is required.")),
        );
    }
    let limit = body
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(24)
        .clamp(1, 50);
    let token = quarry_token(&state, &user, &headers).await;
    let url = format!("{}/v1/search/images", state.quarry_edge_url);
    let payload = json!({ "query": query, "limit": limit });

    match post_quarry(
        &state,
        &url,
        token.as_deref(),
        &user.user_id,
        &payload,
        Duration::from_secs(15),
    )
    .await
    {
        Ok(data) => {
            let payload = json!({ "images": sanitize_images(data.get("images")) });
            serve_hits(&state, &user, &headers, payload, &[("images", "url")]).await
        }
        Err(envelope) => envelope,
    }
}

/// `POST /api/v1/search/suggest` → quarry-edge `/v1/search/suggest` (Exa/Google-
/// style did-you-mean + related searches). Best-effort: always 200 with empty
/// suggestions on any upstream failure so the search UX never breaks. The SPA
/// fires this in parallel with the main search.
async fn search_suggest(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let empty = || {
        ok(json!({ "correctedQuery": Value::Null, "relatedQueries": [], "entity": Value::Null }))
    };
    let query = body
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if query.is_empty() {
        return (StatusCode::OK, Json(empty()));
    }
    let token = quarry_token(&state, &user, &headers).await;
    let url = format!("{}/v1/search/suggest", state.quarry_edge_url);
    let payload = json!({ "query": query });
    match post_quarry(
        &state,
        &url,
        token.as_deref(),
        &user.user_id,
        &payload,
        Duration::from_secs(12),
    )
    .await
    {
        Ok(data) => (
            StatusCode::OK,
            Json(ok(json!({
                "correctedQuery": data.get("corrected_query").cloned().unwrap_or(Value::Null),
                "relatedQueries": data.get("related_queries").cloned().unwrap_or_else(|| json!([])),
                "entity": data.get("entity").cloned().unwrap_or(Value::Null),
            }))),
        ),
        // Suggestions are non-critical — never surface an error to the searchbar.
        Err(_) => (StatusCode::OK, Json(empty())),
    }
}

/// `POST /api/v1/search/videos` → quarry-edge `/v1/search/videos`, sanitized.
///
/// This handler used to call SearXNG directly, and was the only one in this file
/// with no `AuthenticatedUser` extension — which is precisely why the bypass was
/// invisible: with no user id there was no quarry token to mint, so video
/// traffic reached the provider with no org scope, no cache, no host-diversity
/// cap and no metered unit, while every sibling vertical went through the edge.
/// The session guard made it look authenticated; it was never *attributed*.
/// Keep the extension even if a future edit needs nothing else from the user.
async fn search_videos(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let query = body
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if query.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_query", "A non-empty query is required.")),
        );
    }
    let limit = body
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(24)
        .clamp(1, 50);
    let token = quarry_token(&state, &user, &headers).await;
    let url = format!("{}/v1/search/videos", state.quarry_edge_url);
    let payload = json!({ "query": query, "limit": limit });

    match post_quarry(
        &state,
        &url,
        token.as_deref(),
        &user.user_id,
        &payload,
        Duration::from_secs(15),
    )
    .await
    {
        Ok(data) => {
            let payload = json!({ "videos": sanitize_videos(data.get("videos")) });
            serve_hits(&state, &user, &headers, payload, &[("videos", "url")]).await
        }
        Err(envelope) => as_video_error(envelope),
    }
}

/// Re-label `post_quarry`'s generic failure codes as the video-specific ones the
/// SPA's error catalog already translates. `web_search_unavailable` has no entry
/// in `shared/i18n/errors.ts`, so forwarding it verbatim would have swapped a
/// localized "Videosøk er utilgjengelig" for a raw code on screen — a silent UX
/// regression from a change that is only meant to move where the request goes.
fn as_video_error(envelope: (StatusCode, Json<Value>)) -> (StatusCode, Json<Value>) {
    let (status, Json(mut body)) = envelope;
    let relabelled = match body.pointer("/error/code").and_then(Value::as_str) {
        Some("web_search_unavailable") => Some("video_search_unavailable"),
        Some("web_search_timeout") => Some("video_search_timeout"),
        _ => None,
    };
    if let Some(code) = relabelled {
        body["error"]["code"] = json!(code);
    }
    (status, Json(body))
}

/// `GET /api/v1/search/suggestions` → autocomplete-core. Always degrades to an
/// empty list (never an error) so the searchbar dropdown stays resilient.
pub(crate) async fn search_suggestions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    let empty = || (StatusCode::OK, Json(ok(json!({ "suggestions": [] }))));
    // Zero data retention: suggestions are built from what people have searched
    // for, so a ZDR request (header, or an org whose posture is ZDR or unknown)
    // gets none. Checked before anything else, including the autocomplete lookup.
    if crate::zdr::request_zdr(&headers, None) || crate::zdr::org_zdr_enabled(&state, &user).await {
        return empty();
    }

    let query = params.get("q").map(|s| s.trim()).unwrap_or("");
    if query.len() < 2
        || state.autocomplete_token.is_empty()
        || state.autocomplete_core_url.is_empty()
    {
        return empty();
    }

    let scope = params
        .get("scope")
        .map(String::as_str)
        .filter(|s| matches!(*s, "all" | "queries" | "hosts" | "titles"))
        .unwrap_or("queries");
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(8)
        .clamp(1, 20);
    // Org comes from the validated session, never a client header. Autocomplete
    // is per-user (not tenant data), so when the session has no active org we
    // intentionally keep the v2 fallback to the user id for the scope key.
    let resolved_org = crate::upstream::authorized_org_id(&state, &user).await;
    let org_id = if resolved_org.is_empty() {
        user.user_id.clone()
    } else {
        resolved_org
    };
    let url = format!("{}/v1/suggestions", state.autocomplete_core_url);

    let request = state
        .client
        .get(&url)
        .timeout(Duration::from_millis(1500))
        .bearer_auth(&state.autocomplete_token)
        .header("x-org-id", org_id)
        .query(&[
            ("q", query),
            ("scope", scope),
            ("limit", &limit.to_string()),
        ]);

    match request.send().await {
        Ok(resp) if resp.status().is_success() => {
            let data = resp.json::<Value>().await.unwrap_or_else(|_| json!({}));
            let suggestions = data
                .get("suggestions")
                .cloned()
                .unwrap_or_else(|| json!([]));
            (
                StatusCode::OK,
                Json(ok(json!({ "suggestions": suggestions }))),
            )
        }
        _ => empty(),
    }
}

/// `POST /api/v1/search/answer/stream` → quarry-edge `/v1/answer/stream` (SSE).
async fn search_answer_stream(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let token = quarry_token(&state, &user, &headers).await;
    let url = format!("{}/v1/answer/stream", state.quarry_edge_url);
    let query = body
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if query.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_query", "A search query is required.")),
        )
            .into_response();
    }
    let mut payload = json!({ "query": query });
    // Forward an established ZDR posture (request flag or the org's standing
    // posture): quarry-edge can only raise it, and an answer stream is never
    // cached here, so this is the whole of the gateway's part.
    if CacheAdmission::resolve(&state, &user, &headers, &body, false)
        .await
        .zdr_established()
    {
        payload["zdr"] = json!(true);
    }
    let response = proxy_sse_stream(
        &state,
        Method::POST,
        &url,
        Some(payload),
        token.as_deref(),
        None,
        None,
        false,
        None,
    )
    .await;
    // O-23: the sources are known before any text is, so the org's rules judge them first. Off (the default),
    // the stream is forwarded exactly as it came.
    domain_policy::guard_answer_stream(&state, &user, &headers, response).await
}

// ── org web domain rules (O-23) ─────────────────────────────────────────────

/// The refusal of a request for one page whose host the org's rules do not allow, or `None` when the page may be
/// read: the switch is off, Model Gateway's mode does not bind, or the host is allowed. A rules read that fails is a
/// typed 503, never a pass. Under `shadow` the would-be refusal is only counted in the log.
async fn refuse_blocked_target(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    target: &str,
) -> Option<(StatusCode, Json<Value>)> {
    match domain_policy::check_urls(state, user, headers, &[target.to_owned()]).await {
        Ok(None) => None,
        Ok(Some(checked)) => {
            if checked.allows(target) {
                return None;
            }
            tracing::info!(
                enforced = checked.binds(),
                "org web domain policy (BFF): a single-page fetch of a host the organisation does not allow"
            );
            checked.binds().then(domain_policy::blocked_response)
        }
        Err(_) => Some(domain_policy::unavailable_response()),
    }
}

/// A keyword search payload as it is served: the org's rules drop the hits and the citations on refused hosts, and a
/// synthesised answer that lost a source is withheld (its text cannot be cut back). Off (the default), the payload is
/// returned as it is.
async fn serve_search(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    mut data: Value,
) -> (StatusCode, Json<Value>) {
    match domain_policy::filter_hits(
        state,
        user,
        headers,
        &mut data,
        &[("results", "url"), ("citations", "url")],
    )
    .await
    {
        Ok(0) => {}
        Ok(_) => domain_policy::withhold_answer(&mut data),
        Err(_) => return domain_policy::unavailable_response(),
    }
    (StatusCode::OK, Json(ok(data)))
}

/// A vertical's payload as it is served, with the hits on refused hosts dropped.
async fn serve_hits(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    mut data: Value,
    arrays: &[(&str, &str)],
) -> (StatusCode, Json<Value>) {
    match domain_policy::filter_hits(state, user, headers, &mut data, arrays).await {
        Ok(_) => (StatusCode::OK, Json(ok(data))),
        Err(_) => domain_policy::unavailable_response(),
    }
}

// ── helpers ─────────────────────────────────────────────────────────────────

async fn quarry_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Option<String> {
    let cookie = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    get_audience_token(state, &user.user_id, cookie, "quarry").await
}

/// POST a JSON body to quarry-edge with the audience bearer token and forwarded
/// user id, returning the parsed body or a ready-to-send error envelope.
async fn post_quarry(
    state: &AppState,
    url: &str,
    token: Option<&str>,
    user_id: &str,
    body: &Value,
    timeout: Duration,
) -> Result<Value, (StatusCode, Json<Value>)> {
    let mut request = state
        .client
        .post(url)
        .timeout(timeout)
        .header("x-user-id", user_id);
    if let Some(t) = token {
        request = request.bearer_auth(t);
    }

    match request.json(body).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.json::<Value>().await.unwrap_or_else(|_| json!({}));
            if status == 501 {
                return Err((
                    StatusCode::NOT_IMPLEMENTED,
                    Json(error(
                        "search_provider_unconfigured",
                        "Search is unavailable — no search provider is configured.",
                    )),
                ));
            }
            if !(200..300).contains(&status) {
                let message = first_error_message(&body)
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        format!("Search could not be completed (upstream {status}).")
                    });
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(error("web_search_unavailable", message)),
                ));
            }
            Ok(body)
        }
        Err(e) if e.is_timeout() => Err((
            StatusCode::GATEWAY_TIMEOUT,
            Json(error("web_search_timeout", "Search timed out.")),
        )),
        Err(_) => Err((
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        )),
    }
}

fn first_error_message(value: &Value) -> Option<&str> {
    [
        "/error/message",
        "/error",
        "/data/error/message",
        "/data/error",
        "/message",
    ]
    .iter()
    .filter_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
    .map(str::trim)
    .find(|message| !message.is_empty())
}

fn browser_driver_unavailable(value: &Value) -> bool {
    let serialized = value.to_string().to_ascii_lowercase();
    serialized.contains("driver not registered")
        || serialized.contains("browser=driver not registered")
        || serialized.contains("no drivers succeeded in fallback chain")
}

/// A query is treated as a URL when it is an explicit http(s) URL or a bare
/// domain (`example.com`, `sub.example.co.uk/path`) with no whitespace.
fn is_url_input(query: &str) -> bool {
    if query.is_empty() || query.chars().any(char::is_whitespace) {
        return false;
    }
    if query.starts_with("http://") || query.starts_with("https://") {
        return true;
    }
    let host = query.split('/').next().unwrap_or(query);
    let mut parts = host.rsplitn(2, '.');
    let tld = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    !rest.is_empty() && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

fn normalize_url(query: &str) -> String {
    if query.starts_with("http://") || query.starts_with("https://") {
        query.to_owned()
    } else {
        format!("https://{query}")
    }
}

fn is_safe_http(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

/// Parse a JSON value into a `Vec<String>` for domain filters: accepts an array
/// of strings, or a single comma-separated string. Trims entries and drops
/// blanks. Anything else → empty.
fn string_array(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(Value::as_str)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) => s
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// quarry-edge image shape `{img_src, thumbnail_src, source_url, title}` → the
/// SPA's `{url, thumbnailUrl, imageUrl, title}` (drops non-http schemes).
fn sanitize_images(value: Option<&Value>) -> Vec<Value> {
    let Some(arr) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|img| {
            let image_url = img
                .get("img_src")
                .and_then(Value::as_str)
                .filter(|s| is_safe_http(s));
            let thumb = img
                .get("thumbnail_src")
                .and_then(Value::as_str)
                .filter(|s| is_safe_http(s));
            let renderable = thumb.or(image_url)?;
            let source = img
                .get("source_url")
                .and_then(Value::as_str)
                .filter(|s| is_safe_http(s));
            let title = img
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty());
            Some(json!({
                "url": source.or(image_url).unwrap_or(renderable),
                "thumbnailUrl": thumb.unwrap_or(renderable),
                "imageUrl": image_url.unwrap_or(renderable),
                "title": title,
            }))
        })
        .collect()
}

/// quarry-edge video shape `{url, title, thumbnail_src, iframe_src, author,
/// length, published_date, content}` → the SPA's `{url, title, thumbnailUrl,
/// embedUrl, author, length}`. Mirrors `sanitize_images`: it drops non-http
/// schemes and keeps only the fields the VIDEOS tab can actually render, so an
/// upstream that grows fields does not widen what the browser receives.
///
/// The poster field is `thumbnail_src`, not SearXNG's own `thumbnail` — the edge
/// normalizes both `thumbnail` and `img_src` into that one name, so reading the
/// raw SearXNG spelling here would silently yield thumbnail-less results.
fn sanitize_videos(value: Option<&Value>) -> Vec<Value> {
    let Some(arr) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            let url = v
                .get("url")
                .and_then(Value::as_str)
                .filter(|s| is_safe_http(s))?;
            let title = v
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty());
            let thumbnail = v
                .get("thumbnail_src")
                .and_then(Value::as_str)
                .filter(|s| is_safe_http(s));
            let embed = v
                .get("iframe_src")
                .and_then(Value::as_str)
                .filter(|s| is_safe_http(s));
            let author = v
                .get("author")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty());
            let length = v
                .get("length")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty());
            Some(json!({
                "url": url,
                "title": title,
                "thumbnailUrl": thumbnail,
                "embedUrl": embed,
                "author": author,
                "length": length,
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_urls_and_bare_domains() {
        assert!(is_url_input("https://example.com/path?q=1"));
        assert!(is_url_input("http://example.com"));
        assert!(is_url_input("example.com"));
        assert!(is_url_input("sub.example.co.uk/path"));
    }

    #[test]
    fn rejects_keyword_queries() {
        assert!(!is_url_input("how to bake bread"));
        assert!(!is_url_input("hello"));
        assert!(!is_url_input("file."));
        assert!(!is_url_input(""));
    }

    #[test]
    fn normalizes_bare_domains_to_https() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_url("https://x.com"), "https://x.com");
        assert_eq!(normalize_url("http://x.com"), "http://x.com");
    }

    #[test]
    fn sanitize_images_drops_unsafe_and_maps_fields() {
        let raw = json!([
            { "img_src": "https://img/full.jpg", "thumbnail_src": "https://img/t.jpg", "source_url": "https://page", "title": "Hit" },
            { "img_src": "data:image/png;base64,xxx" }
        ]);
        let out = sanitize_images(raw.as_array().map(|_| &raw));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["url"], json!("https://page"));
        assert_eq!(out[0]["thumbnailUrl"], json!("https://img/t.jpg"));
    }

    fn test_user() -> AuthenticatedUser {
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

    /// The bypass verbatim: video search must reach quarry-edge and SearXNG must
    /// see nothing. Asserting "exactly one request to quarry" is the half that
    /// catches a regression where someone keeps the proxy but re-adds a direct
    /// SearXNG call beside it as a "fallback" — the un-attributed path is the
    /// defect, whether or not the attributed one also ran.
    #[tokio::test]
    async fn video_search_proxies_quarry_edge_and_never_calls_searxng() {
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/search/videos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "query": "otters",
                "provider": "searxng",
                "videos": [
                    {
                        "url": "https://videos.invalid/watch/1",
                        "title": "Otters",
                        "thumbnail_src": "https://videos.invalid/t1.jpg",
                        "iframe_src": "https://videos.invalid/embed/1",
                        "author": "Channel",
                        "length": "3:21",
                        "published_date": "2026-01-01",
                        "content": "A snippet the SPA never renders."
                    },
                    // Dropped: no watch URL survives `is_safe_http`.
                    { "url": "javascript:alert(1)", "title": "Hostile" }
                ]
            })))
            .mount(&quarry)
            .await;

        // Mounts nothing on purpose: any request at all shows up in
        // `received_requests`, so the assertion fails loudly rather than
        // depending on what a stubbed SearXNG would have answered.
        let searxng = MockServer::start().await;

        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        state.searxng_url = searxng.uri();

        let (status, Json(body)) = search_videos(
            State(state),
            Extension(test_user()),
            HeaderMap::new(),
            Json(json!({ "query": "otters", "limit": 5 })),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            searxng.received_requests().await.unwrap().is_empty(),
            "the BFF must never talk to SearXNG directly"
        );

        let requests = quarry.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1, "expected exactly one upstream request");
        assert_eq!(requests[0].url.path(), "/v1/search/videos");
        // The user id is the attribution the bypass could not supply.
        assert_eq!(
            requests[0]
                .headers
                .get("x-user-id")
                .map(|v| v.to_str().unwrap()),
            Some("user-1")
        );
        let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(sent["query"], json!("otters"));
        assert_eq!(sent["limit"], json!(5));

        let videos = body["data"]["videos"].as_array().unwrap();
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0]["url"], json!("https://videos.invalid/watch/1"));
        assert_eq!(
            videos[0]["thumbnailUrl"],
            json!("https://videos.invalid/t1.jpg")
        );
        assert_eq!(
            videos[0]["embedUrl"],
            json!("https://videos.invalid/embed/1")
        );
        // Sanitizing is still a BFF job: the edge's extra fields stay behind.
        assert!(videos[0].get("content").is_none());
        assert!(videos[0].get("published_date").is_none());
    }

    /// The tell that identified the bypass was a handler with no authenticated-
    /// user extractor. Pin the signature: axum happily routes a handler that
    /// takes fewer extractors, so dropping it again would compile, pass the
    /// session guard, and silently un-attribute the vertical a second time.
    #[test]
    fn video_search_requires_an_authenticated_user() {
        fn takes_authenticated_user<Fut: std::future::Future>(
            _handler: fn(
                State<AppState>,
                Extension<AuthenticatedUser>,
                HeaderMap,
                Json<Value>,
            ) -> Fut,
        ) {
        }
        takes_authenticated_user(search_videos);
    }

    /// `post_quarry` is shared with the web vertical, so its failure codes are
    /// web-flavoured. The SPA translates `video_search_unavailable` and has no
    /// entry for `web_search_unavailable`.
    #[test]
    fn upstream_failures_keep_the_video_specific_error_code() {
        let (status, Json(body)) = as_video_error((
            StatusCode::BAD_GATEWAY,
            Json(error("web_search_unavailable", "upstream 502")),
        ));
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], json!("video_search_unavailable"));
        assert_eq!(body["error"]["message"], json!("upstream 502"));

        let (_, Json(timeout)) = as_video_error((
            StatusCode::GATEWAY_TIMEOUT,
            Json(error("web_search_timeout", "Search timed out.")),
        ));
        assert_eq!(timeout["error"]["code"], json!("video_search_timeout"));

        // Codes that already say something specific are passed through.
        let (_, Json(other)) = as_video_error((
            StatusCode::NOT_IMPLEMENTED,
            Json(error("search_provider_unconfigured", "no provider")),
        ));
        assert_eq!(
            other["error"]["code"],
            json!("search_provider_unconfigured")
        );
    }

    #[test]
    fn sanitize_videos_reads_the_edge_field_names() {
        let raw = json!([
            {
                "url": "https://videos.invalid/watch/1",
                "title": "Hit",
                "thumbnail_src": "https://videos.invalid/t.jpg",
                "iframe_src": "https://videos.invalid/embed/1"
            },
            // Raw SearXNG spelling: no longer what this layer receives, so the
            // poster is simply absent rather than wrongly populated.
            { "url": "https://videos.invalid/watch/2", "thumbnail": "https://videos.invalid/t2.jpg" }
        ]);
        let out = sanitize_videos(Some(&raw));
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0]["thumbnailUrl"],
            json!("https://videos.invalid/t.jpg")
        );
        assert_eq!(out[0]["embedUrl"], json!("https://videos.invalid/embed/1"));
        assert_eq!(out[1]["thumbnailUrl"], Value::Null);
    }

    #[test]
    fn browser_driver_error_is_detected_for_static_retry() {
        let body = json!({
            "error": {
                "message": "no drivers succeeded in fallback chain: Browser=driver not registered"
            }
        });

        assert!(browser_driver_unavailable(&body));
        assert_eq!(
            first_error_message(&body),
            Some("no drivers succeeded in fallback chain: Browser=driver not registered"),
        );
    }

    // ── result-cache admission (F-t2-003 phase 1) ───────────────────────────

    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn member_of(org: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: format!("user-{org}"),
            user_email: format!("user@{org}.invalid"),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some(org.to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: org.to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    /// org-core answers `zdr` per org id; quarry-edge answers every search with
    /// a body that names the org-scoped answer so a cross-org hit is visible.
    struct World {
        state: AppState,
        quarry: MockServer,
        _org_core: MockServer,
    }

    async fn world(zdr_orgs: &[&str], plain_orgs: &[&str]) -> World {
        let org_core = MockServer::start().await;
        for (org, zdr) in zdr_orgs
            .iter()
            .map(|o| (*o, true))
            .chain(plain_orgs.iter().map(|o| (*o, false)))
        {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/v1/organizations/{org}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "id": org,
                    "metadata": { "interactiveRetention": { "zdr": zdr } }
                })))
                .mount(&org_core)
                .await;
        }
        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    { "title": "r", "url": "https://example.invalid/r", "provider": "tantivy_local" },
                    { "title": "public", "url": "https://example.invalid/public", "provider": "searxng" }
                ],
                "answer": "answer built from the caller's private corpus",
                "citations": []
            })))
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.org_core_url = org_core.uri();
        state.quarry_edge_url = quarry.uri();
        state.cache = crate::cache::ResultCache::in_memory();
        World {
            state,
            quarry,
            _org_core: org_core,
        }
    }

    async fn search_as(
        world: &World,
        org: &str,
        extra_headers: &[(&'static str, &'static str)],
        body: Value,
    ) -> (StatusCode, Value) {
        let mut headers = HeaderMap::new();
        for (name, value) in extra_headers {
            headers.insert(*name, axum::http::HeaderValue::from_static(value));
        }
        let (status, Json(value)) = search_web(
            State(world.state.clone()),
            Extension(member_of(org)),
            headers,
            Json(body),
        )
        .await;
        (status, value)
    }

    async fn quarry_search_calls(world: &World) -> usize {
        world
            .quarry
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.url.path() == "/v1/search")
            .count()
    }

    fn content_keys(world: &World) -> Vec<String> {
        world
            .state
            .cache
            .mem_keys()
            .into_iter()
            .filter(|key| key.starts_with("verevon:gw:search-web:"))
            .collect()
    }

    /// Zero data retention: suggestions are built from what people have searched
    /// for, so a ZDR request gets none, and so does a ZDR org even without the
    /// header. The autocomplete mock must never be called in those cases.
    async fn suggestions_as(
        world: &World,
        org: &str,
        extra_headers: &[(&'static str, &'static str)],
    ) -> (StatusCode, Value) {
        let mut headers = HeaderMap::new();
        for (name, value) in extra_headers {
            headers.insert(*name, axum::http::HeaderValue::from_static(value));
        }
        let params: HashMap<String, String> = HashMap::from([
            ("q".to_owned(), "norwegian salmon".to_owned()),
            ("scope".to_owned(), "queries".to_owned()),
        ]);
        let (status, Json(value)) = search_suggestions(
            State(world.state.clone()),
            Extension(member_of(org)),
            headers,
            Query(params),
        )
        .await;
        (status, value)
    }

    async fn autocomplete_core() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/suggestions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "suggestions": [{ "text": "norwegian salmon exports" }]
            })))
            .mount(&server)
            .await;
        server
    }

    async fn autocomplete_calls(server: &MockServer) -> usize {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.url.path() == "/v1/suggestions")
            .count()
    }

    fn with_autocomplete(world: &mut World, server: &MockServer) {
        world.state.autocomplete_token = "test-token".to_owned();
        world.state.autocomplete_core_url = server.uri();
    }

    #[tokio::test]
    async fn search_suggestions_empty_under_zdr_header() {
        let mut world = world(&[], &["org-plain"]).await;
        let autocomplete = autocomplete_core().await;
        with_autocomplete(&mut world, &autocomplete);

        let (status, value) = suggestions_as(&world, "org-plain", &[("x-zdr", "1")]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["data"]["suggestions"], json!([]));
        assert_eq!(autocomplete_calls(&autocomplete).await, 0);

        // The control: the same request without the header is answered.
        let (_, value) = suggestions_as(&world, "org-plain", &[]).await;
        assert_eq!(value["data"]["suggestions"].as_array().map(Vec::len), Some(1));
    }

    #[tokio::test]
    async fn search_suggestions_empty_under_org_zdr() {
        let mut world = world(&["org-zdr"], &["org-plain"]).await;
        let autocomplete = autocomplete_core().await;
        with_autocomplete(&mut world, &autocomplete);

        let (_, value) = suggestions_as(&world, "org-zdr", &[]).await;
        assert_eq!(value["data"]["suggestions"], json!([]));
        assert_eq!(autocomplete_calls(&autocomplete).await, 0);

        let (_, value) = suggestions_as(&world, "org-plain", &[]).await;
        assert_eq!(value["data"]["suggestions"].as_array().map(Vec::len), Some(1));
    }

    /// The cross-tenant test. The cached search body carries an `answer` that
    /// quarry-edge built from org A's private corpus; org B asking the same
    /// question must reach quarry-edge (which scopes by org), not read A's entry.
    #[tokio::test]
    async fn search_web_isolates_cached_results_by_org() {
        let w = world(&[], &["org-a", "org-b"]).await;
        let body = json!({ "query": "private roadmap", "includeAnswer": true });

        let (status, _) = search_as(&w, "org-a", &[], body.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(quarry_search_calls(&w).await, 1);

        let (status, _) = search_as(&w, "org-b", &[], body.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            quarry_search_calls(&w).await,
            2,
            "org B must reach quarry-edge, not be served org A's cached entry"
        );

        let keys = content_keys(&w);
        assert_eq!(keys.len(), 2, "{keys:?}");
        assert!(keys.iter().any(|k| k.contains(":o:org-a:")));
        assert!(keys.iter().any(|k| k.contains(":o:org-b:")));
    }

    #[tokio::test]
    async fn search_web_same_org_second_request_is_a_cache_hit() {
        let w = world(&[], &["org-a"]).await;
        let body = json!({ "query": "public docs", "includeAnswer": true });

        search_as(&w, "org-a", &[], body.clone()).await;
        let (status, value) = search_as(&w, "org-a", &[], body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["data"]["mode"], json!("search"));
        assert_eq!(
            quarry_search_calls(&w).await,
            1,
            "second call is a cache hit"
        );
    }

    #[tokio::test]
    async fn public_search_uses_live_serps_and_excludes_workspace_results() {
        let w = world(&[], &["org-a"]).await;
        search_as(&w, "org-a", &[], json!({ "query": "public docs" })).await;
        let (status, value) = search_as(
            &w,
            "org-a",
            &[],
            json!({ "query": "public docs", "publicOnly": true, "includeAnswer": true }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["data"]["results"].as_array().map(Vec::len), Some(1));
        assert_eq!(value["data"]["results"][0]["provider"], "searxng");
        assert!(value["data"].get("answer").is_none());

        let requests = w.quarry.received_requests().await.unwrap_or_default();
        assert_eq!(requests.len(), 2, "a workspace cache hit must not serve public search");
        let sent: Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert_eq!(sent["intent"], "fresh");
        assert_eq!(sent["include_answer"], false);
    }

    #[tokio::test]
    async fn a_zdr_org_search_leaves_no_cache_entry_and_tells_quarry() {
        let w = world(&["org-z"], &[]).await;
        let body = json!({ "query": "sensitive question", "includeAnswer": true });

        search_as(&w, "org-z", &[], body.clone()).await;
        search_as(&w, "org-z", &[], body).await;

        assert_eq!(quarry_search_calls(&w).await, 2, "no cache for a ZDR org");
        assert!(content_keys(&w).is_empty(), "{:?}", content_keys(&w));

        // quarry-edge is told, so it applies the same posture to the search.
        let forwarded = w.quarry.received_requests().await.unwrap_or_default();
        let sent: Value = serde_json::from_slice(&forwarded[0].body).unwrap();
        assert_eq!(sent["zdr"], json!(true));
    }

    #[tokio::test]
    async fn explicit_zdr_header_or_body_skips_cache_for_a_non_zdr_org() {
        let w = world(&[], &["org-a"]).await;
        let body = json!({ "query": "temporary chat question" });

        search_as(&w, "org-a", &[("x-zdr", "true")], body.clone()).await;
        let mut with_body = body.clone();
        with_body["zdr"] = json!(true);
        search_as(&w, "org-a", &[], with_body).await;

        assert_eq!(quarry_search_calls(&w).await, 2);
        assert!(content_keys(&w).is_empty());

        // A plain request for the same query still caches (the posture is per request).
        search_as(&w, "org-a", &[], body.clone()).await;
        search_as(&w, "org-a", &[], body).await;
        assert_eq!(quarry_search_calls(&w).await, 3);
        assert_eq!(content_keys(&w).len(), 1);
    }

    /// An org-core outage must skip the cache in both directions but must not
    /// fail the search, and must not be reported to quarry-edge as ZDR.
    #[tokio::test]
    async fn org_core_outage_skips_the_cache_but_not_the_search() {
        let w = world(&[], &[]).await; // org-core has no org record: 404
        let body = json!({ "query": "outage question" });

        let (status, value) = search_as(&w, "org-a", &[], body.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["data"]["mode"], json!("search"));
        search_as(&w, "org-a", &[], body).await;

        assert_eq!(quarry_search_calls(&w).await, 2);
        assert!(content_keys(&w).is_empty());
        let forwarded = w.quarry.received_requests().await.unwrap_or_default();
        let sent: Value = serde_json::from_slice(&forwarded[0].body).unwrap();
        assert!(sent.get("zdr").is_none(), "an outage is not a ZDR posture");
    }

    /// When quarry-edge fails, a stale entry is a fallback only for the org that
    /// wrote it. Plant an old entry under org A's key and fail the upstream.
    #[tokio::test]
    async fn stale_entry_is_not_served_across_orgs_when_quarry_fails() {
        let w = world(&[], &["org-a", "org-b"]).await;
        // Quarry now fails every search.
        w.quarry.reset().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/search"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&w.quarry)
            .await;

        let body = json!({ "query": "stale probe", "includeAnswer": false });
        // Org A has a cached entry, regardless of age.
        let key = {
            let admission = CacheAdmission::resolve(
                &w.state,
                &member_of("org-a"),
                &HeaderMap::new(),
                &body,
                false,
            )
            .await;
            admission
                .key(
                    "search-web",
                    &[
                        "stale probe",
                        &8.to_string(),
                        &false.to_string(),
                        "|||false||",
                    ],
                )
                .expect("org a key")
        };
        w.state
            .cache
            .store_scoped(
                &key,
                "org-a",
                &json!({ "mode": "search", "answer": "A-ONLY" }),
            )
            .await;

        let (status_b, value_b) = search_as(&w, "org-b", &[], body.clone()).await;
        assert!(
            !status_b.is_success(),
            "org B must see the upstream failure, got {value_b}"
        );
        assert!(!value_b.to_string().contains("A-ONLY"));
    }

    /// The search bar reads `data.markdown`, which quarry-edge only inlines
    /// when asked for the `markdown` format. A `maxPages` key is not a scrape
    /// option (the edge warns on it), so it must not be sent.
    #[tokio::test]
    async fn fetch_mode_excerpt_is_populated_from_inline_markdown() {
        let w = world(&[], &["org-a"]).await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "status": 200,
                    "markdown": "# Example\n\nInline page text.",
                    "metadata": { "title": "Example", "content_kind": "html" }
                }
            })))
            .mount(&w.quarry)
            .await;

        let (status, value) = search_as(
            &w,
            "org-a",
            &[],
            json!({ "query": "https://example.invalid/a" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["data"]["mode"], json!("fetch"));
        assert_eq!(value["data"]["title"], json!("Example"));
        assert_eq!(
            value["data"]["excerpt"],
            json!("# Example\n\nInline page text.")
        );

        let sent = w.quarry.received_requests().await.unwrap_or_default();
        let scrapes: Vec<Value> = sent
            .iter()
            .filter(|request| request.url.path() == "/v1/scrape")
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect();
        assert_eq!(scrapes.len(), 1, "one scrape serves the fetch");
        assert_eq!(scrapes[0]["formats"], json!(["markdown"]));
        assert!(scrapes[0].get("maxPages").is_none(), "{}", scrapes[0]);
    }
}

/// O-23: the org's web domain rules bind the BFF's web search (web, similar, images, videos, the answer stream), and a
/// URL query is refused before any upstream call. Model Gateway (wiremock) answers the domain check; quarry-edge
/// (wiremock) answers the search.
#[cfg(test)]
mod web_domain_rules_tests {
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    const BLOCKED: &str = "blocked.example";
    const ALLOWED: &str = "allowed.example";

    /// Model Gateway's domain check: the mode it reports, and `deny` for the blocked host and `allow` for any other.
    struct DomainCheck {
        /// `None` is a Model Gateway whose check fails with a 500.
        mode: Option<&'static str>,
    }

    impl Respond for DomainCheck {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let Some(mode) = self.mode else {
                return ResponseTemplate::new(500);
            };
            let body: Value = serde_json::from_slice(&request.body).unwrap_or_default();
            let mut verdicts = serde_json::Map::new();
            for host in body["hosts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let verdict = if host == BLOCKED { "deny" } else { "allow" };
                verdicts.insert(host.to_owned(), json!(verdict));
            }
            ResponseTemplate::new(200).set_body_json(json!({ "mode": mode, "verdicts": verdicts }))
        }
    }

    struct World {
        state: AppState,
        model: MockServer,
        quarry: MockServer,
        _aux: Vec<MockServer>,
    }

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-o23".to_owned(),
            user_email: "user@o23.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-o23".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-o23".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    /// `check`: the mode Model Gateway reports, or `None` for a Model Gateway whose check fails with 500.
    async fn world(switch: bool, check: Option<&'static str>) -> World {
        world_with(switch, check, true).await
    }

    /// [`world`], where the keyword search either does or does not return a hit on the blocked host.
    async fn world_with(
        switch: bool,
        check: Option<&'static str>,
        blocked_in_search: bool,
    ) -> World {
        let auth = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/[a-z-]+/token$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "token": "t" })))
            .mount(&auth)
            .await;
        let org_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/v1/organizations/.+$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org-o23",
                "metadata": { "interactiveRetention": { "zdr": false } }
            })))
            .mount(&org_core)
            .await;
        let model = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/web/domain-check"))
            .respond_with(DomainCheck { mode: check })
            .mount(&model)
            .await;
        let quarry = MockServer::start().await;
        let mut hits = vec![
            json!({ "title": "a", "url": format!("https://{ALLOWED}/a"), "provider": "searxng" }),
        ];
        let mut cited = vec![json!({ "url": format!("https://{ALLOWED}/a") })];
        if blocked_in_search {
            hits.push(json!({ "title": "b", "url": format!("https://{BLOCKED}/b"), "provider": "searxng" }));
            cited.push(json!({ "url": format!("https://{BLOCKED}/b") }));
        }
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": hits,
                "answer": "built from both sources",
                "citations": cited
            })))
            .mount(&quarry)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "markdown": "page text", "metadata": { "title": "t" } }
            })))
            .mount(&quarry)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/search/similar"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    { "title": "a", "url": format!("https://{ALLOWED}/a") },
                    { "title": "b", "url": format!("https://{BLOCKED}/b") }
                ]
            })))
            .mount(&quarry)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/search/images"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "images": [
                    { "img_src": "https://cdn.example/1.png", "source_url": format!("https://{ALLOWED}/p") },
                    { "img_src": "https://cdn.example/2.png", "source_url": format!("https://{BLOCKED}/p") }
                ]
            })))
            .mount(&quarry)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/search/videos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "videos": [
                    { "url": format!("https://{ALLOWED}/v") },
                    { "url": format!("https://{BLOCKED}/v") }
                ]
            })))
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.org_core_url = org_core.uri();
        state.model_gateway_url = model.uri();
        state.quarry_edge_url = quarry.uri();
        state.cache = crate::cache::ResultCache::in_memory();
        state.space_features.web_domain_policy_bff = switch;
        World {
            state,
            model,
            quarry,
            _aux: vec![auth, org_core],
        }
    }

    async fn calls(server: &MockServer, request_path: &str) -> usize {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.url.path() == request_path)
            .count()
    }

    async fn web(world: &World, body: Value) -> (StatusCode, Value) {
        let (status, Json(value)) = search_web(
            State(world.state.clone()),
            Extension(user()),
            HeaderMap::new(),
            Json(body),
        )
        .await;
        (status, value)
    }

    fn result_urls(value: &Value) -> Vec<String> {
        value["data"]["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|hit| hit["url"].as_str().map(str::to_owned))
            .collect()
    }

    #[tokio::test]
    async fn a_denied_host_hit_is_removed_and_the_answer_built_from_it_is_withheld() {
        let world = world(true, Some("enforce")).await;
        let (status, value) = web(
            &world,
            json!({ "query": "rust news", "includeAnswer": true }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result_urls(&value), [format!("https://{ALLOWED}/a")]);
        let citations = value["data"]["citations"].as_array().expect("citations");
        assert_eq!(citations.len(), 1, "the blocked citation is dropped too");
        assert_eq!(
            value["data"]["answer"],
            Value::Null,
            "text built from a blocked page is withheld"
        );
        assert_eq!(value["data"]["answerStatus"], "unavailable");
        assert_eq!(value["data"]["answerReason"], "org_domain_rules");
    }

    #[tokio::test]
    async fn an_allowed_search_is_unchanged() {
        let world = world_with(true, Some("enforce"), false).await;
        let (status, value) = web(
            &world,
            json!({ "query": "clean query", "includeAnswer": true }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result_urls(&value), [format!("https://{ALLOWED}/a")]);
        assert_eq!(
            value["data"]["answer"], "built from both sources",
            "nothing was dropped, so the answer stays"
        );
        assert_eq!(value["data"]["citations"].as_array().map(Vec::len), Some(1));
        assert_eq!(calls(&world.model, "/v1/web/domain-check").await, 1);
    }

    #[tokio::test]
    async fn shadow_logs_and_changes_nothing() {
        let world = world(true, Some("shadow")).await;
        let (status, value) = web(
            &world,
            json!({ "query": "rust news", "includeAnswer": true }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result_urls(&value).len(), 2, "shadow drops nothing");
        assert_eq!(value["data"]["answer"], "built from both sources");
        assert_eq!(
            calls(&world.model, "/v1/web/domain-check").await,
            1,
            "shadow still reads the verdicts"
        );
    }

    #[tokio::test]
    async fn model_gateway_mode_off_changes_nothing() {
        let world = world(true, Some("off")).await;
        let (_, value) = web(
            &world,
            json!({ "query": "rust news", "includeAnswer": true }),
        )
        .await;
        assert_eq!(result_urls(&value).len(), 2);
        assert_eq!(value["data"]["answer"], "built from both sources");
    }

    #[tokio::test]
    async fn the_switch_off_never_calls_model_gateway_and_changes_nothing() {
        let world = world(false, Some("enforce")).await;
        let (status, value) = web(
            &world,
            json!({ "query": "rust news", "includeAnswer": true }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result_urls(&value).len(), 2);
        assert_eq!(value["data"]["answer"], "built from both sources");
        let (status, _) = web(&world, json!({ "query": format!("https://{BLOCKED}/x") })).await;
        assert_eq!(status, StatusCode::OK, "a URL query is fetched as before");
        assert_eq!(calls(&world.model, "/v1/web/domain-check").await, 0);
    }

    #[tokio::test]
    async fn a_url_or_bare_domain_query_on_a_denied_host_is_refused_before_any_upstream_call() {
        let world = world(true, Some("enforce")).await;
        for query in [format!("https://{BLOCKED}/page"), BLOCKED.to_owned()] {
            let (status, value) = web(&world, json!({ "query": query })).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{query}");
            assert_eq!(value["error"]["code"], "org_site_blocked", "{query}");
        }
        assert_eq!(
            calls(&world.quarry, "/v1/scrape").await,
            0,
            "no upstream call was made"
        );
        assert_eq!(calls(&world.quarry, "/v1/search").await, 0);
        let (status, _) = web(
            &world,
            json!({ "query": format!("https://{ALLOWED}/page") }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "an allowed host is fetched");
        assert_eq!(calls(&world.quarry, "/v1/scrape").await, 1);
    }

    #[tokio::test]
    async fn a_denied_url_query_is_only_logged_under_shadow() {
        let world = world(true, Some("shadow")).await;
        let (status, _) = web(
            &world,
            json!({ "query": format!("https://{BLOCKED}/page") }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(calls(&world.quarry, "/v1/scrape").await, 1);
    }

    #[tokio::test]
    async fn rules_that_cannot_be_read_stop_the_search_with_a_typed_503() {
        let world = world(true, None).await;
        let (status, value) = web(&world, json!({ "query": "rust news" })).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(value["error"]["code"], "org_site_check_unavailable");
        let (status, value) = web(
            &world,
            json!({ "query": format!("https://{ALLOWED}/page") }),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(value["error"]["code"], "org_site_check_unavailable");
        assert_eq!(
            calls(&world.quarry, "/v1/scrape").await,
            0,
            "a URL fetch never runs on an unreadable rule set"
        );
    }

    #[tokio::test]
    async fn the_cached_entry_is_whole_and_the_rules_apply_as_it_is_served() {
        let world = world(true, Some("enforce")).await;
        let (_, first) = web(&world, json!({ "query": "cached query" })).await;
        let (_, second) = web(&world, json!({ "query": "cached query" })).await;
        assert_eq!(
            calls(&world.quarry, "/v1/search").await,
            1,
            "the second read is a cache hit"
        );
        assert_eq!(result_urls(&first), result_urls(&second));
        assert_eq!(
            result_urls(&second),
            [format!("https://{ALLOWED}/a")],
            "the hit is filtered on the cached read too"
        );
    }

    #[tokio::test]
    async fn similar_images_and_videos_drop_the_denied_hosts() {
        let world = world(true, Some("enforce")).await;
        let (status, Json(similar)) = search_similar(
            State(world.state.clone()),
            Extension(user()),
            HeaderMap::new(),
            Json(json!({ "text": "like this" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(similar["data"]["results"].as_array().map(Vec::len), Some(1));
        let (_, Json(images)) = search_images(
            State(world.state.clone()),
            Extension(user()),
            HeaderMap::new(),
            Json(json!({ "query": "cats" })),
        )
        .await;
        let images = images["data"]["images"].as_array().expect("images");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0]["url"], format!("https://{ALLOWED}/p"));
        let (_, Json(videos)) = search_videos(
            State(world.state.clone()),
            Extension(user()),
            HeaderMap::new(),
            Json(json!({ "query": "cats" })),
        )
        .await;
        let videos = videos["data"]["videos"].as_array().expect("videos");
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0]["url"], format!("https://{ALLOWED}/v"));

        let (status, Json(refused)) = search_similar(
            State(world.state.clone()),
            Extension(user()),
            HeaderMap::new(),
            Json(json!({ "url": format!("https://{BLOCKED}/seed") })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "a denied seed URL is not read"
        );
        assert_eq!(refused["error"]["code"], "org_site_blocked");
    }

    async fn stream_world(
        switch: bool,
        mode: &'static str,
        citations: Value,
    ) -> (World, MockServer) {
        let sse = format!(
            "event: citations\ndata: {citations}\n\nevent: delta\ndata: {{\"delta\":\"hello\"}}\n\nevent: done\ndata: {{\"answer\":\"hello\"}}\n\n"
        );
        stream_world_sse(switch, mode, sse).await
    }

    /// [`stream_world`] with the exact SSE body Quarry answers.
    async fn stream_world_sse(
        switch: bool,
        mode: &'static str,
        sse: String,
    ) -> (World, MockServer) {
        let world = world(switch, Some(mode)).await;
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/answer/stream"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&quarry)
            .await;
        let mut world = world;
        world.state.quarry_edge_url = quarry.uri();
        (world, quarry)
    }

    async fn answer_text(world: &World) -> String {
        let response = search_answer_stream(
            State(world.state.clone()),
            Extension(user()),
            HeaderMap::new(),
            Json(json!({ "query": "what is rust" })),
        )
        .await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[tokio::test]
    async fn the_answer_stream_ends_in_a_typed_error_before_any_text_when_a_source_is_denied() {
        let citations = json!([{ "url": format!("https://{ALLOWED}/a") }, { "url": format!("https://{BLOCKED}/b") }]);
        let (world, _quarry) = stream_world(true, "enforce", citations).await;
        let text = answer_text(&world).await;
        assert!(text.contains("event: error"), "{text}");
        assert!(text.contains("org_site_blocked"), "{text}");
        assert!(
            !text.contains("event: delta"),
            "no text is sent from a blocked source: {text}"
        );
        assert!(
            !text.contains(BLOCKED),
            "the blocked host is not echoed: {text}"
        );
    }

    #[tokio::test]
    async fn the_answer_stream_with_only_allowed_sources_is_forwarded_whole() {
        let citations = json!([{ "url": format!("https://{ALLOWED}/a") }]);
        let (world, _quarry) = stream_world(true, "enforce", citations).await;
        let text = answer_text(&world).await;
        assert!(
            text.contains("event: citations")
                && text.contains("event: delta")
                && text.contains("event: done"),
            "{text}"
        );
        assert!(!text.contains("event: error"), "{text}");
    }

    #[tokio::test]
    async fn the_answer_stream_is_untouched_under_shadow_and_with_the_switch_off() {
        let citations = json!([{ "url": format!("https://{BLOCKED}/b") }]);
        for (switch, mode) in [(true, "shadow"), (true, "off"), (false, "enforce")] {
            let (world, _quarry) = stream_world(switch, mode, citations.clone()).await;
            let text = answer_text(&world).await;
            assert!(
                text.contains("event: delta") && !text.contains("event: error"),
                "{switch} {mode}: {text}"
            );
            if !switch {
                assert_eq!(calls(&world.model, "/v1/web/domain-check").await, 0);
            }
        }
    }

    #[tokio::test]
    async fn the_answer_stream_ends_in_a_typed_error_when_the_rules_cannot_be_read() {
        let citations = json!([{ "url": format!("https://{ALLOWED}/a") }]);
        let (mut world, _quarry) = stream_world(true, "enforce", citations).await;
        let model = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&model)
            .await;
        world.state.model_gateway_url = model.uri();
        let text = answer_text(&world).await;
        assert!(text.contains("org_site_check_unavailable"), "{text}");
        assert!(!text.contains("event: delta"), "{text}");
    }

    /// What Quarry's answer cache replays: an empty `citations` event, the cached text, then `done` with `cached: true`.
    const CACHED_REPLAY: &str = "event: citations\ndata: []\n\nevent: delta\ndata: {\"delta\":\"cached words\"}\n\nevent: done\ndata: {\"answer\":\"cached words\",\"cached\":true}\n\n";

    #[tokio::test]
    async fn a_cached_replay_names_no_source_so_enforcement_refuses_it_before_any_text() {
        let (world, _quarry) = stream_world_sse(true, "enforce", CACHED_REPLAY.to_owned()).await;
        let text = answer_text(&world).await;
        assert!(text.contains("event: error"), "{text}");
        assert!(text.contains("org_site_sources_unknown"), "{text}");
        assert!(
            !text.contains("event: delta") && !text.contains("cached words"),
            "no cached text goes out: {text}"
        );
        assert_eq!(
            calls(&world.model, "/v1/web/domain-check").await,
            1,
            "only the mode is read, once"
        );
    }

    #[tokio::test]
    async fn a_cached_replay_that_is_only_a_done_with_text_is_refused_too() {
        let sse = "event: citations\ndata: []\n\nevent: done\ndata: {\"answer\":\"cached words\",\"cached\":true}\n\n";
        let (world, _quarry) = stream_world_sse(true, "enforce", sse.to_owned()).await;
        let text = answer_text(&world).await;
        assert!(text.contains("org_site_sources_unknown"), "{text}");
        assert!(!text.contains("cached words"), "{text}");
    }

    #[tokio::test]
    async fn a_cached_replay_passes_under_shadow_mode_off_and_with_the_switch_off() {
        for (switch, mode) in [(true, "shadow"), (true, "off"), (false, "enforce")] {
            let (world, _quarry) = stream_world_sse(switch, mode, CACHED_REPLAY.to_owned()).await;
            let text = answer_text(&world).await;
            assert_eq!(text, CACHED_REPLAY, "{switch} {mode}: forwarded as it came");
            if !switch {
                assert_eq!(calls(&world.model, "/v1/web/domain-check").await, 0);
            }
        }
    }

    #[tokio::test]
    async fn a_cached_replay_is_refused_when_the_rules_cannot_be_read() {
        let (mut world, _quarry) =
            stream_world_sse(true, "enforce", CACHED_REPLAY.to_owned()).await;
        let model = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&model)
            .await;
        world.state.model_gateway_url = model.uri();
        let text = answer_text(&world).await;
        assert!(text.contains("org_site_check_unavailable"), "{text}");
        assert!(!text.contains("cached words"), "{text}");
    }

    #[tokio::test]
    async fn a_true_no_source_answer_is_forwarded_unchanged_under_enforcement() {
        let sse = "event: citations\ndata: []\n\nevent: done\ndata: {\"answer\":\"\",\"cached\":false}\n\n";
        let (world, _quarry) = stream_world_sse(true, "enforce", sse.to_owned()).await;
        let text = answer_text(&world).await;
        assert_eq!(text, sse);
        assert_eq!(
            calls(&world.model, "/v1/web/domain-check").await,
            0,
            "no text, nothing to judge"
        );
    }
}
