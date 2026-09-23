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
    cache,
    config::AppState,
    envelope::{error, ok},
    middleware::{require_session, AuthenticatedUser},
    upstream::proxy_sse_stream,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/search/web", post(search_web))
        .route("/api/v1/search/similar", post(search_similar))
        .route("/api/v1/search/suggest", post(search_suggest))
        .route("/api/v1/search/images", post(search_images))
        .route("/api/v1/search/videos", post(search_videos))
        .route("/api/v1/search/suggestions", get(search_suggestions))
        .route("/api/v1/search/answer/stream", post(search_answer_stream))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// `POST /api/v1/search/web` — keyword search, or a single-page fetch when the
/// query is a URL / bare domain. Mirrors verevonv2's `search/web` route.
async fn search_web(
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
    let include_answer = body
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
    let include_domains = string_array(
        body.get("includeDomains")
            .or_else(|| body.get("include_domains")),
    );
    let exclude_domains = string_array(
        body.get("excludeDomains")
            .or_else(|| body.get("exclude_domains")),
    );

    let token = quarry_token(&state, &user, &headers).await;

    // URL / bare-domain path → scrape the page so the searchbar can "open" links.
    if is_url_input(&query) {
        let target = normalize_url(&query);
        let url = format!("{}/v1/scrape", state.quarry_edge_url);
        let browser_payload = json!({
            "url": target,
            "maxPages": 1,
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
        let static_payload = json!({
            "url": target,
            "maxPages": 1,
            "formats": ["markdown"],
        });
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
                    target = %target,
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
    let key = cache::cache_key(
        "search-web",
        &[
            &query,
            &limit.to_string(),
            &include_answer.to_string(),
            &filters_sig,
        ],
    );
    let mut stale: Option<Value> = None;
    if let Some(hit) = state.cache.lookup(&key).await {
        if hit.fresh {
            return (StatusCode::OK, Json(ok(hit.data)));
        }
        stale = Some(hit.data);
    }

    let url = format!("{}/v1/search", state.quarry_edge_url);
    let mut payload = json!({
        "query": query,
        "limit": limit,
        "include_answer": include_answer,
        "safe_search": true,
    });
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
    if !include_domains.is_empty() {
        payload["include_domains"] = json!(include_domains);
    }
    if !exclude_domains.is_empty() {
        payload["exclude_domains"] = json!(exclude_domains);
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
            let answer = data
                .get("answer")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(Value::from)
                .unwrap_or(Value::Null);
            let citations = data.get("citations").cloned().unwrap_or_else(|| json!([]));
            let cached_payload = json!({
                "mode": "search",
                "results": results,
                "answer": answer,
                "citations": citations,
            });
            state.cache.store(&key, &cached_payload).await;
            (StatusCode::OK, Json(ok(cached_payload)))
        }
        Err(envelope) => match stale {
            Some(data) => (StatusCode::OK, Json(ok(data))),
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
            (
                StatusCode::OK,
                Json(ok(json!({ "mode": "similar", "results": results }))),
            )
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
        Ok(data) => (
            StatusCode::OK,
            Json(ok(json!({ "images": sanitize_images(data.get("images")) }))),
        ),
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
        Ok(data) => (
            StatusCode::OK,
            Json(ok(json!({ "videos": sanitize_videos(data.get("videos")) }))),
        ),
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
async fn search_suggestions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    let empty = || (StatusCode::OK, Json(ok(json!({ "suggestions": [] }))));

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
    let payload = json!({ "query": query });
    proxy_sse_stream(
        &state,
        Method::POST,
        &url,
        Some(payload),
        token.as_deref(),
        None,
        None,
        false,
    )
    .await
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
}
