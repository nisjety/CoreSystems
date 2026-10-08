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
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use metrics_exporter_prometheus::PrometheusHandle;
use tracing::info;

use crate::auth::JwtVerifier;
use crate::provider::policy_client::PolicyClient;
use crate::provider::routing_policy::RoutingPolicy;
use crate::readiness::GrpcReadiness;

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

/// Start the HTTP health server on :8082. `metrics` is the handle of the
/// process-global recorder ([`crate::metrics::install_recorder`]) that
/// `/metrics` renders.
///
/// # Errors
///
/// Returns an error if the server fails to bind or serve.
pub async fn serve(
    state: PolicyState,
    grpc_readiness: GrpcReadiness,
    metrics: PrometheusHandle,
) -> anyhow::Result<()> {
    let app = router(state, grpc_readiness, metrics);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8082").await?;
    info!("HTTP health listening on :8082");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Every route of the :8082 server. Split from [`serve`] so a test can mount it
/// on an ephemeral port.
pub fn router(
    state: PolicyState,
    grpc_readiness: GrpcReadiness,
    metrics: PrometheusHandle,
) -> Router {
    let policy_routes = Router::new()
        .route(
            "/internal/v1/router-policy",
            get(get_policy).put(put_policy),
        )
        .with_state(state);

    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/version", get(version))
        .merge(metrics_route(metrics))
        .merge(policy_routes)
        .layer(Extension(grpc_readiness))
}

/// `GET /metrics`: the Prometheus text exposition of the recorder behind `handle`.
pub fn metrics_route(handle: PrometheusHandle) -> Router {
    Router::new().route("/metrics", get(metrics_handler).with_state(handle))
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(Extension(grpc_readiness): Extension<GrpcReadiness>) -> Response {
    if grpc_readiness.is_bound() {
        (StatusCode::OK, "ok").into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "grpc listener not bound").into_response()
    }
}

/// Prometheus text exposition format, version 0.0.4.
const PROMETHEUS_TEXT: &str = "text/plain; version=0.0.4; charset=utf-8";

async fn metrics_handler(State(handle): State<PrometheusHandle>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(CONTENT_TYPE, PROMETHEUS_TEXT)],
        handle.render(),
    )
}

/// Reports what is actually running in this container: the git revision and
/// build timestamp baked into the image (see `Dockerfile`'s `SOURCE_REVISION`
/// / `BUILD_DATE` build args, re-exposed as runtime `ENV` so no Docker/registry
/// access is needed to answer "what SHA is deployed here"). Falls back to the
/// same `unverified`/`unknown` defaults the image LABELs use when unset.
async fn version() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "service": "inference-core",
        "revision": std::env::var("SOURCE_REVISION").unwrap_or_else(|_| "unverified".to_string()),
        "build_date": std::env::var("BUILD_DATE").unwrap_or_else(|_| "unknown".to_string()),
        "cargo_version": env!("CARGO_PKG_VERSION"),
    }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readiness::GrpcReadiness;
    use axum::Extension;

    #[tokio::test]
    async fn readyz_fails_until_grpc_listener_is_bound() {
        let readiness = GrpcReadiness::new();

        let before_bind = readyz(Extension(readiness.clone())).await.into_response();
        assert_eq!(before_bind.status(), StatusCode::SERVICE_UNAVAILABLE);

        readiness.mark_bound();
        let after_bind = readyz(Extension(readiness)).await.into_response();
        assert_eq!(after_bind.status(), StatusCode::OK);
    }

    /// `/metrics` is what the recorder rendered, over a real HTTP round trip,
    /// not the fixed 37-byte line it used to answer with whatever had happened.
    #[tokio::test]
    async fn metrics_serves_the_recorders_exposition_over_http() {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        ::metrics::with_local_recorder(&recorder, || {
            crate::metrics::record_request(
                "infer",
                "service:data-orchestrator",
                "gpt-4o-mini",
                "azure-openai",
                "zdr_unavailable",
            );
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral port");
        let address = listener.local_addr().expect("the bound address");
        tokio::spawn(async move { axum::serve(listener, metrics_route(handle)).await });

        let response = reqwest::get(format!("http://{address}/metrics"))
            .await
            .expect("the scrape");

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some(PROMETHEUS_TEXT)
        );
        let body = response.text().await.expect("the body");
        assert!(body.len() > 37, "more than the old stub: {body}");
        assert!(
            body.contains("inference_requests_total{")
                && body.contains("service=\"service:data-orchestrator\"")
                && body.contains("outcome=\"zdr_unavailable\""),
            "{body}"
        );
    }
}
