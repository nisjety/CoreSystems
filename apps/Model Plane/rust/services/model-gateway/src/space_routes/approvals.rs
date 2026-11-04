//! Space approval routes (build plan WS10-S6): the room read, the member decide and the reroute.
//!
//! Thin relays of Session Core's `OrchestrationService`. Authority is Session Core's: each route
//! forwards the caller's verified bearer plus the Control-signed decision and holds none of its
//! own. User principals only. Request URLs and bodies are never logged by these handlers, and
//! refusals carry fixed words, never the upstream text.
//!
//! - `GET /v1/spaces/:space_id/approvals` (query: `run_id`, `status`, `space_read_decision_ref`,
//!   `space_read_decision_token`): the redacted room view.
//! - `POST /v1/spaces/:space_id/approvals/:approval_id/decide` (body: `decision`, `scope`,
//!   `reason`, `space_decision_ref`, `space_decision_token`): a member's decision.
//! - `PATCH /v1/spaces/:space_id/approvals/:approval_id/addressee` (body: `kind`, `subject_id`,
//!   `space_decision_ref`, `space_decision_token`): move a pending approval.
//!
//! Errors, as `{error:{code,message,details}}`: Session Core's switch off 404
//! `approval_member_decide_disabled`, `UNAUTHENTICATED` 401 `unauthorized`, `PERMISSION_DENIED` 403
//! (`approval_not_addressed_to_you`, `space_authority_stale`, else `approval_not_authorized`),
//! `NOT_FOUND` 404 `approval_not_found`, `INVALID_ARGUMENT` 400 `invalid_request`,
//! `FAILED_PRECONDITION` already decided 409 `approval_already_decided`, anything else 503
//! `approval_authority_unavailable`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{
    Addressee, AddresseeKind, ApprovalDecisionSource, ApprovalScope, ApprovalState,
    DecideApprovalRequest, ListApprovalsRequest, RerouteApprovalRequest,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::{
    approval_value, authenticated_session_request, require_non_zdr_durable_mutation,
};
use crate::state::AppState;

const MAX_ID_BYTES: usize = 128;
const MAX_REASON_BYTES: usize = 1024;
/// Session Core's own trailer for a closed error word.
const ERROR_CODE_TRAILER: &str = "x-mp-error-code";

/// The Space approval routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/:space_id/approvals", get(list_space_approvals))
        .route(
            "/v1/spaces/:space_id/approvals/:approval_id/decide",
            post(decide_space_approval),
        )
        .route(
            "/v1/spaces/:space_id/approvals/:approval_id/addressee",
            patch(reroute_space_approval),
        )
}

#[derive(Debug, Default, Deserialize)]
struct ListQuery {
    run_id: Option<String>,
    status: Option<String>,
    space_read_decision_ref: Option<String>,
    space_read_decision_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DecideBody {
    decision: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    space_decision_ref: String,
    space_decision_token: String,
}

#[derive(Debug, Deserialize)]
struct RerouteBody {
    kind: String,
    #[serde(default)]
    subject_id: Option<String>,
    space_decision_ref: String,
    space_decision_token: String,
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
        .into_response()
}

fn invalid(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", message)
}

/// The closed error word Session Core attached to a refusal, when this route knows it.
fn upstream_code(status: &tonic::Status) -> Option<&'static str> {
    let raw = status.metadata().get(ERROR_CODE_TRAILER)?.to_str().ok()?;
    [
        "approval_member_decide_disabled",
        "approval_not_addressed_to_you",
        "approval_addressee_invalid",
        "space_authority_stale",
    ]
    .into_iter()
    .find(|code| *code == raw)
}

/// The HTTP form of an upstream refusal. Messages are fixed words, never the upstream text.
fn status_response(status: &tonic::Status) -> Response {
    let code = upstream_code(status);
    match status.code() {
        tonic::Code::Unauthenticated => {
            error_response(StatusCode::UNAUTHORIZED, "unauthorized", "authentication required")
        }
        tonic::Code::PermissionDenied => error_response(
            StatusCode::FORBIDDEN,
            code.unwrap_or("approval_not_authorized"),
            "this approval action is not permitted",
        ),
        tonic::Code::NotFound => {
            error_response(StatusCode::NOT_FOUND, "approval_not_found", "approval not found")
        }
        tonic::Code::InvalidArgument => match code {
            Some(code) => error_response(StatusCode::BAD_REQUEST, code, "the approval request is invalid"),
            None => invalid("the approval request is invalid"),
        },
        tonic::Code::FailedPrecondition => match code {
            Some("approval_member_decide_disabled") => error_response(
                StatusCode::NOT_FOUND,
                "approval_member_decide_disabled",
                "member approval is not enabled",
            ),
            _ if status.message() == "approval is already decided" => error_response(
                StatusCode::CONFLICT,
                "approval_already_decided",
                "the approval has already been decided",
            ),
            _ => error_response(
                StatusCode::PRECONDITION_FAILED,
                "approval_precondition_failed",
                "the approval cannot be changed now",
            ),
        },
        _ => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "approval_authority_unavailable",
            "approval authority is unavailable",
        ),
    }
}

fn bounded_id(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && value.len() <= MAX_ID_BYTES
}

fn require_user(claims: &Claims) -> Result<(), Response> {
    if claims.user_id.trim().is_empty() {
        Err(error_response(
            StatusCode::FORBIDDEN,
            "approval_not_authorized",
            "a user principal is required",
        ))
    } else {
        Ok(())
    }
}

async fn list_space_approvals(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(space_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Response {
    if let Err(response) = require_user(&claims) {
        return response;
    }
    let space_id = space_id.trim().to_owned();
    if !bounded_id(&space_id) {
        return invalid("space_id is invalid");
    }
    let run_id = query.run_id.unwrap_or_default();
    if run_id.len() > MAX_ID_BYTES {
        return invalid("run_id is invalid");
    }
    let state_filter = match query.status.as_deref().map(str::trim).unwrap_or("") {
        "" => ApprovalState::Unspecified,
        "requested" | "pending" => ApprovalState::Requested,
        "granted" => ApprovalState::Granted,
        "denied" => ApprovalState::Denied,
        "timed_out" => ApprovalState::TimedOut,
        _ => return invalid("status is invalid"),
    };
    let request = match authenticated_session_request(
        ListApprovalsRequest {
            run_id,
            org_id: claims.org_id.clone(),
            space_id,
            space_read_decision_ref: query.space_read_decision_ref.unwrap_or_default(),
            space_read_decision_token: query.space_read_decision_token.unwrap_or_default(),
            state: state_filter as i32,
            ..Default::default()
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err((status, body)) => return (status, body).into_response(),
    };
    match state.orchestration_client.clone().list_approvals(request).await {
        Ok(response) => Json(json!({
            "approvals": response
                .into_inner()
                .approvals
                .iter()
                .map(approval_value)
                .collect::<Vec<Value>>(),
        }))
        .into_response(),
        Err(status) => status_response(&status),
    }
}

async fn decide_space_approval(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path((space_id, approval_id)): Path<(String, String)>,
    Json(body): Json<DecideBody>,
) -> Response {
    if let Err(response) = require_user(&claims) {
        return response;
    }
    let (space_id, approval_id) = (space_id.trim().to_owned(), approval_id.trim().to_owned());
    if !bounded_id(&space_id) || !bounded_id(&approval_id) {
        return invalid("space_id or approval_id is invalid");
    }
    // The decision's reason is free text that the approval keeps: a zero-retention or waived caller is
    // refused first, typed, before any Session Core call.
    if let Err(refusal) = require_non_zdr_durable_mutation(&claims) {
        return refusal.into_response();
    }
    let decision = match body.decision.trim() {
        "granted" | "approved" => ApprovalState::Granted,
        "denied" => ApprovalState::Denied,
        _ => return invalid("decision must be granted or denied"),
    };
    let scope = match body.scope.as_deref().map(str::trim).unwrap_or("once") {
        "once" => ApprovalScope::Once,
        "thread" => ApprovalScope::Thread,
        "run" => ApprovalScope::Run,
        _ => return invalid("scope is invalid"),
    };
    let reason = body.reason.unwrap_or_default();
    if reason.len() > MAX_REASON_BYTES {
        return invalid("reason is too long");
    }
    let request = match authenticated_session_request(
        DecideApprovalRequest {
            approval_id,
            decision: decision as i32,
            decision_reason: reason,
            org_id: claims.org_id.clone(),
            decision_source: ApprovalDecisionSource::User as i32,
            scope: scope as i32,
            space_id,
            space_decision_ref: body.space_decision_ref,
            space_decision_token: body.space_decision_token,
            ..Default::default()
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err((status, body)) => return (status, body).into_response(),
    };
    match state.orchestration_client.clone().decide_approval(request).await {
        Ok(response) => Json(json!({
            "approval": response.into_inner().approval.as_ref().map(approval_value),
        }))
        .into_response(),
        Err(status) => status_response(&status),
    }
}

async fn reroute_space_approval(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path((space_id, approval_id)): Path<(String, String)>,
    Json(body): Json<RerouteBody>,
) -> Response {
    if let Err(response) = require_user(&claims) {
        return response;
    }
    let (space_id, approval_id) = (space_id.trim().to_owned(), approval_id.trim().to_owned());
    if !bounded_id(&space_id) || !bounded_id(&approval_id) {
        return invalid("space_id or approval_id is invalid");
    }
    let kind = match body.kind.trim() {
        "requester" => AddresseeKind::Requester,
        "named" => AddresseeKind::Named,
        "any_editor" => AddresseeKind::AnyEditor,
        "any_manager" => AddresseeKind::AnyManager,
        _ => return invalid("kind is invalid"),
    };
    let request = match authenticated_session_request(
        RerouteApprovalRequest {
            approval_id,
            org_id: claims.org_id.clone(),
            addressee: Some(Addressee {
                kind: kind as i32,
                subject_id: body.subject_id.unwrap_or_default(),
            }),
            space_id,
            space_decision_ref: body.space_decision_ref,
            space_decision_token: body.space_decision_token,
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err((status, body)) => return (status, body).into_response(),
    };
    match state.orchestration_client.clone().reroute_approval(request).await {
        Ok(response) => Json(json!({
            "approval": response.into_inner().approval.as_ref().map(approval_value),
        }))
        .into_response(),
        Err(status) => status_response(&status),
    }
}
