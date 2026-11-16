//! W3-DEC-CANVAS (D2): the room agent's canvas-tools binding switch and the gateway's part of its policy.
//!
//! `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/canvas-tools` with body `{enabled: boolean}` turns one bound
//! agent's room canvas tools (`space.canvas.read`, `space.canvas.write`) on or off in this room. It is a switch of its
//! own, separate from the self-API switch (rename, pin and post card): turning one on never offers the other. Only the
//! room's owner or a manager, decided from Control's live membership BEFORE any Application request, may write it. The
//! Convex mutation (`spaceAgents:setSpaceAgentCanvasToolsForGateway`) re-checks that the binding belongs to this Space
//! and organization and is `active`, then records the switch on the binding.
//!
//! The switch grants nothing. A run gets canvas tools only when Control's run-action decision allows each action and
//! Data Plane's canvas checks pass. On a mention turn the feature `space_canvas_tools` is added only when the flag is on,
//! this binding's switch is on and the organization is not zero-retention ([`canvas_tools_allowed`]); every other turn has
//! the feature removed. Behind `SpaceFeatureFlags::agent_canvas_tools_api` (off answers 404 `feature_disabled`), a rollout
//! control and never an authorization input. The switch reaches the room's agent list as `canvas_tools_enabled`, so this
//! module has no read route of its own.

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::put,
    Json, Router,
};
use serde_json::{json, Map, Value};

use super::room_admin::convex_mutation_message;
use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

type Reply = (StatusCode, Json<Value>);

/// The feature a mention turn carries when its binding allows the canvas tools. Model Gateway honours it only for a
/// Space-scoped agentic turn that is not zero-retention (`space_routes::self_api::tool_profile_for_run`).
pub(crate) const SPACE_CANVAS_TOOLS_FEATURE: &str = "space_canvas_tools";

fn invalid_request(reason: &str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(crate::envelope::error_with_details(
            "invalid_canvas_tools_request",
            "The agent canvas-tools setting is not valid.",
            json!({"reason": reason}),
        )),
    )
}

/// The validated request: `enabled` is the only field, and it must be a JSON boolean.
fn parse_enabled(body: &Value) -> Result<bool, Reply> {
    let Some(object) = body.as_object() else {
        return Err(invalid_request("body_not_an_object"));
    };
    if let Some(field) = object.keys().find(|key| key.as_str() != "enabled") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(crate::envelope::error_with_details(
                "invalid_canvas_tools_request",
                "Only enabled can be set here.",
                json!({"field": field.chars().take(64).collect::<String>()}),
            )),
        ));
    }
    object
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid_request("enabled_required"))
}

fn unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "agent_canvas_tools_unavailable",
            "The agent canvas-tools setting could not be changed. Nothing was altered.",
        )),
    )
}

/// Maps Application's stable mutation messages to the typed browser errors.
fn map_canvas_tools_failure(message: Option<&str>) -> Reply {
    let Some(message) = message else {
        return unavailable();
    };
    if message.contains("agent_binding_not_found") {
        return (
            StatusCode::NOT_FOUND,
            Json(error(
                "agent_binding_not_found",
                "That agent is not bound to this room.",
            )),
        );
    }
    if message.contains("agent_binding_not_active") {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "agent_binding_not_active",
                "That agent is not active in this room.",
            )),
        );
    }
    if message.contains("invalid_canvas_tools_request") {
        return invalid_request("rejected");
    }
    unavailable()
}

/// Axum handler for `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/canvas-tools`.
async fn set_space_agent_canvas_tools_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, binding_ref)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Reply {
    set_space_agent_canvas_tools(&state, &user, &space_ref, &binding_ref, &body).await
}

/// The switch's writer, shared with the `spaces.set_agent_canvas_tools` action so the flag, the owner-or-manager
/// gate and the typed errors exist once.
pub(crate) async fn set_space_agent_canvas_tools(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    binding_ref: &str,
    body: &Value,
) -> Reply {
    // The action reaches this function without the route, so the switch is checked here too.
    if !state.space_features.agent_canvas_tools_api {
        return (
            StatusCode::NOT_FOUND,
            Json(error("feature_disabled", "Agent canvas tools are not enabled.")),
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
        return invalid_request("binding_ref_required");
    }
    let enabled = match parse_enabled(body) {
        Ok(enabled) => enabled,
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
                    "Only a Space owner or manager can turn agent canvas tools on or off.",
                )),
            )
        }
        Err(refusal) => return refusal,
    }

    let args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "spaceRef": space_ref,
        "bindingRef": binding_ref,
        "enabled": enabled,
    });
    match convex_mutation_message(state, "spaceAgents:setSpaceAgentCanvasToolsForGateway", args).await {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({"data": {
                "binding_ref": result.get("bindingRef").and_then(Value::as_str).unwrap_or(binding_ref),
                "canvas_tools_enabled": result.get("canvasToolsEnabled").and_then(Value::as_bool).unwrap_or(enabled),
                "projection_version": result.get("projectionVersion").cloned().unwrap_or(Value::Null),
                "changed": result.get("changed").and_then(Value::as_bool).unwrap_or(false),
            }})),
        ),
        Err(message) => map_canvas_tools_failure(message.as_deref()),
    }
}

/// The one route. It checks the flag inside the handler, as the room's other feature routes do.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/{space_ref}/agents/{binding_ref}/canvas-tools",
        put(set_space_agent_canvas_tools_route),
    )
}

/// Whether one bound agent's canvas tools apply to a mention turn: the feature flag is on, this binding's switch is on,
/// and the organization is not zero-retention (the feature is simply not added for a ZDR org). The binding's own switch
/// is the only per-agent decision; the flag and the posture are the rollout and the privacy floor.
pub(crate) async fn canvas_tools_allowed(
    state: &AppState,
    user: &AuthenticatedUser,
    agent: &Value,
) -> bool {
    if !state.space_features.agent_canvas_tools_api {
        return false;
    }
    if agent.get("canvas_tools_enabled").and_then(Value::as_bool) != Some(true) {
        return false;
    }
    !crate::zdr::org_zdr_lookup(state, user).await.treat_as_zdr()
}

/// Removes a client-named `space_canvas_tools` from a chat body's features. Only the binding policy adds it.
pub(crate) fn strip_canvas_tools_feature(object: &mut Map<String, Value>) {
    if let Some(features) = object.get_mut("features").and_then(Value::as_array_mut) {
        features.retain(|feature| feature.as_str() != Some(SPACE_CANVAS_TOOLS_FEATURE));
    }
}

/// The canvas-tools grant on an allowed mention turn: `agentic` (the run's tool loop; execution-core then offers the
/// canvas rows only under the `space_canvas_tools` marker), `space_canvas_tools`, and `plan_mode` off, because plan
/// mode refuses registry writes.
pub(crate) fn grant_canvas_tools_feature(object: &mut Map<String, Value>) {
    let mut features: Vec<Value> = object
        .get("features")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    features.retain(|feature| feature.as_str() != Some(SPACE_CANVAS_TOOLS_FEATURE));
    if !features.iter().any(|feature| feature.as_str() == Some("agentic")) {
        features.push(json!("agentic"));
    }
    features.push(json!(SPACE_CANVAS_TOOLS_FEATURE));
    object.insert("features".to_owned(), Value::Array(features));
    object.insert("plan_mode".to_owned(), json!(false));
}
