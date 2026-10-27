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
