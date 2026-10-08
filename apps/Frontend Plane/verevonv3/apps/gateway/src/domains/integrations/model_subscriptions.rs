use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{config::AppState, middleware::AuthenticatedUser};

use super::shared::proxy_for_user;

/// Starts the official Codex device-code flow. The browser receives only the
/// short-lived verification URL/code and an opaque connection reference; the
/// subscription credential remains inside Integration Core.
pub(crate) async fn start_openai_codex_subscription(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/model-subscriptions/openai-codex/connect",
        state.integration_core_url
    );
    proxy_for_user(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(if body.is_object() { body } else { json!({}) }),
    )
    .await
}

pub(crate) async fn openai_codex_subscription_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((connection_id, login_id)): Path<(String, String)>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/model-subscriptions/openai-codex/connect/{}/{}",
        state.integration_core_url,
        urlencoding::encode(&connection_id),
        urlencoding::encode(&login_id),
    );
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None).await
}

pub(crate) async fn disconnect_openai_codex_subscription(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(connection_id): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/model-subscriptions/openai-codex/connections/{}",
        state.integration_core_url,
        urlencoding::encode(&connection_id),
    );
    proxy_for_user(&state, &user, &headers, Method::DELETE, &url, None).await
}

/// Reads the connected user's current provider-reported Codex allowance. The
/// browser sees only the scoped usage report; subscription credentials remain
/// inside Integration Core.
pub(crate) async fn openai_codex_subscription_usage(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(connection_id): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/model-subscriptions/openai-codex/connections/{}/usage",
        state.integration_core_url,
        urlencoding::encode(&connection_id),
    );
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, response::IntoResponse};
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

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
    async fn usage_route_forwards_the_opaque_connection_id_to_integration_core() {
        let auth_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/ingestion/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "plane-token"
            })))
            .mount(&auth_core)
            .await;

        let integration_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/api/v1/model-subscriptions/openai-codex/connections/conn-1/usage",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "data": {
                    "scope": "connected_user",
                    "source": "provider_rate_limits",
                    "usage": {"primary": {"usedPercent": 42}}
                }
            })))
            .mount(&integration_core)
            .await;

        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth_core.uri();
        state.integration_core_url = integration_core.uri();
        let response = openai_codex_subscription_usage(
            State(state),
            Extension(user()),
            HeaderMap::new(),
            Path("conn-1".to_owned()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let received = integration_core
            .received_requests()
            .await
            .expect("request recording is enabled by default");
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].method, "GET");
        assert_eq!(
            received[0].url.path(),
            "/api/v1/model-subscriptions/openai-codex/connections/conn-1/usage"
        );
    }
}
