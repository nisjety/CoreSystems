//! E3.2-g1: action dispatchers for the organization browser policy (`browser.get_org_policy`,
//! `browser.set_org_policy`). Both ride the gateway functions in `domains::orgs::browser_policy`,
//! so the flag, the membership and role checks and the shape check exist once. The save carries
//! its revision as `ifMatch`, the If-Match precondition org-core checks; no idempotency key is
//! minted here, because a repeated save at the same revision is refused by org-core, not replayed.

use axum::response::Response;
use serde_json::Value;

use crate::{
    config::AppState,
    domains::orgs::browser_policy::{get_org_policy, set_org_policy},
    middleware::AuthenticatedUser,
};

use super::dispatchers::{owner_json_response_to_envelope, ticket_bad_request, ticket_string};

pub(super) async fn dispatch_browser_get_org_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    if org_id.is_empty() {
        return ticket_bad_request("browser.get_org_policy requires a non-empty 'orgId'");
    }
    let (status, axum::Json(resp)) = get_org_policy(state, user, org_id).await;
    owner_json_response_to_envelope("browser.get_org_policy", user, status, resp)
}

pub(super) async fn dispatch_browser_set_org_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    let Some(if_match) = input.get("ifMatch").and_then(Value::as_i64) else {
        return ticket_bad_request("browser.set_org_policy requires integer 'ifMatch'");
    };
    let Some(document) = input.get("document").filter(|value| value.is_object()) else {
        return ticket_bad_request("browser.set_org_policy requires object 'document'");
    };
    if org_id.is_empty() {
        return ticket_bad_request("browser.set_org_policy requires a non-empty 'orgId'");
    }
    let (status, axum::Json(resp)) = set_org_policy(state, user, org_id, if_match, document).await;
    owner_json_response_to_envelope("browser.set_org_policy", user, status, resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use serde_json::json;

    fn admin() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "admin@example.com".to_owned(),
            user_name: "Admin One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("admin".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "admin".to_owned(),
            }),
        }
    }

    async fn status_and_body(response: impl std::future::Future<Output = Response>) -> (StatusCode, Value) {
        let response = response.await;
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn the_inputs_are_checked_before_the_gateway_is_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        std::env::remove_var("BROWSER_POLICY_ADMIN_API");
        let state = crate::tests::test_state(false);
        let user = admin();
        let (status, body) = status_and_body(dispatch_browser_get_org_policy(&state, &user, &json!({}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"]["message"].as_str().unwrap_or_default().contains("orgId"));
        let save = json!({"orgId": "org-1", "ifMatch": "3", "document": {}});
        let (status, _) = status_and_body(dispatch_browser_set_org_policy(&state, &user, &save)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let save = json!({"orgId": "org-1", "ifMatch": 3});
        let (status, _) = status_and_body(dispatch_browser_set_org_policy(&state, &user, &save)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // Shape is valid but the flag is off, so the gateway answers 404 and nothing is sent.
        let save = json!({"orgId": "org-1", "ifMatch": 3, "document": {}});
        let (status, body) = status_and_body(dispatch_browser_set_org_policy(&state, &user, &save)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "browser_policy_admin_disabled");
    }
}
