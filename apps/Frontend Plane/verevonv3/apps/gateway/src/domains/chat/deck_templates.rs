//! The brand deck template list for the chat's template picker (Wave 3 EP-5 S7).
//!
//! `GET /api/v1/deck-templates` is a read-only pass-through to Model Gateway's `/v1/deck-templates`.
//! The gateway reads the org's skill bundles from capability-core under the user's own capability
//! token, so the BFF forwards the user's minted model and capability tokens only, never an org, owner
//! or Space header of its own, and relays the gateway's typed envelope and status unchanged: 404 while
//! `SKILL_DECK_TEMPLATES_ENABLED` is off there (the picker then hides itself), and the typed
//! zero-retention refusal for a ZDR session. The browser's query is never forwarded.

use axum::{
    extract::{Extension, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use reqwest::Method;

use crate::{config::AppState, middleware::AuthenticatedUser};

use super::shared;

pub(super) async fn list_deck_templates(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    let url = format!("{}/v1/deck-templates", state.model_gateway_url);
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn servers(model_status: u16, model_body: Value) -> (MockServer, MockServer, MockServer) {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for path in ["/api/model-plane/token", "/api/capability-core/token"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(path))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "delegated-token"})),
                )
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let model = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/deck-templates"))
            .respond_with(ResponseTemplate::new(model_status).set_body_json(model_body))
            .mount(&model)
            .await;
        (auth, user_core, model)
    }

    async fn get_templates(
        auth: &MockServer,
        user_core: &MockServer,
        model: &MockServer,
    ) -> (axum::http::StatusCode, Value) {
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/deck-templates?org_id=org-forged")
                    .header("cookie", "better-auth.session_token=session-1")
                    .header("x-verevon-org-id", "org-forged")
                    .header("x-verevon-user-id", "user-forged")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn deck_templates_pass_through_forwards_no_forged_scope() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let listing = json!({"data": {"templates": [{
            "skill_id": "skill-brand", "name": "Aquatiq brand", "description": "",
            "colors": {"primary": "#0a3d62", "secondary": "#3c6382", "text": "#1e272e",
                       "background": "#ffffff", "accent": []},
            "fonts": {"heading": "Carlito", "body": "Liberation Sans"}
        }]}});
        let (auth, user_core, model) = servers(200, listing.clone()).await;
        let (status, body) = get_templates(&auth, &user_core, &model).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{body}");
        assert_eq!(body, listing);
        let requests = model.received_requests().await.unwrap();
        let forwarded = requests
            .iter()
            .find(|request| request.url.path() == "/v1/deck-templates")
            .expect("the list reaches model-gateway");
        // The browser's query and scope headers stay behind; the identity that goes on is the
        // session's, and the user's capability token rides with it.
        assert_eq!(forwarded.url.query(), None);
        assert!(forwarded.headers.get("x-verevon-org-id").is_none());
        assert!(forwarded.headers.get("x-verevon-user-id").is_none());
        let header = |name: &str| {
            forwarded
                .headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        assert_ne!(header("x-org-id").as_deref(), Some("org-forged"));
        assert_eq!(header("x-user-id").as_deref(), Some("user-1"));
        assert!(header("x-capability-authorization").is_some());
    }

    #[tokio::test]
    async fn deck_templates_relay_the_gateway_404_while_its_switch_is_off() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let off = json!({"error": {"code": "not_found", "message": "no such route"}});
        let (auth, user_core, model) = servers(404, off).await;
        let (status, body) = get_templates(&auth, &user_core, &model).await;
        assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
    }
}
