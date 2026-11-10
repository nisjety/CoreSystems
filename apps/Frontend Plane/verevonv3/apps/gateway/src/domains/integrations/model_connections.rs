//! Customer-gateway model connections (W4-BYO-6): the browser's register, list and remove calls, passed
//! through to Integration Core with the caller's own audience-bound bearer.
//!
//! The gateway's API key is the one secret on this path. It arrives in the create body and is forwarded to
//! Integration Core once, as it arrived. Nothing here logs a body, and the reply is Integration Core's own
//! envelope, which never carries the key back.

use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

use super::shared::proxy_for_user;

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
