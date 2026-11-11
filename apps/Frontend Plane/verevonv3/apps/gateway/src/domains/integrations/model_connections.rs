//! Customer-gateway model connections (W4-BYO-6): the browser's register, list and remove calls, passed
//! through to Integration Core with the caller's own audience-bound bearer.
//!
//! The gateway's API key is the one secret on this path. It arrives in the create body and is forwarded to
//! Integration Core once, as it arrived. Nothing here logs a body, and the reply is Integration Core's own
//! envelope, which never carries the key back.
//!
//! The ZDR assessment and acknowledgement routes at the end of this module (ZDR Z-19c) pass Integration Core's
//! record through with the caller's own bearer. They carry ids, digests, closed codes and Integration Core's
//! own wording only, never a body in a log, and they are mounted only while `ZDR_WAIVER_UI_ENABLED` is the
//! literal `true` (`AppState::zdr_waiver_ui_enabled`).

use std::collections::HashMap;

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

use super::shared::{proxy_for_user, proxy_typed_for_user};

/// The Integration Core path of the customer-gateway routes: `POST` registers, `GET` lists and `DELETE`
/// removes one connection, each with the caller's bearer.
const CUSTOMER_GATEWAY_PATH: &str = "/api/v1/model-connections/customer-gateway";

/// The longest connection id the gateway passes on. Integration Core issues `conn_` plus a UUID.
const MAX_CONNECTION_ID_LEN: usize = 128;

/// Registers a customer gateway for the caller's organisation. The body is forwarded once and never logged.
pub(crate) async fn create_customer_gateway(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}{}", state.integration_core_url, CUSTOMER_GATEWAY_PATH);
    proxy_for_user(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(if body.is_object() { body } else { json!({}) }),
    )
    .await
    .into_response()
}

/// Lists the caller's organisation's customer gateways, with their model ids and never their keys.
pub(crate) async fn list_customer_gateways(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let url = format!("{}{}", state.integration_core_url, CUSTOMER_GATEWAY_PATH);
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None)
        .await
        .into_response()
}

/// Removes one customer gateway. The connection id is named in the body as `connection_id`. It is opaque,
/// and Integration Core checks that it belongs to the caller.
pub(crate) async fn delete_customer_gateway(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let connection_id = body
        .get("connection_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if connection_id.is_empty() || connection_id.len() > MAX_CONNECTION_ID_LEN {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_connection_id",
                "A customer gateway connection id is required.",
            )),
        )
            .into_response();
    }
    let url = format!("{}{}", state.integration_core_url, CUSTOMER_GATEWAY_PATH);
    proxy_for_user(
        &state,
        &user,
        &headers,
        Method::DELETE,
        &url,
        Some(json!({ "connection_id": connection_id })),
    )
    .await
    .into_response()
}

/// Integration Core's base path for model connections. The ZDR routes hang off it per connection.
const MODEL_CONNECTIONS_PATH: &str = "/api/v1/model-connections";

/// The locales the assessment text is composed in. A `locale` outside this set is dropped, and Integration
/// Core applies its default.
const ZDR_LOCALES: [&str; 2] = ["no", "en"];

/// The wording variants the assessment text is composed for. A `variant` outside this set is dropped.
const ZDR_VARIANTS: [&str; 3] = ["org_zdr_on", "org_zdr_off", "first_use"];

/// The only fields an acknowledgement is recorded with. Any other key in the browser's body stays behind.
const ZDR_ACKNOWLEDGEMENT_FIELDS: [&str; 5] = [
    "digest",
    "textVersion",
    "locale",
    "variant",
    "acknowledgedVia",
];

/// The largest page of the admin acknowledgement list the gateway asks for.
const ZDR_LIST_MAX_LIMIT: u32 = 200;

/// The longest string value the gateway forwards for one acknowledgement field. A digest is 64 hex characters
/// and the other fields are short codes, so anything longer is not a real value.
const ZDR_FIELD_MAX_LEN: usize = 256;

/// The refusal for a connection id that cannot be a path segment Integration Core issued.
fn invalid_request() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error(
            "invalid_request",
            "A valid model connection id is required.",
        )),
    )
        .into_response()
}

/// Whether `connection_id` is safe to place in an upstream path: not empty, at most 128 characters, and with no
/// '/', whitespace, control character or URL delimiter.
fn valid_connection_id(connection_id: &str) -> bool {
    !connection_id.is_empty()
        && connection_id.len() <= MAX_CONNECTION_ID_LEN
        && !connection_id.chars().any(|c| {
            c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\' | '?' | '#' | '%')
        })
        && connection_id != "."
        && connection_id != ".."
}

/// Builds the assessment query from the closed sets only; anything else the browser sent is dropped.
fn zdr_assessment_query(params: &HashMap<String, String>) -> String {
    let mut pairs: Vec<String> = Vec::new();
    if let Some(locale) = params
        .get("locale")
        .map(|v| v.trim())
        .filter(|v| ZDR_LOCALES.contains(v))
    {
        pairs.push(format!("locale={locale}"));
    }
    if let Some(variant) = params
        .get("variant")
        .map(|v| v.trim())
        .filter(|v| ZDR_VARIANTS.contains(v))
    {
        pairs.push(format!("variant={variant}"));
    }
    if pairs.is_empty() {
        String::new()
    } else {
        format!("?{}", pairs.join("&"))
    }
}

/// Reads the connection's ZDR assessment: the server-composed wording, its digest and the effective state.
pub(crate) async fn get_zdr_assessment(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(connection_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if !valid_connection_id(&connection_id) {
        return invalid_request();
    }
    let url = format!(
        "{}{}/{}/zdr{}",
        state.integration_core_url,
        MODEL_CONNECTIONS_PATH,
        connection_id,
        zdr_assessment_query(&params)
    );
    proxy_typed_for_user(&state, &user, &headers, Method::GET, &url, None)
        .await
        .into_response()
}

/// Records the caller's acknowledgement of the connection's assessment. The body is rebuilt from the five known
/// fields and is never logged; Integration Core owns the record and checks that the caller owns the connection.
pub(crate) async fn acknowledge_zdr(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(connection_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if !valid_connection_id(&connection_id) {
        return invalid_request();
    }
    let mut forwarded = Map::new();
    if let Some(object) = body.as_object() {
        for field in ZDR_ACKNOWLEDGEMENT_FIELDS {
            if let Some(value) = object.get(field).and_then(Value::as_str) {
                if value.len() <= ZDR_FIELD_MAX_LEN {
                    forwarded.insert(field.to_owned(), Value::String(value.to_owned()));
                }
            }
        }
    }
    let url = format!(
        "{}{}/{}/zdr-acknowledgement",
        state.integration_core_url, MODEL_CONNECTIONS_PATH, connection_id
    );
    proxy_typed_for_user(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(Value::Object(forwarded)),
    )
    .await
    .into_response()
}

/// Withdraws the caller's acknowledgement for the connection. Integration Core records the withdrawal.
pub(crate) async fn withdraw_zdr_acknowledgement(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(connection_id): Path<String>,
) -> Response {
    if !valid_connection_id(&connection_id) {
        return invalid_request();
    }
    let url = format!(
        "{}{}/{}/zdr-acknowledgement",
        state.integration_core_url, MODEL_CONNECTIONS_PATH, connection_id
    );
    proxy_typed_for_user(&state, &user, &headers, Method::DELETE, &url, None)
        .await
        .into_response()
}

/// Lists the organisation's acknowledgement record for an owner or admin. `limit` is passed through only as an
/// integer from 1 to 200; Integration Core applies its own role check and default.
pub(crate) async fn list_zdr_acknowledgements(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let query = params
        .get("limit")
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .filter(|limit| (1..=ZDR_LIST_MAX_LIMIT).contains(limit))
        .map(|limit| format!("?limit={limit}"))
        .unwrap_or_default();
    let url = format!(
        "{}{}/zdr-acknowledgements{}",
        state.integration_core_url, MODEL_CONNECTIONS_PATH, query
    );
    proxy_typed_for_user(&state, &user, &headers, Method::GET, &url, None)
        .await
        .into_response()
}
