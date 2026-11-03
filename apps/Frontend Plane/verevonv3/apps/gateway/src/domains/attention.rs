//! `/api/v1/attention`: the caller's threads that need a person or are working,
//! and the route that marks one thread read.
//!
//! Thin authenticated proxy to model-gateway's attention surface (`GET
//! /v1/attention`, `POST /v1/threads/{id}/seen`, and the live stream `GET
//! /v1/attention/stream`), which reads the state Session Core derives. Org and user
//! come from the verified session, never from the request, so a client cannot read or
//! mark another person's threads. The answer is re-keyed to the SPA's camelCase and
//! carries identifiers and the closed state only. The stream is relayed as it arrives,
//! under a per-person cap held by this process. Web push (WS16 S6) is not served yet, so
//! `capabilities.push` reports off.

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, PoisonError},
};

use axum::{
    body::Body,
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::chat::shared::{
        delegated_auth_unavailable, model_token, proxy_model_json_with_session,
        required_session_token,
    },
    envelope::{error, ok},
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
    upstream::{authorized_org_id, proxy_sse_stream_with_session},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/attention", get(get_attention))
        .route("/api/v1/attention/seen", post(mark_seen))
        .route("/api/v1/attention/stream", get(stream_attention))
        // Ordered like run_watchers.rs: `require_session` runs first and inserts the
        // AuthenticatedUser that the rate limiter keys on.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// The most attention streams one person may hold through this process at once. A stream past the cap
/// is refused, so one browser profile with many tabs cannot take every connection the proxy has.
const MAX_STREAMS_PER_USER: usize = 4;

/// The attention streams this process holds open, per organization and user.
static STREAMS: LazyLock<StreamLimiter> =
    LazyLock::new(|| StreamLimiter::new(MAX_STREAMS_PER_USER));

struct StreamLimiter {
    limit: usize,
    open: Mutex<HashMap<(String, String), usize>>,
}

impl StreamLimiter {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            open: Mutex::new(HashMap::new()),
        }
    }

    /// A slot for one more stream of this person, or `None` when the person already holds the cap.
    fn acquire(&'static self, org_id: &str, user_id: &str) -> Option<StreamPermit> {
        let key = (org_id.to_owned(), user_id.to_owned());
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let count = open.entry(key.clone()).or_insert(0);
        if *count >= self.limit {
            return None;
        }
        *count += 1;
        Some(StreamPermit {
            limiter: self,
            key,
        })
    }

    fn release(&self, key: &(String, String)) {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = open.get_mut(key) {
            *count -= 1;
            if *count == 0 {
                open.remove(key);
            }
        }
    }
}

/// One open attention stream. Dropping it frees the slot.
struct StreamPermit {
    limiter: &'static StreamLimiter,
    key: (String, String),
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        self.limiter.release(&self.key);
    }
}

/// Model Gateway's `GET /v1/attention` answer, as the proxy reads it.
#[derive(Deserialize)]
struct UpstreamAttention {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    items: Vec<UpstreamItem>,
    #[serde(default)]
    counts: UpstreamCounts,
    /// Whether Session Core serves the live stream (WS16 S5). Absent or false keeps the browser polling.
    #[serde(default)]
    stream: bool,
}

#[derive(Deserialize)]
struct UpstreamItem {
    thread_id: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    unseen: bool,
    #[serde(default)]
    run_id: Option<String>,
}

#[derive(Default, Deserialize)]
struct UpstreamCounts {
    #[serde(default)]
    needs_you: i64,
    #[serde(default)]
    working: i64,
    #[serde(default)]
    unseen: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkSeenRequest {
    thread_id: String,
}

/// `GET /api/v1/attention`: the caller's attention items and the counts behind the badge.
async fn get_attention(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let token = model_token(&state, &user, &headers).await;
    let session_token = match required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(failure) => return delegated_auth_unavailable(failure).into_response(),
    };
    let url = format!("{}/v1/attention", state.model_gateway_url);
    let (status, Json(payload)) = proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&session_token),
        &user,
    )
    .await;
    if !status.is_success() {
        return (status, Json(payload)).into_response();
    }
    match serde_json::from_value::<UpstreamAttention>(payload) {
        Ok(upstream) => Json(ok(attention_view(upstream))).into_response(),
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "invalid_model_gateway_response",
                "Model Gateway returned an invalid attention listing.",
            )),
        )
            .into_response(),
    }
}

/// `POST /api/v1/attention/seen`: mark one of the caller's threads read. Session Core
/// refuses a zero-retention or waived caller, and that refusal reaches the SPA as Model
/// Gateway sent it.
async fn mark_seen(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<MarkSeenRequest>,
) -> Response {
    let thread_id = body.thread_id.trim();
    if thread_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_request", "threadId is required.")),
        )
            .into_response();
    }
    let token = model_token(&state, &user, &headers).await;
    let session_token = match required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(failure) => return delegated_auth_unavailable(failure).into_response(),
    };
    let url = seen_url(&state.model_gateway_url, thread_id);
    let (status, Json(payload)) = proxy_model_json_with_session(
        &state,
        Method::POST,
        &url,
        None,
        token.as_deref(),
        Some(&session_token),
        &user,
    )
    .await;
    if !status.is_success() {
        return (status, Json(payload)).into_response();
    }
    Json(ok(json!({ "threadId": thread_id }))).into_response()
}

/// `GET /api/v1/attention/stream`: the caller's attention stream, relayed from Model Gateway as it
/// arrives (WS16 S5). A stream past the cap is refused before it opens, and its slot is freed when the
/// stream ends.
async fn stream_attention(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    let Some(permit) = STREAMS.acquire(&org_id, &user.user_id) else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(error(
                "attention_stream_limit",
                "Too many attention streams are open for this account.",
            )),
        )
            .into_response();
    };
    let token = model_token(&state, &user, &headers).await;
    let session_token = match required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(failure) => return delegated_auth_unavailable(failure).into_response(),
    };
    let response = proxy_sse_stream_with_session(
        &state,
        Method::GET,
        &stream_url(&state.model_gateway_url),
        None,
        token.as_deref(),
        Some(session_token.as_str()),
        None,
        None,
        Some((user.user_id.as_str(), org_id.as_str())),
        false,
    )
    .await;
    hold_permit(response, permit)
}

/// Keeps the stream's slot until the body ends, so a stream counts for as long as the browser keeps it.
fn hold_permit(response: Response, permit: StreamPermit) -> Response {
    let (parts, body) = response.into_parts();
    let held = body.into_data_stream().map(move |chunk| {
        let _held = &permit;
        chunk
    });
    Response::from_parts(parts, Body::from_stream(held))
}

fn stream_url(base: &str) -> String {
    format!("{base}/v1/attention/stream")
}

fn seen_url(base: &str, thread_id: &str) -> String {
    format!("{base}/v1/threads/{}/seen", urlencoding::encode(thread_id))
}

/// The SPA's view of the listing: camelCase keys, the closed state as its wire name, and
/// the counts behind the badge. A state this build does not name arrives absent, not guessed.
fn attention_view(upstream: UpstreamAttention) -> Value {
    let items: Vec<Value> = upstream
        .items
        .into_iter()
        .map(|item| {
            json!({
                "threadId": item.thread_id,
                "state": item.state,
                "reason": item.reason,
                "since": item.since,
                "unseen": item.unseen,
                "runId": item.run_id,
            })
        })
        .collect();
    json!({
        "enabled": upstream.enabled,
        "items": items,
        "counts": {
            "needsYou": upstream.counts.needs_you,
            "working": upstream.counts.working,
            "unseen": upstream.counts.unseen,
        },
        "capabilities": { "stream": upstream.stream, "push": false },
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn upstream(value: Value) -> UpstreamAttention {
        serde_json::from_value(value).expect("a listing the proxy can read")
    }

    #[test]
    fn the_listing_reaches_the_spa_in_camel_case_with_the_closed_state() {
        let view = attention_view(upstream(json!({
            "enabled": true,
            "items": [{
                "thread_id": "thread-1",
                "state": "needs_approval",
                "reason": "approval",
                "since": "2026-10-30T09:00:00Z",
                "unseen": true,
                "run_id": "run-1",
            }],
            "counts": { "needs_you": 1, "working": 0, "unseen": 1 },
        })));

        assert_eq!(view["enabled"], true);
        assert_eq!(view["items"][0]["threadId"], "thread-1");
        assert_eq!(view["items"][0]["state"], "needs_approval");
        assert_eq!(view["items"][0]["runId"], "run-1");
        assert_eq!(view["counts"]["needsYou"], 1);
        assert_eq!(view["counts"]["unseen"], 1);
        assert_eq!(view["capabilities"]["stream"], false);
        assert_eq!(view["capabilities"]["push"], false);
    }

    #[test]
    fn a_switched_off_replica_reads_as_disabled_and_empty() {
        let view = attention_view(upstream(json!({
            "enabled": false,
            "items": [],
            "counts": { "needs_you": 0, "working": 0, "unseen": 0 },
        })));

        assert_eq!(view["enabled"], false);
        assert_eq!(view["items"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn a_listing_without_counts_or_a_state_reads_as_zero_and_absent() {
        let view = attention_view(upstream(json!({ "items": [{ "thread_id": "thread-2" }] })));

        assert_eq!(view["enabled"], false);
        assert!(view["items"][0]["state"].is_null());
        assert_eq!(view["counts"]["needsYou"], 0);
    }

    #[test]
    fn the_thread_id_is_one_encoded_path_segment() {
        assert_eq!(
            seen_url("http://model-gateway", "a/b c"),
            "http://model-gateway/v1/threads/a%2Fb%20c/seen"
        );
    }

    #[test]
    fn the_listing_names_the_stream_only_when_model_gateway_serves_it() {
        let view = attention_view(upstream(json!({ "enabled": true, "stream": true })));
        assert_eq!(view["capabilities"]["stream"], true);
        assert_eq!(view["capabilities"]["push"], false);
    }

    #[test]
    fn the_stream_is_relayed_from_the_model_gateway_route() {
        assert_eq!(
            stream_url("http://model-gateway"),
            "http://model-gateway/v1/attention/stream"
        );
    }

    #[test]
    fn a_person_holds_the_stream_cap_per_organization_and_a_slot_frees_on_drop() {
        let limiter: &'static StreamLimiter = Box::leak(Box::new(StreamLimiter::new(1)));
        let held = limiter.acquire("org-1", "user-1").expect("the first stream");
        assert!(limiter.acquire("org-1", "user-1").is_none());
        assert!(limiter.acquire("org-2", "user-1").is_some());
        drop(held);
        assert!(limiter.acquire("org-1", "user-1").is_some());
    }
}
