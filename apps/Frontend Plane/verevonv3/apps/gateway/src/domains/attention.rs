//! `/api/v1/attention`: the caller's threads that need a person or are working,
//! and the route that marks one thread read.
//!
//! Thin authenticated proxy to model-gateway's attention surface (`GET
//! /v1/attention`, `POST /v1/threads/{id}/seen`), which reads the state Session
//! Core derives. Org and user come from the verified session, never from the
//! request, so a client cannot read or mark another person's threads. The answer
//! is re-keyed to the SPA's camelCase and carries identifiers and the closed state
//! only. The attention stream (WS16 S5) and web push (WS16 S6) are not served yet,
//! so `capabilities` reports both off.

use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
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
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/attention", get(get_attention))
        .route("/api/v1/attention/seen", post(mark_seen))
        // Ordered like run_watchers.rs: `require_session` runs first and inserts the
        // AuthenticatedUser that the rate limiter keys on.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
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
        "capabilities": { "stream": false, "push": false },
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
}
