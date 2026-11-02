//! Edit and delete one message of a Space thread: the BFF half of WS12-S7.
//!
//! - `PATCH  /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}` body
//!   `{content, idempotency_key}` edits one message.
//! - `DELETE` on the same path, header `Idempotency-Key`, optional `?moderation_reason=` (closed
//!   set), deletes one message (a tombstone for every reader).
//!
//! Both answer `{data:{message_id, revision_state, revision_no, revised_at, deduplicated}}`, 200.
//!
//! The gateway decides nothing here and stores nothing. Control mints the signed
//! `model.thread.revise` decision under the caller's own delegation, bound to the digest of the new
//! text (the digest of the empty string for a delete); model-gateway relays the call to Session
//! Core, which verifies the decision, works out whether the caller is the author, the agent owner
//! or a moderator, and writes the revision. A forged `role`, `actor_class`, `original_retention`
//! or decision field in the body is a 400 and is never forwarded.
//!
//! Order (each step is a refusal before the next one runs): flag (404
//! `space_message_revise_disabled`), verified organization, request shape, Zero Data Retention
//! (typed 412/503 before any Control or model call: a revision persists free text), Space
//! lifecycle, bearers, Control decision, model-gateway. When the audience moved between the
//! decision and the write, Session Core answers 409 `space_authority_stale`: a fresh decision is
//! minted once and the call is resent.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_message` / `invalid_idempotency_key` /
//! `revision_noop`, 403 `active_membership_required` / `space_message_revise_not_authorized`
//! (Control) / `message_revise_forbidden` (Session Core), 404 `space_message_revise_disabled` /
//! `space_not_found` / `message_not_found` / `thread_not_found`, 409 `message_already_deleted` /
//! `thread_run_active` / `idempotency_key_conflict` / `space_authority_stale` / `room_archived` /
//! `revision_limit` / `message_state_conflict`, 412 the typed ZDR refusal, 503 `zdr_decision_unavailable` /
//! `space_lifecycle_unavailable` / `space_decision_unavailable` / `delegated_auth_unavailable` /
//! `session_unavailable`.
//!
//! The same flow backs the `spaces.edit_message` and `spaces.delete_message` actions, so the flag,
//! the ZDR refusal and the typed errors exist once.

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::patch,
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::human_posts::{filter_model_error, space_context_from_decision, POST_MAX_CHARS};
use crate::{
    config::AppState,
    contracts::ActionActor,
    domains::chat::shared::{
        delegated_auth_unavailable, model_token, proxy_model_json_with_session,
        required_session_token,
    },
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json},
    zdr,
};

type Reply = (StatusCode, Json<Value>);

const MAX_ID_BYTES: usize = 128;
const MAX_SPACE_REF_BYTES: usize = 200;
/// Session Core's edit limit (`mp_contracts::message_revision::MAX_EDIT_BYTES`), reported in
/// `details` so the SPA can word the refusal.
const MAX_EDIT_BYTES: usize = 32_768;
const BODY_LIMIT_BYTES: usize = 64 * 1024;
/// Control's idempotency key for one revision: one browser key names the whole attempt.
const KEY_PREFIX: &str = "msg-rev-";
/// The closed set model-gateway and Session Core accept for a moderator delete.
const MODERATION_REASONS: [&str; 5] = ["policy_violation", "spam", "harassment", "privacy", "other"];

/// Registered through `module_routes()` behind `require_session`. The method router shares no
/// path with the post routes (`.../messages` has no id segment).
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}",
        patch(edit_route)
            .delete(delete_route)
            .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES)),
    )
}

/// What the caller asks for.
pub(crate) enum Revision {
    Edit { content: String },
    Delete { moderation_reason: String },
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn disabled() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "space_message_revise_disabled",
        "Editing and deleting messages is not enabled.",
    )
}

fn invalid_message(field: &str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "invalid_message",
            "The message change is not valid.",
            json!({ "field": field, "max_chars": POST_MAX_CHARS, "max_bytes": MAX_EDIT_BYTES }),
        )),
    )
}

fn invalid_key() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_idempotency_key",
        "idempotency_key must be 16 to 64 letters, digits, '_' or '-'.",
    )
}

fn message_not_found() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "message_not_found",
        "The message was not found.",
    )
}

/// `^[A-Za-z0-9_-]{16,64}$`, the same shape as a post's send id.
fn valid_key(key: &str) -> bool {
    (16..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditBody {
    content: String,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
struct DeleteQuery {
    #[serde(default)]
    moderation_reason: Option<String>,
}

async fn edit_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, thread_id, message_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Reply {
    let body = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    let parsed: EditBody = match serde_json::from_value(body.clone()) {
        Ok(parsed) => parsed,
        Err(_) => return invalid_message("body"),
    };
    revise_message(
        &state,
        &user,
        &headers,
        &space_ref,
        &thread_id,
        &message_id,
        &parsed.idempotency_key,
        Revision::Edit {
            content: parsed.content,
        },
        Some(&body),
    )
    .await
}

async fn delete_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, thread_id, message_id)): Path<(String, String, String)>,
    query: Result<Query<DeleteQuery>, axum::extract::rejection::QueryRejection>,
) -> Reply {
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return invalid_message("query"),
    };
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    revise_message(
        &state,
        &user,
        &headers,
        &space_ref,
        &thread_id,
        &message_id,
        &key,
        Revision::Delete {
            moderation_reason: query.moderation_reason.unwrap_or_default(),
        },
        None,
    )
    .await
}

/// The whole revise flow, shared by the two routes and the two actions. `body` is the untrusted
/// request object (for the request-level ZDR flag); `None` for a delete.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn revise_message(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    thread_id: &str,
    message_id: &str,
    idempotency_key: &str,
    revision: Revision,
    body: Option<&Value>,
) -> Reply {
    if !state.space_features.message_revise_api {
        return disabled();
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to change a message.",
        );
    }
    let space_ref = space_ref.trim();
    if space_ref.is_empty() || space_ref.len() > MAX_SPACE_REF_BYTES {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        );
    }
    let (thread_id, message_id) = (thread_id.trim(), message_id.trim());
    if thread_id.is_empty()
        || thread_id.len() > MAX_ID_BYTES
        || message_id.is_empty()
        || message_id.len() > MAX_ID_BYTES
    {
        return message_not_found();
    }
    if !valid_key(idempotency_key) {
        return invalid_key();
    }
    // A durable write of free text: refused for a Zero Data Retention caller, typed, before any
    // Control or model call.
    if let Some(refusal) = zdr::durable_write_refusal(state, user, Some(headers), body).await {
        return refusal;
    }
    // The content is trimmed once: Control commits to the digest of exactly these bytes and the
    // same bytes are what model-gateway forwards to Session Core.
    let (action, content, moderation_reason) = match &revision {
        Revision::Edit { content } => {
            let content = content.trim();
            if content.is_empty()
                || content.contains('\0')
                || content.chars().count() > POST_MAX_CHARS
                || content.len() > MAX_EDIT_BYTES
            {
                return invalid_message("content");
            }
            ("edit", content.to_owned(), String::new())
        }
        Revision::Delete { moderation_reason } => {
            let reason = moderation_reason.trim();
            if !reason.is_empty() && !MODERATION_REASONS.contains(&reason) {
                return invalid_message("moderation_reason");
            }
            ("delete", String::new(), reason.to_owned())
        }
    };
    match super::space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return refuse(
                StatusCode::NOT_FOUND,
                "space_not_found",
                "Space is not available.",
            )
        }
        Err(()) => {
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            )
        }
    }
    let model_bearer = model_token(state, user, headers).await;
    let session_bearer = match required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(cause) => return delegated_auth_unavailable(cause),
    };
    // The digest of the new text; for a delete, the digest of the empty string.
    let content_digest = format!("sha256:{}", hex::encode(Sha256::digest(content.as_bytes())));
    let control_key = format!("{KEY_PREFIX}{idempotency_key}");
    let url = format!(
        "{}/v1/threads/{}/messages/{}",
        state.model_gateway_url,
        urlencoding::encode(thread_id),
        urlencoding::encode(message_id)
    );
    let method = if action == "edit" {
        Method::PATCH
    } else {
        Method::DELETE
    };
    for attempt in 0..2 {
        let context = match mint_revise_decision(
            state,
            user,
            &org_id,
            space_ref,
            thread_id,
            message_id,
            action,
            &content_digest,
            &control_key,
        )
        .await
        {
            Ok(context) => context,
            Err(reply) => return reply,
        };
        let request = if action == "edit" {
            json!({ "content": content, "space_revise_context": context })
        } else if moderation_reason.is_empty() {
            json!({ "space_revise_context": context })
        } else {
            json!({ "space_revise_context": context, "moderation_reason": moderation_reason })
        };
        let (status, Json(response)) = proxy_model_json_with_session(
            state,
            method.clone(),
            &url,
            Some(request),
            model_bearer.as_deref(),
            Some(&session_bearer),
            user,
        )
        .await;
        let stale = status == StatusCode::CONFLICT
            && response.pointer("/error/code").and_then(Value::as_str)
                == Some("space_authority_stale");
        if stale && attempt == 0 {
            continue;
        }
        return map_model_response(status, response);
    }
    refuse(
        StatusCode::CONFLICT,
        "space_authority_stale",
        "The Space authority changed; try again.",
    )
}

/// Calls Control's `thread-revise-decision` under the caller's delegation and returns the
/// 12-field context model-gateway forwards. Nothing about the decision is decided here.
#[allow(clippy::too_many_arguments)]
async fn mint_revise_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    thread_id: &str,
    message_id: &str,
    action: &str,
    content_digest: &str,
    idempotency_key: &str,
) -> Result<Value, Reply> {
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/thread-revise-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "thread_id": thread_id,
            "message_id": message_id,
            "action": action,
            "content_digest": content_digest,
            "idempotency_key": idempotency_key,
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err(control_refusal(status, &response));
    }
    let data = unwrap_data(&response);
    let decision = data
        .get("decision")
        .and_then(Value::as_object)
        .ok_or_else(super::invalid_decision)?;
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if token.trim().is_empty() {
        return Err(super::invalid_decision());
    }
    space_context_from_decision(decision, token)
}

/// Control's refusal as the revise envelope. Control gives no reason a browser may rely on, so a
/// 403 is always `space_message_revise_not_authorized`.
fn control_refusal(status: StatusCode, body: &Value) -> Reply {
    match status {
        StatusCode::FORBIDDEN => refuse(
            StatusCode::FORBIDDEN,
            "space_message_revise_not_authorized",
            "Changing this message is not permitted.",
        ),
        StatusCode::NOT_FOUND => refuse(
            StatusCode::NOT_FOUND,
            "space_not_found",
            "Space is not available.",
        ),
        StatusCode::BAD_REQUEST => invalid_message("request"),
        StatusCode::CONFLICT
            if body.pointer("/error/code").and_then(Value::as_str) == Some("room_archived") =>
        {
            refuse(
                StatusCode::CONFLICT,
                "room_archived",
                "The room is archived; messages can no longer be changed.",
            )
        }
        StatusCode::CONFLICT => refuse(
            StatusCode::CONFLICT,
            "space_authority_stale",
            "The Space authority changed; try again.",
        ),
        _ => refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_decision_unavailable",
            "The Space decision service is unavailable.",
        ),
    }
}

/// model-gateway's answer as the browser envelope. Its own errors keep their status and code and
/// are bounded by `filter_model_error`; anything else is `session_unavailable`.
fn map_model_response(status: StatusCode, response: Value) -> Reply {
    if !status.is_success() {
        // A 401 here is the delegated bearer being refused, not the browser session; relayed, the
        // SPA would sign the user out.
        if status != StatusCode::UNAUTHORIZED {
            if let Some(body) = filter_model_error(&response) {
                return (status, Json(body));
            }
        }
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_unavailable",
            "The conversation service is unavailable.",
        );
    }
    let text = |field: &str| {
        response
            .get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let (Some(message_id), Some(revision_state)) = (text("message_id"), text("revision_state"))
    else {
        return refuse(
            StatusCode::BAD_GATEWAY,
            "session_unavailable",
            "The conversation service returned an unreadable answer.",
        );
    };
    (
        StatusCode::OK,
        Json(json!({ "data": {
            "message_id": message_id,
            "revision_state": revision_state,
            "revision_no": response.get("revision_no").and_then(Value::as_u64).unwrap_or(0),
            "revised_at": response.get("revised_at").cloned().unwrap_or(Value::Null),
            "deduplicated": response.get("deduplicated").and_then(Value::as_bool).unwrap_or(false),
        }})),
    )
}

/// What the room's UI may show about message lifecycle for this member: whether the organization
/// has edit/delete and moderation switched on and which rule applies to originals. A hint only,
/// never an authorization: every revision still needs a Control decision. `None` when the flag is
/// off or Control did not answer (the SPA then shows no moderator controls).
pub(super) async fn message_policy_hints(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Option<Value> {
    if !state.space_features.message_revise_api {
        return None;
    }
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/{}/message-policy",
        state.user_core_url,
        urlencoding::encode(space_ref)
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return None;
    }
    let data = unwrap_data(&response);
    let flag = |name: &str| data.get(name).and_then(Value::as_bool).unwrap_or(false);
    let retention = match data.get("original_retention").and_then(Value::as_str) {
        Some("retain") => "retain",
        _ => "purge",
    };
    Some(json!({
        "revise_entitled": flag("revise_entitled"),
        "moderate_entitled": flag("moderate_entitled"),
        "original_retention": retention,
    }))
}
