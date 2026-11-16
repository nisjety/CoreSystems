//! WS17-S8: the per-turn personal-memory opt-in, gateway side (build plan WS17-S8; design WS17 D6).
//!
//! A room turn may use the signed-in person's own personal memory for that one answer, and only when the person asked
//! on this turn. The browser sends `use_personal_memory: true` in the chat body. Behind
//! [`super::inject_personal_thread_context_for_wake`], this module turns that request into a Control decision:
//!
//! - the request flag is removed from the body on every turn, and only an exact `true` asks;
//! - a turn with no Space context, or a room turn with no existing thread, gets nothing: there is no room thread to
//!   recall into;
//! - the rollout flag `SPACE_PERSONAL_MEMORY_TURN_API` off answers nothing, so the turn runs without personal memory;
//! - Zero Data Retention refuses the turn with the typed 412 `zdr_durable_mutation_forbidden` before any Control call;
//! - Control's `memory-personal-context-decision` (scope `spaces:issue`, under the caller's own delegation) mints the
//!   per-turn decision: a refusal is 403 `personal_memory_not_authorized`, an outage or unusable answer is 503
//!   `space_decision_unavailable`;
//! - the decision's token and ref go into the turn's `space_context` as `personal_memory_turn`, which model-gateway
//!   forwards to Session Core. The browser never supplies that field (it is a forged authority field).
//!
//! This module stores nothing and decides nothing about authority. Control decides who may use personal memory in
//! this room; Session Core verifies the decision, the owner and the thread again before it returns a single row.

use axum::{http::StatusCode, Json};
use reqwest::Method;
use serde_json::{json, Value};

use super::denial;
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::proxy_json,
    zdr::durable_write_refusal,
};

/// The body flag the browser sends. Only an exact `true` asks; any other value is ignored.
const REQUEST_FIELD: &str = "use_personal_memory";
/// The field, inside `space_context`, that carries the minted decision to model-gateway.
const TURN_FIELD: &str = "personal_memory_turn";
/// The one fixed refusal message for every refusal of the decision.
const DENIED: &str = "Personal memory use in this Space is not authorized";

type Reply = (StatusCode, Json<Value>);

/// Adds the per-turn personal-memory decision to a room turn's `space_context` when the person asked for it.
/// The request flag is always removed. `Ok` means the turn may go on, with or without personal memory; `Err` is a
/// typed refusal the turn returns unchanged.
pub(super) async fn inject_for_turn(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    body: &mut Value,
) -> Result<(), Reply> {
    let Some(object) = body.as_object_mut() else {
        return Ok(());
    };
    let asked = object.remove(REQUEST_FIELD).and_then(|value| value.as_bool()) == Some(true);
    if !asked || !state.space_features.space_personal_memory_turn_api {
        return Ok(());
    }
    let space_ref = object
        .get("space_context")
        .and_then(|context| context.get("space_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let thread_id = object
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let (Some(space_ref), Some(thread_id)) = (space_ref, thread_id) else {
        return Ok(());
    };
    // Zero Data Retention first, before any Control call: no durable personal memory exists under ZDR.
    if let Some(refusal) = durable_write_refusal(state, user, None, None).await {
        return Err(refusal);
    }
    let (decision_ref, token) = mint(state, user, org_id, &space_ref, &thread_id).await?;
    if let Some(context) = object
        .get_mut("space_context")
        .and_then(Value::as_object_mut)
    {
        context.insert(
            TURN_FIELD.to_owned(),
            json!({ "decision_ref": decision_ref, "decision_token": token }),
        );
    }
    Ok(())
}

/// Asks Control for one per-turn decision under the caller's own delegation. A refusal (403, or a 404 from a Control
/// without the endpoint, read as a refusal as the sibling decisions do) is 403 `personal_memory_not_authorized`, typed
/// when Control gave a reason. An outage, a 5xx or an unusable answer is 503 `space_decision_unavailable`. Nothing here
/// grants. The idempotency key is random per turn, so a retry of this turn never reuses another turn's decision.
async fn mint(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    thread_id: &str,
) -> Result<(String, String), Reply> {
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/memory-personal-context-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "thread_id": thread_id,
            "idempotency_key": format!("space-personal-memory-{:016x}", rand::random::<u64>()),
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
                "personal_memory_not_authorized",
                DENIED,
                denial.details_value(),
            ),
            None => error("personal_memory_not_authorized", DENIED),
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

fn decision_unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "space_decision_unavailable",
            "The Space decision is unavailable.",
        )),
    )
}
