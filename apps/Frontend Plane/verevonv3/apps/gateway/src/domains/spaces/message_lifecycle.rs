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
//!
//! # Reactions (WS12-S8)
//!
//! `PUT` and `DELETE /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}/reactions/{emoji}`
//! add and remove the caller's one emoji on one message. Application (Convex `spaceReactions`)
//! stores ids, the emoji and a time only; this module decides nothing about content. Order of
//! refusals for a `PUT`: flag (404 `space_reactions_disabled`), organization, request shape (400
//! `invalid_emoji`), Zero Data Retention (typed 412/503 before any Control, model or Convex call),
//! Space lifecycle, the caller's LIVE Control role (editor or above, else 403
//! `space_role_cannot_react`), the Control shared-read decision (403 `space_read_not_authorized`),
//! proof through model-gateway's target route that the message exists in that Space thread, is
//! under the reader's ceiling and is not deleted (404 `message_not_found`), and only then Convex
//! (409 `reaction_limit`, 503 `application_unavailable`). A `DELETE` needs only an active
//! membership and skips the message proof, so a reaction on a since-deleted message stays removable.
//! A reaction never starts a run and is never read by approval or wake code. The transcript route
//! calls `attach_reactions`; Convex failing there never fails the read.

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{patch, put},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::human_posts::{filter_model_error, space_context_from_decision, POST_MAX_CHARS};
use super::{
    convex_gateway_call, denial, room_admin::convex_mutation_message,
    shared_thread_read_decision_for_typed,
};
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
    Router::new()
        .route(
            "/api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}",
            patch(edit_route)
                .delete(delete_route)
                .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES)),
        )
        .route(
            "/api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}/reactions/{emoji}",
            put(add_reaction_route).delete(remove_reaction_route),
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
        let reply = map_model_response(status, response);
        // WS12-S8: a deleted message keeps no reactions. Best effort: the delete already landed
        // and a reaction on a tombstone is hidden from the transcript either way.
        if action == "delete" && reply.0 == StatusCode::OK && state.space_features.reactions_api {
            let _ = convex_mutation_message(
                state,
                "spaceReactions:clearForMessageForGateway",
                json!({
                    "externalAuthId": user.user_id,
                    "externalOrgId": org_id,
                    "spaceRef": space_ref,
                    "threadId": thread_id,
                    "messageId": message_id,
                }),
            )
            .await;
        }
        return reply;
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

// ---------------------------------------------------------------------------------------------
// Reactions (WS12-S8)
// ---------------------------------------------------------------------------------------------

/// Room roles that may react. A viewer reads reactions, never adds one.
const REACT_ROLES: [&str; 3] = ["editor", "manager", "owner"];
/// The same ceiling as Application's `MAX_EMOJI_BYTES`; Application does the full cluster check.
const MAX_EMOJI_BYTES: usize = 32;

fn reactions_disabled() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "space_reactions_disabled",
        "Reactions are not enabled.",
    )
}

fn application_unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "application_unavailable",
        "The reaction could not be changed. Nothing was changed.",
    )
}

fn invalid_emoji() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_emoji",
        "A single emoji is required.",
    )
}

async fn add_reaction_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, thread_id, message_id, emoji)): Path<(String, String, String, String)>,
) -> Reply {
    set_reaction(&state, &user, &headers, &space_ref, &thread_id, &message_id, &emoji, true).await
}

async fn remove_reaction_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, thread_id, message_id, emoji)): Path<(String, String, String, String)>,
) -> Reply {
    set_reaction(&state, &user, &headers, &space_ref, &thread_id, &message_id, &emoji, false).await
}

/// The whole reaction flow, shared by the two routes and the `spaces.toggle_reaction` action.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn set_reaction(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    thread_id: &str,
    message_id: &str,
    emoji: &str,
    present: bool,
) -> Reply {
    if !state.space_features.reactions_api {
        return reactions_disabled();
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to react.",
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
    if emoji.is_empty()
        || emoji.len() > MAX_EMOJI_BYTES
        || emoji.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return invalid_emoji();
    }
    // Adding persists a new row about the room: refused for a Zero Data Retention caller, typed,
    // before any Control, model or Application call. Removing erases a row and is not refused.
    if present {
        if let Some(refusal) = zdr::durable_write_refusal(state, user, Some(headers), None).await {
            return refusal;
        }
    }
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
    // The live role decides, from Control, before any Application request.
    let role = match super::resolve_space_role(state, user, &org_id, space_ref).await {
        Ok(role) => role,
        Err(refusal) => return refusal,
    };
    if present {
        if !REACT_ROLES.contains(&role.as_str()) {
            return refuse(
                StatusCode::FORBIDDEN,
                "space_role_cannot_react",
                "Only a room editor, manager or owner can react.",
            );
        }
        if let Err(reply) =
            prove_message_reactable(state, user, headers, &org_id, space_ref, thread_id, message_id)
                .await
        {
            return reply;
        }
    }
    match convex_mutation_message(
        state,
        "spaceReactions:setForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "threadId": thread_id,
            "messageId": message_id,
            "emoji": emoji,
            "present": present,
        }),
    )
    .await
    {
        Ok(value) => (
            StatusCode::OK,
            Json(json!({ "data": {
                "message_id": message_id,
                "reactions": reaction_chips(value.get("reactions")),
            }})),
        ),
        Err(Some(message)) if message.contains("reaction_limit") => refuse(
            StatusCode::CONFLICT,
            "reaction_limit",
            "This message already has the most different reactions it can hold.",
        ),
        Err(Some(message)) if message.contains("invalid_emoji") => invalid_emoji(),
        Err(Some(message)) if message.contains("reaction_invalid") => refuse(
            StatusCode::BAD_REQUEST,
            "invalid_reaction",
            "The reaction request is not valid.",
        ),
        Err(Some(message)) if message.contains("reactions_disabled") => reactions_disabled(),
        Err(_) => application_unavailable(),
    }
}

/// Proof that `message_id` is a live message of this Space thread the reactor may read: the
/// Control shared-read decision under the reactor's own delegation, then model-gateway's target
/// route (Session Core applies the audience ceiling and the tombstone). A message of another
/// Space, a missing one, one above the reader's ceiling and a deleted one all answer 404.
async fn prove_message_reactable(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
    space_ref: &str,
    thread_id: &str,
    message_id: &str,
) -> Result<(), Reply> {
    let (read, read_denial) =
        match shared_thread_read_decision_for_typed(state, user, org_id, space_ref, "").await {
            Ok(decision) => decision.split(),
            Err(response) => return Err(response),
        };
    let Some((decision_ref, token)) = read else {
        let message = "Reading this Space's posts is not authorized.";
        return Err(match read_denial {
            Some(denial) => (
                StatusCode::FORBIDDEN,
                Json(error_with_details(
                    denial::OP_READ,
                    message,
                    denial.details_value(),
                )),
            ),
            None => refuse(StatusCode::FORBIDDEN, "space_read_not_authorized", message),
        });
    };
    let session_bearer = match required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(cause) => return Err(delegated_auth_unavailable(cause)),
    };
    let model_bearer = model_token(state, user, headers).await;
    let url = format!(
        "{}/v1/threads/{}/messages/{}/target?space_id={}&space_read_decision_ref={}&space_read_decision_token={}",
        state.model_gateway_url,
        urlencoding::encode(thread_id),
        urlencoding::encode(message_id),
        urlencoding::encode(space_ref),
        urlencoding::encode(&decision_ref),
        urlencoding::encode(&token),
    );
    let (status, Json(response)) = proxy_model_json_with_session(
        state,
        Method::GET,
        &url,
        None,
        model_bearer.as_deref(),
        Some(&session_bearer),
        user,
    )
    .await;
    match status {
        status if status.is_success() => {
            let role = response.get("role").and_then(Value::as_str).unwrap_or_default();
            let deleted =
                response.get("revision_state").and_then(Value::as_str) == Some("deleted");
            if deleted || !matches!(role, "user" | "assistant") {
                return Err(message_not_found());
            }
            Ok(())
        }
        StatusCode::NOT_FOUND => Err(message_not_found()),
        StatusCode::FORBIDDEN => Err(refuse(
            StatusCode::FORBIDDEN,
            "space_read_not_authorized",
            "Reading this Space's posts is not authorized.",
        )),
        _ => Err(refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_unavailable",
            "The conversation service is unavailable.",
        )),
    }
}

/// Application's `[{emoji, count, reactedByMe}]` as the browser's `[{emoji, count, reacted}]`.
fn reaction_chips(raw: Option<&Value>) -> Vec<Value> {
    raw.and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some(json!({
                        "emoji": row.get("emoji")?.as_str()?,
                        "count": row.get("count")?.as_u64()?,
                        "reacted": row.get("reactedByMe").and_then(Value::as_bool).unwrap_or(false),
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Merge the thread's reactions into the transcript turns by `message_id` (WS12-S8): one
/// Application read per transcript, `reactions: [{emoji, count, reacted}]` on a turn that has any
/// and is not deleted. Returns `true` when Application could not be asked, so the transcript says
/// `reactions_unavailable` instead of failing; `false` when the flag is off or the merge worked.
pub(super) async fn attach_reactions(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    thread_id: &str,
    turns: &mut Value,
) -> bool {
    if !state.space_features.reactions_api {
        return false;
    }
    let Ok(value) = convex_gateway_call(
        state,
        "query",
        "spaceReactions:reactionsForThreadForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "threadId": thread_id,
        }),
    )
    .await
    else {
        return true;
    };
    let Some(rows) = value.get("reactions").and_then(Value::as_array) else {
        return true;
    };
    let Some(turns) = turns.as_array_mut() else {
        return false;
    };
    for turn in turns.iter_mut() {
        if turn.get("revision_state").and_then(Value::as_str) == Some("deleted") {
            continue;
        }
        let Some(message_id) = turn
            .get("message_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let mine: Vec<Value> = rows
            .iter()
            .filter(|row| row.get("messageId").and_then(Value::as_str) == Some(message_id.as_str()))
            .cloned()
            .collect();
        let chips = reaction_chips(Some(&Value::Array(mine)));
        if !chips.is_empty() {
            turn["reactions"] = Value::Array(chips);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    //! WS12-S8: the reaction routes' refusal order, error mapping and the transcript merge, driven
    //! through the real router against mock Auth, Control, org-core, Application (Convex) and
    //! Model Gateway. Message edit and delete share the module; the delete test here pins only that
    //! a landed delete clears the message's reactions.
    use super::attach_reactions;
    use crate::config::AppState;
    use crate::middleware::{AuthenticatedUser, AuthorizedMembership};
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{body_partial_json, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const THUMBS: &str = "%F0%9F%91%8D"; // percent-encoded U+1F44D
    const REACTION: &str = "/api/v1/spaces/space-room/threads/thread-1/messages/msg-1/reactions/%F0%9F%91%8D";
    const MESSAGE: &str = "/api/v1/spaces/space-room/threads/thread-1/messages/msg-1";

    struct Fx {
        state: AppState,
        user_core: MockServer,
        application: MockServer,
        model_gateway: MockServer,
        org_core: MockServer,
        _auth: MockServer,
    }

    struct Opts {
        role: &'static str,
        read_decision: u16,
        /// `(status, body)` of model-gateway's `/target` route.
        target: (u16, Value),
        mutation: ResponseTemplate,
        org_zdr: Option<bool>,
        reactions_on: bool,
        revise_on: bool,
    }

    impl Opts {
        fn new(role: &'static str) -> Self {
            Self {
                role,
                read_decision: 200,
                target: (200, json!({"role": "user", "revision_state": "live"})),
                mutation: ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": {
                    "messageId": "msg-1",
                    "reactions": [
                        {"emoji": "\u{1F44D}", "count": 2, "reactedByMe": true},
                        {"emoji": "bad", "count": "x"}
                    ]
                }})),
                org_zdr: Some(false),
                reactions_on: true,
                revise_on: false,
            }
        }
    }

    async fn fx(opts: Opts) -> Fx {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for slug in ["session-core", "model-gateway", "capability-core"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{slug}/token")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})))
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId":"user-1", "orgId":"org-1", "role":"member", "onboardingStatus":"COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": opts.role, "subject_id": "user-1"}
            })))
            .mount(&user_core)
            .await;
        let decision = if opts.read_decision == 200 {
            ResponseTemplate::new(200).set_body_json(json!({
                "data": {"decision": {"decision_ref": "decision-1"}, "token": "decision-token"}
            }))
        } else {
            ResponseTemplate::new(opts.read_decision).set_body_json(json!({"error": {"code": "forbidden"}}))
        };
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-read-decision"))
            .respond_with(decision)
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-revise-decision"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
                "decision": {
                    "space_ref": "space-room", "decision_ref": "decision-2",
                    "recipient_audience_ref": "aud-1", "recipient_audience_revision": 1,
                    "recipient_audience_hash": "hash-1", "privacy_policy_ref": "policy-1",
                    "authority_revision": 1, "resource_authorization_ref": "res-1",
                    "action_schema_hash": "schema-1", "payload_digest": "digest-1",
                    "idempotency_key": "msg-rev-key-0000000000000001"
                },
                "token": "revise-token"
            }})))
            .mount(&user_core)
            .await;

        let org_core = MockServer::start().await;
        match opts.org_zdr {
            Some(zdr) => {
                Mock::given(wm_method("GET"))
                    .and(wm_path("/api/v1/organizations/org-1"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                        "metadata": {"interactiveRetention": {"zdr": zdr}}
                    })))
                    .mount(&org_core)
                    .await;
            }
            None => {
                Mock::given(wm_method("GET"))
                    .and(wm_path("/api/v1/organizations/org-1"))
                    .respond_with(ResponseTemplate::new(500))
                    .mount(&org_core)
                    .await;
            }
        }

        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(body_partial_json(json!({"path": "spaces:spacesForOrgForGateway"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef":"space-room","name":"Room","kind":"room","lifecycle":"active"}
            ]})))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .respond_with(opts.mutation)
            .mount(&application)
            .await;

        let model_gateway = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/threads/thread-1/messages/msg-1/target"))
            .respond_with(ResponseTemplate::new(opts.target.0).set_body_json(opts.target.1))
            .mount(&model_gateway)
            .await;
        Mock::given(wm_method("DELETE"))
            .and(wm_path("/v1/threads/thread-1/messages/msg-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message_id": "msg-1", "revision_state": "deleted", "revision_no": 1,
                "revised_at": "2026-11-02T10:00:00Z", "deduplicated": false
            })))
            .mount(&model_gateway)
            .await;

        let mut state = crate::tests::test_state(false);
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model_gateway.uri();
        state.org_core_url = org_core.uri();
        state.space_features.reactions_api = opts.reactions_on;
        state.space_features.message_revise_api = opts.revise_on;
        Fx { state, user_core, application, model_gateway, org_core, _auth: auth }
    }

    async fn call(fx: &Fx, method: &str, uri: &str, extra_header: Option<(&str, &str)>) -> (u16, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        if let Some((name, value)) = extra_header {
            request = request.header(name, value);
        }
        let response = crate::build_router(fx.state.clone())
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, body)
    }

    fn code(body: &Value) -> &str {
        body["error"]["code"].as_str().unwrap_or_default()
    }

    async fn paths_seen(server: &MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| request.url.path().to_owned())
            .collect()
    }

    async fn mutations(fx: &Fx) -> Vec<Value> {
        fx.application
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() == "/api/mutation")
            .map(|request| serde_json::from_slice(&request.body).unwrap_or(Value::Null))
            .collect()
    }

    /// Nothing but the session middleware's own calls reached Control, Application and
    /// Model Gateway.
    async fn assert_no_domain_calls(fx: &Fx) {
        let control = paths_seen(&fx.user_core).await;
        assert!(
            control.iter().all(|path| path == "/api/v1/me/session-context"),
            "Control was called: {control:?}"
        );
        assert!(paths_seen(&fx.application).await.is_empty(), "Application was called");
        assert!(paths_seen(&fx.model_gateway).await.is_empty(), "Model Gateway was called");
    }

    // ----- the happy path ---------------------------------------------------------------------

    #[tokio::test]
    async fn an_editor_adds_a_reaction_and_gets_the_chips() {
        let fx = fx(Opts::new("editor")).await;
        let (status, body) = call(&fx, "PUT", REACTION, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["message_id"], "msg-1");
        // Application's reactedByMe becomes reacted; a malformed row is dropped.
        assert_eq!(
            body["data"]["reactions"],
            json!([{"emoji": "\u{1F44D}", "count": 2, "reacted": true}])
        );
        let sent = mutations(&fx).await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["path"], "spaceReactions:setForGateway");
        assert_eq!(sent[0]["args"]["present"], true);
        assert_eq!(sent[0]["args"]["emoji"], "\u{1F44D}");
        assert_eq!(sent[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(sent[0]["args"]["externalOrgId"], "org-1");
        assert_eq!(sent[0]["args"]["spaceRef"], "space-room");
        assert_eq!(sent[0]["args"]["threadId"], "thread-1");
        assert_eq!(sent[0]["args"]["messageId"], "msg-1");
        assert!(paths_seen(&fx.model_gateway).await.iter().any(|p| p.ends_with("/msg-1/target")));
    }

    #[tokio::test]
    async fn managers_and_owners_may_react_too() {
        for role in ["manager", "owner"] {
            let fx = fx(Opts::new(role)).await;
            let (status, body) = call(&fx, "PUT", REACTION, None).await;
            assert_eq!(status, 200, "{role}: {body}");
        }
    }

    // ----- who may react ----------------------------------------------------------------------

    #[tokio::test]
    async fn a_viewer_cannot_react_and_nothing_reaches_application() {
        let fx = fx(Opts::new("viewer")).await;
        let (status, body) = call(&fx, "PUT", REACTION, None).await;
        assert_eq!((status, code(&body)), (403, "space_role_cannot_react"));
        assert!(mutations(&fx).await.is_empty());
        assert!(paths_seen(&fx.model_gateway).await.is_empty(), "no message proof for a refused role");
        let control = paths_seen(&fx.user_core).await;
        assert!(!control.iter().any(|path| path.ends_with("thread-read-decision")), "{control:?}");
    }

    #[tokio::test]
    async fn a_declined_shared_read_decision_is_403_and_nothing_is_written() {
        let mut opts = Opts::new("editor");
        opts.read_decision = 403;
        let fx = fx(opts).await;
        let (status, body) = call(&fx, "PUT", REACTION, None).await;
        assert_eq!(status, 403, "{body}");
        assert!(mutations(&fx).await.is_empty());
        assert!(paths_seen(&fx.model_gateway).await.is_empty());
    }

    // ----- the message must be a live, readable message of that thread ------------------------

    #[tokio::test]
    async fn a_missing_deleted_or_system_message_is_404_message_not_found() {
        let targets = [
            (404, json!({"error": {"code": "not_found"}})),
            (200, json!({"role": "user", "revision_state": "deleted"})),
            (200, json!({"role": "system", "revision_state": "live"})),
            (200, json!({"role": "tool", "revision_state": "live"})),
            (200, json!({})),
        ];
        for target in targets {
            let mut opts = Opts::new("editor");
            opts.target = target.clone();
            let fx = fx(opts).await;
            let (status, body) = call(&fx, "PUT", REACTION, None).await;
            assert_eq!((status, code(&body)), (404, "message_not_found"), "{target:?}");
            assert!(mutations(&fx).await.is_empty(), "{target:?}");
        }
    }

    #[tokio::test]
    async fn an_assistant_message_can_be_reacted_to() {
        let mut opts = Opts::new("editor");
        opts.target = (200, json!({"role": "assistant", "revision_state": "live"}));
        let fx = fx(opts).await;
        assert_eq!(call(&fx, "PUT", REACTION, None).await.0, 200);
    }

    #[tokio::test]
    async fn a_forbidden_or_failing_target_route_is_403_or_503() {
        let mut opts = Opts::new("editor");
        opts.target = (403, json!({"error": {"code": "forbidden"}}));
        let forbidden = fx(opts).await;
        let (status, body) = call(&forbidden, "PUT", REACTION, None).await;
        assert_eq!((status, code(&body)), (403, "space_read_not_authorized"));

        let mut opts = Opts::new("editor");
        opts.target = (500, json!({}));
        let broken = fx(opts).await;
        let (status, body) = call(&broken, "PUT", REACTION, None).await;
        assert_eq!((status, code(&body)), (503, "session_unavailable"));
        assert!(mutations(&broken).await.is_empty());
    }

    // ----- Zero Data Retention ----------------------------------------------------------------

    #[tokio::test]
    async fn a_zdr_org_gets_the_typed_412_for_adding_and_no_upstream_call_is_made() {
        let mut opts = Opts::new("editor");
        opts.org_zdr = Some(true);
        let fx = fx(opts).await;
        let (status, body) = call(&fx, "PUT", REACTION, None).await;
        assert_eq!((status, code(&body)), (412, "zdr_durable_mutation_forbidden"));
        assert_no_domain_calls(&fx).await;
    }

    #[tokio::test]
    async fn a_zdr_header_alone_refuses_adding_before_any_lookup() {
        let fx = fx(Opts::new("editor")).await;
        let (status, body) = call(&fx, "PUT", REACTION, Some(("x-zdr", "true"))).await;
        assert_eq!((status, code(&body)), (412, "zdr_durable_mutation_forbidden"));
        assert_no_domain_calls(&fx).await;
    }

    #[tokio::test]
    async fn an_unknown_zdr_posture_is_503_zdr_decision_unavailable_for_adding() {
        let mut opts = Opts::new("editor");
        opts.org_zdr = None;
        let fx = fx(opts).await;
        let (status, body) = call(&fx, "PUT", REACTION, None).await;
        assert_eq!((status, code(&body)), (503, "zdr_decision_unavailable"));
        assert_no_domain_calls(&fx).await;
    }

    #[tokio::test]
    async fn a_zdr_org_may_still_remove_a_reaction() {
        let mut opts = Opts::new("viewer");
        opts.org_zdr = Some(true);
        opts.mutation = ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": {
            "messageId": "msg-1", "reactions": []
        }}));
        let fx = fx(opts).await;
        let (status, body) = call(&fx, "DELETE", REACTION, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["reactions"], json!([]));
        let sent = mutations(&fx).await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["args"]["present"], false);
        // Erasing is not a durable write: the org posture was never even asked.
        assert!(paths_seen(&fx.org_core).await.is_empty());
    }

    // ----- flags ------------------------------------------------------------------------------

    #[tokio::test]
    async fn with_the_flag_off_both_routes_are_404_and_nothing_is_called() {
        let mut opts = Opts::new("editor");
        opts.reactions_on = false;
        let fx = fx(opts).await;
        for method in ["PUT", "DELETE"] {
            let (status, body) = call(&fx, method, REACTION, None).await;
            assert_eq!((status, code(&body)), (404, "space_reactions_disabled"), "{method}");
        }
        assert_no_domain_calls(&fx).await;
        assert!(paths_seen(&fx.org_core).await.is_empty());
    }

    // ----- removing ---------------------------------------------------------------------------

    #[tokio::test]
    async fn removing_needs_no_message_proof_and_works_on_a_since_deleted_message() {
        let mut opts = Opts::new("viewer");
        opts.target = (200, json!({"role": "user", "revision_state": "deleted"}));
        opts.mutation = ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": {
            "messageId": "msg-1", "reactions": [{"emoji": "\u{1F389}", "count": 1, "reactedByMe": false}]
        }}));
        let fx = fx(opts).await;
        let (status, body) = call(&fx, "DELETE", REACTION, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["data"]["reactions"],
            json!([{"emoji": "\u{1F389}", "count": 1, "reacted": false}])
        );
        assert!(paths_seen(&fx.model_gateway).await.is_empty(), "the message proof is skipped");
        let control = paths_seen(&fx.user_core).await;
        assert!(!control.iter().any(|path| path.ends_with("thread-read-decision")), "{control:?}");
        assert_eq!(mutations(&fx).await[0]["args"]["present"], false);
    }

    // ----- Application's errors ---------------------------------------------------------------

    fn convex_error(message: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": message}))
    }

    #[tokio::test]
    async fn application_errors_map_to_typed_answers() {
        let cases: [(ResponseTemplate, u16, &str); 6] = [
            (convex_error("Uncaught Error: reaction_limit"), 409, "reaction_limit"),
            (convex_error("Uncaught Error: invalid_emoji"), 400, "invalid_emoji"),
            (convex_error("Uncaught Error: reaction_invalid:thread_id"), 400, "invalid_reaction"),
            (convex_error("Uncaught Error: reactions_disabled"), 404, "space_reactions_disabled"),
            (convex_error("Uncaught Error: Unauthorized"), 503, "application_unavailable"),
            (ResponseTemplate::new(500).set_body_string("boom"), 503, "application_unavailable"),
        ];
        for (template, want_status, want_code) in cases {
            let mut opts = Opts::new("editor");
            opts.mutation = template;
            let fx = fx(opts).await;
            let (status, body) = call(&fx, "PUT", REACTION, None).await;
            assert_eq!((status, code(&body)), (want_status, want_code));
        }
    }

    #[tokio::test]
    async fn application_not_configured_stops_at_the_lifecycle_check_and_names_no_internals() {
        let mut fx = fx(Opts::new("editor")).await;
        fx.state.application_convex_url = String::new();
        let (status, body) = call(&fx, "PUT", REACTION, None).await;
        // The Space lifecycle is read from Application before anything is written.
        assert_eq!((status, code(&body)), (503, "space_lifecycle_unavailable"));
        assert!(!body.to_string().contains("application-test-key"));
        assert!(mutations(&fx).await.is_empty());
    }

    // ----- request shape ----------------------------------------------------------------------

    #[tokio::test]
    async fn a_bad_emoji_is_400_before_any_upstream_call() {
        for emoji in ["a%20b", "%09", "%0A", &"%F0%9F%91%8D".repeat(9)] {
            let fx = fx(Opts::new("editor")).await;
            let uri = format!("/api/v1/spaces/space-room/threads/thread-1/messages/msg-1/reactions/{emoji}");
            let (status, body) = call(&fx, "PUT", &uri, None).await;
            assert_eq!((status, code(&body)), (400, "invalid_emoji"), "{emoji}");
            assert_no_domain_calls(&fx).await;
        }
        assert_eq!(THUMBS.len(), 12);
    }

    // ----- the transcript merge ---------------------------------------------------------------

    fn turns() -> Value {
        json!([
            {"message_id": "m1", "role": "user", "text": "hello"},
            {"message_id": "m2", "role": "user", "revision_state": "deleted"},
            {"message_id": "m3", "role": "assistant", "text": "hi"},
            {"role": "user", "text": "no id"}
        ])
    }

    async fn reaction_read(template: ResponseTemplate, on: bool) -> (AppState, MockServer) {
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(body_partial_json(json!({"path": "spaceReactions:reactionsForThreadForGateway"})))
            .respond_with(template)
            .mount(&application)
            .await;
        let mut state = crate::tests::test_state(false);
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.reactions_api = on;
        (state, application)
    }

    fn member() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User".to_owned(),
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

    #[tokio::test]
    async fn attach_reactions_merges_chips_by_message_id_and_skips_deleted_turns() {
        let (state, _application) = reaction_read(
            ResponseTemplate::new(200).set_body_json(json!({"value": {"enabled": true, "truncated": false, "reactions": [
                {"messageId": "m1", "emoji": "\u{1F44D}", "count": 2, "reactedByMe": true},
                {"messageId": "m1", "emoji": "\u{1F389}", "count": 1, "reactedByMe": false},
                {"messageId": "m2", "emoji": "\u{1F44D}", "count": 1, "reactedByMe": false},
                {"messageId": "other", "emoji": "\u{1F44D}", "count": 1, "reactedByMe": false}
            ]}})),
            true,
        )
        .await;
        let mut value = turns();
        let unavailable = attach_reactions(&state, &member(), "org-1", "space-room", "thread-1", &mut value).await;
        assert!(!unavailable);
        assert_eq!(
            value[0]["reactions"],
            json!([
                {"emoji": "\u{1F44D}", "count": 2, "reacted": true},
                {"emoji": "\u{1F389}", "count": 1, "reacted": false}
            ])
        );
        assert!(value[1].get("reactions").is_none(), "a deleted turn carries no reactions");
        assert!(value[2].get("reactions").is_none(), "a turn nobody reacted to is left alone");
        assert!(value[3].get("reactions").is_none(), "a turn without an id is left alone");
        assert_eq!(value[0]["text"], "hello", "the turn itself is intact");
    }

    #[tokio::test]
    async fn the_transcript_survives_a_reactions_outage() {
        for template in [
            ResponseTemplate::new(500),
            ResponseTemplate::new(200).set_body_json(json!({"value": {"enabled": true}})),
        ] {
            let (state, _application) = reaction_read(template, true).await;
            let mut value = turns();
            let before = value.clone();
            let unavailable =
                attach_reactions(&state, &member(), "org-1", "space-room", "thread-1", &mut value).await;
            assert!(unavailable, "the caller reports reactions_unavailable");
            assert_eq!(value, before, "the turns are exactly as Session Core sent them");
        }
    }

    #[tokio::test]
    async fn attach_reactions_is_silent_when_the_flag_is_off_or_application_has_them_off() {
        let (state, application) = reaction_read(ResponseTemplate::new(500), false).await;
        let mut value = turns();
        let before = value.clone();
        assert!(!attach_reactions(&state, &member(), "org-1", "space-room", "thread-1", &mut value).await);
        assert_eq!(value, before);
        assert!(paths_seen(&application).await.is_empty(), "flag off: Application is not asked");

        let (state, _application) = reaction_read(
            ResponseTemplate::new(200).set_body_json(json!({"value": {"enabled": false, "truncated": false, "reactions": []}})),
            true,
        )
        .await;
        let mut value = turns();
        assert!(!attach_reactions(&state, &member(), "org-1", "space-room", "thread-1", &mut value).await);
        assert_eq!(value, before);
    }

    // ----- delete clears reactions ------------------------------------------------------------

    #[tokio::test]
    async fn a_landed_delete_clears_the_messages_reactions_best_effort() {
        let mut opts = Opts::new("editor");
        opts.revise_on = true;
        opts.mutation = ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": {"cleared": 2, "more": false}}));
        let fx = fx(opts).await;
        let (status, body) = call(&fx, "DELETE", MESSAGE, Some(("idempotency-key", "key-0000000000000001"))).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["revision_state"], "deleted");
        let sent = mutations(&fx).await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["path"], "spaceReactions:clearForMessageForGateway");
        assert_eq!(sent[0]["args"]["messageId"], "msg-1");
        assert_eq!(sent[0]["args"]["threadId"], "thread-1");
        assert_eq!(sent[0]["args"]["spaceRef"], "space-room");
    }

    #[tokio::test]
    async fn a_delete_still_succeeds_when_clearing_the_reactions_fails() {
        let mut opts = Opts::new("editor");
        opts.revise_on = true;
        opts.mutation = ResponseTemplate::new(500);
        let fx = fx(opts).await;
        let (status, body) = call(&fx, "DELETE", MESSAGE, Some(("idempotency-key", "key-0000000000000001"))).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["revision_state"], "deleted");
    }

    #[tokio::test]
    async fn a_delete_with_reactions_off_does_not_touch_application() {
        let mut opts = Opts::new("editor");
        opts.revise_on = true;
        opts.reactions_on = false;
        let fx = fx(opts).await;
        let (status, _) = call(&fx, "DELETE", MESSAGE, Some(("idempotency-key", "key-0000000000000001"))).await;
        assert_eq!(status, 200);
        assert!(mutations(&fx).await.is_empty());
    }
}
