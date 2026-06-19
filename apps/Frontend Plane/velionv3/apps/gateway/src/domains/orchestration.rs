//! `/api/v1/orchestration/*` — human-in-the-loop run control for agentic runs.
//!
//! Thin authenticated proxy from the SPA to model-gateway's orchestration HTTP
//! surface (plans, todos, approvals, run pause/resume/cancel). This is the API
//! the chat "internal Claude Code" approval UI + plan mode drive: when an
//! agentic run pauses for approval (the `run_paused_for_approval` SSE event),
//! the SPA lists pending approvals and POSTs a decision here, which flows
//! through model-gateway → session-core → execution-core's `resume_run`.
//!
//! Every route carries the model-plane audience token (or dev-bypass) and the
//! verified user id via [`proxy_model_json`]; nothing trusts client input beyond
//! the path id and the decision body, which model-gateway re-validates.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState,
    domains::chat::shared::{model_token, proxy_model_json},
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        // ── reads ──────────────────────────────────────────────────────────
        .route("/api/v1/orchestration/runs/:run_id/plans", get(list_plans))
        .route("/api/v1/orchestration/plans/:plan_id", get(get_plan))
        .route(
            "/api/v1/orchestration/threads/:thread_id/todos",
            get(list_todos),
        )
        .route(
            "/api/v1/orchestration/runs/:run_id/approvals",
            get(list_approvals),
        )
        .route(
            "/api/v1/orchestration/approvals/:approval_id",
            get(get_approval),
        )
        .route(
            "/api/v1/orchestration/threads/:thread_id/lineage",
            get(get_lineage),
        )
        // ── decisions / run control ──────────────────────────────────────────
        .route(
            "/api/v1/orchestration/approvals/:approval_id/decide",
            post(decide_approval),
        )
        .route(
            "/api/v1/orchestration/plans/:plan_id/approve",
            post(approve_plan),
        )
        .route(
            "/api/v1/orchestration/plans/:plan_id/reject",
            post(reject_plan),
        )
        .route(
            "/api/v1/orchestration/todos/:todo_id/status",
            post(update_todo_status),
        )
        .route(
            "/api/v1/orchestration/runs/:run_id/resume",
            post(resume_run),
        )
        .route(
            "/api/v1/orchestration/runs/:run_id/cancel",
            post(cancel_run),
        )
        // Per-org/user rate limiting on this high-risk human-in-the-loop group.
        // Ordering: `require_session` (written last → outer) runs first and
        // inserts `AuthenticatedUser`, so `rate_limit_middleware` (written first
        // → inner) keys by the validated org/user rather than client IP. The
        // shared `RateLimiter` is provided by the global `Extension` layer in
        // `main.rs`, which is outer to this whole router.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

// ── helpers ────────────────────────────────────────────────────────────────

/// Proxy a model-gateway orchestration GET, forwarding the model-plane token +
/// verified user id.
async fn mg_get(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    path: &str,
) -> (StatusCode, Json<Value>) {
    let token = model_token(state, user, headers).await;
    let url = format!("{}{}", state.model_gateway_url, path);
    proxy_model_json(state, Method::GET, &url, None, token.as_deref(), user).await
}

/// Proxy a model-gateway orchestration POST with a JSON body (decisions).
async fn mg_post(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    path: &str,
    body: Value,
) -> (StatusCode, Json<Value>) {
    let token = model_token(state, user, headers).await;
    let url = format!("{}{}", state.model_gateway_url, path);
    proxy_model_json(
        state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        user,
    )
    .await
}

/// Proxy a model-gateway orchestration POST with no body (run resume/cancel).
async fn mg_post_empty(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    path: &str,
) -> (StatusCode, Json<Value>) {
    let token = model_token(state, user, headers).await;
    let url = format!("{}{}", state.model_gateway_url, path);
    proxy_model_json(state, Method::POST, &url, None, token.as_deref(), user).await
}

fn enc(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}

// ── read handlers ────────────────────────────────────────────────────────────

async fn list_plans(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/runs/{}/plans", enc(&run_id)),
    )
    .await
}

async fn get_plan(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/plans/{}", enc(&plan_id)),
    )
    .await
}

async fn list_todos(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/threads/{}/todos", enc(&thread_id)),
    )
    .await
}

async fn list_approvals(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/runs/{}/approvals", enc(&run_id)),
    )
    .await
}

async fn get_approval(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(approval_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/approvals/{}", enc(&approval_id)),
    )
    .await
}

async fn get_lineage(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    mg_get(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/threads/{}/lineage", enc(&thread_id)),
    )
    .await
}

// ── decision / run-control handlers ───────────────────────────────────────────

async fn decide_approval(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(approval_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    mg_post(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/approvals/{}/decide", enc(&approval_id)),
        body,
    )
    .await
}

async fn approve_plan(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    mg_post(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/plans/{}/approve", enc(&plan_id)),
        body,
    )
    .await
}

async fn reject_plan(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    mg_post(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/plans/{}/reject", enc(&plan_id)),
        body,
    )
    .await
}

async fn update_todo_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(todo_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    mg_post(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/todos/{}/status", enc(&todo_id)),
        body,
    )
    .await
}

async fn resume_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    mg_post_empty(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/runs/{}/resume", enc(&run_id)),
    )
    .await
}

async fn cancel_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    mg_post_empty(
        &state,
        &user,
        &headers,
        &format!("/v1/orchestration/runs/{}/cancel", enc(&run_id)),
    )
    .await
}
