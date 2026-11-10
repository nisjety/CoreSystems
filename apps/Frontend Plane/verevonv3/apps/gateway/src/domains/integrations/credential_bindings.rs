//! Credential bindings and the custom REST connector (W4-CC-8): an organisation admin stores one bearer token or
//! API key for one host, and creates a custom REST connector that calls that host with it. These are the browser's
//! create, list and revoke calls, passed through to Integration Core with the caller's own bearer.
//!
//! The secret is the one secret on this path. It arrives in the create body and is forwarded to Integration Core
//! once: the body is rebuilt from a closed field list, nothing here logs a body, and the reply is Integration Core's
//! own envelope, which never carries the secret back. The organisation comes from the caller's session, never from
//! the body, so an `organization_id` sent by the browser is dropped before the request leaves.
//!
//! Dark until `CREDENTIAL_BINDINGS_UI_ENABLED` is the literal `true` (read once in `build_state`): while it is off,
//! every route answers 404 and nothing is sent upstream. Only an owner or admin of the active organisation gets past
//! the gateway; Integration Core checks the same role again.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::{
    config::AppState,
    envelope::error,
    middleware::{has_authorized_org_role, AuthenticatedUser},
};

use super::shared::proxy_for_user;

/// Integration Core's binding routes: `POST` stores a binding, `GET` lists them, `DELETE /{id}` revokes one.
const CREDENTIAL_BINDINGS_PATH: &str = "/api/v1/credential-bindings";

/// Integration Core's custom REST connector route: `POST` creates a connection of provider `custom_rest` that
/// calls through one binding.
const CUSTOM_REST_CONNECTOR_PATH: &str = "/api/v1/connectors/custom-rest";

/// The longest binding id the gateway passes on. Integration Core issues `cb_` plus a UUID.
const MAX_BINDING_ID_LEN: usize = 128;

/// The longest connector name the gateway passes on, in characters.
const MAX_CONNECTOR_NAME_CHARS: usize = 120;

/// The only fields a binding is created with. Any other key in the browser's body, the organisation included, stays
/// behind.
const BINDING_FIELDS: [&str; 6] = [
    "kind",
    "host",
    "header_name",
    "allowed_methods",
    "allowed_path_prefixes",
    "secret",
];

/// The answer while the switch is off. It matches an unknown path, so the routes look as they did before.
fn disabled_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "credential_bindings_disabled",
            "Credential bindings are not turned on in this environment.",
        )),
    )
        .into_response()
}

/// The refusal for a caller who is not an owner or admin of the active organisation, or `None` when they are.
fn admin_refusal(user: &AuthenticatedUser) -> Option<Response> {
    if has_authorized_org_role(user, &["owner", "admin"]) {
        return None;
    }
    Some(
        (
            StatusCode::FORBIDDEN,
            Json(error(
                "org_admin_required",
                "Only an organization owner or admin can manage credential bindings.",
            )),
        )
            .into_response(),
    )
}

/// A `400` with a typed envelope, for a field the gateway can refuse before anything is sent upstream.
fn invalid(code: &'static str, message: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, Json(error(code, message))).into_response()
}

/// A binding id is `cb_` followed by ASCII letters, digits, `-` or `_`, at most 128 bytes.
fn binding_id_valid(id: &str) -> bool {
    id.len() > "cb_".len()
        && id.len() <= MAX_BINDING_ID_LEN
        && id.starts_with("cb_")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// A string field of the browser's body, trimmed. Empty when the field is missing or not a string.
fn text_field<'a>(body: &'a Value, key: &str) -> &'a str {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
}

/// The body Integration Core receives for a new binding: the fields named in [`BINDING_FIELDS`], copied across, and
/// nothing else. The secret moves rather than copies, so the browser's body is not held twice.
fn binding_body(body: Value) -> Value {
    let Value::Object(mut object) = body else {
        return json!({});
    };
    let mut forwarded = Map::new();
    for field in BINDING_FIELDS {
        if let Some(value) = object.remove(field) {
            forwarded.insert(field.to_owned(), value);
        }
    }
    Value::Object(forwarded)
}

/// Stores one credential binding for the caller's organisation. The secret is sent once and is never logged.
pub(crate) async fn create_credential_binding(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !state.credential_bindings_enabled {
        return disabled_response();
    }
    if let Some(refusal) = admin_refusal(&user) {
        return refusal;
    }
    let url = format!("{}{}", state.integration_core_url, CREDENTIAL_BINDINGS_PATH);
    proxy_for_user(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(binding_body(body)),
    )
    .await
    .into_response()
}

/// Lists the organisation's bindings as metadata only: the kind, host, header, methods and path prefixes, never the
/// secret.
pub(crate) async fn list_credential_bindings(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    if !state.credential_bindings_enabled {
        return disabled_response();
    }
    if let Some(refusal) = admin_refusal(&user) {
        return refusal;
    }
    let url = format!("{}{}", state.integration_core_url, CREDENTIAL_BINDINGS_PATH);
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None)
        .await
        .into_response()
}

/// Revokes one of the organisation's bindings. Revoking is never refused, and it empties the stored secret.
pub(crate) async fn revoke_credential_binding(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(binding_id): Path<String>,
) -> Response {
    if !state.credential_bindings_enabled {
        return disabled_response();
    }
    if let Some(refusal) = admin_refusal(&user) {
        return refusal;
    }
    let binding_id = binding_id.trim();
    if !binding_id_valid(binding_id) {
        return invalid("invalid_binding_id", "A credential binding id is required.");
    }
    let url = format!(
        "{}{}/{}",
        state.integration_core_url,
        CREDENTIAL_BINDINGS_PATH,
        urlencoding::encode(binding_id)
    );
    proxy_for_user(&state, &user, &headers, Method::DELETE, &url, None)
        .await
        .into_response()
}

/// Creates a custom REST connector that calls through one of the organisation's bindings. The body names the
/// connector and the binding. The host, methods and path prefixes are the binding's, not the body's.
pub(crate) async fn create_custom_rest_connector(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !state.credential_bindings_enabled {
        return disabled_response();
    }
    if let Some(refusal) = admin_refusal(&user) {
        return refusal;
    }
    let name = text_field(&body, "name");
    if name.is_empty() || name.chars().count() > MAX_CONNECTOR_NAME_CHARS {
        return invalid(
            "invalid_connector_name",
            "A connector name of up to 120 characters is required.",
        );
    }
    let binding_id = text_field(&body, "credential_binding_id");
    if !binding_id_valid(binding_id) {
        return invalid("invalid_binding_id", "A credential binding id is required.");
    }
    let url = format!(
        "{}{}",
        state.integration_core_url, CUSTOM_REST_CONNECTOR_PATH
    );
    proxy_for_user(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(json!({ "name": name, "credential_binding_id": binding_id })),
    )
    .await
    .into_response()
}
