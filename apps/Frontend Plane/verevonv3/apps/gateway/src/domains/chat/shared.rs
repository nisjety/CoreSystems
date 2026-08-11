use axum::{
    http::{header::AUTHORIZATION, HeaderMap, HeaderValue, StatusCode},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

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

/// Stamps the verified org/user identity onto the outbound request so Model
/// Gateway can tell the model who it's talking to (chat: "who are we" / "what
/// do we offer" resolve to the signed-in org, "I"/"my" to the signed-in user).
/// ALWAYS overwrites any client-supplied `org_name`/`user_name` — these are
/// framing text only (never used for authorization or retrieval scoping, which
/// stay keyed off the verified `org_id` claim), but a raw client has no way to
/// know the real display name anyway, so trusting one would only ever be a
/// spoofed persona, never a real value.
/// Record the thread as Zero-Data-Retention when this turn is a ZDR turn.
///
/// Reads the ALREADY-NORMALIZED body, so the marker is written from the exact
/// posture forwarded to the Model Plane — header OR body, never the raw client
/// claim. Without this the BFF's only notion of "temporary chat" was an
/// in-memory `Set` in the browser, which a reload, a replay, or a non-SPA client
/// simply does not have; the thread's own snapshot endpoint then happily stored
/// 90 days of a conversation the Model Plane had guaranteed to leave no trace of.
///
/// A turn with no `thread_id` has nothing to mark — the SPA always sends one
/// (it generates a provisional id before the first send, and a ZDR turn keeps
/// it, since `zdr_direct_stream` creates no server-side thread to rename to).
pub(crate) async fn record_zdr_thread(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    normalized_body: &Value,
) {
    let is_zdr = normalized_body
        .get("zdr")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !is_zdr {
        return;
    }
    let Some(thread_id) = normalized_body
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    super::history::mark_thread_zdr(state, org_id, &user.user_id, thread_id).await;
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
        data_plane_authorization_value, delegated_auth_unavailable, dev_bypass_model_token,
        normalized_model_body, proxy_model_json_with_data_plane, proxy_model_json_with_session,
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
            audience_token_cache: new_audience_token_cache(),
            browser_run_store: crate::domains::browser::new_browser_run_store(),
            rate_limiter: crate::rate_limit::RateLimiter::from_cache(&ResultCache::disabled()),
            cache: ResultCache::disabled(),
            chat_history_store: crate::domains::chat::history::ChatHistoryStore::new(),
            studio_store: crate::domains::studio::StudioStore::new(),
            allow_dev_actor_headers: false,
            allow_dev_auth_bypass,
        }
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
