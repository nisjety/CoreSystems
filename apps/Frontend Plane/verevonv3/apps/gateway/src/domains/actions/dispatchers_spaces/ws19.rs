//! WS19-S4: the two room canvas actions, `spaces.save_canvas` and `spaces.restore_canvas_version`. Each rides
//! the same core function as its REST route in `domains/spaces/canvas.rs`, so the rollout flag, the ZDR
//! refusal, the Space lifecycle, Control's decisions and the typed Data Plane answers exist once. Neither
//! action is model-executable (`model-access.ts` closes both): an agent changes the canvas only through its
//! own run-action tool under its own decision (WS19-S9).

use axum::{http::HeaderMap, response::Response, Json};
use serde_json::Value;

use crate::config::AppState;
use crate::domains::spaces::canvas::{restore_version_core, save_version_core, RestoreBody, SaveBody};
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

const SAVE_FIELDS: &[&str] = &[
    "spaceRef",
    "expectedVersionId",
    "content",
    "summary",
    "clientSaveId",
];
const RESTORE_FIELDS: &[&str] = &["spaceRef", "versionId", "expectedVersionId", "clientSaveId"];

pub(in crate::domains::actions) async fn dispatch_spaces_save_canvas(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.save_canvas requires a non-empty 'spaceRef'");
    }
    let body = match save_input(input) {
        Ok(body) => body,
        Err(message) => return ticket_bad_request(message),
    };
    let (status, Json(payload)) =
        save_version_core(state, user, headers, space_ref, body).await;
    owner_json_response_to_envelope("spaces.save_canvas", user, status, payload)
}

pub(in crate::domains::actions) async fn dispatch_spaces_restore_canvas_version(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let version_id = ticket_string(input, "versionId");
    if space_ref.is_empty() || version_id.is_empty() {
        return ticket_bad_request(
            "spaces.restore_canvas_version requires non-empty 'spaceRef' and 'versionId'",
        );
    }
    let body = match restore_input(input) {
        Ok(body) => body,
        Err(message) => return ticket_bad_request(message),
    };
    let (status, Json(payload)) =
        restore_version_core(state, user, headers, space_ref, version_id, body).await;
    owner_json_response_to_envelope("spaces.restore_canvas_version", user, status, payload)
}

/// The save input in the REST body's shape: the same fields under their action names, and no others.
fn save_input(input: &Value) -> Result<SaveBody, &'static str> {
    known_fields(input, SAVE_FIELDS)?;
    Ok(SaveBody {
        expected_version_id: optional_text(input, "expectedVersionId")?,
        content: required_text(input, "content")?,
        summary: optional_text(input, "summary")?,
        client_save_id: required_text(input, "clientSaveId")?,
    })
}

fn restore_input(input: &Value) -> Result<RestoreBody, &'static str> {
    known_fields(input, RESTORE_FIELDS)?;
    Ok(RestoreBody {
        expected_version_id: optional_text(input, "expectedVersionId")?,
        client_save_id: required_text(input, "clientSaveId")?,
    })
}

/// Refuses an input that carries a field the action does not accept, so no authority field can ride along.
fn known_fields(input: &Value, allowed: &[&str]) -> Result<(), &'static str> {
    let Some(object) = input.as_object() else {
        return Err("the action input must be an object");
    };
    if object.keys().all(|key| allowed.contains(&key.as_str())) {
        Ok(())
    } else {
        Err("the action input carries a field this action does not accept")
    }
}

fn optional_text(input: &Value, key: &str) -> Result<Option<String>, &'static str> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err("a canvas text field must be a string or null"),
    }
}

fn required_text(input: &Value, key: &str) -> Result<String, &'static str> {
    match input.get(key) {
        Some(Value::String(text)) => Ok(text.clone()),
        _ => Err("a required canvas field is missing or is not a string"),
    }
}
