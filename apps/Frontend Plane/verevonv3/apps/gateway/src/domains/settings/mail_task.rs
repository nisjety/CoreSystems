//! Self-service proxy for the personal mail-task inbound alias (chat-wave2
//! mail-task spec §3.6, owner ruling 2026-09-28: Postmark inbound,
//! self-forward only). Thin pass-through to user-core's
//! `/api/v1/users/me/mail-task-address` (GET/POST/DELETE) -- the caller's
//! identity is forwarded as an actor header, never trusted from the request
//! body.
//!
//! Unlike `preferences.rs` (which this was originally pattern-matched on),
//! this endpoint IS organization-scoped -- user-core's alias table is keyed
//! by `(org_id, user_id)`, one live alias per pair -- so `org_id` must be
//! forwarded as `user.active_org_id`, the same way `auth/protected.rs`'s
//! `get_me` does it. Passing `None` here (as `preferences.rs` correctly does,
//! since preferences are NOT org-scoped) would drop the `x-org-id` header
//! entirely (`service_delegation_headers` strips empty values) and user-core
//! would 400 "an active organization is required" on every real request.

use axum::extract::{Extension, State};

use crate::{config::AppState, middleware::AuthenticatedUser, upstream::proxy_json};

use super::shared::actor_for;

fn url(state: &AppState) -> String {
    format!("{}/api/v1/users/me/mail-task-address", state.user_core_url)
}

pub(super) async fn get_mail_task_address(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl axum::response::IntoResponse {
    let target = url(&state);
    let org_id = user.active_org_id.as_deref();
    proxy_json(&state, reqwest::Method::GET, &target, None, org_id, Some(&actor_for(&user)), None).await
}

pub(super) async fn create_mail_task_address(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl axum::response::IntoResponse {
    let target = url(&state);
    let org_id = user.active_org_id.as_deref();
    proxy_json(&state, reqwest::Method::POST, &target, None, org_id, Some(&actor_for(&user)), None).await
}

pub(super) async fn revoke_mail_task_address(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl axum::response::IntoResponse {
    let target = url(&state);
    let org_id = user.active_org_id.as_deref();
    proxy_json(&state, reqwest::Method::DELETE, &target, None, org_id, Some(&actor_for(&user)), None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn test_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".into(),
            user_email: "user1@example.com".into(),
            user_name: "User One".into(),
            user_image: None,
            email_verified: true,
            auth_role: None,
            active_org_id: Some("org-active".into()),
            authorized_membership: None,
        }
    }

    // Regression test: this handler used to pass `org_id: None` to
    // `proxy_json` (copied from the non-org-scoped `preferences.rs`), which
    // silently dropped the `x-org-id` header (service_delegation_headers
    // strips empty values) and made user-core reject every real request with
    // "an active organization is required". This asserts the session's
    // active_org_id actually reaches user-core on all three verbs.
    #[tokio::test]
    async fn get_forwards_the_sessions_active_org_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/users/me/mail-task-address"))
            .and(header("x-org-id", "org-active"))
            .and(header("x-user-id", "user-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "address": "abc123@tasks.verevon.ai"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut state = crate::tests::test_state(false);
        state.user_core_url = server.uri();

        let response = get_mail_task_address(
            axum::extract::State(state),
            axum::extract::Extension(test_user()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn create_forwards_the_sessions_active_org_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/users/me/mail-task-address"))
            .and(header("x-org-id", "org-active"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "address": "abc123@tasks.verevon.ai"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut state = crate::tests::test_state(false);
        state.user_core_url = server.uri();

        let response = create_mail_task_address(
            axum::extract::State(state),
            axum::extract::Extension(test_user()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn revoke_forwards_the_sessions_active_org_id() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/api/v1/users/me/mail-task-address"))
            .and(header("x-org-id", "org-active"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let mut state = crate::tests::test_state(false);
        state.user_core_url = server.uri();

        let response = revoke_mail_task_address(
            axum::extract::State(state),
            axum::extract::Extension(test_user()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
    }
}
