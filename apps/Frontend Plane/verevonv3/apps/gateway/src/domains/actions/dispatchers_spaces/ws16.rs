//! WS16-S8: action dispatcher for the personal sidebar layout (`spaces.update_sidebar`). It rides
//! the same function as `PUT /api/v1/spaces/sidebar`, so the flag, the Zero Data Retention refusal,
//! the compare-and-set and the typed errors exist once. The id is human-only (never
//! model-executable): it is one person's own layout, and an agent has no sidebar to arrange.

use axum::{http::HeaderMap, response::Response};
use serde_json::{json, Value};

use crate::config::AppState;
use crate::domains::spaces::sidebar::save_sidebar;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{owner_json_response_to_envelope, ticket_bad_request};

pub(in crate::domains::actions) async fn dispatch_spaces_update_sidebar(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    if input.get("baseRevision").is_none() || input.get("doc").is_none() {
        return ticket_bad_request("spaces.update_sidebar requires 'baseRevision' and 'doc'");
    }
    // The save flow validates both values and refuses anything else, so they pass on unchecked.
    let body = json!({
        "base_revision": input.get("baseRevision").cloned().unwrap_or(Value::Null),
        "doc": input.get("doc").cloned().unwrap_or(Value::Null),
    });
    let (status, axum::Json(resp)) = save_sidebar(state, user, headers, &body).await;
    owner_json_response_to_envelope("spaces.update_sidebar", user, status, resp)
}
