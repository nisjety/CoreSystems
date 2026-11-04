//! WS10-S4: action dispatchers for the Space approval decision and the standing-rule revoke
//! (`spaces.decide_approval`, `spaces.revoke_approval_grant`). They ride the same gateway
//! functions as the HTTP routes, so the scope rules, the role gate and the typed errors exist
//! once. Both ids are human-only (never model-executable): an agent must not decide its own
//! approval nor revoke the rule that limits it.

use axum::http::HeaderMap;
use axum::response::Response;
use serde_json::{Map, Value};

use crate::config::AppState;
use crate::domains::spaces::approvals::{decide_space_approval, revoke_approval_grant};
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_decide_approval(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
    idempotency_key: &str,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let approval_id = ticket_string(input, "approvalId");
    if space_ref.is_empty() || approval_id.is_empty() {
        return ticket_bad_request(
            "spaces.decide_approval requires non-empty 'spaceRef' and 'approvalId'",
        );
    }
    let mut body = Map::new();
    for (from, to) in [("decision", "decision"), ("scope", "scope"), ("reason", "reason")] {
        if let Some(value) = input.get(from) {
            body.insert(to.to_owned(), value.clone());
        }
    }
    // The action envelope's key wins over a duplicate in the input; Control dedupes a standing rule on it.
    let key = if idempotency_key.trim().is_empty() {
        ticket_string(input, "idempotencyKey")
    } else {
        idempotency_key.trim()
    };
    if !key.is_empty() {
        body.insert("idempotency_key".to_owned(), Value::String(key.to_owned()));
    }
    let (status, axum::Json(resp)) = decide_space_approval(
        state,
        user,
        headers,
        space_ref,
        approval_id,
        Value::Object(body),
    )
    .await;
    owner_json_response_to_envelope("spaces.decide_approval", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_revoke_approval_grant(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let grant_id = ticket_string(input, "grantId");
    if space_ref.is_empty() || grant_id.is_empty() {
        return ticket_bad_request(
            "spaces.revoke_approval_grant requires non-empty 'spaceRef' and 'grantId'",
        );
    }
    let (status, axum::Json(resp)) =
        revoke_approval_grant(state, user, space_ref, grant_id).await;
    owner_json_response_to_envelope("spaces.revoke_approval_grant", user, status, resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use serde_json::json;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "ws10-user".to_owned(),
            user_email: "ws10@example.com".to_owned(),
            user_name: "Ws10".to_owned(),
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

    fn state_for(server: &MockServer, flag: bool) -> AppState {
        let mut state = crate::tests::test_state(false);
        let uri = server.uri();
        state.auth_core_url = uri.clone();
        state.user_core_url = uri.clone();
        state.org_core_url = uri.clone();
        state.model_gateway_url = uri;
        state.space_features.approval_scopes = flag;
        state
    }

    async fn decide(state: &AppState, input: Value, key: &str) -> (StatusCode, Value) {
        read(dispatch_spaces_decide_approval(state, &user(), &HeaderMap::new(), &input, key).await)
            .await
    }

    #[tokio::test]
    async fn decide_requires_space_and_approval_ids_and_makes_no_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        let state = state_for(&server, true);
        for input in [
            json!({ "approvalId": "apr-1", "decision": "approve" }),
            json!({ "spaceRef": "space-1", "decision": "approve" }),
            json!({ "spaceRef": " ", "approvalId": " ", "decision": "approve" }),
        ] {
            let (status, _) = decide(&state, input.clone(), "key-aaaaaaaa").await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
        }
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn decide_uses_the_route_scope_rules_and_the_envelope_key_wins() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        let state = state_for(&server, true);
        // The thread scope has no store: refused, never downgraded.
        let (status, body) = decide(
            &state,
            json!({ "spaceRef": "space-1", "approvalId": "apr-1", "decision": "approve", "scope": "thread" }),
            "key-aaaaaaaa",
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "approval_scope_not_permitted");
        // `always` needs a key: neither the envelope nor the input has one.
        let (status, body) = decide(
            &state,
            json!({ "spaceRef": "space-1", "approvalId": "apr-1", "decision": "approve", "scope": "always" }),
            "  ",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["details"]["field"], "idempotency_key");
        // The envelope's key satisfies it (the flag-off refusal proves the parse passed).
        let off = state_for(&server, false);
        let (status, body) = decide(
            &off,
            json!({ "spaceRef": "space-1", "approvalId": "apr-1", "decision": "approve", "scope": "always" }),
            "key-aaaaaaaa",
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["details"]["reason"], "standing_approvals_disabled");
        // So does the input's own key when the envelope has none.
        let (status, body) = decide(
            &off,
            json!({ "spaceRef": "space-1", "approvalId": "apr-1", "decision": "approve",
                    "scope": "always", "idempotencyKey": "input-key-aaaa" }),
            "",
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["details"]["reason"], "standing_approvals_disabled");
        // No upstream call was made for any of the above.
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn fields_outside_decision_scope_and_reason_are_never_forwarded() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        for (route, token) in [
            ("/api/session-core/token", "session-test-token"),
            ("/api/execution-core/token", "execution-test-token"),
        ] {
            Mock::given(wm_method("GET"))
                .and(wm_path(route))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "token": token })))
                .mount(&server)
                .await;
        }
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-1/membership"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "data": { "role": "member" } })),
            )
            .mount(&server)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/orchestration/approvals/apr-1/decide"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({ "approval": { "id": "apr-1", "status": "GRANTED" } }),
            ))
            .mount(&server)
            .await;
        // The org record the ZDR guard reads before a durable decision: not Zero Data Retention.
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({ "data": { "id": "org-1", "metadata": { "interactiveRetention": { "zdr": false } } } }),
            ))
            .mount(&server)
            .await;
        let state = state_for(&server, true);
        let (status, body) = decide(
            &state,
            json!({ "spaceRef": "space-1", "approvalId": "apr-1", "decision": "approve",
                    "reason": "looks fine", "grant_use_token": "forged-token",
                    "space_approval_decision_token": "forged-proof", "orgId": "org-2" }),
            "key-aaaaaaaa",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["actionId"], "spaces.decide_approval");
        assert_eq!(body["data"]["status"], "completed");
        assert_eq!(body["data"]["result"]["approval"]["status"], "GRANTED");
        assert_eq!(body["data"]["result"]["grant"]["status"], "none");
        let sent = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path().ends_with("/decide"))
            .expect("decide call");
        let upstream: Value = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(upstream, json!({ "decision": "approve", "reason": "looks fine", "scope": "once" }));
        for request in server.received_requests().await.unwrap() {
            let text = String::from_utf8_lossy(&request.body).to_string();
            assert!(!text.contains("forged"), "{}", request.url);
        }
    }

    #[tokio::test]
    async fn revoke_requires_ids_follows_the_flag_and_wraps_the_result() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        Mock::given(wm_method("DELETE"))
            .and(wm_path("/api/v1/internal/spaces/space-1/approval-grants/grant-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({ "data": { "grant": { "id": "grant-1", "status": "revoked" } } }),
            ))
            .mount(&server)
            .await;
        let state = state_for(&server, true);
        for input in [json!({ "grantId": "grant-1" }), json!({ "spaceRef": "space-1" })] {
            let response = dispatch_spaces_revoke_approval_grant(&state, &user(), &input).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{input}");
        }
        assert!(server.received_requests().await.unwrap().is_empty());
        let response = dispatch_spaces_revoke_approval_grant(
            &state,
            &user(),
            &json!({ "spaceRef": "space-1", "grantId": "grant-1" }),
        )
        .await;
        let (status, body) = read(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["actionId"], "spaces.revoke_approval_grant");
        assert_eq!(body["data"]["result"]["grant"]["status"], "revoked");
        let off = state_for(&server, false);
        let response = dispatch_spaces_revoke_approval_grant(
            &off,
            &user(),
            &json!({ "spaceRef": "space-1", "grantId": "grant-1" }),
        )
        .await;
        let (status, body) = read(response).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "feature_disabled");
    }
}
