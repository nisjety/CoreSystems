use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, upstream::proxy_json};

/// Shipping aggregator (shipping-core, Ingestion Plane): carrier fleet,
/// quote comparison, and the full booking lifecycle — two-step confirmation
/// gate (enforced server-side in shipping-core), labels (JSON/base64
/// variant, since this proxy pipe is JSON), customs documents, pickup
/// ordering, tracking, and end-of-day manifests. Velion is the aggregator —
/// one integration covers the whole carrier fleet.
pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/shipping/carriers", get(carriers))
        .route("/api/v1/shipping/quotes", post(quotes))
        .route("/api/v1/shipping/bookings", post(create_booking))
        .route("/api/v1/shipping/bookings", get(list_bookings))
        .route("/api/v1/shipping/bookings/:id", get(get_booking))
        .route(
            "/api/v1/shipping/bookings/:id/confirm",
            post(confirm_booking),
        )
        .route("/api/v1/shipping/bookings/:id/cancel", post(cancel_booking))
        .route("/api/v1/shipping/bookings/:id/label", get(booking_label))
        .route(
            "/api/v1/shipping/bookings/:id/customs-document",
            get(booking_customs_document),
        )
        .route("/api/v1/shipping/bookings/:id/pickup", post(booking_pickup))
        .route(
            "/api/v1/shipping/bookings/:id/tracking",
            get(booking_tracking),
        )
        .route("/api/v1/shipping/bookings/:id/audit", get(booking_audit))
        .route("/api/v1/shipping/manifests", post(create_manifest))
        .with_state(state)
}

async fn get_shipping(state: &AppState, path: String) -> (axum::http::StatusCode, Json<Value>) {
    proxy_json(
        state,
        Method::GET,
        &format!("{}{}", state.shipping_core_url, path),
        None,
        None,
        None,
        None,
    )
    .await
}

async fn post_shipping(
    state: &AppState,
    path: String,
    body: Value,
) -> (axum::http::StatusCode, Json<Value>) {
    proxy_json(
        state,
        Method::POST,
        &format!("{}{}", state.shipping_core_url, path),
        Some(body),
        None,
        None,
        Some("application/json"),
    )
    .await
}

/// GET the registered carrier fleet (name, segment, demo vs live prices).
/// Backs the onboarding/settings "shipping sync" check and the shipping page.
async fn carriers(State(state): State<AppState>) -> impl IntoResponse {
    proxy_json(
        &state,
        Method::GET,
        &format!("{}/api/carriers", state.shipping_core_url),
        None,
        None,
        None,
        None,
    )
    .await
}

/// POST a quote request; shipping-core fans it out to every carrier in
/// parallel and returns options cheapest-first plus per-carrier errors.
/// Body passes through verbatim — shipping-core validates it.
async fn quotes(State(state): State<AppState>, Json(body): Json<Value>) -> impl IntoResponse {
    post_shipping(&state, "/api/quotes".into(), body).await
}

async fn create_booking(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    post_shipping(&state, "/api/bookings".into(), body).await
}

async fn list_bookings(State(state): State<AppState>) -> impl IntoResponse {
    get_shipping(&state, "/api/bookings".into()).await
}

async fn get_booking(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    get_shipping(
        &state,
        format!("/api/bookings/{}", urlencoding::encode(&id)),
    )
    .await
}

async fn confirm_booking(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    post_shipping(
        &state,
        format!("/api/bookings/{}/confirm", urlencoding::encode(&id)),
        body,
    )
    .await
}

async fn cancel_booking(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    post_shipping(
        &state,
        format!("/api/bookings/{}/cancel", urlencoding::encode(&id)),
        serde_json::json!({}),
    )
    .await
}

/// Label via the JSON/base64 variant — this proxy pipe is JSON-only, so the
/// SPA decodes `content_base64` for download. Raw PDF stays on shipping-core.
async fn booking_label(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    get_shipping(
        &state,
        format!(
            "/api/bookings/{}/label?format=json",
            urlencoding::encode(&id)
        ),
    )
    .await
}

async fn booking_customs_document(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    get_shipping(
        &state,
        format!(
            "/api/bookings/{}/customs-document?format=json",
            urlencoding::encode(&id)
        ),
    )
    .await
}

async fn booking_pickup(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    post_shipping(
        &state,
        format!("/api/bookings/{}/pickup", urlencoding::encode(&id)),
        body,
    )
    .await
}

async fn booking_tracking(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    get_shipping(
        &state,
        format!("/api/bookings/{}/tracking", urlencoding::encode(&id)),
    )
    .await
}

async fn booking_audit(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    get_shipping(
        &state,
        format!("/api/bookings/{}/audit", urlencoding::encode(&id)),
    )
    .await
}

async fn create_manifest(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    post_shipping(&state, "/api/manifests".into(), body).await
}
