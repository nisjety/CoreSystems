//! W3-DEC-ORGAGENT (owner decision D19): action dispatchers for the org-admin agent lifecycle
//! contracts (`spaces.suspend_org_agent`, `spaces.reactivate_org_agent`, `spaces.retire_org_agent`).
//! Each rides the same gateway function as the REST route (domains/orgs/org_agents.rs), so the
//! org-admin check and the shape check exist once. Human-only (model-access.ts closes the ids to
//! agents). The revision travels as `expectedRevision`, the same guard as the route body.

use axum::response::Response;
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, domains::orgs::org_agents::change_org_agent, middleware::AuthenticatedUser,
};

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

fn expected_revision(input: &Value) -> Option<i64> {
    input
        .get("expectedRevision")
        .and_then(Value::as_i64)
        .filter(|revision| *revision >= 0)
}

async fn dispatch_org_agent_change(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
    action_id: &str,
    method: Method,
    suffix: &str,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    let agent_subject_id = ticket_string(input, "agentSubjectId");
    if org_id.is_empty() || agent_subject_id.is_empty() {
        return ticket_bad_request(
            "org agent actions require non-empty 'orgId' and 'agentSubjectId'",
        );
    }
    let Some(revision) = expected_revision(input) else {
        return ticket_bad_request("org agent actions require integer 'expectedRevision'");
    };
    let body = json!({ "expected_revision": revision });
    let (status, axum::Json(resp)) = change_org_agent(
        state,
        user,
        org_id,
        agent_subject_id,
        method,
        suffix,
        &body,
    )
    .await;
    owner_json_response_to_envelope(action_id, user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_suspend_org_agent(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    dispatch_org_agent_change(
        state,
        user,
        input,
        "spaces.suspend_org_agent",
        Method::PUT,
        "/suspend",
    )
    .await
}

pub(in crate::domains::actions) async fn dispatch_spaces_reactivate_org_agent(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    dispatch_org_agent_change(
        state,
        user,
        input,
        "spaces.reactivate_org_agent",
        Method::PUT,
        "/reactivate",
    )
    .await
}

pub(in crate::domains::actions) async fn dispatch_spaces_retire_org_agent(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    dispatch_org_agent_change(
        state,
        user,
        input,
        "spaces.retire_org_agent",
        Method::DELETE,
        "",
    )
    .await
}
