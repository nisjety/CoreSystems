//! WS17-S7: the reviewed declassify of one personal memory into a shared Space room, the gateway side.
//!
//! Route (merged by `module_routes()` in `spaces/mod.rs`, behind the session):
//! `POST /api/v1/spaces/{space_ref}/memory/declassify` with the body `{memory_id, reviewed_content,
//! client_request_id}` and no other field (`deny_unknown_fields`: a forged subject, role, audience, policy or
//! revision is a 400). The action `memory.declassify_to_space` (human-only, `dispatchers_spaces/ws17.rs`) runs
//! the same core, so the flag, the Zero Data Retention refusal, the lifecycle, Control's decision and the typed
//! errors exist once.
//!
//! WS17-S7b: `POST /api/v1/spaces/{space_ref}/memory/retract` with the body `{memory_id, client_request_id}` takes
//! back a room copy the caller shared (SD-22: only the declassifying person; Session Core checks the ledger subject).
//! It is behind the same flag and lifecycle, with no Zero Data Retention refusal, since a removal stores nothing. The
//! action `memory.retract_from_space` runs the same core. Control issues a `retract` decision, and model-gateway's
//! Space delete route forwards it to Session Core's DeleteMemory retract branch.
//!
//! This module stores nothing and decides nothing about authority. Control decides who may declassify, under
//! the caller's own delegation (`POST {user-core}/api/v1/internal/spaces/memory-declassify-decision`, the base
//! the other Space decision calls use). Its signed decision ref and token travel to model-gateway in a POST
//! body and never reach the browser. Model-gateway and Session Core verify the decision again and write the room
//! copy. The source memory is never changed.
//!
//! The reviewed text is hashed here (`sha256:` over its exact bytes), so Control binds the content without
//! seeing it. The text is forwarded exactly as sent: it is never trimmed or normalised, and only its emptiness
//! after a trim is tested. Nothing here logs the text, a token or a decision.
//!
//! Order of refusals on every route (each stops the next): flag (404 `feature_disabled`), organization (403
//! `active_membership_required`), Space reference (400 `invalid_space`), Zero Data Retention (typed 412
//! `zdr_durable_mutation_forbidden` before any Control or Convex call; a lookup outage is 503
//! `zdr_decision_unavailable`), request shape (400 `invalid_request`, `invalid_memory`,
//! `invalid_reviewed_content` or `client_request_id_required`), Space lifecycle (404 `space_not_found`, 503
//! `space_lifecycle_unavailable`), Control's decision (403 `memory_declassify_not_authorized`; an outage or an
//! unusable answer is 503 `space_decision_unavailable`), delegated credentials (503 `delegated_auth_unavailable`),
//! then model-gateway. Its typed 412 passes through as the typed ZDR 412, its 403 is 403
//! `memory_declassify_not_authorized`, its 404 is 404 `memory_not_found`, and any other failure is 503
//! `space_decision_unavailable`. A success is 201 `{data:{room_memory_id, space_ref, source_memory_id,
//! deduplicated}}`. Errors are `{error:{code,message,details}}`.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use bytes::Bytes;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{denial, space_lifecycle_by_ref};
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json},
    zdr::{durable_write_refusal, zdr_refusal_body, ZDR_DURABLE_MUTATION_FORBIDDEN},
};

type Reply = (StatusCode, Json<Value>);

/// The longest Space reference, in characters (the private panel's bound).
const MAX_SPACE_REF_CHARS: usize = 200;
/// The longest memory id, in bytes.
const MAX_MEMORY_ID_BYTES: usize = 200;
/// The longest reviewed text, in Unicode characters.
const MAX_REVIEWED_CHARS: usize = 1000;
/// The longest client request id, in characters (the bound the personal-agent create uses).
const MAX_CLIENT_REQUEST_ID_CHARS: usize = 128;
/// The surface a ZDR refusal names (the catalogue code is feature-less).
const DECLASSIFY_SURFACE: &str = "memory_declassify";
/// The operations the memory declassify family binds in Control's decision. A declassify binds the reviewed text's
/// digest; a retract binds none, since Session Core checks the ledger subject.
const OPERATION_DECLASSIFY: &str = "declassify";
const OPERATION_RETRACT: &str = "retract";
const DENIED: &str = "Declassifying this memory to the Space is not authorized.";
const RETRACT_DENIED: &str = "Taking this memory back from the room is not authorized.";

/// The only body fields. `deny_unknown_fields`: every other field, authority fields included, is a 400.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclassifyBody {
    memory_id: String,
    reviewed_content: String,
    client_request_id: String,
}

/// A request that has passed the request-shape rules. `reviewed_content` is exact; `client_request_id` is trimmed.
struct DeclassifyRequest {
    memory_id: String,
    reviewed_content: String,
    client_request_id: String,
}

/// The retract body (WS17-S7b): the two fields only, `deny_unknown_fields`. Taking back needs no content.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetractBody {
    memory_id: String,
    client_request_id: String,
}

/// A retract that has passed the request-shape rules. `client_request_id` is trimmed.
struct RetractRequest {
    memory_id: String,
    client_request_id: String,
}

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/spaces/{space_ref}/memory/declassify",
            post(declassify_memory),
        )
        .route(
            "/api/v1/spaces/{space_ref}/memory/retract",
            post(retract_memory),
        )
}

/// `POST .../memory/declassify`: share one of the caller's personal memories into this room.
async fn declassify_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    body: Bytes,
) -> Reply {
    // A body that is not JSON reads as null here and is refused as a request-shape 400, after the refusals above it.
    let body = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    declassify_memory_core(&state, &user, &headers, &space_ref, body).await
}

/// The route's and the action's work, so the flag, the ZDR refusal, Control's decision and the typed errors exist
/// once. `body` is the request's JSON value; the action passes only its three fields.
pub(crate) async fn declassify_memory_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    body: Value,
) -> Reply {
    // 1. Rollout flag. A rollout control, never an authorization input.
    if !state.space_features.space_memory_declassify_api {
        return feature_disabled();
    }
    // 2. Organization, then the Space reference.
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_membership();
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return invalid_space();
    }
    // 3. Zero Data Retention, before any Control or Convex call.
    if let Some((status, Json(refusal))) =
        durable_write_refusal(state, user, Some(headers), None).await
    {
        return label_zdr(status, refusal);
    }
    // 4. Request shape. No authority is read here.
    let request = match parse_request(body) {
        Ok(request) => request,
        Err(reply) => return reply,
    };
    // 5. Space lifecycle.
    match space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return refuse(StatusCode::NOT_FOUND, "space_not_found", "Space is not available.");
        }
        Err(()) => {
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            );
        }
    }
    // 6. Control's decision for this memory and this exact text.
    let digest = content_digest_of(&request.reviewed_content);
    let (decision_ref, token) = match memory_decision(
        state,
        user,
        &org_id,
        space_ref,
        MemoryDecisionAsk {
            operation: OPERATION_DECLASSIFY,
            memory_id: &request.memory_id,
            content_digest: &digest,
            idempotency_key: &request.client_request_id,
        },
    )
    .await
    {
        Ok(decision) => decision,
        Err(reply) => return reply,
    };
    // 7. Delegated credentials, then model-gateway.
    let session_token =
        match crate::domains::chat::shared::required_session_token(state, user, headers).await {
            Ok(session_token) => session_token,
            Err(reason) => return crate::domains::chat::shared::delegated_auth_unavailable(reason),
        };
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
    let url = format!("{}/v1/memories/declassify", state.model_gateway_url);
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(json!({
            "org_id": org_id,
            "user_id": user.user_id,
            "source_memory_id": request.memory_id,
            "space_id": space_ref,
            "reviewed_content": request.reviewed_content,
            "space_decision_token": token,
            "space_decision_ref": decision_ref,
            "idempotency_key": request.client_request_id,
        })),
        model_token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    relay_answer(status, &response, &request.memory_id, space_ref)
}

/// `POST .../memory/retract` (WS17-S7b): take back a room copy this caller shared.
async fn retract_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    body: Bytes,
) -> Reply {
    let body = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    retract_memory_core(&state, &user, &headers, &space_ref, body).await
}

/// The route's and the action's work for `memory.retract_from_space` (WS17-S7b). Zero Data Retention does not refuse a
/// removal: it stores nothing, and Control and Session Core allow it under every posture. Session Core checks that the
/// caller shared the row and that the share is live; a copy this caller did not share answers 404.
pub(crate) async fn retract_memory_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    body: Value,
) -> Reply {
    // 1. Rollout flag. A rollout control, never an authorization input.
    if !state.space_features.space_memory_declassify_api {
        return feature_disabled();
    }
    // 2. Organization, then the Space reference.
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_membership();
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return invalid_space();
    }
    // 3. Request shape. No authority is read here.
    let request = match parse_retract(body) {
        Ok(request) => request,
        Err(reply) => return reply,
    };
    // 4. Space lifecycle.
    match space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return refuse(StatusCode::NOT_FOUND, "space_not_found", "Space is not available.");
        }
        Err(()) => {
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            );
        }
    }
    // 5. Control's retract decision for this room row. Nothing is sent for the text: a retract has no digest.
    let (decision_ref, token) = match memory_decision(
        state,
        user,
        &org_id,
        space_ref,
        MemoryDecisionAsk {
            operation: OPERATION_RETRACT,
            memory_id: &request.memory_id,
            content_digest: "",
            idempotency_key: &request.client_request_id,
        },
    )
    .await
    {
        Ok(decision) => decision,
        Err(reply) => return reply,
    };
    // 6. Delegated credentials, then model-gateway's Space delete route. It forwards the decision and its ref verbatim;
    // Session Core picks the retract branch from the token's action and verifies the operation and the subject.
    let session_token =
        match crate::domains::chat::shared::required_session_token(state, user, headers).await {
            Ok(session_token) => session_token,
            Err(reason) => return crate::domains::chat::shared::delegated_auth_unavailable(reason),
        };
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
    let url = format!(
        "{}/v1/spaces/{}/memory/delete",
        state.model_gateway_url,
        urlencoding::encode(space_ref),
    );
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(json!({
            "memory_id": request.memory_id.clone(),
            "space_control_decision_ref": decision_ref,
            "space_control_decision_token": token,
        })),
        model_token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    relay_retract_answer(status, &response, &request.memory_id, space_ref)
}

/// The operation-bound fields of one Control decision of the memory declassify family: the memory id (the source row
/// for a declassify, the room row for a retract), the digest of the reviewed text (a declassify only) and the retry key.
struct MemoryDecisionAsk<'a> {
    operation: &'static str,
    memory_id: &'a str,
    content_digest: &'a str,
    idempotency_key: &'a str,
}

/// Asks Control for one decision of the memory declassify family under the caller's own delegation. A refusal (403,
/// or 404 from a Control without the endpoint, read as a refusal as the memory-decision sibling does) is 403, typed
/// when Control gave a reason, with the operation's own code. An outage, a 5xx or an unusable answer is 503
/// `space_decision_unavailable`. Nothing here ever grants.
async fn memory_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    ask: MemoryDecisionAsk<'_>,
) -> Result<(String, String), Reply> {
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/memory-declassify-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "operation": ask.operation,
            "memory_id": ask.memory_id,
            "content_digest": ask.content_digest,
            "idempotency_key": ask.idempotency_key,
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    let status = denial::refusal_status(status, &response);
    if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND {
        let (code, message) = if ask.operation == OPERATION_RETRACT {
            ("memory_retract_not_authorized", RETRACT_DENIED)
        } else {
            ("memory_declassify_not_authorized", DENIED)
        };
        let body = match denial::SpaceDenial::from_response(state, status, &response) {
            Some(denial) => error_with_details(code, message, denial.details_value()),
            None => error(code, message),
        };
        return Err((StatusCode::FORBIDDEN, Json(body)));
    }
    if !status.is_success() {
        return Err(decision_unavailable());
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
        return Err(decision_unavailable());
    }
    Ok((decision_ref, token))
}

/// The browser's answer to a model-gateway answer, rebuilt field by field: the success shape, the typed ZDR
/// refusal, and the closed refusal codes in fixed words. Upstream text and decision material never cross.
fn relay_answer(status: StatusCode, response: &Value, memory_id: &str, space_ref: &str) -> Reply {
    if status.is_success() {
        let Some(room_memory_id) = response
            .get("memory_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        else {
            return refuse(
                StatusCode::BAD_GATEWAY,
                "invalid_model_gateway_response",
                "Model Gateway returned an invalid declassify answer.",
            );
        };
        let deduplicated = response
            .get("deduplicated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        return (
            StatusCode::CREATED,
            Json(json!({"data": {
                "room_memory_id": room_memory_id,
                "space_ref": space_ref,
                "source_memory_id": memory_id,
                "deduplicated": deduplicated,
            }})),
        );
    }
    let code = response
        .pointer("/error/code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if status == StatusCode::PRECONDITION_FAILED && code == ZDR_DURABLE_MUTATION_FORBIDDEN {
        return label_zdr(
            StatusCode::PRECONDITION_FAILED,
            zdr_refusal_body(ZDR_DURABLE_MUTATION_FORBIDDEN),
        );
    }
    if status == StatusCode::FORBIDDEN {
        return refuse(StatusCode::FORBIDDEN, "memory_declassify_not_authorized", DENIED);
    }
    if status == StatusCode::NOT_FOUND {
        return refuse(StatusCode::NOT_FOUND, "memory_not_found", "The memory is not available.");
    }
    decision_unavailable()
}

/// The browser's answer to model-gateway's Space delete for a retract, rebuilt in fixed words: a removal is 200
/// `{data:{memory_id, space_ref, deleted:true}}`; a refusal of the caller is 403 `memory_retract_not_authorized`; a row
/// this caller holds no live share of is 404 `memory_not_found`. Upstream text and decision material never cross.
fn relay_retract_answer(status: StatusCode, response: &Value, memory_id: &str, space_ref: &str) -> Reply {
    if status.is_success() {
        if response.get("deleted").and_then(Value::as_bool) == Some(true) {
            return (
                StatusCode::OK,
                Json(json!({"data": {
                    "memory_id": memory_id,
                    "space_ref": space_ref,
                    "deleted": true,
                }})),
            );
        }
        return refuse(StatusCode::NOT_FOUND, "memory_not_found", "The memory is not available.");
    }
    let code = response
        .pointer("/error/code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if status == StatusCode::FORBIDDEN || code == "space_memory_not_authorized" {
        return refuse(StatusCode::FORBIDDEN, "memory_retract_not_authorized", RETRACT_DENIED);
    }
    if status == StatusCode::NOT_FOUND || code == "memory_not_found" {
        return refuse(StatusCode::NOT_FOUND, "memory_not_found", "The memory is not available.");
    }
    decision_unavailable()
}

/// The retract request shape: the exact fields, then each rule. Reads no authority.
fn parse_retract(body: Value) -> Result<RetractRequest, Reply> {
    let body = serde_json::from_value::<RetractBody>(body).map_err(|_| {
        refuse(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The retract request is not valid.",
        )
    })?;
    if !valid_memory_id(&body.memory_id) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_memory",
            "A valid memory id is required.",
        ));
    }
    let client_request_id = body.client_request_id.trim().to_owned();
    if client_request_id.is_empty()
        || client_request_id.chars().count() > MAX_CLIENT_REQUEST_ID_CHARS
    {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "client_request_id_required",
            "A client_request_id is required to take a memory back.",
        ));
    }
    Ok(RetractRequest {
        memory_id: body.memory_id,
        client_request_id,
    })
}

/// The request shape: the exact fields, then each rule. Reads no authority.
fn parse_request(body: Value) -> Result<DeclassifyRequest, Reply> {
    let body = serde_json::from_value::<DeclassifyBody>(body).map_err(|_| {
        refuse(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The declassify request is not valid.",
        )
    })?;
    if !valid_memory_id(&body.memory_id) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_memory",
            "A valid memory id is required.",
        ));
    }
    if body.reviewed_content.trim().is_empty()
        || body.reviewed_content.chars().count() > MAX_REVIEWED_CHARS
    {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_reviewed_content",
            "The reviewed text is required and may be at most 1000 characters.",
        ));
    }
    let client_request_id = body.client_request_id.trim().to_owned();
    if client_request_id.is_empty()
        || client_request_id.chars().count() > MAX_CLIENT_REQUEST_ID_CHARS
    {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "client_request_id_required",
            "A client_request_id is required to declassify a memory.",
        ));
    }
    Ok(DeclassifyRequest {
        memory_id: body.memory_id,
        reviewed_content: body.reviewed_content,
        client_request_id,
    })
}

/// A Space reference: non-empty, at most 200 characters, with no control character and no path, query or
/// fragment delimiter (the private panel's rule).
fn valid_space_ref(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_SPACE_REF_CHARS
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '?' | '#' | '\\'))
}

/// A memory id: non-empty, at most 200 bytes, with no surrounding whitespace. It is never trimmed.
fn valid_memory_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_MEMORY_ID_BYTES && value.trim() == value
}

/// `sha256:` and the lowercase hex of the SHA-256 over the exact bytes of the reviewed text.
fn content_digest_of(reviewed_content: &str) -> String {
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(reviewed_content.as_bytes()))
    )
}

/// The typed ZDR refusal, labelled with the surface it refuses (the private panel's and the canvas's labelling).
fn label_zdr(status: StatusCode, mut body: Value) -> Reply {
    if status == StatusCode::PRECONDITION_FAILED {
        if let Some(object) = body.get_mut("error").and_then(Value::as_object_mut) {
            object.insert(
                "details".to_owned(),
                json!({ "surface": DECLASSIFY_SURFACE }),
            );
        }
    }
    (status, Json(body))
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn feature_disabled() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "feature_disabled",
        "Declassifying a memory to a Space is not enabled in this environment.",
    )
}

fn no_membership() -> Reply {
    refuse(
        StatusCode::FORBIDDEN,
        "active_membership_required",
        "An active membership is required for this room.",
    )
}

fn invalid_space() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_space",
        "A Space reference is required.",
    )
}

fn decision_unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "space_decision_unavailable",
        "Room authority could not be confirmed right now. Nothing was changed.",
    )
}
