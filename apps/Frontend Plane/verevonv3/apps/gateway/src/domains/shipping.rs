use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    audience_tokens::get_audience_token,
    config::AppState,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    upstream::proxy_bearer_json,
};

/// Shipping aggregator (shipping-core, Ingestion Plane): carrier fleet,
/// quote comparison, and the full booking lifecycle — two-step confirmation
/// gate (enforced server-side in shipping-core), labels (JSON/base64
/// variant, since this proxy pipe is JSON), customs documents, pickup
/// ordering, tracking, and end-of-day manifests. Verevon is the aggregator —
/// one integration covers the whole carrier fleet.
pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/shipping/carriers", get(carriers))
        .route(
            "/api/v1/shipping/carriers/reliability",
            get(carriers_reliability),
        )
        .route("/api/v1/shipping/quotes", post(quotes))
        .route("/api/v1/shipping/quotes/recommend", post(quotes_recommend))
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
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ))
        .with_state(state)
}

async fn get_shipping(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    path: String,
) -> Response {
    let Some(token) = shipping_token(state, user, headers).await else {
        return shipping_auth_unavailable();
    };
    proxy_bearer_json(
        state,
        Method::GET,
        &format!("{}{}", state.shipping_core_url, path),
        None,
        Some(&token),
        &user.user_id,
    )
    .await
    .into_response()
}

async fn post_shipping(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    path: String,
    body: Value,
) -> Response {
    let Some(token) = shipping_token(state, user, headers).await else {
        return shipping_auth_unavailable();
    };
    proxy_bearer_json(
        state,
        Method::POST,
        &format!("{}{}", state.shipping_core_url, path),
        Some(body),
        Some(&token),
        &user.user_id,
    )
    .await
    .into_response()
}

async fn shipping_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Option<String> {
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    get_audience_token(state, &user.user_id, cookie, "ingestion").await
}

fn shipping_auth_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "shipping_auth_unavailable",
            "Shipping authentication is temporarily unavailable.",
        )),
    )
        .into_response()
}

/// GET the registered carrier fleet (name, segment, demo vs live prices).
/// Backs the onboarding/settings "shipping sync" check and the shipping page.
async fn carriers(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    get_shipping(&state, &user, &headers, "/api/carriers".into()).await
}

/// POST a quote request; shipping-core fans it out to every carrier in
/// parallel and returns options cheapest-first plus per-carrier errors.
/// Body passes through verbatim — shipping-core validates it.
async fn quotes(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    post_shipping(&state, &user, &headers, "/api/quotes".into(), body).await
}

/// GET each carrier's F8 on-time delivery score (carriers below the
/// minimum sample size are simply absent — never a fabricated number).
async fn carriers_reliability(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    get_shipping(&state, &user, &headers, "/api/carriers/reliability".into()).await
}

/// POST the same quote request as `/quotes`, plus F5's AI-reasoned
/// recommendation over the resulting comparison table. Body passes
/// through verbatim; the recommendation is honestly `available: false`
/// (never a fabricated pick) when Model Plane is unreachable or
/// unconfigured.
async fn quotes_recommend(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    post_shipping(
        &state,
        &user,
        &headers,
        "/api/quotes/recommend".into(),
        body,
    )
    .await
}

async fn create_booking(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    post_shipping(&state, &user, &headers, "/api/bookings".into(), body).await
}

async fn list_bookings(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    get_shipping(&state, &user, &headers, "/api/bookings".into()).await
}

async fn get_booking(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    get_shipping(
        &state,
        &user,
        &headers,
        format!("/api/bookings/{}", urlencoding::encode(&id)),
    )
    .await
}

async fn confirm_booking(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    post_shipping(
        &state,
        &user,
        &headers,
        format!("/api/bookings/{}/confirm", urlencoding::encode(&id)),
        body,
    )
    .await
}

async fn cancel_booking(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    post_shipping(
        &state,
        &user,
        &headers,
        format!("/api/bookings/{}/cancel", urlencoding::encode(&id)),
        serde_json::json!({}),
    )
    .await
}

/// Label via the JSON/base64 variant — this proxy pipe is JSON-only, so the
/// SPA decodes `content_base64` for download. Raw PDF stays on shipping-core.
async fn booking_label(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    get_shipping(
        &state,
        &user,
        &headers,
        format!(
            "/api/bookings/{}/label?format=json",
            urlencoding::encode(&id)
        ),
    )
    .await
}

async fn booking_customs_document(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    get_shipping(
        &state,
        &user,
        &headers,
        format!(
            "/api/bookings/{}/customs-document?format=json",
            urlencoding::encode(&id)
        ),
    )
    .await
}

async fn booking_pickup(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    post_shipping(
        &state,
        &user,
        &headers,
        format!("/api/bookings/{}/pickup", urlencoding::encode(&id)),
        body,
    )
    .await
}

async fn booking_tracking(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    get_shipping(
        &state,
        &user,
        &headers,
        format!("/api/bookings/{}/tracking", urlencoding::encode(&id)),
    )
    .await
}

async fn booking_audit(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    get_shipping(
        &state,
        &user,
        &headers,
        format!("/api/bookings/{}/audit", urlencoding::encode(&id)),
    )
    .await
}

async fn create_manifest(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    post_shipping(&state, &user, &headers, "/api/manifests".into(), body).await
}
