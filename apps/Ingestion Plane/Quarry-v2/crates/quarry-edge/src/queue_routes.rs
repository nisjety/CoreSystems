//! Internal durable-frontier bridge for Go/Temporal.
//!
//! The queue schema and writer remain Rust-owned. These narrow service-token
//! routes let the orchestrator enqueue/pop/ack work without reimplementing
//! `SKIP LOCKED` semantics or opening a second writer against the database.

use axum::Router;
#[cfg(feature = "postgres-queue")]
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{post, put},
    Json,
};
#[cfg(feature = "postgres-queue")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "postgres-queue")]
use serde_json::Value;

#[cfg(feature = "postgres-queue")]
use quarry_core::envelope::Envelope;
#[cfg(feature = "postgres-queue")]
use quarry_core::error::{ErrorCode, QuarryError};
#[cfg(feature = "postgres-queue")]
use quarry_runtime::request_queue::{Priority, QueuedRequest, RequestQueue};

use crate::state::AppState;

#[cfg(feature = "postgres-queue")]
#[derive(Debug, Deserialize)]
pub struct QueueEnqueue {
    pub org_id: String,
    pub request_id: String,
    pub url: String,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub payload: Value,
}

#[cfg(feature = "postgres-queue")]
#[derive(Debug, Deserialize)]
pub struct QueueOrgRequest {
    pub org_id: String,
}

#[cfg(feature = "postgres-queue")]
#[derive(Debug, Deserialize)]
pub struct QueueAck {
    pub org_id: String,
    pub request_id: String,
}

#[cfg(feature = "postgres-queue")]
#[derive(Debug, Serialize)]
pub struct QueueMutation {
    pub accepted: bool,
}

#[cfg(feature = "postgres-queue")]
type ApiErr = (StatusCode, Json<Envelope<()>>);

#[cfg(feature = "postgres-queue")]
fn error(request_id: &str, error: QuarryError) -> ApiErr {
    (
        StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(Envelope::err(request_id, error)),
    )
}

#[cfg(feature = "postgres-queue")]
fn auth_and_scope(
    state: &AppState,
    headers: &HeaderMap,
    org_id: &str,
    queue_name: &str,
) -> Result<(), QuarryError> {
    crate::routes::verify_runtime_token(headers)?;
    if org_id.trim().is_empty() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "org_id is required",
        ));
    }
    if queue_name.is_empty()
        || queue_name.len() > 128
        || !queue_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "invalid queue name",
        ));
    }
    let signature = headers
        .get("x-quarry-queue-sig")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let Some(signer) = state.internal_signer.as_ref() else {
        return Err(QuarryError::new(
            ErrorCode::RuntimeNotReady,
            "queue org binding is not configured",
        ));
    };
    if !signer.verify_run_binding(org_id, queue_name, signature) {
        return Err(QuarryError::new(
            ErrorCode::Unauthorized,
            "queue org binding invalid",
        ));
    }
    Ok(())
}

#[cfg(feature = "postgres-queue")]
async fn bind(
    state: &AppState,
    org_id: &str,
    queue_name: &str,
) -> Result<quarry_runtime::postgres_queue::PostgresRequestQueue, QuarryError> {
    let Some(pool) = state.queue_pool.clone() else {
        return Err(QuarryError::new(
            ErrorCode::RuntimeNotReady,
            "durable frontier is not configured",
        ));
    };
    quarry_runtime::postgres_queue::PostgresRequestQueue::bind(
        pool,
        org_id.to_owned(),
        queue_name,
        "crawl",
        std::time::Duration::from_secs(300),
    )
    .await
}

#[cfg(feature = "postgres-queue")]
pub async fn enqueue(
    State(state): State<AppState>,
    Path(queue_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<QueueEnqueue>,
) -> Result<Json<Envelope<QueueMutation>>, ApiErr> {
    let request_id = quarry_core::ids::Id::<quarry_core::ids::RequestKind>::new().to_string();
    auth_and_scope(&state, &headers, &body.org_id, &queue_name)
        .map_err(|e| error(&request_id, e))?;
    let queue = bind(&state, &body.org_id, &queue_name)
        .await
        .map_err(|e| error(&request_id, e))?;
    let accepted = queue
        .enqueue(body.request_id, body.url, body.priority, body.payload)
        .await
        .map_err(|e| error(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, QueueMutation { accepted })))
}

#[cfg(feature = "postgres-queue")]
pub async fn pop(
    State(state): State<AppState>,
    Path(queue_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<QueueOrgRequest>,
) -> Result<Json<Envelope<Option<QueuedRequest>>>, ApiErr> {
    let request_id = quarry_core::ids::Id::<quarry_core::ids::RequestKind>::new().to_string();
    auth_and_scope(&state, &headers, &body.org_id, &queue_name)
        .map_err(|e| error(&request_id, e))?;
    let queue = bind(&state, &body.org_id, &queue_name)
        .await
        .map_err(|e| error(&request_id, e))?;
    let item = queue.pop().await.map_err(|e| error(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, item)))
}

#[cfg(feature = "postgres-queue")]
pub async fn ack(
    State(state): State<AppState>,
    Path(queue_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<QueueAck>,
) -> Result<Json<Envelope<QueueMutation>>, ApiErr> {
    let request_id = quarry_core::ids::Id::<quarry_core::ids::RequestKind>::new().to_string();
    auth_and_scope(&state, &headers, &body.org_id, &queue_name)
        .map_err(|e| error(&request_id, e))?;
    let queue = bind(&state, &body.org_id, &queue_name)
        .await
        .map_err(|e| error(&request_id, e))?;
    queue
        .ack(&body.request_id)
        .await
        .map_err(|e| error(&request_id, e))?;
    Ok(Json(Envelope::ok(
        request_id,
        QueueMutation { accepted: true },
    )))
}

#[cfg(feature = "postgres-queue")]
pub fn internal_router() -> Router<AppState> {
    Router::new()
        .route("/v1/internal/queues/:queue_name/enqueue", post(enqueue))
        .route("/v1/internal/queues/:queue_name/pop", post(pop))
        .route("/v1/internal/queues/:queue_name/ack", put(ack).post(ack))
}

#[cfg(not(feature = "postgres-queue"))]
pub fn internal_router() -> Router<AppState> {
    Router::new()
}
