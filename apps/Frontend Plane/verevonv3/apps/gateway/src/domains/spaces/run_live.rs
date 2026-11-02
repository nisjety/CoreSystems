//! Member live view of an agent run, the BFF half of WS09-S4.
//!
//! `GET /api/v1/spaces/{space_ref}/runs/{run_id}/live` (SSE, `Last-Event-ID` accepted) lets a room
//! member watch another member's agent turn: step by step, and with the answer text when
//! `SPACE_RUN_LIVE_TEXT` allows. This hop decides nothing about what a member may see. The frames
//! are projected by model-gateway from an allowlist (`room_run.rs`) after Session Core authorized
//! the attach (`RunService.AuthorizeRoomRunAttach`), and they are relayed here byte for byte.
//!
//! What this hop owns, in order (each step is a refusal before the next one runs):
//!
//! 1. the rollout flag `SPACE_RUN_LIVE_VIEW` (404 `feature_unavailable` when off; never an
//!    authorization input),
//! 2. live organization membership (403 `active_membership_required`) and the request shape (400
//!    `invalid_space` / `invalid_run`),
//! 3. the org Zero Data Retention pre-check, before any Control call: ZDR is a typed refusal
//!    (403 `zdr_feature_not_supported`, feature `agent_run`) and an unknown posture is 503
//!    `zdr_decision_unavailable`; the room live view has no zero-retention form,
//! 4. the Space lifecycle (404 `space_not_found`, 503 `space_lifecycle_unavailable`),
//! 5. a fresh Control `model.thread.read` decision for THIS attach (403 `space_read_not_authorized`).
//!    The decision never reaches the browser and is never cached: it travels gateway to
//!    model-gateway in a POST body, not a URL,
//! 6. the delegated Session Core and Model credentials, then the relay.
//!
//! Errors are `{error:{code,message,details}}`. A model-gateway refusal is relayed only for the
//! closed codes in [`RELAYED`], with this module's fixed words; anything else is `model_error`.
//!
//! # Manager Stop (WS09-S8)
//!
//! `POST /api/v1/spaces/{space_ref}/runs/{run_id}/stop {idempotency_key?}` lets a room manager or
//! owner stop another member's live run. Same order as the live view, with a Control
//! `model.run.control` decision in place of the read decision: flag `SPACE_RUN_MANAGER_STOP` (404
//! `feature_unavailable`), membership, shape, the Zero Data Retention pre-check before any Control
//! call (412 `zdr_durable_mutation_forbidden`), Space lifecycle, then Control (403
//! `space_run_control_not_authorized` for a caller below manager, an org without
//! `run_control_entitled`, or a personal Space). The decision rides to model-gateway in a POST
//! body and is verified again by Session Core, which is the authority. Relayed codes are the closed
//! set in [`STOP_RELAYED`]; `200` carries `{data:{run_id,status,cancelled,receipt_id}}`.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use super::{
    denial, shared_thread_read_decision_for_typed, space_lifecycle_by_ref, unix_nanos,
};
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json, proxy_sse_stream_with_data_plane},
    zdr::{self, OrgZdrPosture},
};

const MAX_SPACE_REF_LEN: usize = 200;
const MAX_RUN_ID_LEN: usize = 64;

/// Model-gateway refusals this route relays, as (code, upstream status, fixed message).
const RELAYED: &[(&str, u16, &str)] = &[
    ("space_read_not_authorized", 403, "Watching this run is not authorized."),
    (
        "zdr_feature_not_supported",
        403,
        "Live agent runs are not supported with zero data retention.",
    ),
    ("run_not_found", 404, "The run is not available."),
    ("feature_unavailable", 404, "The live run view is not enabled."),
    ("run_stream_expired", 410, "The live view of this run has ended."),
    ("too_many_live_views", 429, "Too many open live views."),
    ("run_live_unavailable", 503, "The live run view is temporarily unavailable."),
];

/// Model-gateway refusals of a stop that this route relays, as (code, status, fixed message).
const STOP_RELAYED: &[(&str, StatusCode, &str)] = &[
    (
        "space_run_control_not_authorized",
        StatusCode::FORBIDDEN,
        "Stopping this run is not authorized.",
    ),
    ("run_not_found", StatusCode::NOT_FOUND, "The run is not available."),
    ("feature_unavailable", StatusCode::NOT_FOUND, "Stopping runs is not enabled."),
    (
        "run_control_unavailable",
        StatusCode::SERVICE_UNAVAILABLE,
        "Run control is temporarily unavailable.",
    ),
];

const MAX_IDEMPOTENCY_KEY_LEN: usize = 128;

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/spaces/{space_ref}/runs/{run_id}/live",
            get(space_run_live),
        )
        .route(
            "/api/v1/spaces/{space_ref}/runs/{run_id}/stop",
            post(space_run_stop),
        )
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(error(code, message))).into_response()
}

/// The typed ZDR refusal for the room live view, in the Model Plane catalogue shape
/// (`zdr-refusals.json`), so the browser renders the same words whichever plane refused.
fn zdr_agent_run_refusal() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": {
                "code": "zdr_feature_not_supported",
                "message": "This is not supported with zero data retention yet: agent runs. The request was refused before any model call.",
                "retryable": false,
                "action": "turn_off_feature",
                "feature": "agent_run",
            }
        })),
    )
        .into_response()
}

fn valid_space_ref(space_ref: &str) -> bool {
    !space_ref.is_empty()
        && space_ref.len() <= MAX_SPACE_REF_LEN
        && !space_ref.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

fn valid_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id.len() <= MAX_RUN_ID_LEN
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

async fn space_run_live(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, run_id)): Path<(String, String)>,
) -> Response {
    // 1. Rollout flag.
    if !state.space_features.run_live_view {
        return refuse(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "The live run view is not enabled.",
        );
    }
    // 2. Membership, then shape.
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to watch a run.",
        );
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A valid Space reference is required.",
        );
    }
    let run_id = run_id.trim();
    if !valid_run_id(run_id) {
        return refuse(StatusCode::BAD_REQUEST, "invalid_run", "A valid run id is required.");
    }
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| {
            !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
        })
        .map(str::to_owned);

    // 3. Zero Data Retention fails closed, before any Control call.
    if zdr::request_zdr(&headers, None) {
        return zdr_agent_run_refusal();
    }
    match zdr::org_zdr_lookup(&state, &user).await {
        OrgZdrPosture::Disabled => {}
        OrgZdrPosture::Enabled => return zdr_agent_run_refusal(),
        OrgZdrPosture::Unknown => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(zdr::zdr_refusal_body(zdr::ZDR_DECISION_UNAVAILABLE)),
            )
                .into_response()
        }
    }

    // 4. Space lifecycle.
    match space_lifecycle_by_ref(&state, &user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return refuse(StatusCode::NOT_FOUND, "space_not_found", "Space is not available.")
        }
        Err(()) => {
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            )
        }
    }

    // 5. A fresh Control read decision for this attach.
    let (read, read_denial) =
        match shared_thread_read_decision_for_typed(&state, &user, &org_id, space_ref, "").await {
            Ok(decision) => decision.split(),
            Err(response) => return response.into_response(),
        };
    let Some((decision_ref, token)) = read else {
        let message = "Watching this run is not authorized.";
        return match read_denial {
            Some(denial) => (
                StatusCode::FORBIDDEN,
                Json(error_with_details(
                    denial::OP_READ,
                    message,
                    denial.details_value(),
                )),
            )
                .into_response(),
            None => refuse(StatusCode::FORBIDDEN, denial::OP_READ, message),
        };
    };

    // 6. Delegated credentials, then the relay. The decision rides in the body, never the URL.
    let session_token =
        match crate::domains::chat::shared::required_session_token(&state, &user, &headers).await {
            Ok(token) => token,
            Err(reason) => {
                return crate::domains::chat::shared::delegated_auth_unavailable(reason)
                    .into_response()
            }
        };
    let model_token = crate::domains::chat::shared::model_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/spaces/{}/runs/{}/live",
        state.model_gateway_url,
        urlencoding::encode(space_ref),
        urlencoding::encode(run_id),
    );
    let body: Value = json!({
        "space_read_decision_ref": decision_ref,
        "space_read_decision_token": token,
    });
    proxy_sse_stream_with_data_plane(
        &state,
        Method::POST,
        &url,
        Some(body),
        model_token.as_deref(),
        None,
        None,
        None,
        None,
        None,
        Some(&session_token),
        None,
        None,
        last_event_id.as_deref(),
        Some((&user.user_id, org_id.trim())),
        false,
        RELAYED,
    )
    .await
}

// ---------------------------------------------------------------------------
// Manager Stop (WS09-S8)
// ---------------------------------------------------------------------------

type Reply = (StatusCode, Json<Value>);

fn reply(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn valid_idempotency_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_IDEMPOTENCY_KEY_LEN
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

async fn space_run_stop(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, run_id)): Path<(String, String)>,
    body: Option<Json<Value>>,
) -> Response {
    let body = body.map(|Json(value)| value).unwrap_or(Value::Null);
    let (status, json) = stop_run_core(&state, &user, Some(&headers), &space_ref, &run_id, &body).await;
    (status, json).into_response()
}

/// The stop route's whole work, shared with the `spaces.stop_run` action dispatcher so the flag, the
/// Zero Data Retention refusal, the Control decision and the typed errors exist once.
pub(crate) async fn stop_run_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: Option<&HeaderMap>,
    space_ref: &str,
    run_id: &str,
    body: &Value,
) -> Reply {
    // 1. Rollout flag. A rollout control, never an authorization input.
    if !state.space_features.run_manager_stop {
        return reply(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "Stopping runs is not enabled.",
        );
    }
    // 2. Membership, then shape.
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return reply(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to stop a run.",
        );
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return reply(StatusCode::BAD_REQUEST, "invalid_space", "A valid Space reference is required.");
    }
    let run_id = run_id.trim();
    if !valid_run_id(run_id) {
        return reply(StatusCode::BAD_REQUEST, "invalid_run", "A valid run id is required.");
    }
    let idempotency_key = match body.get("idempotency_key") {
        None | Some(Value::Null) => format!("space-stop-{}", unix_nanos()),
        Some(Value::String(key)) if valid_idempotency_key(key.trim()) => key.trim().to_owned(),
        Some(_) => {
            return reply(
                StatusCode::BAD_REQUEST,
                "invalid_idempotency_key",
                "The idempotency key is not valid.",
            )
        }
    };

    // 3. Zero Data Retention fails closed, before any Control call. A stop writes a durable receipt.
    if let Some(refusal) = crate::zdr::durable_write_refusal(state, user, headers, Some(body)).await {
        return refusal;
    }

    // 4. Space lifecycle.
    match space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => return reply(StatusCode::NOT_FOUND, "space_not_found", "Space is not available."),
        Err(()) => {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            )
        }
    }

    // 5. A fresh Control run-control decision for THIS stop.
    let (decision_ref, token) =
        match run_control_decision(state, user, &org_id, space_ref, run_id, &idempotency_key).await {
            Ok(decision) => decision,
            Err(reply) => return reply,
        };

    // 6. Delegated credentials, then model-gateway. The decision rides in the body, never the URL.
    let empty = HeaderMap::new();
    let headers = headers.unwrap_or(&empty);
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
    let url = format!(
        "{}/v1/spaces/{}/runs/{}/stop",
        state.model_gateway_url,
        urlencoding::encode(space_ref),
        urlencoding::encode(run_id),
    );
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_control_decision_ref": decision_ref,
            "space_control_decision_token": token,
            "idempotency_key": idempotency_key,
        })),
        model_token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    relay_stop_answer(status, &response, run_id)
}

/// Asks Control for the `model.run.control` decision of this stop. A refusal is 403
/// `space_run_control_not_authorized` (typed details when Control gave a reason); an absent endpoint
/// (an older Control) is the same refusal; anything else is an unavailable Control, never a grant.
async fn run_control_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    run_id: &str,
    idempotency_key: &str,
) -> Result<(String, String), Reply> {
    const DENIED: &str = "Stopping this run is not authorized.";
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!("{}/api/v1/internal/spaces/run-control-decision", state.user_core_url);
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "run_id": run_id,
            "operation": "cancel",
            "idempotency_key": idempotency_key,
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    let status = denial::refusal_status(status, &response);
    if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND {
        let typed = denial::SpaceDenial::from_response(state, status, &response);
        let body = match typed {
            Some(denial) => error_with_details(
                "space_run_control_not_authorized",
                DENIED,
                denial.details_value(),
            ),
            None => error("space_run_control_not_authorized", DENIED),
        };
        return Err((StatusCode::FORBIDDEN, Json(body)));
    }
    if !status.is_success() {
        return Err(reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "run_control_unavailable",
            "Run control is temporarily unavailable.",
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
            "Control returned an invalid run-control decision.",
        ));
    }
    Ok((decision_ref, token))
}

/// The browser's answer to a model-gateway stop answer: the success shape rebuilt field by field, a
/// ZDR catalogue refusal passed through, the closed refusal codes in fixed words, everything else
/// `run_control_unavailable`. Upstream text, ids and decision material never cross.
fn relay_stop_answer(status: StatusCode, response: &Value, run_id: &str) -> Reply {
    if status.is_success() {
        let outcome = response.get("status").and_then(Value::as_str).unwrap_or("cancelled");
        let outcome = if outcome == "already_terminal" { "already_terminal" } else { "cancelled" };
        return (
            StatusCode::OK,
            Json(json!({"data": {
                "run_id": run_id,
                "status": outcome,
                "cancelled": response.get("cancelled").and_then(Value::as_bool).unwrap_or(false),
                "receipt_id": response.get("receipt_id").and_then(Value::as_str).unwrap_or_default(),
            }})),
        );
    }
    let code = response
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if code == "zdr_durable_mutation_forbidden" || code == "zdr_feature_not_supported" {
        return (
            StatusCode::PRECONDITION_FAILED,
            Json(zdr::zdr_refusal_body(zdr::ZDR_DURABLE_MUTATION_FORBIDDEN)),
        );
    }
    if let Some((code, status, message)) = STOP_RELAYED.iter().find(|(known, _, _)| *known == code) {
        return (*status, Json(error(*code, *message)));
    }
    reply(
        StatusCode::SERVICE_UNAVAILABLE,
        "run_control_unavailable",
        "Run control is temporarily unavailable.",
    )
}
