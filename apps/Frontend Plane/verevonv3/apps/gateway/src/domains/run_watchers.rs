//! `/api/v1/runs/{run_id}/watchers` — "notify me when this run finishes"
//! subscription toggle for a single agentic run.
//!
//! Thin authenticated proxy from the SPA to model-gateway's watcher surface
//! (`POST` / `DELETE` / `GET /v1/runs/{run_id}/watchers` → capability-core,
//! which owns the durable per-user watcher record and — once the run
//! finishes — hands off to notification-core to actually deliver the
//! notification). Sibling of the live run-events SSE (`chat.rs`'s
//! `/api/v1/runs/{run_id}/events`) and the runs-history read model
//! (`agents_runs.rs`): those observe or read a run, this registers a
//! standing "tell me when it's done" subscription for the caller.
//!
//! Like every other capability-core-backed proxy in this file set (see
//! `agent_actions.rs`'s skills/plugins/cron/capabilities surface), the
//! delegated `x-capability-authorization` bearer is minted server-side from
//! the validated session and is the credential capability-core actually
//! trusts. Org and user identity are resolved the same way every other proxy
//! domain resolves them — from the already-verified `AuthenticatedUser`
//! extension this gateway's session middleware inserts — and forwarded as
//! `x-org-id` / `x-user-id` by the shared proxy helper; nothing here reads an
//! org or user id off the incoming request, so a client cannot widen scope
//! (or watch a run on someone else's behalf) by tampering with anything it
//! sends. capability-core independently re-derives both from the verified
//! capability token's claims before it reads or writes anything, so this is
//! defense in depth, not the actual boundary.
//!
//! A subscription is inherently per-caller and per-run: watching (or
//! unwatching, or checking status) is not an org-wide admin action, so —
//! unlike skill/plugin/cron authoring — these routes are open to any
//! authenticated org member, the same posture as the skills/capabilities
//! *reads* in `agent_actions.rs`.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::get,
    Extension, Router,
};
use reqwest::Method;

use crate::{
    config::AppState,
    domains::chat::shared::{
        delegated_auth_unavailable, model_token, proxy_model_json_with_capability,
        required_capability_token,
    },
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/runs/{run_id}/watchers",
            get(get_watch_status).post(watch_run).delete(unwatch_run),
        )
        // Per-org/user rate limiting, ordered like `memory.rs`/`agents_runs.rs`:
        // `require_session` (written last → outer) runs first and inserts
        // `AuthenticatedUser`, so `rate_limit_middleware` (written first →
        // inner) keys by the validated org/user rather than client IP.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// `POST /api/v1/runs/{run_id}/watchers` — register a "notify me" subscription
/// for the caller on this run.
async fn watch_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let capability = match required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return delegated_auth_unavailable(err),
    };
    let url = watchers_url(&state, &run_id);
    proxy_model_json_with_capability(
        &state,
        Method::POST,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}

/// `DELETE /api/v1/runs/{run_id}/watchers` — cancel the caller's "notify me"
/// subscription on this run, if any.
async fn unwatch_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let capability = match required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return delegated_auth_unavailable(err),
    };
    let url = watchers_url(&state, &run_id);
    proxy_model_json_with_capability(
        &state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}

/// `GET /api/v1/runs/{run_id}/watchers` — whether the caller currently has a
/// "notify me" subscription on this run.
async fn get_watch_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let capability = match required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return delegated_auth_unavailable(err),
    };
    let url = watchers_url(&state, &run_id);
    proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}

fn watchers_url(state: &AppState, run_id: &str) -> String {
    format!(
        "{}/v1/runs/{}/watchers",
        state.model_gateway_url,
        urlencoding::encode(run_id)
    )
}
