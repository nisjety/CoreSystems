use axum::{
    http::{header::AUTHORIZATION, HeaderMap, HeaderValue, StatusCode},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};
use std::time::Duration;

use crate::{
    audience_tokens::{
        get_audience_token, get_model_service_token, require_model_service_token,
        ModelServiceAudience, RequiredAudienceTokenError,
    },
    config::AppState,
    envelope::error,
    middleware::AuthenticatedUser,
};

pub(super) fn cookie_header(headers: &HeaderMap) -> String {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

pub(crate) fn zdr_flag(headers: &HeaderMap) -> bool {
    headers
        .get("x-zdr")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
}

pub(crate) fn normalized_model_body(mut body: Value, headers: &HeaderMap) -> Value {
    if let Some(object) = body.as_object_mut() {
        // Model Gateway's privacy boundary is protobuf-backed and therefore
        // accepts the numeric PrivacyTier ordinal. Older SPA tabs sent the
        // readable catalogue name instead (for example `"global"`), which
        // Axum rejected as 422 before the invoke handler could report a useful
        // error. Keep that deployed-client compatibility here at the BFF
        // boundary. Unknown values are deliberately preserved so Model Gateway
        // still fails closed rather than silently weakening the requested tier.
        if let Some(tier) = object.get("min_privacy_tier").and_then(Value::as_str) {
            let ordinal = match tier.trim().to_ascii_lowercase().as_str() {
                "unspecified" => Some(0),
                "global" => Some(1),
                "eu_resident" | "euresident" => Some(2),
                "zdr_contractual" | "zdrcontractual" => Some(3),
                "sovereign" => Some(4),
                _ => None,
            };
            if let Some(ordinal) = ordinal {
                object.insert("min_privacy_tier".to_owned(), Value::from(ordinal));
            }
        }
        match object.get("zdr") {
            None => {
                object.insert("zdr".to_owned(), Value::Bool(zdr_flag(headers)));
            }
            Some(Value::Bool(body_zdr)) => {
                object.insert(
                    "zdr".to_owned(),
                    Value::Bool(zdr_flag(headers) || *body_zdr),
                );
            }
            Some(_) => {
                // Preserve malformed values so Model Gateway's typed request
                // boundary rejects them instead of silently downgrading ZDR.
            }
        }
    }
    body
}

/// Raise this turn's Zero Data Retention posture to include the ORGANISATION's
/// standing one, in place, before the body goes upstream.
///
/// `normalized_model_body` already ORs the `x-zdr` header with the body's own
/// `zdr` field, so a caller can RAISE the posture and never lower it. What it
/// cannot see is the org: an organisation configured "retain nothing" was
/// honoured only when some client remembered to say so on the request. A UI
/// that forgot the flag — or any non-SPA caller — silently sent `zdr: false`
/// and the Model Plane was entitled to persist the turn.
///
/// This is a one-way raise, matching the rest of the chain: the org can turn
/// retention OFF for everyone, and nothing a request says can turn it back on.
///
/// Replaces an earlier durable per-thread ZDR marker. That marker guarded the
/// BFF's own save path, which no longer exists — Session Core / Model Gateway
/// is the sole conversation owner and this gateway retains no transcript to
/// suppress. Propagating the posture is the part that still matters, and doing
/// it here means the decision is made once, for both the streaming and the
/// JSON path, from the same normalized body that is actually sent.
pub(crate) async fn apply_org_zdr_posture(
    state: &AppState,
    user: &AuthenticatedUser,
    normalized_body: &mut Value,
) {
    let already_zdr = normalized_body
        .get("zdr")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // Skip the org lookup when the request already asks for ZDR: the result
    // could only be the same `true`, and `org_zdr_enabled` fails CLOSED, so a
    // needless call during an org-core blip would cost a cache miss to reach
    // the answer we already have.
    if already_zdr {
        return;
    }
    if !crate::zdr::org_zdr_enabled(state, user).await {
        return;
    }
    if let Some(object) = normalized_body.as_object_mut() {
        object.insert("zdr".to_owned(), Value::Bool(true));
    }
}

pub(crate) fn with_identity_context(mut body: Value, user_name: &str, org_name: &str) -> Value {
    if let Some(object) = body.as_object_mut() {
        if !user_name.trim().is_empty() {
            object.insert(
                "user_name".to_owned(),
                Value::String(user_name.trim().to_owned()),
            );
        }
        if !org_name.trim().is_empty() {
            object.insert(
                "org_name".to_owned(),
                Value::String(org_name.trim().to_owned()),
            );
        }
    }
    body
}

pub(crate) fn data_plane_authorization_value(token: &str) -> Option<HeaderValue> {
    let token = token.trim();
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        return None;
    }
    HeaderValue::from_str(&format!("Bearer {token}")).ok()
}

pub(crate) async fn model_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Option<String> {
    if let Some(token) = get_model_service_token(
        state,
        &user.user_id,
        &cookie_header(headers),
        ModelServiceAudience::ModelGateway,
    )
    .await
    {
        return Some(token);
    }

    if !state.model_gateway_dev_bearer.is_empty() {
        return Some(state.model_gateway_dev_bearer.clone());
    }

    dev_bypass_model_token(state, headers)
}

pub(crate) async fn data_plane_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Option<String> {
    get_audience_token(state, &user.user_id, &cookie_header(headers), "data-plane").await
}

/// Mint the `aud=ingestion` token model-gateway needs to call shipping-core
/// (and any future Ingestion Plane HTTP API) on the caller's behalf for the
/// `shipping.get_quotes` chat tool. Best-effort like `data_plane_token` — a
/// missing credential degrades that one tool call, never the whole chat turn.
pub(crate) async fn ingestion_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Option<String> {
    get_audience_token(state, &user.user_id, &cookie_header(headers), "ingestion").await
}

/// Mint the dedicated interactive `aud=session-core` token. It is forwarded
/// separately from the Model/Data credentials and session-core independently
/// validates it before any durable session, run, or approval operation.
pub(crate) async fn session_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Option<String> {
    get_model_service_token(
        state,
        &user.user_id,
        &cookie_header(headers),
        ModelServiceAudience::SessionCore,
    )
    .await
}

/// True once `inject_personal_thread_context` has attached Control-issued
/// Space authority to this turn. Only the two keys that function itself
/// inserts count; a bare `space_ref` is the browser's *selection*, which that
/// same function consumes before Control is ever asked, so it is never
/// authority. Meaningful only AFTER injection has run — before it, forged
/// copies of these keys may still be present.
pub(crate) fn is_space_scoped_turn(body: &Value) -> bool {
    ["space_context", "space_append_context"]
        .iter()
        .any(|key| matches!(body.get(*key), Some(Value::Object(_))))
}

/// Mint the interactive `aud=sandbox-manager` token — the ninth delegated
/// user bearer — for a Space-scoped turn only. sandbox-manager's `AcquireLease`
/// binds the Space capability decision to the CALLER's verified identity,
/// which must be the Space member the decision names, so execution-core
/// cannot present its own service credential for that one call; it presents
/// this instead, and this alone (every later lease RPC uses its own token).
/// Best-effort like `data_plane_token`: a missing credential degrades one
/// tool (`code_interpreter` in a Space) — execution-core refuses that step
/// rather than falling back to an unscoped workspace — never the whole turn.
/// Nothing is minted for a non-Space turn, so the common path pays no extra
/// auth-core round-trip. See
/// apps/Frontend Plane/verevonv3/docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md
/// §3.5 phase B.2.
pub(crate) async fn sandbox_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    body: &Value,
) -> Option<String> {
    if !is_space_scoped_turn(body) {
        return None;
    }
    get_model_service_token(
        state,
        &user.user_id,
        &cookie_header(headers),
        ModelServiceAudience::SandboxManager,
    )
    .await
}

async fn required_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    audience: ModelServiceAudience,
) -> Result<String, RequiredAudienceTokenError> {
    require_model_service_token(state, &user.user_id, &cookie_header(headers), audience).await
}

pub(crate) async fn required_capability_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<String, RequiredAudienceTokenError> {
    required_token(state, user, headers, ModelServiceAudience::CapabilityCore).await
}

/// Best-effort `aud=capability-core` bearer for a chat-stream call site.
///
/// Unlike `required_inference_token`/`required_execution_token`/etc above —
/// whose callers fail the whole turn on a mint error — a chat stream must
/// keep working when capability-core is briefly unavailable. model-gateway's
/// moderation.rs already fails closed (redacts everything) when this header
/// is absent, so degrading to `None` here is safe; it only costs the org's
/// configured PII/injection-defense policy for this one turn, instead of
/// leaving that policy permanently unreachable from chat by construction
/// (see docs/CHAT_PARITY_AUDIT_2026-09-15.md §3.9, finding F-14).
pub(crate) async fn best_effort_capability_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Option<String> {
    match required_capability_token(state, user, headers).await {
        Ok(token) => Some(token),
        Err(error) => {
            tracing::warn!(
                audience = error.audience.claim(),
                "capability bearer mint failed for chat stream; PII/injection-defense policy lookup falls back to fail-closed redaction"
            );
            None
        }
    }
}

/// The user-bound `aud=sandbox-manager` credential, minted PER REQUEST for
/// the Work tab (S4.2 §7).
///
/// Deliberately not `sandbox_token` below, which only fires inside a
/// Control-injected turn (`is_space_scoped_turn`) — the Work tab is not a
/// turn, so that helper would return nothing and the section would degrade to
/// a gap row forever. This mints the same audience the chat path does, on the
/// same per-user basis, for a caller that has a room open rather than a
/// message in flight.
pub(crate) async fn required_sandbox_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<String, RequiredAudienceTokenError> {
    required_token(state, user, headers, ModelServiceAudience::SandboxManager).await
}

pub(crate) async fn required_cost_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<String, RequiredAudienceTokenError> {
    required_token(state, user, headers, ModelServiceAudience::CostCore).await
}

pub(crate) async fn required_session_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<String, RequiredAudienceTokenError> {
    required_token(state, user, headers, ModelServiceAudience::SessionCore).await
}

pub(crate) async fn required_inference_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<String, RequiredAudienceTokenError> {
    required_token(state, user, headers, ModelServiceAudience::InferenceCore).await
}

pub(crate) async fn required_execution_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<String, RequiredAudienceTokenError> {
    required_token(state, user, headers, ModelServiceAudience::ExecutionCore).await
}

pub(crate) fn delegated_auth_unavailable(
    error_detail: RequiredAudienceTokenError,
) -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "delegated_auth_unavailable",
            format!(
                "Required {} authentication is temporarily unavailable",
                error_detail.audience.claim()
            ),
        )),
    )
}

pub(crate) fn invalid_chat_request(message: &'static str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(error("invalid_request", message)),
    )
}

fn dev_bypass_model_token(state: &AppState, headers: &HeaderMap) -> Option<String> {
    if !state.allow_dev_auth_bypass {
        return None;
    }

    let raw = headers.get(AUTHORIZATION)?.to_str().ok()?.trim();
    raw.strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| *token == "dev-bypass")
        .map(str::to_owned)
}

pub(crate) async fn proxy_model_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    proxy_model_json_with_delegations(
        state,
        method,
        url,
        body,
        bearer_token,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        user,
    )
    .await
}

// Adapter kept explicit because each credential is independently audience-bound;
// collapsing them into an untyped collection would weaken that contract.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn proxy_model_json_with_data_plane(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    data_plane_bearer: Option<&str>,
    inference_bearer: Option<&str>,
    execution_bearer: Option<&str>,
    cost_bearer: Option<&str>,
    session_bearer: Option<&str>,
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    proxy_model_json_with_delegations(
        state,
        method,
        url,
        body,
        bearer_token,
        data_plane_bearer,
        None,
        inference_bearer,
        execution_bearer,
        cost_bearer,
        session_bearer,
        None,
        None,
        user,
    )
    .await
}

// Subscription-backed model invocations can legitimately run longer than the
// gateway's standard 25-second upstream budget. Keep that exception explicit
// and local to chat invocation rather than weakening every upstream call.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn proxy_model_json_with_data_plane_request_timeout(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    data_plane_bearer: Option<&str>,
    inference_bearer: Option<&str>,
    execution_bearer: Option<&str>,
    cost_bearer: Option<&str>,
    session_bearer: Option<&str>,
    sandbox_bearer: Option<&str>,
    request_timeout: Option<Duration>,
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    proxy_model_json_with_delegations(
        state,
        method,
        url,
        body,
        bearer_token,
        data_plane_bearer,
        None,
        inference_bearer,
        execution_bearer,
        cost_bearer,
        session_bearer,
        sandbox_bearer,
        request_timeout,
        user,
    )
    .await
}

pub(crate) async fn proxy_model_json_with_capability(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    capability_bearer: Option<&str>,
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    proxy_model_json_with_delegations(
        state,
        method,
        url,
        body,
        bearer_token,
        None,
        capability_bearer,
        None,
        None,
        None,
        None,
        None,
        None,
        user,
    )
    .await
}

/// Proxy to model-gateway carrying the sandbox-manager delegation, for the
/// Work tab's process reads. The same slot the chat path's Space-scoped turn
/// uses; the difference is only which caller fills it.
pub(crate) async fn proxy_model_json_with_sandbox(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    sandbox_bearer: Option<&str>,
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    proxy_model_json_with_delegations(
        state,
        method,
        url,
        body,
        bearer_token,
        None,
        None,
        None,
        None,
        None,
        None,
        sandbox_bearer,
        None,
        user,
    )
    .await
}

pub(crate) async fn proxy_model_json_with_session(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    session_bearer: Option<&str>,
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    proxy_model_json_with_delegations(
        state,
        method,
        url,
        body,
        bearer_token,
        None,
        None,
        None,
        None,
        None,
        session_bearer,
        None,
        None,
        user,
    )
    .await
}

/// Same as `proxy_model_json_with_session`, plus the `aud=capability-core`
/// bearer — needed on the durable browser-event replay read (see F-14,
/// docs/CHAT_PARITY_AUDIT_2026-09-15.md §3.9) so the org's PII/
/// injection-defense policy is reachable there too, not just on the live
/// stream. A dedicated wrapper rather than adding the parameter to
/// `proxy_model_json_with_session` itself: that function has many unrelated
/// callers outside the chat-stream path that have no capability bearer to
/// give it.
pub(crate) async fn proxy_model_json_with_session_and_capability(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    session_bearer: Option<&str>,
    capability_bearer: Option<&str>,
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    proxy_model_json_with_delegations(
        state,
        method,
        url,
        body,
        bearer_token,
        None,
        capability_bearer,
        None,
        None,
        None,
        session_bearer,
        None,
        None,
        user,
    )
    .await
}

pub(crate) async fn proxy_model_json_with_inference(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    inference_bearer: Option<&str>,
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    proxy_model_json_with_delegations(
        state,
        method,
        url,
        body,
        bearer_token,
        None,
        None,
        inference_bearer,
        None,
        None,
        None,
        None,
        None,
        user,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn proxy_model_json_with_delegations(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    data_plane_bearer: Option<&str>,
    capability_bearer: Option<&str>,
    inference_bearer: Option<&str>,
    execution_bearer: Option<&str>,
    cost_bearer: Option<&str>,
    session_bearer: Option<&str>,
    // Present only for a Space-scoped turn (see `sandbox_token`).
    sandbox_bearer: Option<&str>,
    request_timeout: Option<Duration>,
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    // Forward the validated session role only as a presentation/compatibility
    // hint. Model Plane authorization is derived from verified token claims and
    // scopes; this header must never grant admin authority. A missing role
    // degrades to the non-privileged `member` value.
    let user_role = user
        .auth_role
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .unwrap_or("member");
    let mut req = state
        .client
        .request(method, url)
        .header("x-user-id", &user.user_id)
        .header("x-user-role", user_role);
    if let Some(timeout) = request_timeout {
        req = req.timeout(timeout);
    }
    if !org_id.trim().is_empty() {
        req = req.header("x-org-id", org_id);
    }

    if let Some(token) = bearer_token {
        req = req.bearer_auth(token);
    }
    if let Some(value) = data_plane_bearer.and_then(data_plane_authorization_value) {
        req = req.header("x-data-plane-authorization", value);
    }
    if let Some(value) = capability_bearer.and_then(data_plane_authorization_value) {
        req = req.header("x-capability-authorization", value);
    }
    if let Some(value) = inference_bearer.and_then(data_plane_authorization_value) {
        req = req.header("x-inference-authorization", value);
    }
    if let Some(value) = execution_bearer.and_then(data_plane_authorization_value) {
        req = req.header("x-execution-authorization", value);
    }
    if let Some(value) = cost_bearer.and_then(data_plane_authorization_value) {
        req = req.header("x-cost-authorization", value);
    }
    if let Some(value) = session_bearer.and_then(data_plane_authorization_value) {
        req = req.header("x-session-authorization", value);
    }
    if let Some(value) = sandbox_bearer.and_then(data_plane_authorization_value) {
        req = req.header("x-sandbox-authorization", value);
    }

    if let Some(b) = body {
        req = req.json(&b);
    }

    match req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let b = resp.json::<Value>().await.unwrap_or_else(|_| json!({}));
            (status, Json(b))
        }
        Err(cause) if cause.is_timeout() => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(error(
                "model_request_timeout",
                "The model did not respond before the request deadline",
            )),
        ),
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        http::{HeaderMap, HeaderValue, StatusCode},
        Json,
    };
    use serde_json::json;

    use crate::{
        audience_tokens::{
            new_audience_token_cache, ModelServiceAudience, RequiredAudienceTokenError,
        },
        cache::ResultCache,
        config::AppState,
        middleware::AuthenticatedUser,
    };

    use super::{
        apply_org_zdr_posture, data_plane_authorization_value, delegated_auth_unavailable,
        dev_bypass_model_token, is_space_scoped_turn, normalized_model_body,
        proxy_model_json_with_data_plane, proxy_model_json_with_data_plane_request_timeout,
        proxy_model_json_with_session, proxy_model_json_with_session_and_capability,
        sandbox_token,
    };

    fn test_state(allow_dev_auth_bypass: bool) -> AppState {
        AppState {
            client: reqwest::Client::new(),
            streaming_client: reqwest::Client::new(),
            internal_api_key: "test-key".into(),
            enforcement_mode: "off".to_string(),
            auth_core_url: "http://127.0.0.1:1".into(),
            verevon_public_origin: "http://localhost:5173".into(),
            session_core_url: "http://127.0.0.1:1".into(),
            session_core_service_token: "0123456789abcdef0123456789abcdef".into(),
            user_core_service_token: "abcdef0123456789abcdef0123456789".into(),
            billing_core_url: "http://127.0.0.1:1".into(),
            billing_core_service_token: "billing-test-secret-at-least-32-bytes".into(),
            cost_core_url: "http://127.0.0.1:1".into(),
            org_core_url: "http://127.0.0.1:1".into(),
            org_core_service_token: "org-test-secret-at-least-32-bytes".into(),
            integration_core_url: "http://127.0.0.1:1".into(),
            audit_core_url: "http://127.0.0.1:1".into(),
            audit_core_service_token: "audit-test-secret-at-least-32-bytes".into(),
            insight_core_url: "http://127.0.0.1:1".into(),
            leads_core_url: "http://127.0.0.1:1".into(),
            shipping_core_url: "http://127.0.0.1:1".into(),
            user_core_url: "http://127.0.0.1:1".into(),
            application_convex_url: String::new(),
            application_convex_service_key: String::new(),
            graph_index_url: "http://127.0.0.1:1".into(),
            quarry_edge_url: "http://127.0.0.1:1".into(),
            model_recommend_url: "http://127.0.0.1:1".into(),
            model_gateway_url: "http://127.0.0.1:1".into(),
            model_gateway_dev_bearer: String::new(),
            inference_core_url: "http://127.0.0.1:1".into(),
            documents_api_url: "http://127.0.0.1:1".into(),
            retrieval_engine_url: "http://127.0.0.1:1".into(),
            wiki_store_url: "http://127.0.0.1:1".into(),
            embedding_engine_url: "http://127.0.0.1:1".into(),
            quickwit_adapter_url: "http://127.0.0.1:1".into(),
            finspo_core_url: "http://127.0.0.1:1".into(),
            imports_api_url: "http://127.0.0.1:1".into(),
            notification_core_url: "http://127.0.0.1:1".into(),
            notification_core_service_token: "notification-test-secret-at-least-32-bytes".into(),
            conversation_core_service_token: "conversation-test-secret-at-least-32-bytes".into(),
            information_core_url: "http://127.0.0.1:1".into(),
            conversation_core_url: "http://127.0.0.1:1".into(),
            social_core_url: "http://127.0.0.1:1".into(),
            searxng_url: "http://127.0.0.1:1".into(),
            autocomplete_core_url: "http://127.0.0.1:1".into(),
            autocomplete_token: String::new(),
            zammad_api_url: "http://127.0.0.1:1".into(),
            zammad_api_token: String::new(),
            remote_support_rendezvous_url: String::new(),
            remote_support_relay_url: String::new(),
            remote_support_server_public_key: String::new(),
            audience_token_cache: new_audience_token_cache(),
            browser_run_store: crate::domains::browser::new_browser_run_store(),
            rate_limiter: crate::rate_limit::RateLimiter::from_cache(&ResultCache::disabled()),
            cache: ResultCache::disabled(),
            studio_store: crate::domains::studio::StudioStore::new(),
            allow_dev_actor_headers: false,
            allow_dev_auth_bypass,
        }
    }

    #[tokio::test]
    async fn an_orgs_standing_zdr_raises_a_request_that_did_not_ask_for_it() {
        // The gap this closes: `normalized_model_body` ORs the header with the
        // body, so a client can raise the posture — but an org configured
        // "retain nothing" was honoured only when a client remembered to say
        // so. A UI that forgot the flag, or any non-SPA caller, sent
        // `zdr: false` and the Model Plane was entitled to persist the turn.
        let state = test_state(false);
        let user = AuthenticatedUser {
            user_id: "user-test".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User Test".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-test".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-test".to_owned(),
                role: "member".to_owned(),
            }),
        };

        // `test_state` points org-core at an unroutable address, and
        // `org_zdr_enabled` fails CLOSED — so this also pins the failure
        // direction: during an org-core outage the turn is treated as ZDR
        // rather than persistable.
        let mut body = json!({"content": "hello", "zdr": false});
        apply_org_zdr_posture(&state, &user, &mut body).await;
        assert_eq!(
            body["zdr"],
            json!(true),
            "an org posture (or an unavailable org-core) must raise the turn"
        );

        // And the raise is one-way: an explicit true is never revisited.
        let mut already = json!({"content": "hello", "zdr": true});
        apply_org_zdr_posture(&state, &user, &mut already).await;
        assert_eq!(already["zdr"], json!(true));
    }

    #[test]
    fn a_turn_is_space_scoped_only_once_control_issued_its_context() {
        assert!(!is_space_scoped_turn(&json!({"content": "hello"})));
        // A bare `space_ref` is the browser's SELECTION, not authority — it is
        // exactly what `inject_personal_thread_context` consumes before Control
        // is asked, so it must never be enough on its own.
        assert!(!is_space_scoped_turn(
            &json!({"content": "hello", "space_ref": "space-1"})
        ));
        assert!(!is_space_scoped_turn(&json!({"space_context": null})));
        assert!(!is_space_scoped_turn(&json!({"space_context": "space-1"})));
        assert!(is_space_scoped_turn(
            &json!({"space_context": {"space_id": "space-1"}})
        ));
        assert!(is_space_scoped_turn(
            &json!({"space_append_context": {"space_id": "space-1"}})
        ));
    }

    #[tokio::test]
    async fn the_sandbox_credential_is_minted_only_for_a_space_scoped_turn() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let auth_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/sandbox-manager/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "sandbox-token",
                "expiresInSeconds": 300,
            })))
            .expect(1)
            .mount(&auth_core)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth_core.uri();
        let user = AuthenticatedUser {
            user_id: "user-test".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User Test".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-test".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-test".to_owned(),
                role: "member".to_owned(),
            }),
        };
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "better-auth.session=verified".parse().expect("cookie"),
        );

        // The common, non-Space turn: no credential, and — the point — no
        // round-trip to auth-core at all.
        assert!(
            sandbox_token(&state, &user, &headers, &json!({"content": "hello"}))
                .await
                .is_none()
        );
        assert!(auth_core
            .received_requests()
            .await
            .expect("recording enabled")
            .is_empty());

        assert_eq!(
            sandbox_token(
                &state,
                &user,
                &headers,
                &json!({"content": "hello", "space_context": {"space_id": "space-1"}}),
            )
            .await
            .as_deref(),
            Some("sandbox-token")
        );
    }

    #[tokio::test]
    async fn chat_proxy_forwards_the_sandbox_credential_in_its_own_header() {
        use reqwest::Method;
        use wiremock::{
            matchers::{header, method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/invoke"))
            .and(header("authorization", "Bearer model-token"))
            .and(header("x-sandbox-authorization", "Bearer sandbox-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;

        let state = test_state(false);
        let user = AuthenticatedUser {
            user_id: "user-test".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User Test".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-test".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-test".to_owned(),
                role: "member".to_owned(),
            }),
        };
        let (status, _) = proxy_model_json_with_data_plane_request_timeout(
            &state,
            Method::POST,
            &format!("{}/invoke", server.uri()),
            Some(json!({"prompt": "hello"})),
            Some("model-token"),
            None,
            None,
            None,
            None,
            None,
            Some("sandbox-token"),
            None,
            &user,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // Absent credential, absent header — never an empty `Bearer `.
        let (_, _) = proxy_model_json_with_data_plane_request_timeout(
            &state,
            Method::POST,
            &format!("{}/invoke", server.uri()),
            Some(json!({"prompt": "hello"})),
            Some("model-token"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &user,
        )
        .await;
        let requests = server.received_requests().await.expect("recording enabled");
        assert_eq!(requests.len(), 2);
        assert!(!requests[1].headers.iter().any(|(name, _)| name
            .as_str()
            .eq_ignore_ascii_case("x-sandbox-authorization")));
    }

    #[tokio::test]
    async fn sse_proxy_forwards_the_sandbox_credential_in_its_own_header() {
        use reqwest::Method;
        use wiremock::{
            matchers::{header, method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/invoke/stream"))
            .and(header("authorization", "Bearer model-token"))
            .and(header("x-sandbox-authorization", "Bearer sandbox-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string("data: {}\n\n"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let state = test_state(false);
        let response = crate::upstream::proxy_sse_stream_with_data_plane(
            &state,
            Method::POST,
            &format!("{}/invoke/stream", server.uri()),
            Some(json!({"content": "hello"})),
            Some("model-token"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("sandbox-token"),
            None,
            None,
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    // F-14 (docs/CHAT_PARITY_AUDIT_2026-09-15.md §3.9): `stream_chat` is the
    // live chat-turn path — this confirms a successfully minted capability
    // bearer reaches model-gateway on it as `x-capability-authorization`,
    // which is what unblocks the org's PII/injection-defense policy lookup.
    #[tokio::test]
    async fn sse_proxy_forwards_the_capability_credential_in_its_own_header() {
        use reqwest::Method;
        use wiremock::{
            matchers::{header, method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/invoke/stream"))
            .and(header("authorization", "Bearer model-token"))
            .and(header("x-capability-authorization", "Bearer capability-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string("data: {}\n\n"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let state = test_state(false);
        let response = crate::upstream::proxy_sse_stream_with_data_plane(
            &state,
            Method::POST,
            &format!("{}/invoke/stream", server.uri()),
            Some(json!({"content": "hello"})),
            Some("model-token"),
            None,
            Some("capability-token"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn session_proxy_forwards_dedicated_credential_in_separate_header() {
        use reqwest::Method;
        use wiremock::{
            matchers::{header, method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/session-bound"))
            .and(header("authorization", "Bearer model-token"))
            .and(header("x-session-authorization", "Bearer session-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;

        let state = test_state(false);
        let user = AuthenticatedUser {
            user_id: "user-test".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User Test".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-test".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-test".to_owned(),
                role: "member".to_owned(),
            }),
        };
        let (status, _) = proxy_model_json_with_session(
            &state,
            Method::GET,
            &format!("{}/session-bound", server.uri()),
            None,
            Some("model-token"),
            Some("session-token"),
            &user,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
    }

    // F-14 (docs/CHAT_PARITY_AUDIT_2026-09-15.md §3.9): the chat-stream call
    // sites must forward `x-capability-authorization` on a successful mint so
    // model-gateway's moderation.rs can consult the org's actual PII/
    // injection-defense policy instead of always failing closed. This exercises
    // `run_events_replay`'s proxy function directly, the same way
    // `session_proxy_forwards_dedicated_credential_in_separate_header` above
    // exercises the plain session-only variant.
    #[tokio::test]
    async fn session_proxy_with_capability_forwards_both_credentials_in_separate_headers() {
        use reqwest::Method;
        use wiremock::{
            matchers::{header, method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/events/replay"))
            .and(header("authorization", "Bearer model-token"))
            .and(header("x-session-authorization", "Bearer session-token"))
            .and(header("x-capability-authorization", "Bearer capability-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;

        let state = test_state(false);
        let user = AuthenticatedUser {
            user_id: "user-test".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User Test".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-test".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-test".to_owned(),
                role: "member".to_owned(),
            }),
        };
        let (status, _) = proxy_model_json_with_session_and_capability(
            &state,
            Method::GET,
            &format!("{}/events/replay", server.uri()),
            None,
            Some("model-token"),
            Some("session-token"),
            Some("capability-token"),
            &user,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn chat_proxy_forwards_independent_inference_execution_cost_and_session_credentials() {
        use reqwest::Method;
        use wiremock::{
            matchers::{header, method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/invoke"))
            .and(header("authorization", "Bearer model-token"))
            .and(header("x-data-plane-authorization", "Bearer data-token"))
            .and(header(
                "x-inference-authorization",
                "Bearer inference-token",
            ))
            .and(header(
                "x-execution-authorization",
                "Bearer execution-token",
            ))
            .and(header("x-cost-authorization", "Bearer cost-token"))
            .and(header("x-session-authorization", "Bearer session-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;

        let state = test_state(false);
        let user = AuthenticatedUser {
            user_id: "user-test".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User Test".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-test".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-test".to_owned(),
                role: "member".to_owned(),
            }),
        };
        let (status, _) = proxy_model_json_with_data_plane(
            &state,
            Method::POST,
            &format!("{}/invoke", server.uri()),
            Some(json!({"prompt": "hello"})),
            Some("model-token"),
            Some("data-token"),
            Some("inference-token"),
            Some("execution-token"),
            Some("cost-token"),
            Some("session-token"),
            &user,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn chat_proxy_can_override_the_standard_client_timeout_for_slow_models() {
        use std::time::Duration;

        use reqwest::Method;
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/slow-invoke"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(60))
                    .set_body_json(json!({"ok": true})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut state = test_state(false);
        state.client = reqwest::Client::builder()
            .timeout(Duration::from_millis(10))
            .build()
            .expect("test client");
        let user = AuthenticatedUser {
            user_id: "user-test".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User Test".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-test".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-test".to_owned(),
                role: "member".to_owned(),
            }),
        };

        let (status, _) = proxy_model_json_with_data_plane_request_timeout(
            &state,
            Method::POST,
            &format!("{}/slow-invoke", server.uri()),
            Some(json!({"prompt": "hello"})),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Duration::from_millis(250)),
            &user,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn chat_proxy_returns_a_typed_gateway_timeout_when_the_model_deadline_expires() {
        use std::time::Duration;

        use reqwest::Method;
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/timed-out-invoke"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(100))
                    .set_body_json(json!({"ok": true})),
            )
            .mount(&server)
            .await;

        let state = test_state(false);
        let user = AuthenticatedUser {
            user_id: "user-test".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User Test".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-test".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-test".to_owned(),
                role: "member".to_owned(),
            }),
        };

        let (status, Json(body)) = proxy_model_json_with_data_plane_request_timeout(
            &state,
            Method::POST,
            &format!("{}/timed-out-invoke", server.uri()),
            Some(json!({"prompt": "hello"})),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Duration::from_millis(10)),
            &user,
        )
        .await;

        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(body["error"]["code"], "model_request_timeout");
    }

    #[test]
    fn required_delegated_token_failure_is_explicit_and_service_unavailable() {
        let (status, Json(body)) = delegated_auth_unavailable(RequiredAudienceTokenError {
            audience: ModelServiceAudience::InferenceCore,
        });
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "delegated_auth_unavailable");
        assert!(body["error"]["message"]
            .as_str()
            .expect("message")
            .contains("inference-core"));
    }

    #[test]
    fn dev_bypass_model_token_requires_explicit_gate_and_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer dev-bypass"),
        );

        assert_eq!(
            dev_bypass_model_token(&test_state(true), &headers),
            Some("dev-bypass".to_owned())
        );
        assert_eq!(dev_bypass_model_token(&test_state(false), &headers), None);

        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer other-token"),
        );
        assert_eq!(dev_bypass_model_token(&test_state(true), &headers), None);
    }

    #[test]
    fn authenticated_zdr_header_forces_request_body_posture() {
        let mut headers = HeaderMap::new();
        headers.insert("x-zdr", HeaderValue::from_static("true"));
        assert_eq!(
            normalized_model_body(json!({"content": "q"}), &headers)["zdr"],
            true
        );
        assert_eq!(
            normalized_model_body(json!({"content": "q", "zdr": true}), &HeaderMap::new())["zdr"],
            true
        );
    }

    #[test]
    fn missing_zdr_posture_defaults_explicitly_off() {
        let normalized = normalized_model_body(json!({"content": "q"}), &HeaderMap::new());

        assert_eq!(normalized["zdr"], false);
    }

    #[test]
    fn malformed_zdr_posture_is_not_coerced_off() {
        let normalized =
            normalized_model_body(json!({"content": "q", "zdr": "true"}), &HeaderMap::new());

        assert_eq!(normalized["zdr"], "true");
    }

    #[test]
    fn legacy_named_privacy_tiers_are_normalized_to_proto_ordinals() {
        for (legacy, ordinal) in [
            ("unspecified", 0),
            ("global", 1),
            ("eu_resident", 2),
            ("euResident", 2),
            ("zdr_contractual", 3),
            ("zdrContractual", 3),
            ("sovereign", 4),
        ] {
            let normalized = normalized_model_body(
                json!({"content": "q", "min_privacy_tier": legacy}),
                &HeaderMap::new(),
            );
            assert_eq!(
                normalized["min_privacy_tier"], ordinal,
                "legacy tier: {legacy}"
            );
        }
    }

    #[test]
    fn unknown_privacy_tier_is_preserved_for_fail_closed_rejection() {
        let normalized = normalized_model_body(
            json!({"content": "q", "min_privacy_tier": "future-tier"}),
            &HeaderMap::new(),
        );

        assert_eq!(normalized["min_privacy_tier"], "future-tier");
    }

    #[test]
    fn delegated_data_plane_token_uses_a_separate_authorization_header() {
        assert_eq!(
            data_plane_authorization_value("signed-data-token")
                .expect("valid header")
                .to_str()
                .unwrap(),
            "Bearer signed-data-token"
        );
        assert!(data_plane_authorization_value("").is_none());
    }
}
