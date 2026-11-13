//! Governed conversation snapshots (W4-SH-6, F-t3-046): the owner takes an immutable, redacted copy of
//! one of their own threads, shares it with named members of their organisation, and a granted member
//! reads it. There is no public link: a snapshot is read by its owner or by a member Control holds a
//! `view` grant for, and by nobody else.
//!
//! - `POST /api/v1/chat/threads/{thread_id}/snapshots` takes a snapshot (201).
//! - `GET /api/v1/chat/threads/{thread_id}/snapshots` lists the caller's live snapshots of the thread.
//! - `DELETE /api/v1/chat/snapshots/{snapshot_id}` revokes one; its body and its member grants go.
//! - `POST /api/v1/chat/snapshots/{snapshot_id}/grants`, body `{"grantee_user_id"}` and nothing else, shares
//!   a live snapshot with a member (201).
//! - `DELETE /api/v1/chat/snapshots/{snapshot_id}/grants`, the same body, stops sharing it with that member.
//! - `GET /api/v1/chat/snapshots/{snapshot_id}/grants` lists the members it is shared with.
//! - `GET /api/v1/chat/shared-snapshots/{snapshot_id}` reads a snapshot the caller owns, or one a member was
//!   granted a view of: the redacted copy as static text.
//!
//! Each route relays Model Gateway's `/v1/threads/:id/snapshots`, `/v1/snapshots/:id[/grants]` and
//! `/v1/shared-snapshots/:id`. This hop decides nothing about who may share or read. Each step is a refusal
//! before the next one runs:
//!
//! 1. the rollout switch `CHAT_SNAPSHOTS_ENABLED`, read once in `build_state` (404 `chat_snapshots_disabled`
//!    while it is off; never an authorization input),
//! 2. live organization membership (403 `org_scope_required`), then the request shape: a thread id and a
//!    snapshot id are 1 to 200 characters of `[A-Za-z0-9_.:-]`, and a grant body names `grantee_user_id` and no
//!    other field (400 `invalid_request`),
//! 3. the caller's own `aud=session-core` bearer, minted per request (503 `delegated_auth_unavailable` when it
//!    cannot be), and the model-gateway bearer,
//! 4. the relay. The grant routes, the revoke and the shared read also carry the caller's own
//!    `aud=data-plane` bearer, which Session Core uses to act for the owner in Control; the gateway holds no
//!    grant authority and writes no grant.
//!
//! The org and the user are this server's own (set when the call is proxied), never the browser's: no header,
//! owner, organisation or grantee org from the browser is copied, and a body field beyond `grantee_user_id` is
//! refused rather than dropped. Model Gateway and Session Core refuse a zero-retention or waived caller, a Space
//! thread and a thread with a run kept outside retention, and that refusal reaches the browser as written. A
//! snapshot holds the owner's conversation, so no answer from these routes is cached.

use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState, envelope::error, middleware::AuthenticatedUser, upstream::authorized_org_id,
};

use super::shared;

/// The most characters a thread id or a snapshot id may have. Session Core's ids are far shorter.
const MAX_ID_LEN: usize = 200;

/// The typed refusal while the rollout switch is off.
const DISABLED: &str = "chat_snapshots_disabled";

/// The grant routes' body: the member to share with, and nothing else. The owner and the organisation are the
/// session's, so a body that names either is refused rather than dropped.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantBody {
    /// The member's user id. Control decides whether they belong to the owner's organisation.
    grantee_user_id: String,
}

/// What a route relays and which credentials ride with it.
#[derive(Debug, Clone, Copy)]
struct Relay {
    /// Whether the call carries the caller's `aud=data-plane` bearer, which Session Core needs to act for them
    /// in Control: the grant routes, the revoke (to remove the grants) and the shared read (a member's check).
    delegated: bool,
}

/// `POST /api/v1/chat/threads/{thread_id}/snapshots`.
pub(super) async fn create_snapshot(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Response {
    if !state.chat_snapshots_enabled {
        return disabled_response();
    }
    let thread_id = match checked_id(&thread_id, "thread_id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let url = format!(
        "{}/v1/threads/{}/snapshots",
        state.model_gateway_url,
        urlencoding::encode(&thread_id)
    );
    relay(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        None,
        Relay { delegated: false },
    )
    .await
}

/// `GET /api/v1/chat/threads/{thread_id}/snapshots`.
pub(super) async fn list_snapshots(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Response {
    if !state.chat_snapshots_enabled {
        return disabled_response();
    }
    let thread_id = match checked_id(&thread_id, "thread_id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let url = format!(
        "{}/v1/threads/{}/snapshots",
        state.model_gateway_url,
        urlencoding::encode(&thread_id)
    );
    relay(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        Relay { delegated: false },
    )
    .await
}

/// `DELETE /api/v1/chat/snapshots/{snapshot_id}`. Revoking makes no copy, so it is open to a caller whose
/// organisation has since gone zero-retention; Model Gateway decides.
pub(super) async fn revoke_snapshot(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(snapshot_id): Path<String>,
) -> Response {
    if !state.chat_snapshots_enabled {
        return disabled_response();
    }
    let snapshot_id = match checked_id(&snapshot_id, "snapshot_id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let url = format!(
        "{}/v1/snapshots/{}",
        state.model_gateway_url,
        urlencoding::encode(&snapshot_id)
    );
    relay(
        &state,
        &user,
        &headers,
        Method::DELETE,
        &url,
        None,
        Relay { delegated: true },
    )
    .await
}

/// `POST /api/v1/chat/snapshots/{snapshot_id}/grants`.
pub(super) async fn grant_snapshot(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(snapshot_id): Path<String>,
    body: Bytes,
) -> Response {
    if !state.chat_snapshots_enabled {
        return disabled_response();
    }
    relay_grant(&state, &user, &headers, &snapshot_id, Method::POST, &body).await
}

/// `DELETE /api/v1/chat/snapshots/{snapshot_id}/grants`. The member to stop sharing with is in the body.
pub(super) async fn ungrant_snapshot(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(snapshot_id): Path<String>,
    body: Bytes,
) -> Response {
    if !state.chat_snapshots_enabled {
        return disabled_response();
    }
    relay_grant(&state, &user, &headers, &snapshot_id, Method::DELETE, &body).await
}

/// `GET /api/v1/chat/snapshots/{snapshot_id}/grants`.
pub(super) async fn list_snapshot_grants(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(snapshot_id): Path<String>,
) -> Response {
    if !state.chat_snapshots_enabled {
        return disabled_response();
    }
    let snapshot_id = match checked_id(&snapshot_id, "snapshot_id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let url = grants_url(&state, &snapshot_id);
    relay(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        Relay { delegated: true },
    )
    .await
}

/// `GET /api/v1/chat/shared-snapshots/{snapshot_id}`: the redacted copy, as static text. Nothing here runs a
/// model, a tool or a connector.
pub(super) async fn get_shared_snapshot(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(snapshot_id): Path<String>,
) -> Response {
    if !state.chat_snapshots_enabled {
        return disabled_response();
    }
    let snapshot_id = match checked_id(&snapshot_id, "snapshot_id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let url = format!(
        "{}/v1/shared-snapshots/{}",
        state.model_gateway_url,
        urlencoding::encode(&snapshot_id)
    );
    relay(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        Relay { delegated: true },
    )
    .await
}

/// One grant call: the id and the body are checked, and the body is rebuilt from the one field it may carry.
async fn relay_grant(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    snapshot_id: &str,
    method: Method,
    body: &[u8],
) -> Response {
    let snapshot_id = match checked_id(snapshot_id, "snapshot_id") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let grantee_user_id = match grantee_from(body) {
        Ok(grantee) => grantee,
        Err(response) => return response,
    };
    let url = grants_url(state, &snapshot_id);
    relay(
        state,
        user,
        headers,
        method,
        &url,
        Some(json!({ "grantee_user_id": grantee_user_id })),
        Relay { delegated: true },
    )
    .await
}

fn grants_url(state: &AppState, snapshot_id: &str) -> String {
    format!(
        "{}/v1/snapshots/{}/grants",
        state.model_gateway_url,
        urlencoding::encode(snapshot_id)
    )
}

/// The steps of the module doc after the switch and the shape checks: membership, bearers, relay. Each public handler
/// checks the switch before it checks any id or body, so this is reached only while the switch is on.
async fn relay(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    url: &str,
    body: Option<Value>,
    mode: Relay,
) -> Response {
    // 2. Membership. The org is this server's own, and a caller without one has nothing to share with.
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_store(refuse(
            StatusCode::FORBIDDEN,
            "org_scope_required",
            "An authorized organization scope is required.",
        ));
    }
    // 3. The caller's own session-core bearer and the model-gateway bearer, minted per request.
    let model_token = shared::model_token(state, user, headers).await;
    let session_token = match shared::required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(reason) => return no_store(shared::delegated_auth_unavailable(reason).into_response()),
    };
    // The delegated credential is best-effort here: without it Model Gateway answers the grant routes with its
    // own typed `delegated_credential_required`, and a revoke or an owner's read still works.
    let data_plane_token = if mode.delegated {
        shared::data_plane_token(state, user, headers).await
    } else {
        None
    };
    // 4. The relay. Neither the browser's headers nor its body reach the upstream beyond the fields above.
    let (status, Json(payload)) = shared::proxy_model_json_with_delegations(
        state,
        method,
        url,
        body,
        model_token.as_deref(),
        data_plane_token.as_deref(),
        None,
        None,
        None,
        None,
        Some(&session_token),
        None,
        None,
        None,
        user,
    )
    .await;
    no_store((status, Json(payload)).into_response())
}

/// A thread or snapshot id as the upstream path takes it: trimmed, 1 to 200 characters of letters, digits and
/// `-`, `_`, `.`, `:`. Anything else is refused here instead of being encoded into a path.
fn checked_id(value: &str, field: &'static str) -> Result<String, Response> {
    let value = value.trim();
    if valid_id(value) {
        return Ok(value.to_owned());
    }
    let message = if field == "thread_id" {
        "thread_id is not valid."
    } else {
        "snapshot_id is not valid."
    };
    Err(no_store(refuse(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        message,
    )))
}

fn valid_id(value: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

/// The member a grant body names. The body is `{"grantee_user_id": "..."}` and nothing else; an empty body, a
/// body that is not JSON, an unknown field and a blank or over-long id are all refused.
fn grantee_from(body: &[u8]) -> Result<String, Response> {
    let invalid = || {
        no_store(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "grantee_user_id is required and is the only field.",
        ))
    };
    let parsed: GrantBody = serde_json::from_slice(body).map_err(|_| invalid())?;
    let grantee = parsed.grantee_user_id.trim();
    if grantee.is_empty() || grantee.len() > MAX_ID_LEN || grantee.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(grantee.to_owned())
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(error(code, message))).into_response()
}

/// A snapshot is the owner's conversation, and a grant list names members, so no cache keeps an answer.
fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The answer while the rollout switch is off. Every public handler returns it before it checks the id or the body,
/// so a malformed request gets the same answer as a well-formed one.
fn disabled_response() -> Response {
    no_store(refuse(
        StatusCode::NOT_FOUND,
        DISABLED,
        "Sharing a conversation as a snapshot is not enabled.",
    ))
}
