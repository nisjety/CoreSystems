//! WS10-S4: the Space approval decision, the standing-rule list and the approval policy read.
//!
//! * `POST /api/v1/spaces/{space_ref}/approvals/{approval_id}/decide`
//!   body `{decision, scope?, reason?, idempotency_key?}`. The decision itself is the same
//!   owner-path call the chat console makes (`/v1/orchestration/approvals/{id}/decide`): only the
//!   member who started the run can record it, and the approved action runs as that member. The
//!   scope is how far the answer reaches. `once` is always available. `always` is a Control
//!   standing rule (MCP tools only, entitlement and role checked by Control), recorded AFTER the
//!   decision; a failed rule call leaves a plain `once` decision and answers
//!   `{grant:{status:"not_recorded"}}`, so a lost rule never turns into a lost approval nor into
//!   a broader one. `thread` has no store yet (Model Plane `approval_scope_grants` is not built)
//!   and is refused, never downgraded silently.
//! * `GET /api/v1/spaces/{space_ref}/approval-grants` and `DELETE .../{grant_id}`: Control's list
//!   and revoke, relayed under the caller's delegation. Control decides who may.
//! * `GET /api/v1/spaces/{space_ref}/approval-policy`: Control's Space ceiling
//!   (`max_grant_scope`), which the scope picker reads to offer only what is allowed.
//!
//! Authority never comes from the body: the actor and organization are the session's, the
//! browser-supplied proofs (`grant_use_token`, `space_approval_decision_token`) are refused as
//! unknown fields, and a standing rule is built from the approval's own projection, never from
//! client fields. The standing-rule routes sit behind `SpaceFeatureFlags::approval_scopes` (off
//! answers 404 `feature_disabled`), a rollout control and never an authorization input. Rules,
//! keys and levels hold no message content; Zero Data Retention is refused by Control (412
//! `zdr_durable_mutation_forbidden`) and by the model path, not by this module.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::proxy_json,
};

type Reply = (StatusCode, Json<Value>);

const REASON_MAX_CHARS: usize = 500;
/// Control's bounds for a rule's idempotency key (`approval_scope.go`).
const KEY_MIN_CHARS: usize = 8;
const KEY_MAX_CHARS: usize = 200;
const MAX_RELAYED_MESSAGE_CHARS: usize = 300;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/spaces/{space_ref}/approvals/{approval_id}/decide",
            post(decide_space_approval_route),
        )
        .route(
            "/api/v1/spaces/{space_ref}/approval-grants",
            get(list_approval_grants_route),
        )
        .route(
            "/api/v1/spaces/{space_ref}/approval-grants/{grant_id}",
            delete(revoke_approval_grant_route),
        )
        .route(
            "/api/v1/spaces/{space_ref}/approval-policy",
            get(get_approval_policy_route),
        )
}

fn feature_disabled() -> Reply {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "feature_disabled",
            "Standing approvals are not enabled.",
        )),
    )
}

fn unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "approval_authority_unavailable",
            "Approval authority is unavailable. Nothing was changed.",
        )),
    )
}

fn invalid(field: &'static str, message: &'static str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "invalid_request",
            message,
            json!({ "field": field }),
        )),
    )
}

fn scope_not_permitted(reason: &'static str) -> Reply {
    (
        StatusCode::FORBIDDEN,
        Json(error_with_details(
            "approval_scope_not_permitted",
            "This approval scope is not available here.",
            json!({ "reason": reason }),
        )),
    )
}

fn actor_of(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn control_url(state: &AppState, space_ref: &str, tail: &str) -> String {
    format!(
        "{}/api/v1/internal/spaces/{}/{}",
        state.user_core_url,
        urlencoding::encode(space_ref),
        tail
    )
}

/// The organization and trimmed Space ref, or the typed refusal every Space route gives.
async fn scope_of(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
) -> Result<(String, String), Reply> {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let space_ref = space_ref.trim().to_owned();
    if org_id.is_empty() || space_ref.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        ));
    }
    Ok((org_id, space_ref))
}

/// A well-formed lowercase snake_case code, as Control's typed errors use.
fn is_error_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The typed `{code, message, details}` of a Control failure body, bounded and scalar-only, or
/// `None` when the body is not a well-formed typed error.
fn typed_control_error(body: &Value) -> Option<Value> {
    let error = body.get("error").and_then(Value::as_object)?;
    let code = error.get("code")?.as_str().filter(|code| is_error_code(code))?;
    let message: String = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(MAX_RELAYED_MESSAGE_CHARS)
        .collect();
    let mut out = Map::new();
    out.insert("code".to_owned(), json!(code));
    out.insert("message".to_owned(), json!(message));
    if let Some(details) = error.get("details").and_then(Value::as_object) {
        let scalars: Map<String, Value> = details
            .iter()
            .filter(|(_, value)| !value.is_array() && !value.is_object())
            .take(16)
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        out.insert("details".to_owned(), Value::Object(scalars));
    }
    Some(Value::Object(out))
}

/// Control's failure as the browser envelope: a typed body keeps its status; anything else (and a
/// typed 401, which the SPA would read as an expired session) is the 503 that says nothing changed.
fn relay_control_error(status: StatusCode, body: &Value) -> Reply {
    match typed_control_error(body) {
        Some(error) if status != StatusCode::UNAUTHORIZED => {
            (status, Json(json!({ "error": error })))
        }
        _ => unavailable(),
    }
}

// ---------------------------------------------------------------------------------------------
// Decide
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecideRequest {
    decision: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    idempotency_key: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Scope {
    Once,
    Thread,
    Always,
}

/// The validated decision. Nothing here has been trusted yet except its shape.
#[derive(Debug, PartialEq, Eq)]
struct ParsedDecision {
    approve: bool,
    scope: Scope,
    reason: String,
    idempotency_key: Option<String>,
}

fn parse_decision(raw: Value) -> Result<ParsedDecision, Reply> {
    let body: DecideRequest = serde_json::from_value(raw).map_err(|_| {
        invalid(
            "body",
            "A decision takes decision, scope, reason and idempotency_key only.",
        )
    })?;
    let approve = match body.decision.as_str() {
        "approve" => true,
        "reject" => false,
        _ => return Err(invalid("decision", "The decision is approve or reject.")),
    };
    let scope = match body.scope.as_deref().map(str::trim) {
        None | Some("") | Some("once") => Scope::Once,
        Some("thread") => Scope::Thread,
        Some("always") => Scope::Always,
        Some(_) => return Err(invalid("scope", "The scope is once, thread or always.")),
    };
    if !approve && scope != Scope::Once {
        return Err(invalid("scope", "Only an approval can carry a wider scope."));
    }
    let reason = body.reason.unwrap_or_default();
    if reason.chars().count() > REASON_MAX_CHARS {
        return Err(invalid("reason", "The reason is too long."));
    }
    let idempotency_key = body
        .idempotency_key
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty());
    if let Some(key) = &idempotency_key {
        let length = key.chars().count();
        if !(KEY_MIN_CHARS..=KEY_MAX_CHARS).contains(&length) {
            return Err(invalid(
                "idempotency_key",
                "The idempotency key is 8 to 200 characters.",
            ));
        }
    }
    if scope == Scope::Always && idempotency_key.is_none() {
        return Err(invalid(
            "idempotency_key",
            "A standing approval needs an idempotency key.",
        ));
    }
    Ok(ParsedDecision {
        approve,
        scope,
        reason,
        idempotency_key,
    })
}

/// The refusal for a failed owner-path decision, in the Space vocabulary. Only the status of the
/// upstream answer is used: its body is never forwarded on a failure.
fn map_decide_failure(status: StatusCode) -> Reply {
    match status {
        StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => (
            StatusCode::FORBIDDEN,
            Json(error(
                "approval_not_addressed_to_you",
                "This approval is waiting for the member who started the run.",
            )),
        ),
        StatusCode::NOT_FOUND => (
            StatusCode::NOT_FOUND,
            Json(error("approval_not_found", "That approval was not found.")),
        ),
        StatusCode::CONFLICT | StatusCode::PRECONDITION_FAILED => (
            StatusCode::CONFLICT,
            Json(error(
                "approval_already_decided",
                "This approval has already been decided.",
            )),
        ),
        StatusCode::GONE | StatusCode::REQUEST_TIMEOUT => (
            StatusCode::CONFLICT,
            Json(error("approval_expired", "This approval has expired.")),
        ),
        status if status.is_client_error() => (
            status,
            Json(error(
                "invalid_request",
                "The approval decision was not accepted.",
            )),
        ),
        _ => unavailable(),
    }
}

/// The approval inside an owner-path decide answer, whichever shape the wire used.
fn approval_of(upstream: &Value) -> &Value {
    upstream.get("approval").unwrap_or(upstream)
}

fn string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// The standing-rule body Control needs, built from the approval's own projection. For an MCP tool
/// the action key is the tool key (Control refuses anything else). The agent and the schema hash
/// are not on the approval projection yet (WS10-S6 `ResolveApprovalAuthority` carries them), so
/// an incomplete projection yields `None` and the decision stays a plain `once`.
fn rule_body(
    space_ref: &str,
    approval_id: &str,
    approval: &Value,
    idempotency_key: &str,
) -> Option<Value> {
    let tool_key = string_field(approval, &["tool_key"])
        .or_else(|| approval.get("cause").and_then(|cause| string_field(cause, &["tool"])))?;
    let action_key = approval
        .get("cause")
        .and_then(|cause| string_field(cause, &["action_key"]))
        .or_else(|| string_field(approval, &["action_key"]))
        .unwrap_or(tool_key);
    let agent_subject_id = string_field(approval, &["agent_subject_id"])?;
    let schema_hash = string_field(approval, &["action_schema_hash", "schema_hash"])?;
    Some(json!({
        "space_ref": space_ref,
        "scope": "always",
        "approval_id": approval_id,
        "agent_subject_id": agent_subject_id,
        "tool_key": tool_key,
        "action_key": action_key,
        "action_schema_hash": schema_hash,
        "idempotency_key": idempotency_key,
    }))
}

fn not_recorded(reason: &str) -> Value {
    json!({ "status": "not_recorded", "reason": reason })
}

/// Records the standing rule after an approve. Never fails the decision: any problem is a
/// `not_recorded` grant with a bounded reason.
async fn record_standing_rule(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    approval_id: &str,
    upstream: &Value,
    idempotency_key: &str,
) -> Value {
    let Some(body) = rule_body(space_ref, approval_id, approval_of(upstream), idempotency_key)
    else {
        return not_recorded("approval_projection_incomplete");
    };
    let url = format!(
        "{}/api/v1/internal/spaces/approval-grants",
        state.user_core_url
    );
    let (status, Json(control)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(body),
        Some(org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if status.is_success() {
        let data = unwrap_data(&control);
        let rule = data.get("grant").cloned().unwrap_or(Value::Null);
        return json!({ "status": "recorded", "rule": rule });
    }
    let code = typed_control_error(&control)
        .and_then(|error| error.get("code").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| "approval_authority_unavailable".to_owned());
    not_recorded(&code)
}

async fn decide_space_approval_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, approval_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Reply {
    decide_space_approval(&state, &user, &headers, &space_ref, &approval_id, body).await
}

/// The route's body, shared with the `spaces.decide_approval` action so the scope rules, the role
/// gate and the typed errors exist once.
pub(crate) async fn decide_space_approval(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    approval_id: &str,
    body: Value,
) -> Reply {
    let decision = match parse_decision(body) {
        Ok(decision) => decision,
        Err(reply) => return reply,
    };
    let approval_id = approval_id.trim();
    if approval_id.is_empty() {
        return invalid("approval_id", "An approval id is required.");
    }
    match decision.scope {
        Scope::Once => {}
        Scope::Thread => return scope_not_permitted("thread_scope_not_available"),
        Scope::Always => {
            if !state.space_features.approval_scopes {
                return scope_not_permitted("standing_approvals_disabled");
            }
        }
    }
    let (org_id, space_ref) = match scope_of(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    // The caller must be a member of THIS Space; a viewer never decides. Who owns the approval is
    // then Session Core's call (only the run's owner can record it).
    match super::resolve_space_role(state, user, &org_id, &space_ref).await {
        Ok(role) if role == "viewer" => {
            return (
                StatusCode::FORBIDDEN,
                Json(error(
                    "approval_not_addressed_to_you",
                    "A viewer cannot decide an approval.",
                )),
            )
        }
        Ok(role) if role.is_empty() => return unavailable(),
        Ok(_) => {}
        Err((status, Json(control))) => return relay_control_error(status, &control),
    }

    let mut upstream_body = json!({
        "decision": if decision.approve { "approve" } else { "reject" },
        "reason": decision.reason,
        "scope": "once",
    });
    if decision.reason.is_empty() {
        if let Some(object) = upstream_body.as_object_mut() {
            object.remove("reason");
        }
    }
    let path = format!(
        "/v1/orchestration/approvals/{}/decide",
        urlencoding::encode(approval_id)
    );
    let (status, Json(upstream)) = crate::domains::orchestration::mg_post_decide_with_retry(
        state,
        user,
        headers,
        &path,
        upstream_body,
    )
    .await;
    if !status.is_success() {
        return map_decide_failure(status);
    }

    let grant = match (&decision.scope, decision.approve, &decision.idempotency_key) {
        (Scope::Always, true, Some(key)) => {
            record_standing_rule(state, user, &org_id, &space_ref, approval_id, &upstream, key)
                .await
        }
        _ => json!({ "status": "none", "scope": "once" }),
    };
    (
        StatusCode::OK,
        Json(json!({ "data": { "approval": approval_of(&upstream).clone(), "grant": grant } })),
    )
}

// ---------------------------------------------------------------------------------------------
// Standing rules and policy (Control relays)
// ---------------------------------------------------------------------------------------------

async fn list_approval_grants_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
) -> Reply {
    list_approval_grants(&state, &user, &space_ref).await
}

pub(crate) async fn list_approval_grants(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
) -> Reply {
    if !state.space_features.approval_scopes {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope_of(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let (status, Json(control)) = proxy_json(
        state,
        Method::GET,
        &control_url(state, &space_ref, "approval-grants"),
        None,
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, &control);
    }
    let data = unwrap_data(&control);
    let grants = data.get("grants").cloned().unwrap_or_else(|| json!([]));
    (StatusCode::OK, Json(json!({ "data": { "grants": grants } })))
}

async fn revoke_approval_grant_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, grant_id)): Path<(String, String)>,
) -> Reply {
    revoke_approval_grant(&state, &user, &space_ref, &grant_id).await
}

pub(crate) async fn revoke_approval_grant(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    grant_id: &str,
) -> Reply {
    if !state.space_features.approval_scopes {
        return feature_disabled();
    }
    let grant_id = grant_id.trim();
    if grant_id.is_empty() || grant_id.chars().count() > 128 {
        return invalid("grant_id", "A standing approval id is required.");
    }
    let (org_id, space_ref) = match scope_of(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let tail = format!("approval-grants/{}", urlencoding::encode(grant_id));
    let (status, Json(control)) = proxy_json(
        state,
        Method::DELETE,
        &control_url(state, &space_ref, &tail),
        None,
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, &control);
    }
    let data = unwrap_data(&control);
    let grant = data.get("grant").cloned().unwrap_or(Value::Null);
    (StatusCode::OK, Json(json!({ "data": { "grant": grant } })))
}

async fn get_approval_policy_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
) -> Reply {
    if !state.space_features.approval_scopes {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope_of(&state, &user, &space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let (status, Json(control)) = proxy_json(
        &state,
        Method::GET,
        &control_url(&state, &space_ref, "approval-policy"),
        None,
        Some(&org_id),
        Some(&actor_of(&user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, &control);
    }
    let data = unwrap_data(&control);
    let policy = data.get("policy").cloned().unwrap_or(Value::Null);
    (StatusCode::OK, Json(json!({ "data": { "policy": policy } })))
}
