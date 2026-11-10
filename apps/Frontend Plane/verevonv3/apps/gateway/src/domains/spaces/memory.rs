//! Space memory for the Space's owner or admin, the BFF half of EP-8 S7.
//!
//! `GET /api/v1/spaces/{space_ref}/memory` lists what a shared room has learned (its `space` scope
//! rows), each marked when the room's current audience does not recall it, and `DELETE
//! /api/v1/spaces/{space_ref}/memory/{memory_id}` removes one row. This hop decides nothing about
//! who may do either. In order (each step is a refusal before the next one runs):
//!
//! 1. the rollout flag `GATEWAY_SPACE_MEMORY_UI` (404 `feature_unavailable` when off; never an
//!    authorization input),
//! 2. live organization membership (403 `active_membership_required`) and the request shape (400
//!    `invalid_space` / `invalid_memory`),
//! 3. the Space lifecycle (404 `space_not_found`, 503 `space_lifecycle_unavailable`),
//! 4. a fresh Control `model.space.memory.manage` decision for THIS list or THIS row, which Control
//!    issues to the Space's owner or admin only (403 `space_memory_not_authorized`). The decision
//!    never reaches the browser and is never cached: it travels to model-gateway in a POST body,
//! 5. the delegated Session Core and Model credentials, then model-gateway, and Session Core, which
//!    verifies the decision again and is the authority.
//!
//! The Space is the path's `space_ref`, resolved here and bound into the Control decision; no client
//! `space_id` is read from a query or a body. Zero Data Retention needs no pre-check: neither route
//! stores content, a zero-retention list is answered empty by Session Core, and a delete is allowed
//! under every posture. Errors are `{error:{code,message,details}}` with `details` an object.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use super::{denial, space_lifecycle_by_ref};
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json},
};

const MAX_SPACE_REF_LEN: usize = 200;
const MAX_MEMORY_ID_LEN: usize = 64;

/// The two operations a Control Space memory decision binds; Session Core's names.
const OPERATION_LIST: &str = "list";
const OPERATION_DELETE: &str = "delete";
/// The decision operation that erases one named agent's memory in the Space (EP-6 P3). Its
/// `memory_id` is the agent's subject id, so one decision erases one agent's rows only.
const OPERATION_ERASE_AGENT: &str = "erase_agent";

const DENIED: &str = "Managing this Space's memory is not authorized.";

type Reply = (StatusCode, Json<Value>);

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/spaces/{space_ref}/memory", get(list_space_memory))
        .route(
            "/api/v1/spaces/{space_ref}/memory/{memory_id}",
            delete(delete_space_memory),
        )
}

fn reply(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn valid_space_ref(space_ref: &str) -> bool {
    !space_ref.is_empty()
        && space_ref.len() <= MAX_SPACE_REF_LEN
        && !space_ref.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

fn valid_memory_id(memory_id: &str) -> bool {
    !memory_id.is_empty()
        && memory_id.len() <= MAX_MEMORY_ID_LEN
        && memory_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The model-gateway path and body of one Space memory call. The Space comes from the resolved
/// path `space_ref` only; the decision rides in the body, never the URL.
fn model_request(
    model_gateway_url: &str,
    space_ref: &str,
    operation: &str,
    memory_id: &str,
    decision_ref: &str,
    token: &str,
) -> (String, Value) {
    let url = format!(
        "{}/v1/spaces/{}/memory/{}",
        model_gateway_url,
        urlencoding::encode(space_ref),
        operation,
    );
    let mut body = json!({
        "space_control_decision_ref": decision_ref,
        "space_control_decision_token": token,
    });
    if operation == OPERATION_DELETE {
        body["memory_id"] = json!(memory_id);
    }
    (url, body)
}

async fn list_space_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
) -> Response {
    let (status, Json(body)) =
        space_memory_core(&state, &user, &headers, &space_ref, OPERATION_LIST, "").await;
    (status, Json(body)).into_response()
}

async fn delete_space_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, memory_id)): Path<(String, String)>,
) -> Response {
    let (status, Json(body)) = space_memory_core(
        &state,
        &user,
        &headers,
        &space_ref,
        OPERATION_DELETE,
        &memory_id,
    )
    .await;
    (status, Json(body)).into_response()
}

/// Both routes' work, so the flag, the Control decision and the typed errors exist once.
async fn space_memory_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    operation: &'static str,
    memory_id: &str,
) -> Reply {
    // 1. Rollout flag. A rollout control, never an authorization input.
    if !state.space_features.memory_ui {
        return reply(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "Space memory is not enabled.",
        );
    }
    // 2. Membership, then shape.
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return reply(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required for Space memory.",
        );
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return reply(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A valid Space reference is required.",
        );
    }
    let memory_id = memory_id.trim();
    if operation == OPERATION_DELETE && !valid_memory_id(memory_id) {
        return reply(
            StatusCode::BAD_REQUEST,
            "invalid_memory",
            "A valid memory id is required.",
        );
    }

    // 3. Space lifecycle.
    match space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return reply(
                StatusCode::NOT_FOUND,
                "space_not_found",
                "Space is not available.",
            )
        }
        Err(()) => {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            )
        }
    }

    // 4. A fresh Control decision for this list or this row.
    let (decision_ref, token) =
        match space_memory_decision(state, user, &org_id, space_ref, operation, memory_id).await {
            Ok(decision) => decision,
            Err(reply) => return reply,
        };

    // 5. Delegated credentials, then model-gateway.
    let session_token =
        match crate::domains::chat::shared::required_session_token(state, user, headers).await {
            Ok(token) => token,
            Err(reason) => {
                let response =
                    crate::domains::chat::shared::delegated_auth_unavailable(reason).into_response();
                return (
                    response.status(),
                    Json(error(
                        "delegated_auth_unavailable",
                        "Delegated credentials are unavailable.",
                    )),
                );
            }
        };
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
    let (url, body) = model_request(
        &state.model_gateway_url,
        space_ref,
        operation,
        memory_id,
        &decision_ref,
        &token,
    );
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(body),
        model_token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    relay_answer(status, &response, operation, memory_id)
}

/// Best-effort erase of one named agent's memory once its binding is revoked (EP-6 P3, EP-8 S10).
///
/// Runs after the revocation has landed and never changes the revoke's answer: any failure here is
/// logged by status only and the revoke still succeeds. Not behind the Space memory UI flag, since
/// that flag gates a screen, not whether a revoked agent's notes may outlive its binding. Session
/// Core is the authority: it verifies the fresh Control decision issued for THIS agent, and erases
/// nothing (answers zero) while agent memory is off. Allowed under every retention posture: the
/// call removes rows and carries ids, never content, and only a row count is logged.
pub(super) async fn erase_agent_memory_after_revocation(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
    space_ref: &str,
    agent_subject_id: &str,
) {
    let space_ref = space_ref.trim();
    let agent_subject_id = agent_subject_id.trim();
    if org_id.trim().is_empty() || !valid_space_ref(space_ref) || !valid_memory_id(agent_subject_id) {
        tracing::warn!("revoked agent memory erase skipped: no valid Space or agent subject");
        return;
    }
    let (decision_ref, token) = match space_memory_decision(
        state,
        user,
        org_id,
        space_ref,
        OPERATION_ERASE_AGENT,
        agent_subject_id,
    )
    .await
    {
        Ok(decision) => decision,
        Err((status, _)) => {
            tracing::warn!(%status, "revoked agent memory erase skipped: Control gave no decision");
            return;
        }
    };
    let session_token =
        match crate::domains::chat::shared::required_session_token(state, user, headers).await {
            Ok(token) => token,
            Err(_) => {
                tracing::warn!("revoked agent memory erase skipped: delegated credentials unavailable");
                return;
            }
        };
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
    let url = format!(
        "{}/v1/spaces/{}/agents/{}/memory/erase",
        state.model_gateway_url,
        urlencoding::encode(space_ref),
        urlencoding::encode(agent_subject_id),
    );
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_control_decision_ref": decision_ref,
            "space_control_decision_token": token,
        })),
        model_token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    if status.is_success() {
        // A count only, never content.
        let erased = response.get("erased").and_then(Value::as_u64).unwrap_or(0);
        tracing::info!(erased, "revoked agent memory erased");
    } else {
        tracing::warn!(%status, "revoked agent memory erase failed");
    }
}

/// Asks Control for the `model.space.memory.manage` decision of this call. A refusal (a member
/// below admin, a personal Space, an older Control without the endpoint) is 403
/// `space_memory_not_authorized`, typed when Control gave a reason; anything else is an unavailable
/// Control, never a grant.
async fn space_memory_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    operation: &str,
    memory_id: &str,
) -> Result<(String, String), Reply> {
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/memory-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "operation": operation,
            "memory_id": memory_id,
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    let status = denial::refusal_status(status, &response);
    if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND {
        let body = match denial::SpaceDenial::from_response(state, status, &response) {
            Some(denial) => error_with_details(
                "space_memory_not_authorized",
                DENIED,
                denial.details_value(),
            ),
            None => error("space_memory_not_authorized", DENIED),
        };
        return Err((StatusCode::FORBIDDEN, Json(body)));
    }
    if !status.is_success() {
        return Err(reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_memory_unavailable",
            "Space memory is temporarily unavailable.",
        ));
    }
    let data = unwrap_data(&response);
    let decision_ref = data
        .get("decision")
        .and_then(Value::as_object)
        .and_then(|decision| decision.get("decision_ref"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    if decision_ref.is_empty() || token.is_empty() {
        return Err(reply(
            StatusCode::BAD_GATEWAY,
            "invalid_space_decision",
            "Control returned an invalid Space memory decision.",
        ));
    }
    Ok((decision_ref, token))
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The browser's answer to a model-gateway answer: the success shape rebuilt field by field, the
/// closed refusal codes in fixed words, everything else `space_memory_unavailable`. Upstream text
/// and decision material never cross.
fn relay_answer(status: StatusCode, response: &Value, operation: &str, memory_id: &str) -> Reply {
    if status.is_success() {
        if operation == OPERATION_DELETE {
            return (
                StatusCode::OK,
                Json(json!({"data": {
                    "memory_id": memory_id,
                    "deleted": response.get("deleted").and_then(Value::as_bool).unwrap_or(false),
                }})),
            );
        }
        let memories: Vec<Value> = response
            .get("memories")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .map(|row| {
                        json!({
                            "memory_id": text(row, "memory_id"),
                            "content": text(row, "content"),
                            "updated_at": text(row, "updated_at"),
                            "provenance": text(row, "provenance"),
                            "source_label": text(row, "source_label"),
                            "thread_id": text(row, "thread_id"),
                            "withheld": !text(row, "withheld_reason").is_empty(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let withheld = memories
            .iter()
            .filter(|row| row["withheld"].as_bool().unwrap_or(false))
            .count();
        return (
            StatusCode::OK,
            Json(json!({"data": {"memories": memories, "withheld": withheld}})),
        );
    }
    let code = response
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    match code {
        "space_memory_not_authorized" => reply(StatusCode::FORBIDDEN, "space_memory_not_authorized", DENIED),
        "memory_not_found" => reply(
            StatusCode::NOT_FOUND,
            "memory_not_found",
            "The memory is not available.",
        ),
        _ => reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_memory_unavailable",
            "Space memory is temporarily unavailable.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Space is resolved server side: the model-gateway call is built from the path
    /// `space_ref` alone, and the decision rides in the body, never the URL.
    #[test]
    fn space_memory_route_resolves_space_server_side() {
        let (url, body) = model_request(
            "http://mg",
            "space a/b",
            OPERATION_LIST,
            "",
            "ref-1",
            "token-1",
        );
        assert_eq!(url, "http://mg/v1/spaces/space%20a%2Fb/memory/list");
        assert!(!url.contains("token-1") && !url.contains("ref-1"));
        assert_eq!(
            body,
            json!({"space_control_decision_ref": "ref-1", "space_control_decision_token": "token-1"})
        );
        let (url, body) = model_request("http://mg", "space-a", OPERATION_DELETE, "mem-1", "r", "t");
        assert_eq!(url, "http://mg/v1/spaces/space-a/memory/delete");
        assert_eq!(body["memory_id"], "mem-1");
        assert!(body.get("space_id").is_none(), "no client space id is ever forwarded");
    }

    #[test]
    fn space_memory_ids_are_validated() {
        assert!(valid_space_ref("space-1"));
        assert!(!valid_space_ref(""));
        assert!(!valid_space_ref("space 1"));
        assert!(valid_memory_id("01JABCDEF"));
        assert!(!valid_memory_id("../x"));
        assert!(!valid_memory_id(&"a".repeat(65)));
    }

    #[test]
    fn space_memory_answer_is_rebuilt_field_by_field() {
        let upstream = json!({
            "memories": [
                {"memory_id": "m1", "content": "Bergen on Fridays", "updated_at": "2026-11-03T10:00:00Z",
                 "provenance": "stated", "source_label": "room_statement", "thread_id": "t1",
                 "withheld_reason": "audience_changed", "secret": "never"},
                {"memory_id": "m2", "content": "Oslo", "updated_at": "", "provenance": "unknown",
                 "source_label": "member_save", "thread_id": "t2"}
            ],
            "withheld": 1,
            "degradation_reason": "internal detail"
        });
        let (status, Json(body)) = relay_answer(StatusCode::OK, &upstream, OPERATION_LIST, "");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["withheld"], 1);
        assert_eq!(body["data"]["memories"][0]["withheld"], true);
        assert_eq!(body["data"]["memories"][1]["withheld"], false);
        assert!(!body.to_string().contains("never") && !body.to_string().contains("internal detail"));

        let refused = json!({"error": {"code": "space_memory_not_authorized", "message": "upstream words"}});
        let (status, Json(body)) = relay_answer(StatusCode::FORBIDDEN, &refused, OPERATION_LIST, "");
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!body.to_string().contains("upstream words"));
        assert!(body["error"]["details"].is_object() || body["error"].get("details").is_none());
        let (status, _) = relay_answer(StatusCode::BAD_GATEWAY, &json!({}), OPERATION_DELETE, "m1");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }
}
