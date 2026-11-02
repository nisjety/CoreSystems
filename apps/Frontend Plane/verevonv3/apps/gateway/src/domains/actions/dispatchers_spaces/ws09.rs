//! WS09-S8: the `spaces.stop_run` action dispatcher. It rides the same function as `POST
//! /api/v1/spaces/{space_ref}/runs/{run_id}/stop` in `domains/spaces/run_live.rs`
//! (`stop_run_core`), so the rollout flag, the Zero Data Retention refusal, the Control
//! `model.run.control` decision and the typed errors exist once. The id is not model-executable
//! (`model-access.ts` closes it): a model stopping a human's run is an authority inversion.

use axum::{http::HeaderMap, response::Response};
use serde_json::{Map, Value};

use crate::config::AppState;
use crate::domains::spaces::run_live::stop_run_core;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_stop_run(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let run_id = ticket_string(input, "runId");
    if space_ref.is_empty() || run_id.is_empty() {
        return ticket_bad_request("spaces.stop_run requires a non-empty 'spaceRef' and 'runId'");
    }
    let mut body = Map::new();
    if let Some(value) = input.get("idempotencyKey") {
        body.insert("idempotency_key".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) = stop_run_core(
        state,
        user,
        Some(headers),
        space_ref,
        run_id,
        &Value::Object(body),
    )
    .await;
    owner_json_response_to_envelope("spaces.stop_run", user, status, resp)
}
