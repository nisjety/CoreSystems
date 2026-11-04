//! WS16-S8: action dispatcher for the personal sidebar layout (`spaces.update_sidebar`). It rides
//! the same function as `PUT /api/v1/spaces/sidebar`, so the flag, the Zero Data Retention refusal,
//! the compare-and-set and the typed errors exist once. The id is human-only (never
//! model-executable): it is one person's own layout, and an agent has no sidebar to arrange.

use axum::{http::HeaderMap, response::Response};
use serde_json::{json, Value};

use crate::config::AppState;
use crate::domains::spaces::sidebar::save_sidebar;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{owner_json_response_to_envelope, ticket_bad_request};

pub(in crate::domains::actions) async fn dispatch_spaces_update_sidebar(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    if input.get("baseRevision").is_none() || input.get("doc").is_none() {
        return ticket_bad_request("spaces.update_sidebar requires 'baseRevision' and 'doc'");
    }
    // The save flow validates both values and refuses anything else, so they pass on unchecked.
    let body = json!({
        "base_revision": input.get("baseRevision").cloned().unwrap_or(Value::Null),
        "doc": input.get("doc").cloned().unwrap_or(Value::Null),
    });
    let (status, axum::Json(resp)) = save_sidebar(state, user, headers, &body).await;
    owner_json_response_to_envelope("spaces.update_sidebar", user, status, resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    async fn read(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    struct Fx {
        state: AppState,
        app: MockServer,
        _org: MockServer,
    }

    async fn fixture(flag: bool, zdr: bool) -> Fx {
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}),
            ))
            .mount(&org)
            .await;
        let app = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"status": "success", "value": {"status": "ok", "revision": 5}}),
            ))
            .mount(&app)
            .await;
        let mut state = crate::tests::test_state(false);
        state.space_features.sidebar = flag;
        state.org_core_url = org.uri();
        state.application_convex_url = app.uri();
        state.application_convex_service_key = "application-test-key".into();
        Fx { state, app, _org: org }
    }

    fn input() -> Value {
        json!({"baseRevision": 4, "doc": {"starred": ["space-a"]}})
    }

    #[tokio::test]
    async fn missing_base_revision_or_doc_is_400_invalid_input() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(true, false).await;
        for input in [json!({"doc": {}}), json!({"baseRevision": 1}), json!({})] {
            let response = dispatch_spaces_update_sidebar(&fx.state, &user(), &HeaderMap::new(), &input).await;
            let (status, body) = read(response).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["error"]["code"], "invalid_input");
        }
        assert!(fx.app.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_save_receipt_is_wrapped_in_the_owner_envelope() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(true, false).await;
        let response = dispatch_spaces_update_sidebar(&fx.state, &user(), &HeaderMap::new(), &input()).await;
        let (status, body) = read(response).await;
        assert_eq!(status, StatusCode::OK);
        let data = &body["data"];
        assert_eq!(data["actionId"], "spaces.update_sidebar");
        assert_eq!(data["status"], "completed");
        assert_eq!(data["runId"], "spaces.update_sidebar_user-1");
        assert_eq!(data["result"], json!({"revision": 5, "doc": {"starred": ["space-a"]}}));
        let sent: Value = serde_json::from_slice(&fx.app.received_requests().await.unwrap()[0].body).unwrap();
        assert_eq!(sent["path"], "spaceSidebar:putForGateway");
        assert_eq!(sent["args"]["baseRevision"], 4);
    }

    #[tokio::test]
    async fn flag_off_is_404_and_a_zdr_org_is_412_without_a_convex_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let off = fixture(false, false).await;
        let (status, body) = read(dispatch_spaces_update_sidebar(&off.state, &user(), &HeaderMap::new(), &input()).await).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "space_sidebar_disabled");
        let zdr = fixture(true, true).await;
        let (status, body) = read(dispatch_spaces_update_sidebar(&zdr.state, &user(), &HeaderMap::new(), &input()).await).await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert!(off.app.received_requests().await.unwrap().is_empty());
        assert!(zdr.app.received_requests().await.unwrap().is_empty());
    }
}
