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
