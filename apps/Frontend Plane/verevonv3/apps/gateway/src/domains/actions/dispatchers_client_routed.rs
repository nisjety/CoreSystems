//! FU-02 (D-F05-5): dispatcher arms for registry actions the browser does not run through
//! `POST /api/v1/actions/execute`. The action-surface contract wants an arm for every registry id, so
//! each of these answers a typed refusal instead of falling through to the 501 of an unknown
//! action. None of the three paths here has a side effect of its own.
//!
//! - `client_executed`: the action runs on its own REST route or inside the page (the browser calls
//!   that route directly), so the execute endpoint refuses it and names the route. D-f09-5 is the
//!   long-term replacement: an executor field on the registry row instead of a refusal per id.
//! - `owner_not_wired`: the browser does send the id here, but the owner plane's executor for it is
//!   not built yet, so the refusal is honest rather than a made-up success.
//! - `agents.admitted_actions.list` is a real arm. It reads the answer `GET
//!   /api/v1/agents/admitted-actions` serves, through the same gateway function, so the capability
//!   bearer, the organisation from the session and the Zero Data Retention refusal exist once.

use axum::{
    extract::{Extension as ExtensionExtractor, State as StateExtractor},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::{
    config::AppState,
    domains::agent_actions::list_admitted_owner_actions,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
};

use super::dispatchers::{owner_json_response_to_envelope, response_to_status_and_json};

/// Refuses an action that runs on the client, naming the client route. `route` is a REST path with
/// `{placeholders}`, or `ui:<surface>` for an action that only changes what the page shows.
pub(super) async fn dispatch_client_executed(action_id: &str, route: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(error_with_details(
            "client_executed",
            format!(
                "{action_id} is run by the client on {route}; it is not executed through /api/v1/actions/execute."
            ),
            json!({ "actionId": action_id, "clientRoute": route }),
        )),
    )
        .into_response()
}

/// Refuses an action the browser sends to the execute endpoint but whose owner executor is not
/// wired in the gateway yet. Nothing is queued, stored or sent.
pub(super) async fn dispatch_owner_not_wired(action_id: &str) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(error(
            "owner_not_wired",
            format!("{action_id} has no owner executor behind the gateway yet; nothing was done."),
        )),
    )
        .into_response()
}

/// `agents.admitted_actions.list`: the owner actions this organization admits for agent runs. The
/// input is the empty object; the organization comes from the verified session.
pub(super) async fn dispatch_agents_admitted_actions_list(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Response {
    let response = list_admitted_owner_actions(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("agents.admitted_actions.list", user, status, payload)
}
