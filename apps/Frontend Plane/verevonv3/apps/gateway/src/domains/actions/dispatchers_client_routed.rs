//! Dispatchers for registry actions the browser does not run through
//! `POST /api/v1/actions/execute`.
//!
//! The registry declares who runs each action (`executor`, O-26). The generated contract
//! `action-executors.json` carries that answer, and the gateway reads it here, so the client-routed
//! ids need no arm of their own in `handlers.rs`.
//!
//! - `client`: the action runs on its own REST route or inside the page (the browser calls that route
//!   directly), so the execute endpoint refuses it with `client_executed` and names the route. Nothing
//!   is queued, stored or sent.
//! - `agents.admitted_actions.list` is a real arm. It reads the answer `GET
//!   /api/v1/agents/admitted-actions` serves, through the same gateway function, so the capability
//!   bearer, the organisation from the session and the Zero Data Retention refusal exist once.

use std::{collections::HashMap, sync::OnceLock};

use axum::{
    extract::{Extension as ExtensionExtractor, State as StateExtractor},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use super::dispatchers::{owner_json_response_to_envelope, response_to_status_and_json};
use crate::{
    config::AppState, domains::agent_actions::list_admitted_owner_actions,
    envelope::error_with_details, middleware::AuthenticatedUser,
};

/// The generated executor contract (O-26): one row per registry action with its `executor`, and for a
/// client action the `clientRoute` it runs on. The action-surface contract test checks it against the
/// registry.
const ACTION_EXECUTORS_JSON: &str = include_str!("action-executors.json");

/// The route a client-routed registry action runs on: a REST path with `{placeholders}`, or `ui:<surface>`
/// for an action that only changes what the page shows. `None` for an action the gateway runs and for an
/// id the registry does not know.
pub(super) fn client_route_for(action_id: &str) -> Option<&'static str> {
    static CLIENT_ROUTES: OnceLock<HashMap<String, String>> = OnceLock::new();
    CLIENT_ROUTES
        .get_or_init(|| {
            let contract: Value = serde_json::from_str(ACTION_EXECUTORS_JSON)
                .expect("action-executors.json is valid JSON");
            contract["actions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| row["executor"] == "client")
                .filter_map(|row| {
                    Some((
                        row["id"].as_str()?.to_owned(),
                        row["clientRoute"].as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .get(action_id)
        .map(String::as_str)
}

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
