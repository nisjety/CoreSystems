use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use reqwest::Method;
use serde_json::Value;
use std::time::Duration;

use crate::{config::AppState, domains::chat::shared, middleware::AuthenticatedUser};

// Integration Core permits subscription-backed Codex requests to run for up
// to 90 seconds. Leave a small proxy/response margin so a successful provider
// result is not discarded by the gateway first.
const SUBSCRIPTION_CHAT_INVOKE_TIMEOUT: Duration = Duration::from_secs(100);

fn chat_invoke_timeout(body: &Value) -> Option<Duration> {
    (body.get("provider").and_then(Value::as_str) == Some("openai-codex-subscription"))
        .then_some(SUBSCRIPTION_CHAT_INVOKE_TIMEOUT)
}

pub(super) async fn invoke_chat(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let data_plane_token = shared::data_plane_token(&state, &user, &headers).await;
    let inference_token = match shared::required_inference_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let execution_token = match shared::required_execution_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let cost_token = match shared::required_cost_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let session_token = match shared::required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let org_name = crate::domains::auth::resolve_org_name(&state, &user, &org_id).await;
    let url = format!("{}/v1/invoke", state.model_gateway_url);
    let outbound_body = shared::normalized_model_body(body, &headers);
    let mut outbound_body = super::support::enrich_model_body(&state, &user, outbound_body).await;
    if let Err(message) = super::history::enforce_support_thread_policy(&mut outbound_body) {
        return shared::invalid_chat_request(message);
    }
    // ADR-0003 — must run before `inject_personal_thread_context`, which
    // consumes the Space reference fields this one still needs to peek at.
    if let Err((status, body)) = crate::domains::spaces::inject_authored_instructions(
        &state,
        &user,
        &org_id,
        &mut outbound_body,
    )
    .await
    {
        return (status, body);
    }
    if let Err((status, body)) = crate::domains::spaces::inject_personal_thread_context(
        &state,
        &user,
        &org_id,
        &mut outbound_body,
    )
    .await
    {
        return (status, body);
    }
    let mut outbound_body =
        shared::with_identity_context(outbound_body, &user.user_name, &org_name);
    shared::apply_org_zdr_posture(&state, &user, &mut outbound_body).await;
    let request_timeout = chat_invoke_timeout(&outbound_body);
    // After `inject_personal_thread_context`, not with the other bearers above:
    // it is only minted once Control has actually scoped this turn to a Space.
    let sandbox_token = shared::sandbox_token(&state, &user, &headers, &outbound_body).await;
    // Same rule as the streaming path: mark before dispatch, so a ZDR turn that
    // fails still leaves the thread non-persistable.
    shared::proxy_model_json_with_data_plane_request_timeout(
        &state,
        Method::POST,
        &url,
        Some(outbound_body),
        token.as_deref(),
        data_plane_token.as_deref(),
        Some(&inference_token),
        Some(&execution_token),
        Some(&cost_token),
        Some(&session_token),
        sandbox_token.as_deref(),
        request_timeout,
        &user,
    )
    .await
}

pub(crate) async fn cancel_invocation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/invoke/{}/cancel",
        state.model_gateway_url,
        urlencoding::encode(&request_id),
    );
    shared::proxy_model_json(&state, Method::POST, &url, None, token.as_deref(), &user).await
}

/// Deliver a message the user typed mid-stream to the running agent.
///
/// The browser may name the Space it is in (`space_ref`) and the thread it
/// believes it is on, but it supplies **no authority**:
/// `inject_personal_thread_context` strips every authority field and mints a
/// fresh, content-bound `thread:append` decision from Control for this exact
/// message — the same rule the ordinary send path follows. A scoped thread that
/// cannot get a decision is refused here rather than appended without one.
pub(crate) async fn queue_invocation_input(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let mut outbound_body = body;
    if let Err((status, body)) = crate::domains::spaces::inject_personal_thread_context(
        &state,
        &user,
        &org_id,
        &mut outbound_body,
    )
    .await
    {
        return (status, body).into_response();
    }
    let url = format!(
        "{}/v1/invoke/{}/queue",
        state.model_gateway_url,
        urlencoding::encode(&request_id),
    );
    // model-gateway's /v1/invoke/:id/queue (invoke_queue_input) requires the
    // delegated session bearer (VerifiedModelBearer aliases VerifiedSessionBearer
    // in model-gateway's http_routes.rs), the same as submit_feedback above —
    // without it the call 401s before the queue logic ever runs.
    let session_token = shared::session_token(&state, &user, &headers).await;
    normalize_queue_response(
        shared::proxy_model_json_with_session(
            &state,
            Method::POST,
            &url,
            Some(outbound_body),
            token.as_deref(),
            session_token.as_deref(),
            &user,
        )
        .await,
    )
    .into_response()
}

/// A run ending between the browser's keystroke and the enqueue request is an
/// expected lifecycle race, not a missing API resource. Model Gateway uses 404
/// to keep inactive and foreign request ids indistinguishable; at the
/// browser-facing boundary we retain that indistinguishable payload while
/// returning 200 so DevTools does not report a handled retry as a failed HTTP
/// request. The explicit flag prevents unrelated upstream 404s from being
/// normalized.
fn normalize_queue_response(response: (StatusCode, Json<Value>)) -> (StatusCode, Json<Value>) {
    let (status, body) = response;
    let run_ended = status == StatusCode::NOT_FOUND
        && body.0.get("resend_as_new_turn").and_then(Value::as_bool) == Some(true);
    if run_ended {
        (StatusCode::OK, body)
    } else {
        (status, body)
    }
}

pub(super) async fn get_thread_messages(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let session_token = shared::session_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/threads/{}/messages",
        state.model_gateway_url,
        urlencoding::encode(&thread_id),
    );
    shared::proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await
}

/// Read-only proxy for the canonical Session Core event replay. The BFF does
/// not deserialize or reimplement the event log; Model Gateway owns the
/// authenticated replay and returns its safe envelope-only projection.
pub(super) async fn get_thread_events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Query(query): Query<ThreadEventsQuery>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let session_token = shared::session_token(&state, &user, &headers).await;
    let mut url = format!(
        "{}/v1/threads/{}/events",
        state.model_gateway_url,
        urlencoding::encode(&thread_id),
    );
    let mut params = Vec::new();
    if let Some(after_event_id) = query
        .after_event_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        params.push(format!(
            "after_event_id={}",
            urlencoding::encode(after_event_id)
        ));
    }
    if let Some(limit) = query.limit.filter(|value| *value > 0) {
        params.push(format!("limit={limit}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    shared::proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await
}

#[derive(serde::Deserialize)]
pub(super) struct ThreadEventsQuery {
    #[serde(default)]
    after_event_id: Option<String>,
    limit: Option<u32>,
}

/// Itemized context window for a thread — the context inspector's read.
///
/// A pure proxy, like every other handler here: model-gateway owns the shape and
/// the authorization, and this forwards the caller's own credentials. `run_id` is
/// passed through as a query parameter rather than re-derived, so the inspector
/// can scope to one run.
pub(super) async fn get_thread_context(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Query(query): Query<ThreadContextQuery>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let session_token = shared::session_token(&state, &user, &headers).await;
    let mut url = format!(
        "{}/v1/threads/{}/context",
        state.model_gateway_url,
        urlencoding::encode(&thread_id),
    );
    if let Some(run_id) = query
        .run_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        url.push_str(&format!("?run_id={}", urlencoding::encode(run_id)));
    }
    shared::proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await
}

#[derive(serde::Deserialize)]
pub(super) struct ThreadContextQuery {
    #[serde(default)]
    run_id: Option<String>,
}

/// Relay a browser-run artifact (a screenshot) from Quarry-v2 to the browser.
///
/// # Why this route exists
///
/// `BrowserObservationReceived.screenshot_ref` is Quarry-v2's own artifact id
/// (`execution-core/src/quarry_agent.rs` maps `screenshot_artifact_id` straight
/// onto it), and Quarry has stored the bytes and served them at
/// `GET /v1/artifacts/{id}` all along. Nothing in between exposed them, so the
/// ref reached the UI with nothing able to display it.
///
/// The token is minted for Quarry **on behalf of this user**, never a service
/// credential: Quarry authorizes the read from the token's own org claim, and a
/// service identity would make every tenant's artifacts readable to anyone
/// holding an id — which is precisely what Quarry's own doc comment warns
/// against ("possession of an id is not by itself authority to read the bytes").
pub(super) async fn get_browser_artifact(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(artifact_id): Path<String>,
) -> impl IntoResponse {
    let cookie = shared::cookie_header(&headers);
    // Quarry's OWN audience, minted for this user. `shared::model_token` would be
    // the wrong credential entirely — Quarry validates the `quarry` audience and
    // reads the org claim off it to authorize the read.
    let token =
        crate::audience_tokens::get_audience_token(&state, &user.user_id, &cookie, "quarry").await;
    let url = format!(
        "{}/v1/artifacts/{}",
        state.quarry_edge_url,
        urlencoding::encode(&artifact_id),
    );
    crate::upstream::proxy_artifact_bytes(&state, &url, token.as_deref(), &user.user_id).await
}

pub(super) async fn list_models(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    // model-gateway's /v1/models proxies to inference-core ListModels, whose
    // handler requires the delegated inference credential (VerifiedInferenceBearer).
    // Forwarding only the model-gateway token 401s the catalog, so the model
    // selector renders empty for EVERY model. Mint + forward the inference bearer.
    let inference_token = match shared::required_inference_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(detail) => return shared::delegated_auth_unavailable(detail),
    };
    let url = format!("{}/v1/models", state.model_gateway_url);
    shared::proxy_model_json_with_inference(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&inference_token),
        &user,
    )
    .await
}

pub(crate) async fn submit_feedback(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    // model-gateway's /v1/feedback (ingest_feedback) requires the delegated
    // session bearer (VerifiedModelBearer is a local alias for
    // VerifiedSessionBearer in model-gateway's http_routes.rs) the same way
    // get_thread_messages/get_thread_context do — forwarding only the
    // model-gateway token 401s the call before resolve_feedback_target ever
    // runs, exactly the gap that left this route unreachable.
    let session_token = shared::session_token(&state, &user, &headers).await;
    let url = format!("{}/v1/feedback", state.model_gateway_url);
    shared::proxy_model_json_with_session(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn subscription_chat_uses_the_extended_model_deadline() {
        assert_eq!(
            chat_invoke_timeout(&json!({"provider": "openai-codex-subscription"})),
            Some(SUBSCRIPTION_CHAT_INVOKE_TIMEOUT),
        );
        assert_eq!(chat_invoke_timeout(&json!({"provider": "openai"})), None);
        assert_eq!(chat_invoke_timeout(&json!({})), None);
    }

    #[test]
    fn ended_queue_race_is_a_successful_browser_protocol_response() {
        let (status, Json(body)) = normalize_queue_response((
            StatusCode::NOT_FOUND,
            Json(json!({
                "queued": false,
                "error": "no active stream",
                "resend_as_new_turn": true,
            })),
        ));

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["queued"], false);
        assert_eq!(body["resend_as_new_turn"], true);
    }

    #[test]
    fn unrelated_not_found_response_stays_not_found() {
        let (status, Json(body)) = normalize_queue_response((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "route not found"})),
        ));

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "route not found");
    }
}
