//! Audit read surface.
//!
//! Proxies the Control Plane audit-core read API (internal-key auth) so the SPA
//! Trust Center can aggregate tool-action events (`event=tool_action`, with
//! `details.tool` / `details.data_category`) per connected integration.
//!
//! The org is resolved server-side from the validated session — the browser
//! never gets to pick which org's audit log it reads. Only the `event`,
//! `user_id`, `since`, `until`, and `limit` filters are forwarded from the
//! query string; `org_id` is always the caller's authorized org.

use axum::{
    extract::{Extension, Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;

use crate::{
    config::AppState,
    contracts::ActionActor,
    middleware::{require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_json},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/audit", get(list_audit))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Read-side audit filters the SPA is allowed to set. `org_id` is intentionally
/// absent — it is always the authorized org, never client-supplied.
#[derive(Debug, Default, Deserialize)]
pub(super) struct AuditQuery {
    event: Option<String>,
    user_id: Option<String>,
    since: Option<String>,
    until: Option<String>,
    limit: Option<u32>,
}

async fn list_audit(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<AuditQuery>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;

    // audit-core scopes every read by org_id and requires it; without an
    // authorized org there is nothing to read, so return an empty envelope
    // rather than a 400 the SPA would have to special-case.
    if org_id.trim().is_empty() {
        return Json(serde_json::json!({
            "data": [],
            "meta": { "count": 0 },
            "error": null,
        }))
        .into_response();
    }

    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let mut params: Vec<(&str, String)> = vec![("org_id", org_id.clone())];
    if let Some(event) = query.event.filter(|v| !v.trim().is_empty()) {
        params.push(("event", event));
    }
    if let Some(user_id) = query.user_id.filter(|v| !v.trim().is_empty()) {
        params.push(("user_id", user_id));
    }
    if let Some(since) = query.since.filter(|v| !v.trim().is_empty()) {
        params.push(("since", since));
    }
    if let Some(until) = query.until.filter(|v| !v.trim().is_empty()) {
        params.push(("until", until));
    }
    if let Some(limit) = query.limit {
        params.push(("limit", limit.to_string()));
    }

    let query_string = params
        .iter()
        .map(|(key, value)| format!("{}={}", key, urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    let url = format!("{}/v1/audit?{}", state.audit_core_url, query_string);

    let (status, body) = proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    (status, body).into_response()
}
