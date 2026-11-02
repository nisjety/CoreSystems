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

#[cfg(test)]
mod tests {
    use super::{
        decide_space_approval, get_approval_policy_route, list_approval_grants, map_decide_failure,
        parse_decision, relay_control_error, revoke_approval_grant, rule_body, typed_control_error,
        Scope,
    };
    use crate::config::AppState;
    use crate::middleware::{AuthenticatedUser, AuthorizedMembership};
    use axum::{
        extract::{Extension, Path, State},
        http::{HeaderMap, StatusCode},
        Json,
    };
    use serde_json::{json, Value};
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SPACE: &str = "space-1";

    fn member() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-appr".to_owned(),
            user_email: "appr@example.com".to_owned(),
            user_name: "Appr".to_owned(),
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

    fn state_with(server: &MockServer, approval_scopes: bool) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = server.uri();
        state.model_gateway_url = server.uri();
        state.user_core_url = server.uri();
        state.space_features.approval_scopes = approval_scopes;
        state
    }

    async fn mount_tokens(server: &MockServer) {
        for (token_path, token) in [
            ("/api/session-core/token", "session-test-token"),
            ("/api/execution-core/token", "execution-test-token"),
        ] {
            Mock::given(wm_method("GET"))
                .and(wm_path(token_path))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "token": token })))
                .mount(server)
                .await;
        }
    }

    async fn mount_membership(server: &MockServer, role: &str) {
        Mock::given(wm_method("GET"))
            .and(wm_path(format!(
                "/api/v1/internal/spaces/{SPACE}/membership"
            )))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "data": { "role": role } })),
            )
            .mount(server)
            .await;
    }

    async fn mount_decide(server: &MockServer, template: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/orchestration/approvals/apr-1/decide"))
            .respond_with(template)
            .mount(server)
            .await;
    }

    async fn requests_to(server: &MockServer, needle: &str) -> usize {
        server
            .received_requests()
            .await
            .expect("requests")
            .iter()
            .filter(|request| request.url.path().contains(needle))
            .count()
    }

    async fn decide(state: &AppState, body: Value) -> (StatusCode, Value) {
        let (status, Json(value)) =
            decide_space_approval(state, &member(), &HeaderMap::new(), SPACE, "apr-1", body).await;
        (status, value)
    }

    fn refused_field(body: &Value) -> Option<&str> {
        body["error"]["details"]["field"].as_str()
    }

    // ----- forged authority ------------------------------------------------------------------

    #[test]
    fn forged_space_authority_fields_strip_the_approval_proofs() {
        let fields = super::super::FORGED_SPACE_AUTHORITY_FIELDS;
        assert!(fields.contains(&"grant_use_token"));
        assert!(fields.contains(&"space_approval_decision_token"));
        // The older proofs stay on the list.
        assert!(fields.contains(&"space_decision_token"));
        assert!(fields.contains(&"payload_digest"));
        // No duplicates: one entry per field.
        let mut sorted = fields.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), fields.len());
    }

    // ----- parse_decision --------------------------------------------------------------------

    #[test]
    fn parse_decision_defaults_to_once_and_trims_the_scope() {
        let parsed = parse_decision(json!({ "decision": "approve" })).expect("valid");
        assert!(parsed.approve);
        assert_eq!(parsed.scope, Scope::Once);
        assert_eq!(parsed.reason, "");
        assert_eq!(parsed.idempotency_key, None);
        let parsed = parse_decision(json!({ "decision": "approve", "scope": "  " })).unwrap();
        assert_eq!(parsed.scope, Scope::Once);
        let parsed = parse_decision(json!({ "decision": "reject", "scope": " once " })).unwrap();
        assert!(!parsed.approve);
        assert_eq!(parsed.scope, Scope::Once);
        let parsed = parse_decision(json!({ "decision": "approve", "scope": "thread" })).unwrap();
        assert_eq!(parsed.scope, Scope::Thread);
    }

    #[test]
    fn parse_decision_refuses_every_browser_supplied_proof_and_unknown_key() {
        for key in [
            "grant_use_token",
            "space_approval_decision_token",
            "decided_by",
            "org_id",
            "user_id",
            "decision_source",
            "tool_key",
        ] {
            let mut body = json!({ "decision": "approve" });
            body[key] = json!("forged");
            let (status, Json(refused)) = parse_decision(body).expect_err("unknown key refused");
            assert_eq!(status, StatusCode::BAD_REQUEST, "{key}");
            assert_eq!(refused["error"]["code"], "invalid_request", "{key}");
            assert_eq!(refused_field(&refused), Some("body"), "{key}");
        }
    }

    #[test]
    fn parse_decision_refuses_a_bad_decision_scope_and_a_wide_reject() {
        for bad in ["", "APPROVE", "allow", "approved"] {
            let (status, Json(refused)) =
                parse_decision(json!({ "decision": bad })).expect_err("bad decision");
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(refused_field(&refused), Some("decision"));
        }
        let (_, Json(refused)) =
            parse_decision(json!({ "decision": "approve", "scope": "forever" })).unwrap_err();
        assert_eq!(refused_field(&refused), Some("scope"));
        for scope in ["thread", "always"] {
            let (status, Json(refused)) = parse_decision(
                json!({ "decision": "reject", "scope": scope, "idempotency_key": "key-aaaaaaaa" }),
            )
            .expect_err("a reject cannot carry a wider scope");
            assert_eq!(status, StatusCode::BAD_REQUEST, "{scope}");
            assert_eq!(refused_field(&refused), Some("scope"), "{scope}");
        }
        // The decision field is required.
        assert!(parse_decision(json!({ "scope": "once" })).is_err());
        assert!(parse_decision(json!("approve")).is_err());
    }

    #[test]
    fn parse_decision_bounds_the_reason_at_500_characters() {
        let at_limit = "r".repeat(500);
        let parsed =
            parse_decision(json!({ "decision": "approve", "reason": at_limit })).expect("500 ok");
        assert_eq!(parsed.reason.chars().count(), 500);
        // Characters, not bytes.
        let multibyte = "\u{00e5}".repeat(500);
        assert!(parse_decision(json!({ "decision": "approve", "reason": multibyte })).is_ok());
        let (_, Json(refused)) =
            parse_decision(json!({ "decision": "approve", "reason": "r".repeat(501) }))
                .expect_err("over the bound");
        assert_eq!(refused_field(&refused), Some("reason"));
    }

    #[test]
    fn parse_decision_always_needs_a_key_of_8_to_200_characters() {
        let (_, Json(refused)) =
            parse_decision(json!({ "decision": "approve", "scope": "always" }))
                .expect_err("always needs a key");
        assert_eq!(refused_field(&refused), Some("idempotency_key"));
        // A blank key is no key.
        let (_, Json(refused)) = parse_decision(
            json!({ "decision": "approve", "scope": "always", "idempotency_key": "   " }),
        )
        .unwrap_err();
        assert_eq!(refused_field(&refused), Some("idempotency_key"));
        for (key, ok) in [
            ("a".repeat(7), false),
            ("a".repeat(8), true),
            ("a".repeat(200), true),
            ("a".repeat(201), false),
        ] {
            let result = parse_decision(
                json!({ "decision": "approve", "scope": "always", "idempotency_key": key }),
            );
            assert_eq!(result.is_ok(), ok, "key length {}", key.len());
        }
        // The key is trimmed before it is measured and kept.
        let parsed = parse_decision(
            json!({ "decision": "approve", "scope": "always", "idempotency_key": "  key-aaaaaaaa  " }),
        )
        .unwrap();
        assert_eq!(parsed.idempotency_key.as_deref(), Some("key-aaaaaaaa"));
        // A too-short key is refused even for a once decision.
        assert!(parse_decision(
            json!({ "decision": "approve", "scope": "once", "idempotency_key": "short" })
        )
        .is_err());
    }

    // ----- map_decide_failure ----------------------------------------------------------------

    #[test]
    fn map_decide_failure_gives_the_space_vocabulary_and_never_the_upstream_body() {
        let cases = [
            (StatusCode::FORBIDDEN, 403, "approval_not_addressed_to_you"),
            (StatusCode::UNAUTHORIZED, 403, "approval_not_addressed_to_you"),
            (StatusCode::NOT_FOUND, 404, "approval_not_found"),
            (StatusCode::CONFLICT, 409, "approval_already_decided"),
            (StatusCode::PRECONDITION_FAILED, 409, "approval_already_decided"),
            (StatusCode::GONE, 409, "approval_expired"),
            (StatusCode::REQUEST_TIMEOUT, 409, "approval_expired"),
            (StatusCode::UNPROCESSABLE_ENTITY, 422, "invalid_request"),
            (StatusCode::BAD_REQUEST, 400, "invalid_request"),
            (StatusCode::BAD_GATEWAY, 503, "approval_authority_unavailable"),
            (StatusCode::SERVICE_UNAVAILABLE, 503, "approval_authority_unavailable"),
            (StatusCode::INTERNAL_SERVER_ERROR, 503, "approval_authority_unavailable"),
        ];
        for (upstream, status, code) in cases {
            let (got, Json(body)) = map_decide_failure(upstream);
            assert_eq!(got.as_u16(), status, "{upstream}");
            assert_eq!(body["error"]["code"], code, "{upstream}");
        }
    }

    // ----- rule_body -------------------------------------------------------------------------

    #[test]
    fn rule_body_is_none_when_the_projection_lacks_the_agent_or_the_schema_hash() {
        let complete = json!({
            "tool_key": "mcp__github__create_issue",
            "agent_subject_id": "agent-1",
            "action_schema_hash": "hhhh",
        });
        assert!(rule_body(SPACE, "apr-1", &complete, "key-aaaaaaaa").is_some());
        let mut no_agent = complete.clone();
        no_agent.as_object_mut().unwrap().remove("agent_subject_id");
        assert!(rule_body(SPACE, "apr-1", &no_agent, "key-aaaaaaaa").is_none());
        let mut blank_agent = complete.clone();
        blank_agent["agent_subject_id"] = json!("   ");
        assert!(rule_body(SPACE, "apr-1", &blank_agent, "key-aaaaaaaa").is_none());
        let mut no_hash = complete.clone();
        no_hash.as_object_mut().unwrap().remove("action_schema_hash");
        assert!(rule_body(SPACE, "apr-1", &no_hash, "key-aaaaaaaa").is_none());
        let mut no_tool = complete.clone();
        no_tool.as_object_mut().unwrap().remove("tool_key");
        assert!(rule_body(SPACE, "apr-1", &no_tool, "key-aaaaaaaa").is_none());
        // The ordinary approval projection today carries neither field.
        assert!(rule_body(
            SPACE,
            "apr-1",
            &json!({ "id": "apr-1", "status": "GRANTED" }),
            "key-aaaaaaaa"
        )
        .is_none());
    }

    #[test]
    fn rule_body_projects_a_complete_mcp_approval_and_never_takes_client_fields() {
        let body = rule_body(
            SPACE,
            "apr-1",
            &json!({
                "tool_key": "mcp__github__create_issue",
                "agent_subject_id": "agent-1",
                "action_schema_hash": "hhhh",
            }),
            "key-aaaaaaaa",
        )
        .expect("complete");
        assert_eq!(
            body,
            json!({
                "space_ref": SPACE,
                "scope": "always",
                "approval_id": "apr-1",
                "agent_subject_id": "agent-1",
                "tool_key": "mcp__github__create_issue",
                "action_key": "mcp__github__create_issue",
                "action_schema_hash": "hhhh",
                "idempotency_key": "key-aaaaaaaa",
            })
        );
        // The alternate wire shape: the tool and action keys inside `cause`, schema_hash alias.
        let body = rule_body(
            SPACE,
            "apr-1",
            &json!({
                "cause": { "tool": "mcp__x__y", "action_key": "mcp__x__y" },
                "agent_subject_id": "agent-2",
                "schema_hash": "gggg",
            }),
            "key-aaaaaaaa",
        )
        .expect("cause shape");
        assert_eq!(body["tool_key"], "mcp__x__y");
        assert_eq!(body["action_schema_hash"], "gggg");
        assert_eq!(body["agent_subject_id"], "agent-2");
    }

    #[test]
    fn typed_control_error_keeps_a_bounded_scalar_body_and_drops_anything_else() {
        let typed = typed_control_error(&json!({
            "error": {
                "code": "grant_scope_exceeds_ceiling",
                "message": "m".repeat(400),
                "details": { "ceiling": "once", "nested": { "a": 1 }, "list": [1] }
            }
        }))
        .expect("typed");
        assert_eq!(typed["code"], "grant_scope_exceeds_ceiling");
        assert_eq!(typed["message"].as_str().unwrap().chars().count(), 300);
        assert_eq!(typed["details"], json!({ "ceiling": "once" }));
        for malformed in [
            json!({}),
            json!({ "error": "boom" }),
            json!({ "error": { "message": "no code" } }),
            json!({ "error": { "code": "Has Spaces" } }),
            json!({ "error": { "code": "UPPER" } }),
            json!({ "error": { "code": "" } }),
        ] {
            assert!(typed_control_error(&malformed).is_none(), "{malformed}");
        }
    }

    // ----- decide: authority guards ----------------------------------------------------------

    #[tokio::test]
    async fn thread_scope_is_refused_403_before_any_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        let state = state_with(&server, true);
        let (status, body) =
            decide(&state, json!({ "decision": "approve", "scope": "thread" })).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "approval_scope_not_permitted");
        assert_eq!(body["error"]["details"]["reason"], "thread_scope_not_available");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn always_with_the_flag_off_is_refused_before_any_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        let state = state_with(&server, false);
        let (status, body) = decide(
            &state,
            json!({ "decision": "approve", "scope": "always", "idempotency_key": "key-aaaaaaaa" }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "approval_scope_not_permitted");
        assert_eq!(body["error"]["details"]["reason"], "standing_approvals_disabled");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_viewer_cannot_decide_and_the_decision_never_reaches_session_core() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_tokens(&server).await;
        mount_membership(&server, "viewer").await;
        mount_decide(&server, ResponseTemplate::new(200).set_body_json(json!({}))).await;
        let state = state_with(&server, true);
        let (status, body) = decide(&state, json!({ "decision": "approve" })).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "approval_not_addressed_to_you");
        assert_eq!(requests_to(&server, "/decide").await, 0);
    }

    #[tokio::test]
    async fn a_non_member_gets_controls_typed_refusal_and_no_decision_is_made() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_tokens(&server).await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("/api/v1/internal/spaces/{SPACE}/membership")))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": { "code": "not_a_space_member", "message": "Not a member.",
                           "details": { "space_ref": SPACE } }
            })))
            .mount(&server)
            .await;
        mount_decide(&server, ResponseTemplate::new(200).set_body_json(json!({}))).await;
        let state = state_with(&server, true);
        let (status, body) = decide(&state, json!({ "decision": "approve" })).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "not_a_space_member");
        assert_eq!(body["error"]["details"]["space_ref"], SPACE);
        assert_eq!(requests_to(&server, "/decide").await, 0);
    }

    #[tokio::test]
    async fn an_untyped_membership_failure_is_the_503_that_says_nothing_changed() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_tokens(&server).await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("/api/v1/internal/spaces/{SPACE}/membership")))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;
        let state = state_with(&server, true);
        let (status, body) = decide(&state, json!({ "decision": "approve" })).await;
        // proxy_json types a bare upstream failure as `upstream_error` before relay_control_error
        // sees it, so the typed branch keeps the status (DEPTH-GW2-1: the 503 word is unreachable
        // through proxy_json; the decision still never happens).
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], "upstream_error");
        assert_eq!(requests_to(&server, "/decide").await, 0);
    }

    #[test]
    fn relay_control_error_keeps_a_typed_status_and_hides_anything_untyped() {
        let (status, Json(body)) = relay_control_error(
            StatusCode::FORBIDDEN,
            &json!({ "error": { "code": "role_forbidden", "message": "m" } }),
        );
        assert_eq!((status, body["error"]["code"].clone()), (StatusCode::FORBIDDEN, json!("role_forbidden")));
        for untyped in [json!("down"), json!({}), json!({ "error": "boom" }), Value::Null] {
            let (status, Json(body)) = relay_control_error(StatusCode::INTERNAL_SERVER_ERROR, &untyped);
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{untyped}");
            assert_eq!(body["error"]["code"], "approval_authority_unavailable");
        }
        // A typed 401 would read as an expired session in the SPA: it is the 503 instead.
        let (status, _) = relay_control_error(
            StatusCode::UNAUTHORIZED,
            &json!({ "error": { "code": "unauthenticated", "message": "m" } }),
        );
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn a_user_without_an_org_membership_is_refused_before_any_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        let state = state_with(&server, true);
        let mut user = member();
        user.authorized_membership = None;
        let (status, Json(body)) = decide_space_approval(
            &state,
            &user,
            &HeaderMap::new(),
            SPACE,
            "apr-1",
            json!({ "decision": "approve" }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "active_membership_required");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    // ----- decide: outcomes ------------------------------------------------------------------

    #[tokio::test]
    async fn a_once_approval_sends_only_the_decision_upstream_and_reports_no_grant() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_tokens(&server).await;
        mount_membership(&server, "member").await;
        mount_decide(
            &server,
            ResponseTemplate::new(200)
                .set_body_json(json!({ "approval": { "id": "apr-1", "status": "GRANTED" } })),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body) = decide(&state, json!({ "decision": "approve", "reason": "ok" })).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["approval"]["status"], "GRANTED");
        assert_eq!(body["data"]["grant"], json!({ "status": "none", "scope": "once" }));
        let sent = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path().ends_with("/decide"))
            .expect("decide call");
        let upstream: Value = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(upstream, json!({ "decision": "approve", "reason": "ok", "scope": "once" }));
        // No standing rule was written.
        assert_eq!(requests_to(&server, "approval-grants").await, 0);
    }

    #[tokio::test]
    async fn an_empty_reason_is_omitted_and_the_upstream_scope_is_always_once() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_tokens(&server).await;
        mount_membership(&server, "member").await;
        mount_decide(
            &server,
            ResponseTemplate::new(200).set_body_json(json!({ "id": "apr-1", "status": "DENIED" })),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body) = decide(&state, json!({ "decision": "reject" })).await;
        assert_eq!(status, StatusCode::OK);
        // A bare approval body (no `approval` wrapper) is returned as the approval.
        assert_eq!(body["data"]["approval"]["status"], "DENIED");
        let sent = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path().ends_with("/decide"))
            .unwrap();
        let upstream: Value = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(upstream, json!({ "decision": "reject", "scope": "once" }));
    }

    #[tokio::test]
    async fn an_owner_path_failure_is_mapped_and_its_body_is_not_forwarded() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_tokens(&server).await;
        mount_membership(&server, "member").await;
        mount_decide(
            &server,
            ResponseTemplate::new(412)
                .set_body_json(json!({ "error": { "code": "internal_detail", "message": "secret-ish" } })),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body) = decide(&state, json!({ "decision": "approve" })).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "approval_already_decided");
        assert!(!body.to_string().contains("secret-ish"));
    }

    #[tokio::test]
    async fn always_on_a_complete_projection_records_the_rule_after_the_decision() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_tokens(&server).await;
        mount_membership(&server, "manager").await;
        mount_decide(
            &server,
            ResponseTemplate::new(200).set_body_json(json!({
                "approval": {
                    "id": "apr-1", "status": "GRANTED",
                    "tool_key": "mcp__github__create_issue",
                    "agent_subject_id": "agent-1",
                    "action_schema_hash": "hhhh"
                }
            })),
        )
        .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/approval-grants"))
            .respond_with(ResponseTemplate::new(201).set_body_json(
                json!({ "data": { "grant": { "id": "grant-1", "scope": "always" } } }),
            ))
            .mount(&server)
            .await;
        let state = state_with(&server, true);
        let (status, body) = decide(
            &state,
            json!({ "decision": "approve", "scope": "always", "idempotency_key": "key-aaaaaaaa" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["grant"]["status"], "recorded");
        assert_eq!(body["data"]["grant"]["rule"]["id"], "grant-1");
        let sent = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path().ends_with("/approval-grants"))
            .expect("rule call");
        let rule: Value = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(rule["agent_subject_id"], "agent-1");
        assert_eq!(rule["idempotency_key"], "key-aaaaaaaa");
        assert_eq!(rule["space_ref"], SPACE);
        assert!(sent.headers.get("x-delegation-signature").is_some(), "a signed delegation");
    }

    #[tokio::test]
    async fn always_on_an_incomplete_projection_stays_a_once_decision_with_not_recorded() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_tokens(&server).await;
        mount_membership(&server, "manager").await;
        mount_decide(
            &server,
            ResponseTemplate::new(200)
                .set_body_json(json!({ "approval": { "id": "apr-1", "status": "GRANTED" } })),
        )
        .await;
        let state = state_with(&server, true);
        let (status, body) = decide(
            &state,
            json!({ "decision": "approve", "scope": "always", "idempotency_key": "key-aaaaaaaa" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["grant"]["status"], "not_recorded");
        assert_eq!(body["data"]["grant"]["reason"], "approval_projection_incomplete");
        assert_eq!(requests_to(&server, "approval-grants").await, 0);
    }

    #[tokio::test]
    async fn a_failed_rule_call_leaves_the_decision_200_with_the_controls_code() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_tokens(&server).await;
        mount_membership(&server, "manager").await;
        mount_decide(
            &server,
            ResponseTemplate::new(200).set_body_json(json!({
                "approval": {
                    "id": "apr-1", "status": "GRANTED",
                    "tool_key": "mcp__github__create_issue",
                    "agent_subject_id": "agent-1",
                    "action_schema_hash": "hhhh"
                }
            })),
        )
        .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/approval-grants"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": { "code": "grant_scope_exceeds_ceiling", "message": "ceiling" }
            })))
            .mount(&server)
            .await;
        let state = state_with(&server, true);
        let (status, body) = decide(
            &state,
            json!({ "decision": "approve", "scope": "always", "idempotency_key": "key-aaaaaaaa" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["approval"]["status"], "GRANTED");
        assert_eq!(body["data"]["grant"]["status"], "not_recorded");
        assert_eq!(body["data"]["grant"]["reason"], "grant_scope_exceeds_ceiling");
    }

    #[tokio::test]
    async fn a_rule_call_with_an_untyped_failure_reports_the_bounded_upstream_code() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        mount_tokens(&server).await;
        mount_membership(&server, "manager").await;
        mount_decide(
            &server,
            ResponseTemplate::new(200).set_body_json(json!({
                "approval": {
                    "id": "apr-1", "tool_key": "mcp__a__b",
                    "agent_subject_id": "agent-1", "action_schema_hash": "hhhh"
                }
            })),
        )
        .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/approval-grants"))
            .respond_with(ResponseTemplate::new(500).set_body_string("down"))
            .mount(&server)
            .await;
        let state = state_with(&server, true);
        let (status, body) = decide(
            &state,
            json!({ "decision": "approve", "scope": "always", "idempotency_key": "key-aaaaaaaa" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // proxy_json types the bare 500 as `upstream_error`; the reason is that bounded code.
        assert_eq!(body["data"]["grant"]["reason"], "upstream_error");
    }

    #[tokio::test]
    async fn an_always_reject_is_refused_as_invalid_before_any_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        let state = state_with(&server, true);
        let (status, body) = decide(
            &state,
            json!({ "decision": "reject", "scope": "always", "idempotency_key": "key-aaaaaaaa" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(refused_field(&body), Some("scope"));
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    // ----- list / revoke / policy ------------------------------------------------------------

    #[tokio::test]
    async fn list_revoke_and_policy_are_404_feature_disabled_with_the_flag_off() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        let state = state_with(&server, false);
        let user = member();
        let (status, Json(body)) = list_approval_grants(&state, &user, SPACE).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "feature_disabled");
        let (status, Json(body)) = revoke_approval_grant(&state, &user, SPACE, "grant-1").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "feature_disabled");
        let (status, Json(body)) = get_approval_policy_route(
            State(state.clone()),
            Extension(user.clone()),
            Path(SPACE.to_owned()),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "feature_disabled");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_relays_the_grants_and_a_control_refusal_keeps_its_typed_code() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("/api/v1/internal/spaces/{SPACE}/approval-grants")))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({ "data": { "grants": [{ "id": "grant-1" }, { "id": "grant-2" }] } }),
            ))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let state = state_with(&server, true);
        let (status, Json(body)) = list_approval_grants(&state, &member(), SPACE).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["grants"].as_array().unwrap().len(), 2);

        server.reset().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("/api/v1/internal/spaces/{SPACE}/approval-grants")))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": { "code": "role_forbidden", "message": "managers only" }
            })))
            .mount(&server)
            .await;
        let (status, Json(body)) = list_approval_grants(&state, &member(), SPACE).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "role_forbidden");
    }

    #[tokio::test]
    async fn revoke_validates_the_grant_id_and_relays_the_delete() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        Mock::given(wm_method("DELETE"))
            .and(wm_path(format!(
                "/api/v1/internal/spaces/{SPACE}/approval-grants/grant-1"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({ "data": { "grant": { "id": "grant-1", "status": "revoked" } } }),
            ))
            .mount(&server)
            .await;
        let state = state_with(&server, true);
        let user = member();
        for bad in ["", "   ", &"g".repeat(129)] {
            let (status, Json(body)) = revoke_approval_grant(&state, &user, SPACE, bad).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(refused_field(&body), Some("grant_id"));
        }
        assert!(server.received_requests().await.unwrap().is_empty());
        let (status, Json(body)) = revoke_approval_grant(&state, &user, SPACE, "grant-1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["grant"]["status"], "revoked");
    }

    #[tokio::test]
    async fn policy_returns_the_ceiling_and_a_typed_401_is_not_relayed_as_an_expired_session() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("/api/v1/internal/spaces/{SPACE}/approval-policy")))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({ "data": { "policy": { "max_grant_scope": "always" } } }),
            ))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let state = state_with(&server, true);
        let (status, Json(body)) = get_approval_policy_route(
            State(state.clone()),
            Extension(member()),
            Path(SPACE.to_owned()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["policy"]["max_grant_scope"], "always");

        server.reset().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("/api/v1/internal/spaces/{SPACE}/approval-policy")))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": { "code": "unauthenticated", "message": "no" }
            })))
            .mount(&server)
            .await;
        let (status, Json(body)) = get_approval_policy_route(
            State(state),
            Extension(member()),
            Path(SPACE.to_owned()),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "approval_authority_unavailable");
    }
}
