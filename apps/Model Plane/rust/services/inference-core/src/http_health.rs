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
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use tracing::info;

use crate::provider::policy_client::PolicyClient;
use crate::provider::routing_policy::RoutingPolicy;

/// Shared state for the routing-policy admin routes.
#[derive(Clone)]
pub struct PolicyState {
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
async fn get_policy(State(state): State<PolicyState>) -> Json<RoutingPolicy> {
    Json(RoutingPolicy::clone(&state.policy.load_full()))
}

/// Persist a new routing policy to session-core, then hot-swap the live copy.
/// Returns the stored policy (as session-core echoes it back). 503 when no
/// session-core store is configured.
async fn put_policy(
    State(state): State<PolicyState>,
    Json(policy): Json<RoutingPolicy>,
) -> impl IntoResponse {
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
