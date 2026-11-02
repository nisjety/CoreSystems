//! Direct messages and group DMs: the BFF half of WS12-S3.
//!
//! `POST /api/v1/spaces/dm {participants:[subject_id], client_request_id}` opens (or returns) the
//! DM for exactly that set of people; `POST` and `DELETE /api/v1/spaces/{space_ref}/hide` hide and
//! show one DM in the caller's own sidebar. This module holds no Space state and decides no
//! access: Control answers whether the caller may open a DM and with whom (`dm-eligibility`),
//! Application (Convex `openDmForGateway`, `spaceDmHides`) stores the Space record and the hides,
//! and Control registers the roster afterwards (the Space stays `pending_registration` until then,
//! so the reply is 202). A DM has a fixed roster, no managers and no agents.
//!
//! Gate order of the open route: flag (404 `dm_disabled`), verified organization (403
//! `active_membership_required`), Zero Data Retention (typed 412/503 before any Control or
//! Convex call: a DM persists message content), body shape (400 `dm_participants_invalid`), the
//! Control pre-check, then Application. The caller and the organization come from the session,
//! never from the body; the body can name neither.
//!
//! Errors are `{error:{code,message,details}}`: 400 `dm_participants_invalid`
//! (`details.reason`), 403 `dm_not_entitled` (`details.missing[]`), 404 `dm_disabled`, 409
//! `dm_registration_failed` / `space_not_hideable` / `dm_agent_bind_refused`, 412
//! `zdr_durable_mutation_forbidden`, 503 `zdr_decision_unavailable` /
//! `space_provisioning_unavailable` / `dm_hide_unavailable`.
//!
//! Hide is presentation only (ids and a time, no content): it never touches Control membership,
//! and a post in the DM clears every viewer's hide so the DM resurfaces ([`resurface`]).

use std::collections::HashSet;

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Extension, Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
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
    upstream::{authorized_org_id, proxy_json},
    zdr::durable_write_refusal,
};

use super::sidebar::convex_call;

type Reply = (StatusCode, Json<Value>);

/// Participants besides the caller: a group DM holds 3 to 9 people in all.
const MAX_OTHER_PARTICIPANTS: usize = 8;
const MAX_ID_LEN: usize = 128;
const MAX_SPACE_REF_LEN: usize = 200;
const MAX_REQUEST_ID_LEN: usize = 200;
const BODY_LIMIT_BYTES: usize = 16 * 1024;

/// Registered through `module_routes()` behind `require_session`. The static `dm` segment wins
/// over the `{space_ref}` matchers of the main table.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/spaces/dm",
            post(open_dm).layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES)),
        )
        .route(
            "/api/v1/spaces/{space_ref}/hide",
            post(hide_space).delete(unhide_space),
        )
}

/// True for the two direct-message kinds.
pub(super) fn is_dm_kind(kind: &str) -> bool {
    matches!(kind, "dm" | "group_dm")
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn feature_off() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "dm_disabled",
        "Direct messages are not enabled.",
    )
}

fn membership_required() -> Reply {
    refuse(
        StatusCode::FORBIDDEN,
        "active_membership_required",
        "An active organization membership is required to use direct messages.",
    )
}

fn provisioning_unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "space_provisioning_unavailable",
        "The direct message could not be opened. Nothing was provisioned.",
    )
}

fn invalid_participants(reason: &str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "dm_participants_invalid",
            "These people cannot form a direct message.",
            json!({ "reason": reason }),
        )),
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenDmRequest {
    participants: Vec<String>,
    client_request_id: String,
}

/// The reason words Control and Application share, bounded so nothing else from an upstream
/// message is ever returned.
fn reason_token(raw: &str) -> String {
    let token: String = raw
        .chars()
        .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .take(32)
        .collect();
    if token.is_empty() {
        "shape".to_owned()
    } else {
        token
    }
}

/// Shape of the body, before any upstream call. Returns the other people, trimmed, in the order
/// given.
fn parse_request(bytes: &[u8], caller: &str) -> Result<Vec<String>, Reply> {
    let request: OpenDmRequest =
        serde_json::from_slice(bytes).map_err(|_| invalid_participants("shape"))?;
    let request_id = request.client_request_id.trim();
    if request_id.is_empty() || request_id.len() > MAX_REQUEST_ID_LEN {
        return Err(invalid_participants("shape"));
    }
    let count = request.participants.len();
    if count == 0 || count > MAX_OTHER_PARTICIPANTS {
        return Err(invalid_participants("count"));
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut others = Vec::with_capacity(count);
    for raw in request.participants {
        let id = raw.trim();
        if id.is_empty() || id.len() > MAX_ID_LEN {
            return Err(invalid_participants("shape"));
        }
        if id == caller {
            return Err(invalid_participants("self"));
        }
        if !seen.insert(id.to_owned()) {
            return Err(invalid_participants("duplicate"));
        }
        others.push(id.to_owned());
    }
    Ok(others)
}

fn actor_of(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

/// What Control answered to the pre-check.
struct Eligibility {
    participant_hash: String,
    /// The canonical roster including the caller.
    participant_ids: Vec<String>,
}

/// `POST /api/v1/internal/spaces/dm-eligibility` under the caller's signed delegation. A typed
/// Control refusal becomes the browser envelope (rebuilt from bounded fields, never passed
/// through); anything else is a 503.
async fn control_eligibility(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    others: &[String],
) -> Result<Eligibility, Reply> {
    let url = format!("{}/api/v1/internal/spaces/dm-eligibility", state.user_core_url);
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({ "participant_ids": others })),
        Some(org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if status.is_success() {
        let data = unwrap_data(&response);
        let hash = data
            .get("participant_hash")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|hash| !hash.is_empty());
        let ids: Option<Vec<String>> = data
            .get("participant_ids")
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            });
        return match (data.get("eligible").and_then(Value::as_bool), hash, ids) {
            (Some(true), Some(hash), Some(ids)) if !ids.is_empty() => Ok(Eligibility {
                participant_hash: hash.to_owned(),
                participant_ids: ids,
            }),
            _ => Err(provisioning_unavailable()),
        };
    }
    let code = response
        .pointer("/error/code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Err(match (status, code) {
        (StatusCode::NOT_FOUND, "dm_disabled") => feature_off(),
        (StatusCode::FORBIDDEN, "dm_not_entitled") => {
            let missing: Vec<String> = response
                .pointer("/error/details/missing")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(reason_token)
                        .take(8)
                        .collect()
                })
                .unwrap_or_default();
            (
                StatusCode::FORBIDDEN,
                Json(error_with_details(
                    "dm_not_entitled",
                    "Direct messages are not enabled for this organization.",
                    json!({ "missing": missing }),
                )),
            )
        }
        (StatusCode::UNPROCESSABLE_ENTITY, "dm_participants_invalid")
        | (StatusCode::BAD_REQUEST, "dm_participants_invalid") => {
            let reason = response
                .pointer("/error/details/reason")
                .and_then(Value::as_str)
                .map(reason_token)
                .unwrap_or_else(|| "shape".to_owned());
            invalid_participants(&reason)
        }
        _ => provisioning_unavailable(),
    })
}

/// Application's thrown error as the browser envelope.
fn open_error(message: Option<String>) -> Reply {
    let Some(message) = message else {
        return provisioning_unavailable();
    };
    if message.contains("dm_disabled") {
        return feature_off();
    }
    if let Some(start) = message.find("dm_participants_invalid:") {
        return invalid_participants(&reason_token(
            &message[start + "dm_participants_invalid:".len()..],
        ));
    }
    if message.contains("dm_participant_not_member") {
        return invalid_participants("not_in_org");
    }
    if message.contains("dm_registration_failed") {
        return refuse(
            StatusCode::CONFLICT,
            "dm_registration_failed",
            "A direct message with these people could not be registered. Ask an administrator to clear it.",
        );
    }
    provisioning_unavailable()
}

/// `POST /api/v1/spaces/dm`.
async fn open_dm(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Bytes,
) -> Reply {
    open_dm_core(&state, &user, &headers, &body).await
}

/// The whole open, shared with the `spaces.open_dm` action: flag, organization, ZDR, body shape,
/// the Control pre-check, then Application.
pub(crate) async fn open_dm_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    body: &[u8],
) -> Reply {
    if !state.space_features.dm_api {
        return feature_off();
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return membership_required();
    }
    // A DM persists message content: refused typed for a Zero Data Retention org before any
    // Control or Convex call.
    if let Some(refusal) = durable_write_refusal(state, user, Some(headers), None).await {
        return refusal;
    }
    let others = match parse_request(body, user.user_id.trim()) {
        Ok(others) => others,
        Err(reply) => return reply,
    };
    let eligibility = match control_eligibility(state, user, &org_id, &others).await {
        Ok(eligibility) => eligibility,
        Err(reply) => return reply,
    };
    // Application takes the OTHER people and adds the caller itself.
    let roster_others: Vec<&String> = eligibility
        .participant_ids
        .iter()
        .filter(|id| id.trim() != user.user_id.trim())
        .collect();
    let opened = match convex_call(
        state,
        "mutation",
        "spaces:openDmForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "participantExternalAuthIds": roster_others,
            "expectedHash": eligibility.participant_hash,
        }),
    )
    .await
    {
        Ok(opened) => opened,
        Err(message) => return open_error(message),
    };
    let created = opened.get("created").and_then(Value::as_bool).unwrap_or(false);
    let Some(space) = opened.get("space").filter(|space| space.is_object()) else {
        return provisioning_unavailable();
    };
    let Some(space_ref) = space
        .get("spaceRef")
        .and_then(Value::as_str)
        .filter(|space_ref| !space_ref.trim().is_empty())
    else {
        return provisioning_unavailable();
    };
    let participants: Vec<Value> = space
        .get("participantExternalAuthIds")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(|id| json!({ "subject_id": id }))
                .collect()
        })
        .unwrap_or_default();
    let status = if created {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    (
        status,
        Json(json!({"data": {
            "space": {
                "space_ref": space_ref,
                "name": space.get("name").and_then(Value::as_str).unwrap_or("Direct message"),
                "kind": space.get("kind").and_then(Value::as_str).unwrap_or("dm"),
                "lifecycle": space
                    .get("lifecycle")
                    .and_then(Value::as_str)
                    .unwrap_or("pending_registration"),
                "participants": participants,
            },
            "created": created,
        }})),
    )
}

/// Flag, organization, space ref shape, then Control's own index: the caller must be a
/// participant (the index lists only Spaces the caller belongs to) and the Space must be a DM.
async fn gate_hide(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
) -> Result<(String, String), Reply> {
    if !state.space_features.dm_api {
        return Err(feature_off());
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(membership_required());
    }
    let space_ref = space_ref.trim();
    if space_ref.is_empty() || space_ref.len() > MAX_SPACE_REF_LEN {
        return Err(refuse(
            StatusCode::NOT_FOUND,
            "space_not_found",
            "Space is not available.",
        ));
    }
    let Some(index) = super::control_space_index(state, user, &org_id).await else {
        return Err(hide_unavailable());
    };
    let entry = index
        .iter()
        .find(|entry| entry.get("space_ref").and_then(Value::as_str) == Some(space_ref));
    let Some(entry) = entry else {
        return Err(refuse(
            StatusCode::NOT_FOUND,
            "space_not_found",
            "Space is not available.",
        ));
    };
    if !is_dm_kind(entry.get("kind").and_then(Value::as_str).unwrap_or_default()) {
        return Err(not_hideable());
    }
    Ok((org_id, space_ref.to_owned()))
}

fn hide_unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "dm_hide_unavailable",
        "The direct message could not be updated. Nothing was changed.",
    )
}

fn not_hideable() -> Reply {
    refuse(
        StatusCode::CONFLICT,
        "space_not_hideable",
        "Only a direct message can be hidden.",
    )
}

fn hide_error(message: Option<String>) -> Reply {
    match message.as_deref() {
        Some(message) if message.contains("space_not_hideable") => not_hideable(),
        Some(message) if message.contains("space_not_found") => refuse(
            StatusCode::NOT_FOUND,
            "space_not_found",
            "Space is not available.",
        ),
        _ => hide_unavailable(),
    }
}

/// `POST /api/v1/spaces/{space_ref}/hide`.
async fn hide_space(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
) -> Reply {
    set_hidden(&state, &user, &space_ref, true).await
}

/// `DELETE /api/v1/spaces/{space_ref}/hide`.
async fn unhide_space(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
) -> Reply {
    set_hidden(&state, &user, &space_ref, false).await
}

/// The hide and the unhide, shared with the `spaces.hide_space` action.
pub(crate) async fn set_hidden(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    hidden: bool,
) -> Reply {
    let (org_id, space_ref) = match gate_hide(state, user, space_ref).await {
        Ok(gate) => gate,
        Err(reply) => return reply,
    };
    let path = if hidden {
        "spaceDmHides:hideForGateway"
    } else {
        "spaceDmHides:unhideForGateway"
    };
    match convex_call(
        state,
        "mutation",
        path,
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({"data": {
                "space_ref": space_ref,
                "hidden": hidden,
                "hidden_at": if hidden {
                    result.get("hiddenAt").cloned().unwrap_or(Value::Null)
                } else {
                    Value::Null
                },
            }})),
        ),
        Err(message) => hide_error(message),
    }
}

/// The caller's hidden DMs (space refs). Degrade-safe: a failed read lists every DM, because a
/// DM that is wrongly shown costs a click and one that is wrongly hidden loses a conversation.
pub(super) async fn hidden_set(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> HashSet<String> {
    if !state.space_features.dm_api {
        return HashSet::new();
    }
    let Ok(rows) = convex_call(
        state,
        "query",
        "spaceDmHides:hiddenForViewerForGateway",
        json!({ "externalAuthId": user.user_id, "externalOrgId": org_id }),
    )
    .await
    else {
        return HashSet::new();
    };
    rows.as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("spaceRef").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Adds the DM facts to one `GET /api/v1/spaces` row: `agent_scope` (Control's fact, derived
/// from the kind), `participants` (id and display name, DM kinds only) and `hidden` (the
/// caller's own hide; false for every other kind).
pub(super) fn decorate_index_row(row: &mut Value, entry: &Value, hidden: &HashSet<String>) {
    let Some(object) = row.as_object_mut() else {
        return;
    };
    let kind = entry.get("kind").and_then(Value::as_str).unwrap_or_default();
    if let Some(scope) = entry.get("agent_scope").and_then(Value::as_str) {
        object.insert("agent_scope".to_owned(), json!(scope));
    }
    if is_dm_kind(kind) {
        let participants: Vec<Value> = entry
            .get("participants")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|participant| {
                        let subject_id = participant.get("subject_id").and_then(Value::as_str)?;
                        let mut out = Map::new();
                        out.insert("subject_id".to_owned(), json!(subject_id));
                        out.insert(
                            "display_name".to_owned(),
                            json!(participant
                                .get("display_name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()),
                        );
                        Some(Value::Object(out))
                    })
                    .collect()
            })
            .unwrap_or_default();
        object.insert("participants".to_owned(), Value::Array(participants));
    }
    let space_ref = entry.get("space_ref").and_then(Value::as_str).unwrap_or_default();
    object.insert(
        "hidden".to_owned(),
        json!(is_dm_kind(kind) && hidden.contains(space_ref)),
    );
}

/// New activity in a DM: clear every viewer's hide so it resurfaces. Best effort, after the post
/// was accepted; a failure never changes the post's answer.
pub(super) async fn resurface(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) {
    let _ = convex_call(
        state,
        "mutation",
        "spaceDmHides:clearForSpaceForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await;
}

/// A DM has no agents: the two agent-attach routes answer a typed 409 before Control or
/// Application is asked (Application and Control refuse independently). `None` when the Space is
/// not a DM, the flag is off or the lookup failed (those paths keep their own refusals).
pub(super) async fn agent_attach_refusal(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Option<Reply> {
    if !state.space_features.dm_api {
        return None;
    }
    let space = super::space_lifecycle_by_ref(state, user, org_id, space_ref)
        .await
        .ok()??;
    if !is_dm_kind(space.get("kind").and_then(Value::as_str).unwrap_or_default()) {
        return None;
    }
    Some(refuse(
        StatusCode::CONFLICT,
        "dm_agent_bind_refused",
        "A direct message cannot have agents.",
    ))
}
