//! Space memory for the Space's owner or admin, the BFF half of EP-8 S7.
//!
//! `GET /api/v1/spaces/{space_ref}/memory` lists what a shared room has learned (its `space` scope
//! rows), each marked when the room's current audience does not recall it, and `DELETE
//! /api/v1/spaces/{space_ref}/memory/{memory_id}` removes one row. With the optional query
//! `?agent_subject_id=` the list returns one named agent's notes in that Space instead (EP-6 P3).
//! This hop decides nothing about who may do either. In order (each step is a refusal before the next
//! one runs):
//!
//! 1. the rollout flag `GATEWAY_SPACE_MEMORY_UI` (404 `feature_unavailable` when off; never an
//!    authorization input),
//! 2. live organization membership (403 `active_membership_required`) and the request shape (400
//!    `invalid_space` / `invalid_memory` / `invalid_agent`),
//! 3. the Space lifecycle (404 `space_not_found`, 503 `space_lifecycle_unavailable`),
//! 4. a fresh Control `model.space.memory.manage` decision for THIS list or THIS row, which Control
//!    issues to the Space's owner or admin only (403 `space_memory_not_authorized`). The decision
//!    never reaches the browser and is never cached: it travels to model-gateway in a POST body,
//! 5. the delegated Session Core and Model credentials, then model-gateway, and Session Core, which
//!    verifies the decision again and is the authority.
//!
//! The Space is the path's `space_ref`, resolved here and bound into the Control decision; no client
//! `space_id` is read from a query or a body. Zero Data Retention needs no pre-check: neither route
//! stores content, a zero-retention Space list is answered empty by Session Core, and a delete is
//! allowed under every posture. A zero-retention caller's agent list is refused `named_agent` (the
//! catalogue refusal, shaped as `zdr-refusals.json` writes it). Errors are
//! `{error:{code,message,details}}` with `details` an object, except that catalogue refusal.

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{future::Future, time::Duration};

use super::{denial, space_lifecycle_by_ref};
// The one hold definition of the BFF (V3d-bff-2), shared with the account erase in `domains::privacy`.
use crate::domains::privacy::{hold_answer, HoldAnswer};
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json},
};

const MAX_SPACE_REF_LEN: usize = 200;
const MAX_MEMORY_ID_LEN: usize = 64;

/// The two operations a Control Space memory decision binds; Session Core's names.
const OPERATION_LIST: &str = "list";
const OPERATION_DELETE: &str = "delete";
/// The decision operation that erases one named agent's memory in the Space (EP-6 P3). Its
/// `memory_id` is the agent's subject id, so one decision erases one agent's rows only.
const OPERATION_ERASE_AGENT: &str = "erase_agent";

const DENIED: &str = "Managing this Space's memory is not authorized.";

/// The catalogue code and feature key of the zero-retention refusal of one named agent's notes
/// (EP-6 P3, EP-6 §5). The SPA renders the words from its own copy of the catalogue.
const ZDR_FEATURE_NOT_SUPPORTED: &str = "zdr_feature_not_supported";
const NAMED_AGENT_FEATURE: &str = "named_agent";

type Reply = (StatusCode, Json<Value>);

/// The optional query of the Space memory list. `agent_subject_id` names one bound agent whose notes
/// the list returns (EP-6 P3); absent or blank, the list is the Space's own rows.
#[derive(Deserialize)]
struct SpaceMemoryListQuery {
    agent_subject_id: Option<String>,
}

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/spaces/{space_ref}/memory", get(list_space_memory))
        .route(
            "/api/v1/spaces/{space_ref}/memory/{memory_id}",
            delete(delete_space_memory),
        )
}

fn reply(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn valid_space_ref(space_ref: &str) -> bool {
    !space_ref.is_empty()
        && space_ref.len() <= MAX_SPACE_REF_LEN
        && !space_ref.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

fn valid_memory_id(memory_id: &str) -> bool {
    !memory_id.is_empty()
        && memory_id.len() <= MAX_MEMORY_ID_LEN
        && memory_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The model-gateway path and body of one Space memory call. The Space comes from the resolved
/// path `space_ref` only; the decision rides in the body, never the URL.
fn model_request(
    model_gateway_url: &str,
    space_ref: &str,
    operation: &str,
    memory_id: &str,
    decision_ref: &str,
    token: &str,
) -> (String, Value) {
    let url = format!(
        "{}/v1/spaces/{}/memory/{}",
        model_gateway_url,
        urlencoding::encode(space_ref),
        operation,
    );
    let mut body = json!({
        "space_control_decision_ref": decision_ref,
        "space_control_decision_token": token,
    });
    if operation == OPERATION_DELETE {
        body["memory_id"] = json!(memory_id);
    }
    (url, body)
}

async fn list_space_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    Query(query): Query<SpaceMemoryListQuery>,
) -> Response {
    let agent_subject_id = query.agent_subject_id.unwrap_or_default();
    let (status, Json(body)) = space_memory_core(
        &state,
        &user,
        &headers,
        &space_ref,
        OPERATION_LIST,
        "",
        agent_subject_id.trim(),
    )
    .await;
    (status, Json(body)).into_response()
}

async fn delete_space_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, memory_id)): Path<(String, String)>,
) -> Response {
    let (status, Json(body)) = space_memory_core(
        &state,
        &user,
        &headers,
        &space_ref,
        OPERATION_DELETE,
        &memory_id,
        "",
    )
    .await;
    (status, Json(body)).into_response()
}

/// Both routes' work, so the flag, the Control decision and the typed errors exist once. The agent id
/// is read only by the list (`agent_subject_id`, EP-6 P3); the delete passes none.
async fn space_memory_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    operation: &'static str,
    memory_id: &str,
    agent_subject_id: &str,
) -> Reply {
    // 1. Rollout flag. A rollout control, never an authorization input.
    if !state.space_features.memory_ui {
        return reply(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "Space memory is not enabled.",
        );
    }
    // 2. Membership, then shape.
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return reply(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required for Space memory.",
        );
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return reply(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A valid Space reference is required.",
        );
    }
    let memory_id = memory_id.trim();
    if operation == OPERATION_DELETE && !valid_memory_id(memory_id) {
        return reply(
            StatusCode::BAD_REQUEST,
            "invalid_memory",
            "A valid memory id is required.",
        );
    }
    let agent_subject_id = agent_subject_id.trim();
    if !agent_subject_id.is_empty() && !valid_memory_id(agent_subject_id) {
        return reply(
            StatusCode::BAD_REQUEST,
            "invalid_agent",
            "A valid agent id is required.",
        );
    }

    // 3. Space lifecycle.
    match space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return reply(
                StatusCode::NOT_FOUND,
                "space_not_found",
                "Space is not available.",
            )
        }
        Err(()) => {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            )
        }
    }

    // 4. A fresh Control decision for this list or this row.
    let (decision_ref, token) =
        match space_memory_decision(state, user, &org_id, space_ref, operation, memory_id).await {
            Ok(decision) => decision,
            Err(reply) => return reply,
        };

    // 5. Delegated credentials, then model-gateway.
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
    let (url, mut body) = model_request(
        &state.model_gateway_url,
        space_ref,
        operation,
        memory_id,
        &decision_ref,
        &token,
    );
    if !agent_subject_id.is_empty() {
        body["agent_subject_id"] = json!(agent_subject_id);
    }
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(body),
        model_token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    relay_answer(status, &response, operation, memory_id)
}

/// `spaces.memory.list` (EP-8 S7-b): the work of `GET /api/v1/spaces/{space_ref}/memory`, behind the same
/// switch, membership, lifecycle and Control decision. The action names no agent, so it lists the Space's
/// own rows.
pub(in crate::domains) async fn list_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
) -> Reply {
    space_memory_core(state, user, headers, space_ref, OPERATION_LIST, "", "").await
}

/// `spaces.memory.delete` (EP-8 S7-b): the work of `DELETE /api/v1/spaces/{space_ref}/memory/{memory_id}`,
/// behind the same switch, membership, lifecycle and Control decision.
pub(in crate::domains) async fn delete_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    memory_id: &str,
) -> Reply {
    space_memory_core(
        state,
        user,
        headers,
        space_ref,
        OPERATION_DELETE,
        memory_id,
        "",
    )
    .await
}

/// The closed reasons a revocation erase did not complete (V3-sc-1). The two typed hold answers of
/// model-gateway stay apart: a legal hold is final until the hold is released, a hold-check outage is
/// retryable. Every other answer is `upstream`. Never an id, a count or content.
const REASON_LEGAL_HOLD: &str = "legal_hold";
const REASON_HOLD_CHECK: &str = "hold_check_unavailable";
const REASON_UPSTREAM: &str = "upstream";

/// What a revocation's erase of one agent's notes came to, as the revoke's answer reports it under
/// `agent_memory_erase` (V3d-bff-2). A closed word set, never an id, a count or content. `NotApplicable`
/// is the answer while the Space memory feature is off in this gateway: no erase is attempted there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentMemoryErase {
    /// The erase ran; the agent's notes in this Space are gone, or there were none.
    Erased,
    /// A legal hold covers the notes, so they were kept.
    Held,
    /// The notes were kept because the erase could not be made or did not complete.
    Unavailable,
    /// The Space memory feature is off, so no erase was attempted.
    NotApplicable,
}

impl AgentMemoryErase {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Erased => "erased",
            Self::Held => "held",
            Self::Unavailable => "unavailable",
            Self::NotApplicable => "not_applicable",
        }
    }
}

/// The pause before the one retry of a revocation erase whose hold check was unavailable (V3c-bff-2-rel1).
const ERASE_OUTAGE_RETRY_DELAY: Duration = Duration::from_secs(1);

/// Counts one failed revocation erase under its closed reason. Never an id, a count or content.
fn record_erase_failure(reason: &'static str) {
    metrics::counter!("verevon_space_agent_erase_failures_total", "reason" => reason).increment(1);
}

/// The closed reason of a failed revocation erase, read from the upstream's typed hold answer only (V3-sc-1,
/// V3d-bff-2). The upstream message and every other field are ignored, and a hold needs its `retryable` flag.
fn erase_failure_reason(response: &Value) -> &'static str {
    match hold_answer(response) {
        Some(HoldAnswer::LegalHold) => REASON_LEGAL_HOLD,
        Some(HoldAnswer::HoldCheckUnavailable) => REASON_HOLD_CHECK,
        None => REASON_UPSTREAM,
    }
}

/// Counts and logs one failed revocation erase by its upstream status and closed reason only. The
/// revoke's answer does not change.
fn note_erase_failure(status: StatusCode, reason: &'static str) {
    record_erase_failure(reason);
    tracing::warn!(%status, reason, "revoked agent memory erase did not complete");
}

/// How one pass of a revocation erase ended (V3c-bff-2-rel1). A skip (no Control decision, no
/// delegated credentials) is counted where it happens and is never retried. A refused erase carries
/// its closed reason to the caller, which decides whether the erase runs again.
#[derive(Debug, PartialEq, Eq)]
enum ErasePass {
    /// The erase ran; the count is the rows removed.
    Erased(u64),
    /// The erase did not run, and its skip was counted under its own reason.
    Skipped,
    /// Model-gateway refused the erase under a closed reason.
    Refused {
        status: StatusCode,
        reason: &'static str,
    },
}

impl ErasePass {
    /// A hold check that could not be made: nothing was erased, and the upstream says to try again.
    fn is_hold_check_outage(&self) -> bool {
        matches!(self, ErasePass::Refused { reason, .. } if *reason == REASON_HOLD_CHECK)
    }
}

/// Runs one erase pass, and runs it once more after [`ERASE_OUTAGE_RETRY_DELAY`] when its hold check
/// was unavailable (V3c-bff-2-rel1). The typed `erasure_hold_check_unavailable` says nothing was
/// erased, so the retry is safe, and the pass runs again whole so the retry mints a fresh Control
/// decision. A legal hold is never retried: it stands until it is released. Every other refusal and
/// every skip is final.
async fn run_erase_with_outage_retry<F, Fut>(delay: Duration, mut pass: F) -> ErasePass
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ErasePass>,
{
    let first = pass().await;
    if !first.is_hold_check_outage() {
        return first;
    }
    tracing::info!("revoked agent memory erase retried once after a hold-check outage");
    tokio::time::sleep(delay).await;
    pass().await
}

/// One pass of a revocation erase: a fresh Control decision for THIS agent, the delegated
/// credentials, then model-gateway's erase route. Each pass mints its own decision, since a decision
/// is never cached.
async fn erase_agent_memory_pass(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
    space_ref: &str,
    agent_subject_id: &str,
) -> ErasePass {
    let (decision_ref, token) = match space_memory_decision(
        state,
        user,
        org_id,
        space_ref,
        OPERATION_ERASE_AGENT,
        agent_subject_id,
    )
    .await
    {
        Ok(decision) => decision,
        Err((status, _)) => {
            tracing::warn!(%status, "revoked agent memory erase skipped: Control gave no decision");
            record_erase_failure("control_decision");
            return ErasePass::Skipped;
        }
    };
    let session_token =
        match crate::domains::chat::shared::required_session_token(state, user, headers).await {
            Ok(token) => token,
            Err(_) => {
                tracing::warn!(
                    "revoked agent memory erase skipped: delegated credentials unavailable"
                );
                record_erase_failure("credentials");
                return ErasePass::Skipped;
            }
        };
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
    let url = format!(
        "{}/v1/spaces/{}/agents/{}/memory/erase",
        state.model_gateway_url,
        urlencoding::encode(space_ref),
        urlencoding::encode(agent_subject_id),
    );
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_control_decision_ref": decision_ref,
            "space_control_decision_token": token,
        })),
        model_token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    if status.is_success() {
        // A count only, never content.
        let erased = response.get("erased").and_then(Value::as_u64).unwrap_or(0);
        return ErasePass::Erased(erased);
    }
    ErasePass::Refused {
        status,
        reason: erase_failure_reason(&response),
    }
}

/// Counts and logs the final outcome of one revocation erase, and returns what the revoke's answer reports
/// (V3d-bff-2). A success logs its row count only. A skip was counted where it happened, and the notes it
/// leaves behind read as `Unavailable`.
fn settle_erase_pass(pass: ErasePass) -> AgentMemoryErase {
    match pass {
        ErasePass::Erased(erased) => {
            tracing::info!(erased, "revoked agent memory erased");
            AgentMemoryErase::Erased
        }
        ErasePass::Skipped => AgentMemoryErase::Unavailable,
        ErasePass::Refused { status, reason } => {
            note_erase_failure(status, reason);
            if reason == REASON_LEGAL_HOLD {
                AgentMemoryErase::Held
            } else {
                AgentMemoryErase::Unavailable
            }
        }
    }
}

/// Best-effort erase of one named agent's memory once its binding is revoked (EP-6 P3, EP-8 S10).
///
/// The caller runs this only when `GATEWAY_SPACE_MEMORY_UI` is on (`SpaceFeatureFlags::memory_ui`,
/// dark by default). It runs after the revocation has landed and never changes whether the revoke
/// succeeds: any failure here is logged by status only, counted under a closed reason, and the revoke
/// still succeeds. Its outcome is returned so the revoke's answer can tell the admin what happened to
/// the notes (`agent_memory_erase`, V3d-bff-2). A legal hold (`erasure_blocked_legal_hold`) is counted
/// under its own reason and is not retried: it stands until it is released. A hold-check outage
/// (`erasure_hold_check_unavailable`) is retried once after [`ERASE_OUTAGE_RETRY_DELAY`] and counted only
/// if the retry fails too. Repeating the revoke runs this erase again, the other retry path. Nothing is
/// recorded durably: a held or still-failing erase is counted and logged only. Session Core is the
/// authority: it verifies the fresh Control decision issued for THIS agent and erases that agent's rows
/// in this Space, whatever its own agent switch says. Allowed under every retention posture: the call
/// removes rows and carries ids, never content, and only a row count is logged.
pub(super) async fn erase_agent_memory_after_revocation(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
    space_ref: &str,
    agent_subject_id: &str,
) -> AgentMemoryErase {
    let space_ref = space_ref.trim();
    let agent_subject_id = agent_subject_id.trim();
    if org_id.trim().is_empty() || !valid_space_ref(space_ref) || !valid_memory_id(agent_subject_id)
    {
        tracing::warn!("revoked agent memory erase skipped: no valid Space or agent subject");
        record_erase_failure("invalid_input");
        return AgentMemoryErase::Unavailable;
    }
    let pass = run_erase_with_outage_retry(ERASE_OUTAGE_RETRY_DELAY, || {
        erase_agent_memory_pass(state, user, headers, org_id, space_ref, agent_subject_id)
    })
    .await;
    settle_erase_pass(pass)
}

/// Asks Control for the `model.space.memory.manage` decision of this call. A refusal (a member
/// below admin, a personal Space, an older Control without the endpoint) is 403
/// `space_memory_not_authorized`, typed when Control gave a reason; anything else is an unavailable
/// Control, never a grant.
async fn space_memory_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    operation: &str,
    memory_id: &str,
) -> Result<(String, String), Reply> {
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/memory-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "operation": operation,
            "memory_id": memory_id,
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    let status = denial::refusal_status(status, &response);
    if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND {
        let body = match denial::SpaceDenial::from_response(state, status, &response) {
            Some(denial) => error_with_details(
                "space_memory_not_authorized",
                DENIED,
                denial.details_value(),
            ),
            None => error("space_memory_not_authorized", DENIED),
        };
        return Err((StatusCode::FORBIDDEN, Json(body)));
    }
    if !status.is_success() {
        return Err(reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_memory_unavailable",
            "Space memory is temporarily unavailable.",
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
            "Control returned an invalid Space memory decision.",
        ));
    }
    Ok((decision_ref, token))
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The browser's answer to a model-gateway answer: the success shape rebuilt field by field, the
/// closed refusal codes in fixed words, everything else `space_memory_unavailable`. Upstream text
/// and decision material never cross.
fn relay_answer(status: StatusCode, response: &Value, operation: &str, memory_id: &str) -> Reply {
    if status.is_success() {
        if operation == OPERATION_DELETE {
            return (
                StatusCode::OK,
                Json(json!({"data": {
                    "memory_id": memory_id,
                    "deleted": response.get("deleted").and_then(Value::as_bool).unwrap_or(false),
                }})),
            );
        }
        let memories: Vec<Value> = response
            .get("memories")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .map(|row| {
                        json!({
                            "memory_id": text(row, "memory_id"),
                            "content": text(row, "content"),
                            "updated_at": text(row, "updated_at"),
                            "provenance": text(row, "provenance"),
                            "source_label": text(row, "source_label"),
                            "thread_id": text(row, "thread_id"),
                            "withheld": !text(row, "withheld_reason").is_empty(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let withheld = memories
            .iter()
            .filter(|row| row["withheld"].as_bool().unwrap_or(false))
            .count();
        return (
            StatusCode::OK,
            Json(json!({"data": {"memories": memories, "withheld": withheld}})),
        );
    }
    let typed = response.get("error");
    let code = typed
        .and_then(|detail| detail.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let feature = typed
        .and_then(|detail| detail.get("feature"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    // A zero-retention refusal of one named agent's notes keeps the catalogue code and feature, so the
    // browser shows the zdr-refusals copy for it (EP-6 P3). Model-gateway's status is 412 for it.
    if code == ZDR_FEATURE_NOT_SUPPORTED && feature == NAMED_AGENT_FEATURE {
        return (StatusCode::PRECONDITION_FAILED, Json(named_agent_refusal()));
    }
    // A legal hold on a delete, or a hold check that could not be made, keeps its typed code and retryable
    // flag (V3d-bff-2). Model-gateway sends both; a hold code without its flag stays the generic answer.
    if let Some(hold) = hold_answer(response) {
        return hold_reply(hold);
    }
    match code {
        "space_memory_not_authorized" => reply(StatusCode::FORBIDDEN, "space_memory_not_authorized", DENIED),
        "memory_not_found" => reply(
            StatusCode::NOT_FOUND,
            "memory_not_found",
            "The memory is not available.",
        ),
        _ => reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_memory_unavailable",
            "Space memory is temporarily unavailable.",
        ),
    }
}

/// A typed hold answer for a Space memory delete, in fixed words with its retryable flag (V3d-bff-2). A hold
/// is final until it is released (409, not retryable); a hold-check outage is retryable (503). Nothing upstream
/// crosses.
fn hold_reply(hold: HoldAnswer) -> Reply {
    match hold {
        HoldAnswer::LegalHold => (
            StatusCode::CONFLICT,
            Json(error_with_details(
                "erasure_blocked_legal_hold",
                "A legal hold covers this data, so it was not deleted. It can be deleted once the hold is released.",
                json!({"retryable": false}),
            )),
        ),
        HoldAnswer::HoldCheckUnavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error_with_details(
                "erasure_hold_check_unavailable",
                "Legal holds could not be checked, so nothing was deleted. Try again shortly.",
                json!({"retryable": true}),
            )),
        ),
    }
}

/// The catalogue refusal for one named agent's notes under Zero Data Retention (EP-6 P3): the Model
/// Plane's `{error:{code,message,retryable,action,feature}}` shape with its fixed English words. The
/// browser renders its own Norwegian or English copy of the same code and feature.
fn named_agent_refusal() -> Value {
    json!({"error": {
        "code": ZDR_FEATURE_NOT_SUPPORTED,
        "message": "This is not supported with zero data retention yet: named agent memory. The request was refused before any model call.",
        "retryable": false,
        "action": "turn_off_feature",
        "feature": NAMED_AGENT_FEATURE,
    }})
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// The Space is resolved server side: the model-gateway call is built from the path
    /// `space_ref` alone, and the decision rides in the body, never the URL.
    #[test]
    fn space_memory_route_resolves_space_server_side() {
        let (url, body) = model_request(
            "http://mg",
            "space a/b",
            OPERATION_LIST,
            "",
            "ref-1",
            "token-1",
        );
        assert_eq!(url, "http://mg/v1/spaces/space%20a%2Fb/memory/list");
        assert!(!url.contains("token-1") && !url.contains("ref-1"));
        assert_eq!(
            body,
            json!({"space_control_decision_ref": "ref-1", "space_control_decision_token": "token-1"})
        );
        let (url, body) = model_request("http://mg", "space-a", OPERATION_DELETE, "mem-1", "r", "t");
        assert_eq!(url, "http://mg/v1/spaces/space-a/memory/delete");
        assert_eq!(body["memory_id"], "mem-1");
        assert!(body.get("space_id").is_none(), "no client space id is ever forwarded");
    }

    #[test]
    fn space_memory_ids_are_validated() {
        assert!(valid_space_ref("space-1"));
        assert!(!valid_space_ref(""));
        assert!(!valid_space_ref("space 1"));
        assert!(valid_memory_id("01JABCDEF"));
        assert!(!valid_memory_id("../x"));
        assert!(!valid_memory_id(&"a".repeat(65)));
    }

    #[test]
    fn space_memory_answer_is_rebuilt_field_by_field() {
        let upstream = json!({
            "memories": [
                {"memory_id": "m1", "content": "Bergen on Fridays", "updated_at": "2026-11-03T10:00:00Z",
                 "provenance": "stated", "source_label": "room_statement", "thread_id": "t1",
                 "withheld_reason": "audience_changed", "secret": "never"},
                {"memory_id": "m2", "content": "Oslo", "updated_at": "", "provenance": "unknown",
                 "source_label": "member_save", "thread_id": "t2"}
            ],
            "withheld": 1,
            "degradation_reason": "internal detail"
        });
        let (status, Json(body)) = relay_answer(StatusCode::OK, &upstream, OPERATION_LIST, "");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["withheld"], 1);
        assert_eq!(body["data"]["memories"][0]["withheld"], true);
        assert_eq!(body["data"]["memories"][1]["withheld"], false);
        assert!(!body.to_string().contains("never") && !body.to_string().contains("internal detail"));

        let refused = json!({"error": {"code": "space_memory_not_authorized", "message": "upstream words"}});
        let (status, Json(body)) = relay_answer(StatusCode::FORBIDDEN, &refused, OPERATION_LIST, "");
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!body.to_string().contains("upstream words"));
        assert!(body["error"]["details"].is_object() || body["error"].get("details").is_none());
        let (status, _) = relay_answer(StatusCode::BAD_GATEWAY, &json!({}), OPERATION_DELETE, "m1");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn a_revocation_erase_failure_classifies_by_its_typed_code() {
        // The typed code and its retryable flag decide. The upstream message is never read, whatever it says.
        let held = json!({"error": {
            "code": "erasure_blocked_legal_hold",
            "message": "hold lh_0123 on org-9",
            "details": {"retryable": false},
        }});
        let outage = json!({"error": {
            "code": "erasure_hold_check_unavailable",
            "message": "org-core down",
            "details": {"retryable": true},
        }});
        assert_eq!(erase_failure_reason(&held), REASON_LEGAL_HOLD);
        assert_eq!(erase_failure_reason(&outage), REASON_HOLD_CHECK);
        // Every other answer is the generic reason, whatever its status and words.
        for other in [
            json!({"error": {"code": "space_memory_not_authorized", "message": "denied"}}),
            json!({"error": {"code": "memory_not_found", "message": "gone"}}),
            json!({"error": {
                "code": "space_memory_unavailable",
                "message": "erasure_blocked_legal_hold",
            }}),
            json!({"error": "erasure_blocked_legal_hold"}),
            json!({"error": {"message": "erasure_hold_check_unavailable"}}),
            json!({}),
        ] {
            assert_eq!(erase_failure_reason(&other), REASON_UPSTREAM, "{other}");
        }
    }

    /// V3d-bff-2: the BFF classifies a legal hold as the Convex copy does (`isOwnerLegalHoldRefusal`): the exact
    /// code with `details.retryable: false`. The code alone is not a hold, and neither is a hold code with the
    /// outage's flag. The same table is pinned on the Convex side in test/space-deletion-hold-classification.test.cjs
    /// and test/space-deletion-model.test.cjs ("the hold shape is exact").
    #[test]
    fn a_legal_hold_needs_the_code_and_its_retryable_false() {
        let body = |code: &str, details: Option<Value>| {
            let mut error = json!({"code": code, "message": "upstream-detail"});
            if let Some(details) = details {
                error["details"] = details;
            }
            json!({ "error": error })
        };
        let held = |details: Option<Value>| body("erasure_blocked_legal_hold", details);
        assert_eq!(
            hold_answer(&held(Some(json!({"retryable": false})))),
            Some(HoldAnswer::LegalHold)
        );
        assert_eq!(
            hold_answer(&held(None)),
            None,
            "the code alone is not a hold"
        );
        assert_eq!(hold_answer(&held(Some(json!({"retryable": true})))), None);
        assert_eq!(
            hold_answer(&held(Some(json!({"retryable": "false"})))),
            None,
            "a string is not false"
        );
        assert_eq!(hold_answer(&held(Some(json!({})))), None);
        assert_eq!(
            hold_answer(&held(Some(json!(false)))),
            None,
            "details must be an object"
        );
        let outage = |details: Option<Value>| body("erasure_hold_check_unavailable", details);
        assert_eq!(
            hold_answer(&outage(Some(json!({"retryable": true})))),
            Some(HoldAnswer::HoldCheckUnavailable)
        );
        assert_eq!(hold_answer(&outage(None)), None);
        assert_eq!(
            hold_answer(&outage(Some(json!({"retryable": false})))),
            None
        );
        assert_eq!(erase_failure_reason(&held(None)), REASON_UPSTREAM);
        assert_eq!(erase_failure_reason(&outage(None)), REASON_UPSTREAM);
    }

    /// V3d-bff-2: a settled erase reports the outcome the revoke's answer carries.
    #[test]
    fn a_settled_erase_reports_the_outcome_the_revoke_answers_with() {
        assert_eq!(
            settle_erase_pass(ErasePass::Erased(0)),
            AgentMemoryErase::Erased
        );
        assert_eq!(
            settle_erase_pass(ErasePass::Erased(4)),
            AgentMemoryErase::Erased
        );
        assert_eq!(
            settle_erase_pass(ErasePass::Skipped),
            AgentMemoryErase::Unavailable
        );
        assert_eq!(settle_erase_pass(held_pass()), AgentMemoryErase::Held);
        assert_eq!(
            settle_erase_pass(outage_pass()),
            AgentMemoryErase::Unavailable
        );
        assert_eq!(
            settle_erase_pass(other_refusal_pass()),
            AgentMemoryErase::Unavailable
        );
        assert_eq!(AgentMemoryErase::Erased.as_str(), "erased");
        assert_eq!(AgentMemoryErase::Held.as_str(), "held");
        assert_eq!(AgentMemoryErase::Unavailable.as_str(), "unavailable");
        assert_eq!(AgentMemoryErase::NotApplicable.as_str(), "not_applicable");
    }

    /// V3d-bff-2: a held delete keeps the typed code and its flag, and an outage keeps its own. A hold code
    /// without its flag is the generic answer, as the classifier reads it.
    #[test]
    fn a_held_delete_keeps_the_typed_hold_answer_and_its_flag() {
        let held = json!({"error": {
            "code": "erasure_blocked_legal_hold",
            "message": "hold lh_0123 on org-9",
            "details": {"retryable": false},
        }});
        let (status, Json(body)) =
            relay_answer(StatusCode::CONFLICT, &held, OPERATION_DELETE, "m1");
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "erasure_blocked_legal_hold");
        assert_eq!(body["error"]["details"]["retryable"], false);
        assert!(!body.to_string().contains("lh_0123"), "{body}");

        let outage = json!({"error": {
            "code": "erasure_hold_check_unavailable",
            "message": "org-core down",
            "details": {"retryable": true},
        }});
        let (status, Json(body)) = relay_answer(
            StatusCode::SERVICE_UNAVAILABLE,
            &outage,
            OPERATION_DELETE,
            "m1",
        );
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "erasure_hold_check_unavailable");
        assert_eq!(body["error"]["details"]["retryable"], true);
        assert!(!body.to_string().contains("org-core"), "{body}");

        let bare = json!({"error": {"code": "erasure_blocked_legal_hold", "message": "x"}});
        let (status, Json(body)) =
            relay_answer(StatusCode::CONFLICT, &bare, OPERATION_DELETE, "m1");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "space_memory_unavailable");
    }

    #[test]
    fn a_held_erase_and_an_outage_are_counted_apart() {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let _guard = metrics::set_default_local_recorder(&recorder);
        let held = json!({"error": {
            "code": "erasure_blocked_legal_hold",
            "message": "held",
            "details": {"retryable": false},
        }});
        let outage = json!({"error": {
            "code": "erasure_hold_check_unavailable",
            "message": "down",
            "details": {"retryable": true},
        }});
        note_erase_failure(StatusCode::CONFLICT, erase_failure_reason(&held));
        note_erase_failure(
            StatusCode::SERVICE_UNAVAILABLE,
            erase_failure_reason(&outage),
        );
        for (status, body) in [
            (
                StatusCode::FORBIDDEN,
                json!({"error": {"code": "space_memory_not_authorized"}}),
            ),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error": {"code": "space_memory_unavailable"}}),
            ),
            (
                StatusCode::GATEWAY_TIMEOUT,
                json!({"error": {"code": "model_request_timeout"}}),
            ),
        ] {
            note_erase_failure(status, erase_failure_reason(&body));
        }
        let rendered = handle.render();
        let row = |reason: &str, count: u64| {
            format!("verevon_space_agent_erase_failures_total{{reason=\"{reason}\"}} {count}")
        };
        assert!(
            rendered
                .lines()
                .any(|line| line == row(REASON_LEGAL_HOLD, 1)),
            "{rendered}"
        );
        assert!(
            rendered
                .lines()
                .any(|line| line == row(REASON_HOLD_CHECK, 1)),
            "{rendered}"
        );
        assert!(
            rendered.lines().any(|line| line == row(REASON_UPSTREAM, 3)),
            "{rendered}"
        );
    }

    #[test]
    fn a_revocation_erase_failure_logs_only_fixed_words() {
        let capture = crate::zdr_log::test_capture::Capture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .finish();
        let held = json!({"error": {
            "code": "erasure_blocked_legal_hold",
            "message": "hold lh_0123 on org-9",
            "details": {"retryable": false},
        }});
        let outage = json!({"error": {
            "code": "erasure_hold_check_unavailable",
            "message": "org-core down",
            "details": {"retryable": true},
        }});
        tracing::subscriber::with_default(subscriber, || {
            note_erase_failure(StatusCode::CONFLICT, erase_failure_reason(&held));
            note_erase_failure(
                StatusCode::SERVICE_UNAVAILABLE,
                erase_failure_reason(&outage),
            );
        });
        let log = capture.text();
        assert!(log.contains("legal_hold"), "{log}");
        assert!(log.contains("hold_check_unavailable"), "{log}");
        assert!(log.contains("409 Conflict"), "{log}");
        assert!(log.contains("503 Service Unavailable"), "{log}");
        for upstream_words in ["lh_0123", "org-9", "org-core"] {
            assert!(
                !log.contains(upstream_words),
                "upstream text reached the log: {log}"
            );
        }
    }

    /// A hold-check outage: nothing was erased, and the upstream says to try again.
    fn outage_pass() -> ErasePass {
        ErasePass::Refused {
            status: StatusCode::SERVICE_UNAVAILABLE,
            reason: REASON_HOLD_CHECK,
        }
    }

    /// A legal hold: final until the hold is released.
    fn held_pass() -> ErasePass {
        ErasePass::Refused {
            status: StatusCode::CONFLICT,
            reason: REASON_LEGAL_HOLD,
        }
    }

    /// Any other refusal, which is final.
    fn other_refusal_pass() -> ErasePass {
        ErasePass::Refused {
            status: StatusCode::SERVICE_UNAVAILABLE,
            reason: REASON_UPSTREAM,
        }
    }

    /// Runs the retry rule over scripted passes, each answering with the next script entry. A call past
    /// the script panics, so each test also pins how many passes ran. Returns the outcome and that count.
    async fn run_scripted(script: Vec<ErasePass>) -> (ErasePass, usize) {
        let calls = std::cell::Cell::new(0_usize);
        let script = std::cell::RefCell::new(script.into_iter());
        let outcome = run_erase_with_outage_retry(Duration::ZERO, || {
            calls.set(calls.get() + 1);
            let next = script
                .borrow_mut()
                .next()
                .expect("no erase pass beyond the script");
            std::future::ready(next)
        })
        .await;
        (outcome, calls.get())
    }

    #[tokio::test]
    async fn a_hold_check_outage_is_retried_once_and_the_retry_is_the_answer() {
        let (outcome, passes) = run_scripted(vec![outage_pass(), ErasePass::Erased(2)]).await;
        assert_eq!(outcome, ErasePass::Erased(2));
        assert_eq!(passes, 2);
    }

    #[tokio::test]
    async fn an_outage_that_persists_is_retried_only_once() {
        let (outcome, passes) = run_scripted(vec![outage_pass(), outage_pass()]).await;
        assert_eq!(outcome, outage_pass());
        assert_eq!(passes, 2);
    }

    #[tokio::test]
    async fn a_legal_hold_is_never_retried() {
        let (outcome, passes) = run_scripted(vec![held_pass()]).await;
        assert_eq!(outcome, held_pass());
        assert_eq!(passes, 1);
    }

    #[tokio::test]
    async fn a_skip_or_any_other_refusal_is_never_retried() {
        let (outcome, passes) = run_scripted(vec![ErasePass::Skipped]).await;
        assert_eq!(outcome, ErasePass::Skipped);
        assert_eq!(passes, 1);
        let (outcome, passes) = run_scripted(vec![other_refusal_pass()]).await;
        assert_eq!(outcome, other_refusal_pass());
        assert_eq!(passes, 1);
    }

    #[tokio::test]
    async fn a_retried_outage_is_counted_only_by_its_final_answer() {
        // The first outage is not counted: only the answer that stands is.
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let _guard = metrics::set_default_local_recorder(&recorder);
        let (held_after_outage, _) = run_scripted(vec![outage_pass(), held_pass()]).await;
        settle_erase_pass(held_after_outage);
        let (erased_after_outage, _) =
            run_scripted(vec![outage_pass(), ErasePass::Erased(1)]).await;
        settle_erase_pass(erased_after_outage);
        let rendered = handle.render();
        let row = |reason: &str, count: u64| {
            format!("verevon_space_agent_erase_failures_total{{reason=\"{reason}\"}} {count}")
        };
        assert!(
            rendered
                .lines()
                .any(|line| line == row(REASON_LEGAL_HOLD, 1)),
            "{rendered}"
        );
        assert!(
            !rendered
                .lines()
                .any(|line| line.contains(REASON_HOLD_CHECK)),
            "{rendered}"
        );
    }

    // ----- the revoke answer (V3d-bff-2) ------------------------------------------------------

    /// One agent call through the real router, against mock Auth, Control, Application (Convex) and Model
    /// Gateway. `binding_status` is what Application's binding mutation answers; `erase` is what model-gateway's
    /// erase route answers. Returns the status, the body and how many erase calls reached model-gateway.
    async fn agent_call(
        method: &str,
        json_body: Option<Value>,
        memory_ui: bool,
        binding_status: &str,
        erase: ResponseTemplate,
    ) -> (u16, Value, usize) {
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
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})),
                )
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": "owner", "subject_id": "user-1"}
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/memory-decision"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"decision": {"decision_ref": "decision-1"}, "token": "decision-token"}
            })))
            .mount(&user_core)
            .await;

        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": {
                "bindingRef": "sab-1", "subjectId": "agent-7", "status": binding_status, "changed": true
            }})))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/action"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"value": {"status": "applied"}})),
            )
            .mount(&application)
            .await;

        let model_gateway = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/spaces/space-room/agents/agent-7/memory/erase"))
            .respond_with(erase)
            .mount(&model_gateway)
            .await;

        let mut state = crate::tests::test_state(false);
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model_gateway.uri();
        state.space_features.memory_ui = memory_ui;

        let mut request = Request::builder()
            .method(method)
            .uri("/api/v1/spaces/space-room/agents/sab-1")
            .header("cookie", "better-auth.session_token=session-1");
        let body = match json_body {
            Some(value) => {
                request = request.header("content-type", "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let response = crate::build_router(state)
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let answer: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        let erase_calls = model_gateway.received_requests().await.unwrap().len();
        (status, answer, erase_calls)
    }

    /// A legal hold on the erase: the revoke still succeeds, names the held notes, and does not retry.
    #[tokio::test]
    async fn a_revoke_names_a_held_erase() {
        let held = ResponseTemplate::new(409).set_body_json(json!({"error": {
            "code": "erasure_blocked_legal_hold",
            "message": "hold lh_0123 on org-9",
            "details": {"retryable": false},
        }}));
        let (status, body, erase_calls) = agent_call("DELETE", None, true, "revoked", held).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(erase_calls, 1, "a legal hold is not retried");
        assert_eq!(
            body["data"],
            json!({"binding_ref": "sab-1", "status": "revoked", "changed": true, "agent_memory_erase": "held"})
        );
        assert!(!body.to_string().contains("lh_0123"), "{body}");
    }

    /// An erase that ran names `erased`; a generic refusal is final and names `unavailable`.
    #[tokio::test]
    async fn a_revoke_names_an_erase_that_ran_and_one_that_failed() {
        let ran = ResponseTemplate::new(200).set_body_json(json!({"erased": 3}));
        let (status, body, _) = agent_call("DELETE", None, true, "revoked", ran).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["agent_memory_erase"], "erased");

        let refused = ResponseTemplate::new(500).set_body_json(json!({"error": {
            "code": "space_memory_unavailable",
            "message": "upstream-words",
        }}));
        let (status, body, erase_calls) =
            agent_call("DELETE", None, true, "revoked", refused).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(erase_calls, 1, "a generic refusal is not retried");
        assert_eq!(body["data"]["agent_memory_erase"], "unavailable");
        assert!(!body.to_string().contains("upstream-words"), "{body}");
    }

    /// With the Space memory feature off no erase is attempted, and the answer says so.
    #[tokio::test]
    async fn a_revoke_says_not_applicable_while_the_memory_feature_is_off() {
        let ran = ResponseTemplate::new(200).set_body_json(json!({"erased": 0}));
        let (status, body, erase_calls) = agent_call("DELETE", None, false, "revoked", ran).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(erase_calls, 0, "no erase is attempted with the feature off");
        assert_eq!(body["data"]["agent_memory_erase"], "not_applicable");
    }

    /// Pause and resume never erase, and their answer keeps the three fields it always had.
    #[tokio::test]
    async fn a_pause_answer_carries_no_erase_field() {
        let (status, body, erase_calls) = agent_call(
            "PATCH",
            Some(json!({"status": "paused"})),
            true,
            "paused",
            ResponseTemplate::new(200),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(erase_calls, 0);
        assert_eq!(
            body["data"],
            json!({"binding_ref": "sab-1", "status": "paused", "changed": true})
        );
    }
}
