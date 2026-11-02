//! WS08-S5: the binding wake policy writer.
//!
//! `PATCH /api/v1/spaces/{space_ref}/agents/{binding_ref}/policy` body
//! `{respond_to, respond_to_subjects?, expected_updated_at}` sets who may wake one bound agent
//! in this room (`members` = any editor, manager or owner, `managers`, or an `allowlist` of at
//! most 50 Control subject ids). It is a human decision: only the room's owner or a manager,
//! decided from Control's live membership BEFORE any Application request, may write it. The
//! Convex mutation re-checks that the binding belongs to this Space and organization, compares
//! `expected_updated_at` (a second manager's concurrent change answers 409 `policy_conflict`)
//! and appends one `spaceAgentPolicyEvents` row. The policy narrows or widens who the mention
//! path lets wake the agent; it grants nothing, and Control still decides who may post or wake.
//! No message content moves here, so Zero Data Retention organizations behave the same and no
//! model is called. Behind `SpaceFeatureFlags::agent_wake_policy_api` (off answers 404
//! `feature_disabled`), a rollout control and never an authorization input.

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use super::room_admin::convex_mutation_message;
use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

type Reply = (StatusCode, Json<Value>);

/// Most subject ids an allowlist may carry (the Convex mutation enforces the same bound).
const MAX_RESPOND_TO_SUBJECTS: usize = 50;
const MAX_SUBJECT_ID_CHARS: usize = 128;
const ALLOWED_FIELDS: [&str; 3] = ["respond_to", "respond_to_subjects", "expected_updated_at"];

fn invalid_policy(reason: &str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(crate::envelope::error_with_details(
            "invalid_policy",
            "The wake policy is not valid.",
            json!({"reason": reason}),
        )),
    )
}

fn unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "agent_binding_unavailable",
            "The agent's wake policy could not be changed. Nothing was altered.",
        )),
    )
}

/// The validated request: nothing here has been trusted yet except its shape.
#[derive(Debug, PartialEq)]
struct PolicyRequest {
    respond_to: String,
    respond_to_subjects: Vec<String>,
    expected_updated_at: u64,
}

fn parse_policy(body: &Value) -> Result<PolicyRequest, Reply> {
    let Some(object) = body.as_object() else {
        return Err(invalid_policy("body_not_an_object"));
    };
    if let Some(field) = object.keys().find(|key| !ALLOWED_FIELDS.contains(&key.as_str())) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(crate::envelope::error_with_details(
                "unsupported_policy_field",
                "Only respond_to, respond_to_subjects and expected_updated_at can be set here.",
                json!({"field": field.chars().take(64).collect::<String>()}),
            )),
        ));
    }
    // `group` is retired and is not a value here: only the three modes below exist.
    let respond_to = match object.get("respond_to").and_then(Value::as_str).map(str::trim) {
        Some(mode @ ("members" | "managers" | "allowlist")) => mode.to_owned(),
        Some(_) => return Err(invalid_policy("unknown_respond_to")),
        None => return Err(invalid_policy("respond_to_required")),
    };
    let mut subjects: Vec<String> = Vec::new();
    match object.get("respond_to_subjects") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            for item in items {
                let Some(id) = item.as_str().map(str::trim) else {
                    return Err(invalid_policy("bad_subject_id"));
                };
                if id.is_empty() || id.chars().count() > MAX_SUBJECT_ID_CHARS {
                    return Err(invalid_policy("bad_subject_id"));
                }
                if !subjects.iter().any(|existing| existing == id) {
                    subjects.push(id.to_owned());
                }
            }
        }
        Some(_) => return Err(invalid_policy("bad_subject_list")),
    }
    if subjects.len() > MAX_RESPOND_TO_SUBJECTS {
        return Err(invalid_policy("too_many_subjects"));
    }
    if respond_to == "allowlist" && subjects.is_empty() {
        return Err(invalid_policy("allowlist_empty"));
    }
    if respond_to != "allowlist" && !subjects.is_empty() {
        return Err(invalid_policy("subjects_need_allowlist"));
    }
    let expected_updated_at = object
        .get("expected_updated_at")
        .and_then(|value| value.as_u64().or_else(|| value.as_f64().filter(|f| *f >= 0.0 && f.fract() == 0.0).map(|f| f as u64)))
        .ok_or_else(|| invalid_policy("expected_updated_at_required"))?;
    Ok(PolicyRequest {
        respond_to,
        respond_to_subjects: subjects,
        expected_updated_at,
    })
}

/// Maps Application's stable mutation messages to the typed browser errors.
fn map_policy_failure(message: Option<&str>) -> Reply {
    let Some(message) = message else {
        return unavailable();
    };
    if message.contains("policy_conflict") {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "policy_conflict",
                "The agent's wake policy changed since you loaded it. Reload and try again.",
            )),
        );
    }
    if message.contains("agent_binding_not_found") {
        return (
            StatusCode::NOT_FOUND,
            Json(error(
                "agent_binding_not_found",
                "That agent is not bound to this room.",
            )),
        );
    }
    if message.contains("invalid_policy") {
        return invalid_policy("rejected");
    }
    unavailable()
}

/// Axum handler for `PATCH /api/v1/spaces/{space_ref}/agents/{binding_ref}/policy`.
pub(super) async fn set_space_agent_wake_policy_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, binding_ref)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Reply {
    set_space_agent_wake_policy(&state, &user, &space_ref, &binding_ref, &body).await
}

/// The route's body, shared with the `spaces.set_agent_wake_policy` action so the flag, the
/// owner-or-manager gate and the typed errors exist once.
pub(crate) async fn set_space_agent_wake_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    binding_ref: &str,
    body: &Value,
) -> Reply {
    if !state.space_features.agent_wake_policy_api {
        return (
            StatusCode::NOT_FOUND,
            Json(error(
                "feature_disabled",
                "Agent wake policy is not enabled.",
            )),
        );
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let space_ref = space_ref.trim();
    let binding_ref = binding_ref.trim();
    if org_id.is_empty() || space_ref.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        );
    }
    if binding_ref.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "binding_ref_required",
                "A binding reference is required.",
            )),
        );
    }
    let request = match parse_policy(body) {
        Ok(request) => request,
        Err(reply) => return reply,
    };

    // Role first, and against THIS room, from Control's live membership: owner or manager only.
    match super::resolve_space_role(state, user, &org_id, space_ref).await {
        Ok(role) if super::CREATE_AGENT_ROLES.contains(&role.as_str()) => {}
        Ok(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(error(
                    "space_role_cannot_manage_agent",
                    "Only a Space owner or manager can change who may wake an agent.",
                )),
            )
        }
        Err(refusal) => return refusal,
    }

    let mut args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "spaceRef": space_ref,
        "bindingRef": binding_ref,
        "respondTo": request.respond_to,
        "expectedUpdatedAt": request.expected_updated_at,
    });
    if request.respond_to == "allowlist" {
        args["respondToSubjects"] = json!(request.respond_to_subjects);
    }
    match convex_mutation_message(
        state,
        "spaceAgents:setSpaceAgentWakePolicyForGateway",
        args,
    )
    .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({"data": {
                "binding_ref": result.get("bindingRef").and_then(Value::as_str).unwrap_or(binding_ref),
                "respond_to": result.get("respondTo").and_then(Value::as_str).unwrap_or(&request.respond_to),
                "respond_to_subjects": result.get("respondToSubjects").cloned().unwrap_or_else(|| json!([])),
                "projection_version": result.get("projectionVersion").cloned().unwrap_or(Value::Null),
                "updated_at": result.get("updatedAt").cloned().unwrap_or(Value::Null),
                "changed": result.get("changed").and_then(Value::as_bool).unwrap_or(false),
            }})),
        ),
        Err(message) => map_policy_failure(message.as_deref()),
    }
}
