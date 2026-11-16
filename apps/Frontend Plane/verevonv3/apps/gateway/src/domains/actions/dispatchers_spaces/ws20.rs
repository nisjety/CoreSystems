//! WS20-S5: the room browser run start action, `spaces.browser_run.start`. It rides the same core function as its REST
//! route in `domains/spaces/browser_runs.rs` (R1), so the rollout flag, the zero-retention refusal, the Space lifecycle,
//! Control's decisions and model-gateway's admission exist once. The action is not model-executable
//! (`model-access.ts` closes it): an agent starts no room browser run in v1 (D-WS20-5).

use axum::{http::HeaderMap, response::Response, Json};
use serde_json::{Map, Value};

use crate::config::AppState;
use crate::domains::spaces::browser_runs::submit_start;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

/// The action's camelCase input names, and the REST body names they map to. Nothing else of the input is forwarded.
const START_FIELDS: &[(&str, &str)] = &[
    ("threadId", "thread_id"),
    ("goal", "goal"),
    ("allowedDomains", "allowed_domains"),
    ("startUrl", "start_url"),
    ("maxSteps", "max_steps"),
    ("maxRuntimeSeconds", "max_runtime_s"),
    ("shareView", "share_view"),
    ("clientSendId", "client_send_id"),
];

pub(in crate::domains::actions) async fn dispatch_spaces_browser_run_start(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.browser_run.start requires a non-empty 'spaceRef'");
    }
    let body = start_body(input);
    let (status, Json(payload)) = submit_start(state, user, headers, space_ref, &body).await;
    owner_json_response_to_envelope("spaces.browser_run.start", user, status, payload)
}

/// The REST body the action's input names: the same fields under their REST names.
fn start_body(input: &Value) -> Value {
    let mut body = Map::new();
    for (action_name, rest_name) in START_FIELDS {
        if let Some(value) = input.get(*action_name) {
            body.insert((*rest_name).to_owned(), value.clone());
        }
    }
    Value::Object(body)
}
