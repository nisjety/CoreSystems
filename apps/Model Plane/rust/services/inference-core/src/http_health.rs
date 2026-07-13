//! HTTP health/metrics endpoints for inference-core on :8082.
//!
//! Also hosts the internal runtime routing-policy admin surface:
//! `GET /internal/v1/router-policy` returns the currently-loaded effective
//! [`RoutingPolicy`]; `PUT /internal/v1/router-policy` persists a new one to
//! session-core and hot-swaps the live copy immediately.

use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use tracing::info;

use crate::auth::JwtVerifier;
use crate::provider::policy_client::PolicyClient;
use crate::provider::routing_policy::RoutingPolicy;

/// Shared state for the routing-policy admin routes.
#[derive(Clone)]
pub struct PolicyState {
    /// Auth Core verifier shared with the gRPC boundary. Policy routes require
    /// a verified `inference-core` audience plus a dedicated admin scope.
    pub auth: JwtVerifier,
    /// Live policy, hot-swapped by the refresh loop and the PUT write-through.
    pub policy: Arc<ArcSwap<RoutingPolicy>>,
    /// session-core client for the write path. `None` → PUT returns 503.
    pub client: Option<Arc<PolicyClient>>,
}

/// Start the HTTP health server on :8082.
///
/// # Errors
///
/// Returns an error if the server fails to bind or serve.
pub async fn serve(state: PolicyState) -> anyhow::Result<()> {
    let policy_routes = Router::new()
        .route(
            "/internal/v1/router-policy",
            get(get_policy).put(put_policy),
        )
        .with_state(state);

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics_handler))
        .merge(policy_routes);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8082").await?;
    info!("HTTP health listening on :8082");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz() -> &'static str {
    "ok"
}

async fn metrics_handler() -> impl IntoResponse {
    (StatusCode::OK, "# HELP inference_core_requests_total\n")
}

/// Return the currently-loaded effective routing policy.
async fn get_policy(State(state): State<PolicyState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize_policy_admin(&state.auth, &headers).await {
        return response;
    }
    Json(RoutingPolicy::clone(&state.policy.load_full())).into_response()
}

/// Persist a new routing policy to session-core, then hot-swap the live copy.
/// Returns the stored policy (as session-core echoes it back). 503 when no
/// session-core store is configured.
async fn put_policy(
    State(state): State<PolicyState>,
    headers: HeaderMap,
    Json(policy): Json<RoutingPolicy>,
) -> impl IntoResponse {
    if let Err(response) = authorize_policy_admin(&state.auth, &headers).await {
        return response;
    }
    let Some(client) = state.client else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": {
                    "code": "policy_store_unconfigured",
                    "message": "SESSION_CORE_URL is not set; the routing policy store is unavailable"
                }
            })),
        )
            .into_response();
    };
    match client.set(&policy, "inference-core-admin").await {
        Ok(stored) => {
            state.policy.store(Arc::new(stored.clone()));
            (StatusCode::OK, Json(stored)).into_response()
        }
        Err(message) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": { "code": "policy_store_write_failed", "message": message }
            })),
        )
            .into_response(),
    }
}

async fn authorize_policy_admin(
    verifier: &JwtVerifier,
    headers: &HeaderMap,
) -> Result<(), Response> {
    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| auth_error(StatusCode::UNAUTHORIZED, "verified credential required"))?;
    let metadata = authorization
        .parse()
        .map_err(|_| auth_error(StatusCode::UNAUTHORIZED, "invalid credential"))?;
    let mut request = tonic::Request::new(());
    request.metadata_mut().insert("authorization", metadata);
    let principal = verifier
        .authenticate(&request)
        .await
        .map_err(|status| map_auth_status(&status))?;
    principal
        .authorize_policy_admin()
        .map_err(|status| map_auth_status(&status))
}

fn map_auth_status(status: &tonic::Status) -> Response {
    let http_status = match status.code() {
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::UNAUTHORIZED,
    };
    auth_error(http_status, status.message())
}

fn auth_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": { "code": "authorization_failed", "message": message }
        })),
    )
        .into_response()
}
