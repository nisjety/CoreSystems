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

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use super::{denial, shared_thread_read_decision_for_typed, space_lifecycle_by_ref};
use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_sse_stream_with_data_plane},
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

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/{space_ref}/runs/{run_id}/live",
        get(space_run_live),
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
