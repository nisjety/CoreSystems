//! Room pins: the BFF half of WS13-S8.
//!
//! `PUT` and `DELETE /api/v1/spaces/{space_ref}/pins/{thread_id}` pin and unpin a post for the
//! whole room, and `list_pins` is the `pins` section of the thread listing. Application (Convex
//! `spacePins`) stores identifiers and a timestamp only; this module decides nothing about
//! content and stores nothing itself. Session Core's owner-only `pinned_at` is a different,
//! personal fact and is untouched.
//!
//! Order of refusals for a pin (each one stops before the next runs): flag, organization, ids,
//! Space lifecycle, the caller's LIVE Control role (editor, manager or owner), the Control
//! shared-read decision, then proof that the thread exists in this Space and is readable by the
//! pinner (a one-turn read through model-gateway under the caller's own bearers), and only then
//! the Convex write. An unpin skips the read proof on purpose: a pin whose thread was archived or
//! erased must stay removable. The pin stores no title and no content, so a Zero Data Retention
//! organization has nothing to withhold; reading a pinned post still goes through Session Core
//! under each viewer's own decision.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_space` / `invalid_thread`, 403
//! `active_membership_required` / `room_role_forbidden` / `space_read_not_authorized`, 404
//! `feature_disabled` / `space_not_found` / `thread_not_found`, 409 `room_pins_limit`, 503
//! `space_lifecycle_unavailable` / `delegated_auth_unavailable` / `room_pins_unavailable`.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    routing::put,
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use super::{
    convex_gateway_call, denial, room_admin::convex_mutation_message,
    shared_thread_read_decision_for_typed, space_lifecycle_by_ref,
};
use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

type Reply = (StatusCode, Json<Value>);

/// Control roles that may pin or unpin. A viewer reads the pins, never changes them.
const PIN_ROLES: [&str; 3] = ["editor", "manager", "owner"];
const MAX_ID_LEN: usize = 200;

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/{space_ref}/pins/{thread_id}",
        put(pin_thread).delete(unpin_thread),
    )
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn feature_off() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "feature_disabled",
        "Room pins are not enabled.",
    )
}

fn unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "room_pins_unavailable",
        "The pin could not be changed. Nothing was changed.",
    )
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_LEN && !value.chars().any(char::is_control)
}

/// Organization, trimmed Space ref and thread id, with the Space lifecycle and the caller's live
/// role already checked. Everything both verbs share.
async fn authorize_change(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    thread_id: &str,
) -> Result<(String, String, String), Reply> {
    if !state.space_features.room_pins {
        return Err(feature_off());
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to pin a post.",
        ));
    }
    let space_ref = space_ref.trim();
    if !valid_id(space_ref) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        ));
    }
    let thread_id = thread_id.trim();
    if !valid_id(thread_id) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_thread",
            "A thread id is required.",
        ));
    }
    match space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return Err(refuse(
                StatusCode::NOT_FOUND,
                "space_not_found",
                "Space is not available.",
            ))
        }
        Err(()) => {
            return Err(refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            ))
        }
    }
    // The live role decides, from Control, before any Application request.
    match super::resolve_space_role(state, user, &org_id, space_ref).await {
        Ok(role) if PIN_ROLES.contains(&role.as_str()) => {}
        Ok(_) => {
            return Err(refuse(
                StatusCode::FORBIDDEN,
                "room_role_forbidden",
                "Only a room editor, manager or owner can pin a post.",
            ))
        }
        Err(refusal) => return Err(refusal),
    }
    Ok((org_id, space_ref.to_owned(), thread_id.to_owned()))
}

/// Proof that `thread_id` is a post in this Space the pinner may read: a one-turn read through
/// model-gateway under the pinner's own shared-read decision and bearers. A thread of another
/// Space, a missing one and one the pinner cannot read all answer 404 `thread_not_found`.
async fn prove_thread_in_space(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
    space_ref: &str,
    thread_id: &str,
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
    let session_token =
        match crate::domains::chat::shared::required_session_token(state, user, headers).await {
            Ok(token) => token,
            Err(reason) => return Err(crate::domains::chat::shared::delegated_auth_unavailable(reason)),
        };
    let url = format!(
        "{}/v1/threads/{}/messages?space_id={}&limit=1&space_read_decision_ref={}&space_read_decision_token={}",
        state.model_gateway_url,
        urlencoding::encode(thread_id),
        urlencoding::encode(space_ref),
        urlencoding::encode(&decision_ref),
        urlencoding::encode(&token),
    );
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
    let (status, Json(_)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::GET,
        &url,
        None,
        model_token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    match status {
        status if status.is_success() => Ok(()),
        StatusCode::NOT_FOUND | StatusCode::FORBIDDEN => Err(refuse(
            StatusCode::NOT_FOUND,
            "thread_not_found",
            "That post is not available in this room.",
        )),
        _ => Err(unavailable()),
    }
}

/// `PUT /api/v1/spaces/{space_ref}/pins/{thread_id}`. Idempotent: pinning a pinned post answers
/// 200 with the original pin time.
async fn pin_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, thread_id)): Path<(String, String)>,
) -> Reply {
    let (org_id, space_ref, thread_id) =
        match authorize_change(&state, &user, &space_ref, &thread_id).await {
            Ok(scope) => scope,
            Err(reply) => return reply,
        };
    if let Err(reply) =
        prove_thread_in_space(&state, &user, &headers, &org_id, &space_ref, &thread_id).await
    {
        return reply;
    }
    match convex_mutation_message(
        &state,
        "spacePins:setForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "threadId": thread_id,
        }),
    )
    .await
    {
        Ok(value) => (
            StatusCode::OK,
            Json(json!({"data": {
                "space_ref": space_ref,
                "thread_id": thread_id,
                "pinned": true,
                "pinned_at": value.get("pinnedAt").cloned().unwrap_or(Value::Null),
            }})),
        ),
        Err(Some(message)) if message.contains("room_pins_limit") => refuse(
            StatusCode::CONFLICT,
            "room_pins_limit",
            "This room has reached its pin limit. Unpin a post first.",
        ),
        Err(Some(message)) if message.contains("pin_invalid") => refuse(
            StatusCode::BAD_REQUEST,
            "invalid_thread",
            "The space or thread id is not valid.",
        ),
        Err(_) => unavailable(),
    }
}

/// `DELETE /api/v1/spaces/{space_ref}/pins/{thread_id}`. Idempotent: an absent pin answers 200.
async fn unpin_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, thread_id)): Path<(String, String)>,
) -> Reply {
    let (org_id, space_ref, thread_id) =
        match authorize_change(&state, &user, &space_ref, &thread_id).await {
            Ok(scope) => scope,
            Err(reply) => return reply,
        };
    match convex_mutation_message(
        &state,
        "spacePins:clearForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "threadId": thread_id,
        }),
    )
    .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"data": {
                "space_ref": space_ref,
                "thread_id": thread_id,
                "pinned": false,
            }})),
        ),
        Err(Some(message)) if message.contains("pin_invalid") => refuse(
            StatusCode::BAD_REQUEST,
            "invalid_thread",
            "The space or thread id is not valid.",
        ),
        Err(_) => unavailable(),
    }
}

/// The room's pins for the thread listing: `[{thread_id, pinned_by, pinned_at}]`, newest first.
/// `None` when the flag is off or Application could not be asked (the listing then names the gap
/// instead of failing), distinct from a room with no pins, which is `Some([])`.
pub(super) async fn list_pins(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Option<Value> {
    let result = convex_gateway_call(
        state,
        "query",
        "spacePins:listForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await
    .ok()?;
    let value = result.get("value").unwrap_or(&result);
    let rows = value.get("pins")?.as_array()?;
    let pins: Vec<Value> = rows
        .iter()
        .filter_map(|row| {
            Some(json!({
                "thread_id": row.get("threadId")?.as_str()?,
                "pinned_by": row.get("pinnedByExternalAuthId")?.as_str()?,
                "pinned_at": row.get("pinnedAt")?.as_u64()?,
            }))
        })
        .collect();
    Some(Value::Array(pins))
}
