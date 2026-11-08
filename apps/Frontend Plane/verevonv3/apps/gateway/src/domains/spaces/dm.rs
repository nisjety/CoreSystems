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

#[cfg(test)]
mod tests {
    use super::{decorate_index_row, hidden_set, open_dm_core, parse_request, resurface, set_hidden};
    use crate::config::AppState;
    use crate::middleware::{AuthenticatedUser, AuthorizedMembership};
    use axum::{
        extract::{Extension, State},
        http::HeaderMap,
    };
    use serde_json::{json, Value};
    use std::collections::HashSet;
    use wiremock::matchers::{body_partial_json, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ELIGIBILITY: &str = "/api/v1/internal/spaces/dm-eligibility";
    const INDEX: &str = "/api/v1/internal/spaces";

    fn member() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "Caller".to_owned(),
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

    struct Fx {
        state: AppState,
        control: MockServer,
        app: MockServer,
        _org: MockServer,
    }

    fn eligible() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {
            "eligible": true,
            "participant_hash": "hash-1",
            "participant_ids": ["user-1", "user-2"],
        }}))
    }

    fn opened(created: bool) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": {
            "created": created,
            "space": {
                "spaceRef": "space-dm-1", "name": "Ann", "kind": "dm",
                "lifecycle": "pending_registration",
                "participantExternalAuthIds": ["user-1", "user-2"],
            }
        }}))
    }

    /// DM flag on. Control answers the pre-check as given and the caller's Space index with a
    /// room and a DM; Application answers `open` for `spaces:openDmForGateway` and the hide
    /// functions and lists generically; the org is not ZDR unless `zdr` says so (`None` = down).
    async fn fixture(eligibility: ResponseTemplate, open: ResponseTemplate, zdr: Option<bool>) -> Fx {
        let control = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path(ELIGIBILITY))
            .respond_with(eligibility)
            .mount(&control)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path(INDEX))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"spaces": [
                {"space_ref": "space-room", "kind": "room", "role": "editor", "agent_scope": "room"},
                {"space_ref": "space-dm-1", "kind": "dm", "role": "member", "agent_scope": "none",
                 "participants": [
                    {"subject_id": "user-1", "display_name": "Caller", "email": "leak@example.com"},
                    {"subject_id": "user-2", "display_name": "Ann"}
                 ]},
                {"space_ref": "space-gdm-1", "kind": "group_dm", "role": "member", "participants": []},
            ]}})))
            .mount(&control)
            .await;
        let org = MockServer::start().await;
        let org_reply = match zdr {
            Some(flag) => ResponseTemplate::new(200).set_body_json(
                json!({"data": {"metadata": {"interactiveRetention": {"zdr": flag}}}}),
            ),
            None => ResponseTemplate::new(500).set_body_json(json!({"error": "down"})),
        };
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org_reply)
            .mount(&org)
            .await;
        let app = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .and(body_partial_json(json!({"path": "spaces:openDmForGateway"})))
            .respond_with(open)
            .mount(&app)
            .await;
        for path in ["spaceDmHides:hideForGateway", "spaceDmHides:unhideForGateway"] {
            Mock::given(wm_method("POST"))
                .and(wm_path("/api/mutation"))
                .and(body_partial_json(json!({"path": path})))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    json!({"status": "success", "value": {"hiddenAt": 1_700_000_000_000_u64}}),
                ))
                .mount(&app)
                .await;
        }
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .and(body_partial_json(json!({"path": "spaceDmHides:clearForSpaceForGateway"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": null})))
            .mount(&app)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(body_partial_json(json!({"path": "spaceDmHides:hiddenForViewerForGateway"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"status": "success", "value": [{"spaceRef": "space-dm-1"}]}),
            ))
            .mount(&app)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(body_partial_json(json!({"path": "spaces:spacesForOrgForGateway"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef": "space-room", "name": "Room", "kind": "room", "lifecycle": "active"},
                {"spaceRef": "space-dm-1", "name": "Ann", "kind": "dm", "lifecycle": "active"},
            ]})))
            .mount(&app)
            .await;
        let mut state = crate::tests::test_state(false);
        state.space_features.dm_api = true;
        state.user_core_url = control.uri();
        state.org_core_url = org.uri();
        state.application_convex_url = app.uri();
        state.application_convex_service_key = "application-test-key".into();
        Fx { state, control, app, _org: org }
    }

    async fn reply_of(reply: (axum::http::StatusCode, axum::Json<Value>)) -> (u16, Value) {
        (reply.0.as_u16(), reply.1 .0)
    }

    async fn open(fx: &Fx, body: Value) -> (u16, Value) {
        reply_of(open_dm_core(&fx.state, &member(), &HeaderMap::new(), &serde_json::to_vec(&body).unwrap()).await).await
    }

    fn open_body() -> Value {
        json!({"participants": ["user-2"], "client_request_id": "req-1"})
    }

    async fn n_requests(server: &MockServer) -> usize {
        server.received_requests().await.unwrap().len()
    }

    async fn bodies(server: &MockServer) -> Vec<Value> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| serde_json::from_slice(&request.body).unwrap_or(Value::Null))
            .collect()
    }

    #[tokio::test]
    async fn open_dm_checks_eligibility_before_convex() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let refused = [
            (
                ResponseTemplate::new(403).set_body_json(json!({"error": {"code": "dm_not_entitled",
                    "details": {"missing": ["dm_enabled", "Bad Token!"]}, "message": "internal text"}})),
                403,
                "dm_not_entitled",
            ),
            (
                ResponseTemplate::new(422).set_body_json(json!({"error": {"code": "dm_participants_invalid",
                    "details": {"reason": "not_in_org: secret"}}})),
                400,
                "dm_participants_invalid",
            ),
            (ResponseTemplate::new(404).set_body_json(json!({"error": {"code": "dm_disabled"}})), 404, "dm_disabled"),
            (ResponseTemplate::new(500).set_body_json(json!({"error": "boom"})), 503, "space_provisioning_unavailable"),
            (ResponseTemplate::new(200).set_body_json(json!({"data": {"eligible": false}})), 503, "space_provisioning_unavailable"),
        ];
        for (template, status, code) in refused {
            let fx = fixture(template, opened(true), Some(false)).await;
            let (got, body) = open(&fx, open_body()).await;
            assert_eq!((got, body["error"]["code"].as_str().unwrap()), (status, code));
            assert_eq!(n_requests(&fx.app).await, 0, "Application is not asked after a Control refusal");
            assert!(!body.to_string().contains("internal text") && !body.to_string().contains("secret"));
        }
        let fx = fixture(
            ResponseTemplate::new(403).set_body_json(json!({"error": {"code": "dm_not_entitled",
                "details": {"missing": ["dm_enabled", "Bad Token!"]}}})),
            opened(true),
            Some(false),
        )
        .await;
        let (_, body) = open(&fx, open_body()).await;
        assert_eq!(body["error"]["details"]["missing"], json!(["dm_enabled", "shape"]));
    }

    #[tokio::test]
    async fn open_dm_new_is_202_and_sends_the_other_people_and_the_hash_only() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(eligible(), opened(true), Some(false)).await;
        let (status, body) = open(&fx, open_body()).await;
        assert_eq!(status, 202);
        assert_eq!(body["data"]["created"], true);
        assert_eq!(body["data"]["space"]["space_ref"], "space-dm-1");
        assert_eq!(body["data"]["space"]["lifecycle"], "pending_registration");
        assert_eq!(
            body["data"]["space"]["participants"],
            json!([{"subject_id": "user-1"}, {"subject_id": "user-2"}])
        );
        let sent = bodies(&fx.app).await;
        let args = &sent[0]["args"];
        assert_eq!(args["externalAuthId"], "user-1");
        assert_eq!(args["externalOrgId"], "org-1");
        assert_eq!(args["participantExternalAuthIds"], json!(["user-2"]), "the caller is not in the others");
        assert_eq!(args["expectedHash"], "hash-1");
        let control_sent = bodies(&fx.control).await;
        assert_eq!(control_sent[0]["participant_ids"], json!(["user-2"]));
    }

    #[tokio::test]
    async fn open_dm_existing_returns_200_not_created() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(eligible(), opened(false), Some(false)).await;
        let (status, body) = open(&fx, open_body()).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["created"], false);
        assert_eq!(body["data"]["space"]["space_ref"], "space-dm-1");
    }

    #[tokio::test]
    async fn open_dm_application_errors_map_to_typed_envelopes() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (message, status, code) in [
            ("Uncaught Error: dm_registration_failed ...", 409, "dm_registration_failed"),
            ("dm_participant_not_member", 400, "dm_participants_invalid"),
            ("dm_participants_invalid:count and more", 400, "dm_participants_invalid"),
            ("dm_disabled", 404, "dm_disabled"),
            ("anything else", 503, "space_provisioning_unavailable"),
        ] {
            let fx = fixture(
                eligible(),
                ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": message})),
                Some(false),
            )
            .await;
            let (got, body) = open(&fx, open_body()).await;
            assert_eq!((got, body["error"]["code"].as_str().unwrap()), (status, code), "{message}");
        }
    }

    #[tokio::test]
    async fn zdr_org_dm_is_a_typed_412_with_no_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(eligible(), opened(true), Some(true)).await;
        let (status, body) = open(&fx, open_body()).await;
        assert_eq!(status, 412);
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert_eq!(n_requests(&fx.control).await, 0);
        assert_eq!(n_requests(&fx.app).await, 0);
        // A request-level ZDR header is the same refusal, and an org-core outage is the retryable 503.
        let mut headers = HeaderMap::new();
        headers.insert("x-zdr", "true".parse().unwrap());
        let fx = fixture(eligible(), opened(true), Some(false)).await;
        let (status, _) = reply_of(open_dm_core(&fx.state, &member(), &headers, &serde_json::to_vec(&open_body()).unwrap()).await).await;
        assert_eq!(status, 412);
        let fx = fixture(eligible(), opened(true), None).await;
        let (status, body) = open(&fx, open_body()).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "zdr_decision_unavailable");
        assert_eq!(n_requests(&fx.control).await, 0);
    }

    #[tokio::test]
    async fn flags_off_are_404_dm_disabled_with_zero_upstream_requests() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fixture(eligible(), opened(true), Some(false)).await;
        fx.state.space_features.dm_api = false;
        let (status, body) = open(&fx, open_body()).await;
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (404, "dm_disabled"));
        for hidden in [true, false] {
            let (status, body) = reply_of(set_hidden(&fx.state, &member(), "space-dm-1", hidden).await).await;
            assert_eq!((status, body["error"]["code"].as_str().unwrap()), (404, "dm_disabled"));
        }
        assert_eq!(n_requests(&fx.control).await + n_requests(&fx.app).await, 0);
        // The hidden set is empty without a read, and the resurface never reaches Convex... the
        // flag only gates the read; the clear is best effort and covered below.
        assert!(hidden_set(&fx.state, &member(), "org-1").await.is_empty());
        assert_eq!(n_requests(&fx.app).await, 0);
    }

    #[tokio::test]
    async fn open_dm_without_an_authorized_org_is_403_and_a_bad_body_is_400_before_control() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(eligible(), opened(true), Some(false)).await;
        let mut user = member();
        user.authorized_membership = None;
        let (status, body) = reply_of(open_dm_core(&fx.state, &user, &HeaderMap::new(), b"{}").await).await;
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (403, "active_membership_required"));
        for (body, reason) in [
            (json!({"participants": [], "client_request_id": "r"}), "count"),
            (json!({"participants": (0..9).map(|i| format!("u-{i}")).collect::<Vec<_>>(), "client_request_id": "r"}), "count"),
            (json!({"participants": ["user-1"], "client_request_id": "r"}), "self"),
            (json!({"participants": ["a", "a"], "client_request_id": "r"}), "duplicate"),
            (json!({"participants": [" "], "client_request_id": "r"}), "shape"),
            (json!({"participants": ["a"], "client_request_id": " "}), "shape"),
            (json!({"participants": ["a"], "client_request_id": "r", "org_id": "org-2"}), "shape"),
        ] {
            let (status, got) = open(&fx, body.clone()).await;
            assert_eq!(status, 400, "{body}");
            assert_eq!(got["error"]["code"], "dm_participants_invalid");
            assert_eq!(got["error"]["details"]["reason"], reason, "{body}");
        }
        assert_eq!(n_requests(&fx.control).await, 0);
        assert_eq!(n_requests(&fx.app).await, 0);
    }

    #[test]
    fn parse_request_trims_and_keeps_order() {
        let body = serde_json::to_vec(&json!({"participants": [" b ", "a"], "client_request_id": "r"})).unwrap();
        assert_eq!(parse_request(&body, "me").unwrap(), ["b", "a"]);
    }

    #[tokio::test]
    async fn hide_on_a_non_dm_is_409_space_not_hideable_and_needs_control_index_membership() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(eligible(), opened(true), Some(false)).await;
        let (status, body) = reply_of(set_hidden(&fx.state, &member(), "space-room", true).await).await;
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (409, "space_not_hideable"));
        // Not in Control's index for this caller: the same 404 as a Space that does not exist.
        let (status, body) = reply_of(set_hidden(&fx.state, &member(), "space-elsewhere", true).await).await;
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (404, "space_not_found"));
        assert_eq!(n_requests(&fx.app).await, 0, "no Application call for either refusal");
        // Control's index unavailable is a typed 503, not a guess.
        let mut down = fx.state.clone();
        down.user_core_url = "http://127.0.0.1:1".into();
        let (status, body) = reply_of(set_hidden(&down, &member(), "space-dm-1", true).await).await;
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (503, "dm_hide_unavailable"));
    }

    #[tokio::test]
    async fn hide_and_unhide_a_dm_call_the_matching_mutation() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(eligible(), opened(true), Some(false)).await;
        let (status, body) = reply_of(set_hidden(&fx.state, &member(), "space-dm-1", true).await).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["hidden"], true);
        assert_eq!(body["data"]["space_ref"], "space-dm-1");
        assert_eq!(body["data"]["hidden_at"], 1_700_000_000_000_u64);
        let (status, body) = reply_of(set_hidden(&fx.state, &member(), "space-gdm-1", false).await).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["hidden"], false);
        assert!(body["data"]["hidden_at"].is_null());
        let sent = bodies(&fx.app).await;
        assert_eq!(sent[0]["path"], "spaceDmHides:hideForGateway");
        assert_eq!(sent[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(sent[0]["args"]["externalOrgId"], "org-1");
        assert_eq!(sent[1]["path"], "spaceDmHides:unhideForGateway");
        assert_eq!(sent[1]["args"]["spaceRef"], "space-gdm-1");
    }

    #[test]
    fn decorate_index_row_adds_participants_agent_scope_and_hidden_for_dms_only() {
        let hidden: HashSet<String> = ["space-dm-1".to_owned(), "space-room".to_owned()].into();
        let dm = json!({"space_ref": "space-dm-1", "kind": "dm", "agent_scope": "none", "participants": [
            {"subject_id": "user-1", "display_name": "Caller", "email": "leak@example.com"},
            {"subject_id": "user-2"},
            {"display_name": "No id"},
        ]});
        let mut row = json!({"space_ref": "space-dm-1"});
        decorate_index_row(&mut row, &dm, &hidden);
        assert_eq!(row["agent_scope"], "none");
        assert_eq!(row["hidden"], true);
        assert_eq!(
            row["participants"],
            json!([{"subject_id": "user-1", "display_name": "Caller"}, {"subject_id": "user-2", "display_name": ""}])
        );
        assert!(!row.to_string().contains("leak@example.com"));
        // A room is never hidden or given participants, even when its ref is in the set.
        let room = json!({"space_ref": "space-room", "kind": "room", "agent_scope": "room"});
        let mut row = json!({"space_ref": "space-room"});
        decorate_index_row(&mut row, &room, &hidden);
        assert_eq!(row["hidden"], false);
        assert_eq!(row["agent_scope"], "room");
        assert!(row.get("participants").is_none());
    }

    #[tokio::test]
    async fn the_listing_adds_dm_facts_and_drops_dm_kinds_with_the_flag_off() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(eligible(), opened(true), Some(false)).await;
        let list = |state: AppState| async move {
            let (status, axum::Json(body)) =
                super::super::list_personal_spaces(State(state), Extension(member())).await;
            (status.as_u16(), body)
        };
        let (status, body) = list(fx.state.clone()).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["features"]["dm"], true);
        let spaces = body["data"]["spaces"].as_array().unwrap();
        let by_ref = |r: &str| spaces.iter().find(|s| s["space_ref"] == r).unwrap().clone();
        assert_eq!(by_ref("space-dm-1")["hidden"], true);
        assert_eq!(by_ref("space-dm-1")["kind"], "dm");
        assert_eq!(by_ref("space-dm-1")["participants"][1]["subject_id"], "user-2");
        assert_eq!(by_ref("space-dm-1")["agent_scope"], "none");
        assert_eq!(by_ref("space-gdm-1")["hidden"], false);
        assert_eq!(by_ref("space-room")["hidden"], false);
        assert_eq!(by_ref("space-room")["agent_scope"], "room");

        let mut off = fx.state.clone();
        off.space_features.dm_api = false;
        let (status, body) = list(off).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["features"]["dm"], false);
        let refs: Vec<&str> = body["data"]["spaces"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["space_ref"].as_str())
            .collect();
        assert_eq!(refs, ["space-room"], "no DM kind is listed with the flag off");
    }

    #[tokio::test]
    async fn a_post_in_a_dm_clears_the_hides_through_resurface() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(eligible(), opened(true), Some(false)).await;
        resurface(&fx.state, &member(), "org-1", "space-dm-1").await;
        let sent = bodies(&fx.app).await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["path"], "spaceDmHides:clearForSpaceForGateway");
        assert_eq!(sent[0]["args"]["spaceRef"], "space-dm-1");
        assert_eq!(sent[0]["args"]["externalOrgId"], "org-1");
        // Best effort: an unreachable Application never panics or surfaces.
        let mut down = fx.state.clone();
        down.application_convex_url = "http://127.0.0.1:1".into();
        resurface(&down, &member(), "org-1", "space-dm-1").await;
    }

    #[tokio::test]
    async fn the_agent_attach_routes_refuse_a_dm_with_409_dm_agent_bind_refused() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(eligible(), opened(true), Some(false)).await;
        let refusal = super::agent_attach_refusal(&fx.state, &member(), "org-1", "space-dm-1").await;
        let (status, body) = reply_of(refusal.expect("a DM refuses agents")).await;
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (409, "dm_agent_bind_refused"));
        // A room, an unknown Space and the flag off keep their own paths.
        assert!(super::agent_attach_refusal(&fx.state, &member(), "org-1", "space-room").await.is_none());
        assert!(super::agent_attach_refusal(&fx.state, &member(), "org-1", "space-missing").await.is_none());
        let mut off = fx.state.clone();
        off.space_features.dm_api = false;
        assert!(super::agent_attach_refusal(&off, &member(), "org-1", "space-dm-1").await.is_none());
    }
}
