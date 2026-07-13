//! `/api/v1/agents/runs` — the runs-history read surface for the Agent Run
//! Console.
//!
//! Thin authenticated proxy from the SPA to model-gateway's run read model
//! (`GET /v1/runs` + `GET /v1/runs/{run_id}` → session-core `RunService`). It is
//! the additive history sibling of the live run-events SSE (`chat.rs`) and the
//! human-in-the-loop control surface (`orchestration.rs`): those drive ONE live
//! run, this lists past runs for a thread and hydrates a single run's telemetry.
//!
//! Org scope is resolved server-side from the validated session — the browser
//! never picks which org's runs it reads. The only client inputs are the
//! `thread_id` / filter query params and the path `run_id`, both re-validated by
//! model-gateway against the model-plane token's verified claims.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Extension, Json, Router,
};
use reqwest::Method;
use serde::Deserialize;

use crate::{
    config::AppState,
    domains::chat::shared::{model_token, proxy_model_json_with_session, session_token},
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
    upstream::authorized_org_id,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/agents/runs", get(list_runs))
        .route("/api/v1/agents/runs/:run_id", get(get_run))
        // Per-org/user rate limiting, ordered like `orchestration.rs`:
        // `require_session` (written last → outer) runs first and inserts
        // `AuthenticatedUser`, so `rate_limit_middleware` (written first → inner)
        // keys by the validated org/user rather than client IP.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Runs-history filters the SPA is allowed to set. `thread_id` scopes the list;
/// `status` / `after` / `limit` are optional filter + pagination knobs. The org
/// is never client-supplied — model-gateway derives it from the model-plane
/// token's verified claims.
#[derive(Debug, Default, Deserialize)]
struct RunsQuery {
    thread_id: Option<String>,
    status: Option<String>,
    after: Option<String>,
    limit: Option<u32>,
}

async fn list_runs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(query): Query<RunsQuery>,
) -> impl IntoResponse {
    let thread_id = query.thread_id.unwrap_or_default();
    if thread_id.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(crate::envelope::error(
                "thread_id_required",
                "thread_id query parameter is required",
            )),
        )
            .into_response();
    }

    // Resolve the caller's org server-side. model-gateway re-derives org from the
    // verified token, but resolving here keeps the authority boundary explicit
    // and lets a missing org short-circuit before the upstream hop.
    let _org_id = authorized_org_id(&state, &user).await;

    let mut params: Vec<(&str, String)> = vec![("thread_id", thread_id)];
    if let Some(status) = query.status.filter(|v| !v.trim().is_empty()) {
        params.push(("status", status));
    }
    if let Some(after) = query.after.filter(|v| !v.trim().is_empty()) {
        params.push(("after", after));
    }
    if let Some(limit) = query.limit {
        params.push(("limit", limit.to_string()));
    }
    let query_string = params
        .iter()
        .map(|(key, value)| format!("{}={}", key, urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&");

    let token = model_token(&state, &user, &headers).await;
    let session_token = session_token(&state, &user, &headers).await;
    let url = format!("{}/v1/runs?{}", state.model_gateway_url, query_string);
    let (status, body) = proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await;
    (status, body).into_response()
}

async fn get_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let session_token = session_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/runs/{}",
        state.model_gateway_url,
        urlencoding::encode(&run_id)
    );
    let (status, body) = proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await;
    (status, body).into_response()
}
