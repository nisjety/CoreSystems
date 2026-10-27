//! WS03-S4: action dispatchers for the org-admin Space effect-policy contracts
//! (`spaces.set_effect_entitlements`, `spaces.apply_effect_preset`).
//!
//! Both ride the same gateway function as `PUT /api/v1/orgs/:id/space-policy/entitlements`
//! so the flag, the owner-or-admin check and the shape check exist once. The
//! caller-supplied `idempotencyKey` is required and forwarded unchanged: a retry must
//! reuse it, so the dispatcher never mints one.

use axum::response::Response;
use serde_json::{json, Value};

use crate::{config::AppState, domains::orgs::space_policy::apply_space_policy_entitlements};
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

fn expected_revision(input: &Value) -> Option<i64> {
    input
        .get("expectedEntitlementRevision")
        .and_then(Value::as_i64)
        .filter(|revision| *revision >= 0)
}

pub(in crate::domains::actions) async fn dispatch_spaces_set_effect_entitlements(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
    idempotency_key: &str,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    let Some(revision) = expected_revision(input) else {
        return ticket_bad_request(
            "spaces.set_effect_entitlements requires integer 'expectedEntitlementRevision'",
        );
    };
    let Some(entitlements) = input.get("entitlements").filter(|value| value.is_object()) else {
        return ticket_bad_request("spaces.set_effect_entitlements requires object 'entitlements'");
    };
    if org_id.is_empty() {
        return ticket_bad_request("spaces.set_effect_entitlements requires a non-empty 'orgId'");
    }
    let body = json!({
        "idempotency_key": idempotency_key,
        "expected_entitlement_revision": revision,
        "entitlements": entitlements,
    });
    let (status, axum::Json(resp)) =
        apply_space_policy_entitlements(state, user, org_id, &body).await;
    owner_json_response_to_envelope("spaces.set_effect_entitlements", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_apply_effect_preset(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
    idempotency_key: &str,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    let preset_id = ticket_string(input, "presetId");
    let Some(revision) = expected_revision(input) else {
        return ticket_bad_request(
            "spaces.apply_effect_preset requires integer 'expectedEntitlementRevision'",
        );
    };
    if org_id.is_empty() || preset_id.is_empty() {
        return ticket_bad_request(
            "spaces.apply_effect_preset requires non-empty 'orgId' and 'presetId'",
        );
    }
    let body = json!({
        "idempotency_key": idempotency_key,
        "expected_entitlement_revision": revision,
        "preset_id": preset_id,
    });
    let (status, axum::Json(resp)) =
        apply_space_policy_entitlements(state, user, org_id, &body).await;
    owner_json_response_to_envelope("spaces.apply_effect_preset", user, status, resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn admin() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "admin@example.com".to_owned(),
            user_name: "Admin One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("admin".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "admin".to_owned(),
            }),
        }
    }

    async fn read(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    fn enabled_state(user_core_url: Option<String>) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.space_features.policy_admin_api = true;
        if let Some(url) = user_core_url {
            state.user_core_url = url;
        }
        state
    }

    #[tokio::test]
    async fn both_dispatchers_400_without_the_expected_entitlement_revision() {
        let state = enabled_state(None);
        let user = admin();
        for input in [
            json!({"orgId": "org-1", "entitlements": {"effects_enabled": true}}),
            json!({"orgId": "org-1", "entitlements": {}, "expectedEntitlementRevision": -1}),
            json!({"orgId": "org-1", "entitlements": {}, "expectedEntitlementRevision": "3"}),
        ] {
            let (status, body) = read(
                dispatch_spaces_set_effect_entitlements(&state, &user, &input, "idem-key-1234")
                    .await,
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
            assert_eq!(body["error"]["code"], "invalid_input");
        }
        for input in [
            json!({"orgId": "org-1", "presetId": "balanced"}),
            json!({"orgId": "org-1", "presetId": "balanced", "expectedEntitlementRevision": 1.5}),
        ] {
            let (status, body) = read(
                dispatch_spaces_apply_effect_preset(&state, &user, &input, "idem-key-1234").await,
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
            assert_eq!(body["error"]["code"], "invalid_input");
        }
    }

    #[tokio::test]
    async fn dispatchers_400_without_org_preset_or_entitlements_object() {
        let state = enabled_state(None);
        let user = admin();
        // set: entitlements must be an object, orgId non-empty.
        for input in [
            json!({"orgId": "org-1", "expectedEntitlementRevision": 1}),
            json!({"orgId": "org-1", "expectedEntitlementRevision": 1, "entitlements": ["a"]}),
            json!({"orgId": "org-1", "expectedEntitlementRevision": 1, "entitlements": "x"}),
            json!({"orgId": " ", "expectedEntitlementRevision": 1, "entitlements": {}}),
            json!({"expectedEntitlementRevision": 1, "entitlements": {}}),
        ] {
            let (status, _) = read(
                dispatch_spaces_set_effect_entitlements(&state, &user, &input, "idem-key-1234")
                    .await,
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
        }
        // preset: orgId and presetId non-empty.
        for input in [
            json!({"expectedEntitlementRevision": 1, "presetId": "balanced"}),
            json!({"orgId": "org-1", "expectedEntitlementRevision": 1}),
            json!({"orgId": "org-1", "expectedEntitlementRevision": 1, "presetId": "  "}),
        ] {
            let (status, _) = read(
                dispatch_spaces_apply_effect_preset(&state, &user, &input, "idem-key-1234").await,
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
        }
    }

    /// The dispatcher never mints a key: an empty one reaches the shared validator and is refused.
    #[tokio::test]
    async fn an_empty_idempotency_key_is_refused_by_the_shared_validator() {
        let state = enabled_state(None);
        let user = admin();
        let set = json!({
            "orgId": "org-1", "expectedEntitlementRevision": 2,
            "entitlements": {"effects_enabled": true},
        });
        let (status, body) =
            read(dispatch_spaces_set_effect_entitlements(&state, &user, &set, "").await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "space_policy_invalid_request");
        assert_eq!(body["error"]["details"]["field"], "idempotency_key");
        let preset = json!({"orgId": "org-1", "expectedEntitlementRevision": 2, "presetId": "balanced"});
        let (status, body) =
            read(dispatch_spaces_apply_effect_preset(&state, &user, &preset, "short").await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["details"]["field"], "idempotency_key");
    }

    #[tokio::test]
    async fn flag_off_gives_404_from_both_dispatchers() {
        let state = crate::tests::test_state(false);
        assert!(!state.space_features.policy_admin_api);
        let user = admin();
        let set = json!({
            "orgId": "org-1", "expectedEntitlementRevision": 2,
            "entitlements": {"effects_enabled": true},
        });
        let (status, body) = read(
            dispatch_spaces_set_effect_entitlements(&state, &user, &set, "idem-key-1234").await,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "space_policy_admin_disabled");
        let preset = json!({"orgId": "org-1", "expectedEntitlementRevision": 2, "presetId": "balanced"});
        let (status, body) = read(
            dispatch_spaces_apply_effect_preset(&state, &user, &preset, "idem-key-1234").await,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "space_policy_admin_disabled");
    }

    #[tokio::test]
    async fn success_wraps_the_control_data_in_the_owner_envelope() {
        let control = MockServer::start().await;
        Mock::given(wm_method("PUT"))
            .and(wm_path("/api/v1/internal/spaces/org-policy/entitlements"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"entitlement_revision": 3, "entitlements": {"effects_enabled": true}}
            })))
            .mount(&control)
            .await;
        let state = enabled_state(Some(control.uri()));
        let user = admin();
        let set = json!({
            "orgId": "org-1", "expectedEntitlementRevision": 2,
            "entitlements": {"effects_enabled": true},
        });
        let (status, body) = read(
            dispatch_spaces_set_effect_entitlements(&state, &user, &set, "idem-key-1234").await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["actionId"], "spaces.set_effect_entitlements");
        assert_eq!(body["data"]["status"], "completed");
        assert_eq!(body["data"]["result"]["entitlement_revision"], 3);
        let preset = json!({"orgId": "org-1", "expectedEntitlementRevision": 3, "presetId": "balanced"});
        let (status, body) = read(
            dispatch_spaces_apply_effect_preset(&state, &user, &preset, "idem-key-1234").await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["actionId"], "spaces.apply_effect_preset");
        let requests = control.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(sent["idempotency_key"], "idem-key-1234");
        assert_eq!(sent["expected_entitlement_revision"], 2);
        let sent: Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert_eq!(sent["preset_id"], "balanced");
        assert!(sent.get("entitlements").is_none());
    }
}
