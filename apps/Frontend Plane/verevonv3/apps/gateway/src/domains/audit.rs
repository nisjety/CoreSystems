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
//!
//! Role gate (WS05-S1): org owners and admins read the whole org trail. Any other
//! member is narrowed to their own events here, before the upstream call, and a
//! foreign `user_id` is refused with 403 `audit_scope_denied`. The one exception
//! is `event=tool_action` with no `user_id`, which audit-core answers with a
//! redacted, content-free org-wide projection (the Trust Center). audit-core
//! re-derives the same decision from the signed role, so this layer is a second
//! check, never the only one.

use axum::{
    extract::{Extension, Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use reqwest::{Method, StatusCode};
use serde::Deserialize;

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error_with_details,
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_json},
};

/// Largest page a gateway caller may ask for; audit-core's own ceiling is higher.
const MAX_AUDIT_LIMIT: u32 = 200;
const TOOL_ACTION_EVENT: &str = "tool_action";

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
    let event = query.event.filter(|v| !v.trim().is_empty());
    let user_id = query.user_id.filter(|v| !v.trim().is_empty());
    let user_id = match scoped_user_id(
        has_authorized_org_role(&user, &["owner", "admin"]),
        &user.user_id,
        event.as_deref(),
        user_id,
    ) {
        Ok(user_id) => user_id,
        Err(()) => {
            return (
                StatusCode::FORBIDDEN,
                Json(error_with_details(
                    "audit_scope_denied",
                    "You can only read your own audit events.",
                    serde_json::json!({ "scope": "self" }),
                )),
            )
                .into_response();
        }
    };
    let mut params: Vec<(&str, String)> = vec![("org_id", org_id.clone())];
    if let Some(event) = event {
        params.push(("event", event));
    }
    if let Some(user_id) = user_id {
        params.push(("user_id", user_id));
    }
    if let Some(since) = query.since.filter(|v| !v.trim().is_empty()) {
        params.push(("since", since));
    }
    if let Some(until) = query.until.filter(|v| !v.trim().is_empty()) {
        params.push(("until", until));
    }
    if let Some(limit) = query.limit {
        params.push(("limit", limit.clamp(1, MAX_AUDIT_LIMIT).to_string()));
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

/// Decides the `user_id` filter forwarded upstream. Admins keep whatever they
/// asked for. Everyone else is pinned to their own id: a foreign `user_id` is an
/// `Err` (403), and a request with no `user_id` is rewritten to the caller's id
/// unless it is the redacted org-wide `tool_action` view, which audit-core serves.
fn scoped_user_id(
    is_admin: bool,
    caller_id: &str,
    event: Option<&str>,
    requested: Option<String>,
) -> Result<Option<String>, ()> {
    if is_admin {
        return Ok(requested);
    }
    match requested {
        Some(requested) if requested != caller_id => Err(()),
        Some(requested) => Ok(Some(requested)),
        None if event == Some(TOOL_ACTION_EVENT) => Ok(None),
        None => Ok(Some(caller_id.to_string())),
    }
}
