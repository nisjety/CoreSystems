//! Authenticated Pulse boundary. Marketing Core is currently single-organization;
//! its explicit deployment binding MUST match live canonical membership before
//! its server key is used. No implicit demo scope or browser bearer is accepted.
mod contract;
mod assignees;
mod tenant_routes;
mod projection;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod audit_tests;
#[cfg(test)]
mod monitoring_tests;
#[cfg(test)]
mod insights_tests;
#[cfg(test)]
mod preparation_tests;
#[cfg(test)]
mod analytics_sync_tests;
mod website_list_tests;
#[cfg(test)]
mod public_signals_tests;
#[cfg(test)]
mod trends_tests;
#[cfg(test)]
mod web_vitals_tests;
#[cfg(test)]
mod site_sources_tests;

use crate::{
    config::AppState,
    envelope::error,
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Extension, OriginalUri, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Json, Router,
};
use contract::{route_spec, validate_command, validate_query};
use futures_util::StreamExt;
use serde_json::json;
#[cfg(test)]
use axum::body::Body;

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/marketing/{*path}", any(handle))
        .route("/api/v1/visibility/{*path}", any(handle))
        .layer(DefaultBodyLimit::max(8192))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

fn scope_allowed(binding: &str, user: &AuthenticatedUser, write: bool) -> bool {
    !binding.is_empty()
        && user.active_org_id.as_deref() == Some(binding)
        && user
            .authorized_membership
            .as_ref()
            .is_some_and(|membership| membership.organization_id == binding)
        && has_authorized_org_role(
            user,
            if write {
                &["owner", "admin", "member"]
            } else {
                &["owner", "admin", "member", "viewer"]
            },
        )
}

fn failure(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (
        status,
        [
            ("cache-control", "no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        Json(error(code, message)),
    )
        .into_response()
}

async fn handle(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let route = uri.path().strip_prefix("/api/v1/visibility/")
        .or_else(|| uri.path().strip_prefix("/api/v1/marketing/")).unwrap_or("");
    if uri.path().starts_with("/api/v1/visibility/") && route == "outcomes" {
        return failure(StatusCode::NOT_FOUND, "visibility_route_not_found", "Lead outcomes belong to Leads Core.");
    }
    proxy(
        &state,
        &user,
        &headers,
        method,
        route,
        uri.query().unwrap_or(""),
        body,
    )
    .await
}

async fn proxy(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    route: &str,
    query: &str,
    body: Bytes,
) -> Response {
    if contract::retired_dns_route(route) { return failure(StatusCode::GONE, "dns_verification_retired", "Studio DNS verification has been retired. Connect the relevant provider account."); }
    let Some(spec) = route_spec(route, &method) else {
        return failure(
            StatusCode::NOT_FOUND,
            "marketing_route_not_found",
            "This marketing operation is not supported.",
        );
    };
    let tenant=match tenant_routes::resolve(state,user.active_org_id.as_deref()) {
        Ok(value)=>value,Err(())=>return failure(StatusCode::SERVICE_UNAVAILABLE,"visibility_workspace_not_configured","Visibility is not configured for the active workspace."),
    };
    let write = method != Method::GET;
    if !scope_allowed(&tenant.organization_id, user, write) {
        return failure(
            StatusCode::FORBIDDEN,
            "marketing_scope_unavailable",
            "Marketing is not available in the active workspace.",
        );
    }
    if spec.administrative && !has_authorized_org_role(user, &["owner", "admin"]) {
        return failure(
            StatusCode::FORBIDDEN,
            "marketing_admin_required",
            "A workspace administrator must manage integrations.",
        );
    }
    if write
        && headers.get("origin").and_then(|value| value.to_str().ok())
            != Some(state.verevon_public_origin.as_str())
    {
        return failure(
            StatusCode::FORBIDDEN,
            "marketing_same_origin_required",
            "A same-origin request is required.",
        );
    }
    if write
        && headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            != Some("application/json")
    {
        return failure(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "marketing_json_required",
            "A JSON request is required.",
        );
    }
    let command = if write {
        if !query.is_empty() {
            return failure(
                StatusCode::BAD_REQUEST,
                "invalid_marketing_request",
                "Choose a valid marketing operation.",
            );
        }
        match validate_command(&spec, &body) {
            Ok(value) => Some(value),
            Err(()) => {
                return failure(
                    StatusCode::BAD_REQUEST,
                    "invalid_marketing_request",
                    "Choose a valid marketing operation.",
                )
            }
        }
    } else {
        if !body.is_empty() || validate_query(&spec, query).is_err() {
            return failure(
                StatusCode::BAD_REQUEST,
                "invalid_marketing_request",
                "Choose a valid registered website.",
            );
        }
        None
    };
    if route=="work/assignees" {
        return match assignees::members(state,user,headers).await {Ok(members)=>(StatusCode::OK,[("cache-control","no-store")],Json(json!({"data":{"members":members}}))).into_response(),Err(())=>unavailable()};
    }
    if route.ends_with("/planning") {
        if let Some(id)=command.as_ref().and_then(|v|v.get("assigneeId")).and_then(serde_json::Value::as_str).filter(|v|!v.is_empty()) {
            match assignees::members(state,user,headers).await {
                Ok(members) if members.iter().any(|m|m.get("id").and_then(serde_json::Value::as_str)==Some(id))=>{},
                Ok(_)=>return failure(StatusCode::BAD_REQUEST,"invalid_task_assignee","Choose a current workspace member."),
                Err(())=>return unavailable(),
            }
        }
    }
    // The URL is anchored exclusively to deployment configuration. Site URLs
    // are content for Marketing/Quarry, never destinations for the BFF to dial.
    let target = format!(
        "{}/api/v1/marketing/{}{}{}",
        tenant.url.trim_end_matches('/'),
        route,
        if query.is_empty() { "" } else { "?" },
        query
    );
    let mut request = state
        .client
        .request(method, &target)
        .header("accept", "application/json")
        .header("x-internal-api-key", &tenant.service_key);
    if route.ends_with("/google/resources"){request=request.timeout(std::time::Duration::from_secs(22));}
    if let Some(body) = command {
        if route == "trends" { request = request.timeout(std::time::Duration::from_secs(40)); }
        if route == "site-sources/sync" && body.get("provider").and_then(serde_json::Value::as_str) == Some("algolia") { request = request.timeout(std::time::Duration::from_secs(40)); }
        request = request.json(&body);
    }
    let Ok(upstream) = request.send().await else {
        return unavailable();
    };
    let status = upstream.status();
    if !status.is_success() {
        let status = match status.as_u16() {
            400 => StatusCode::BAD_REQUEST,
            404 => StatusCode::NOT_FOUND,
            409 => StatusCode::CONFLICT,
            429 => StatusCode::TOO_MANY_REQUESTS,
            503 => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::BAD_GATEWAY,
        };
        return failure(
            status,
            "marketing_request_unavailable",
            "The marketing request could not be completed. Refresh and retry.",
        );
    }
    if ![200, 201, 202].contains(&status.as_u16())
        || !upstream
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/json"))
    {
        return unavailable();
    }
    let mut stream = upstream.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return unavailable();
        };
        let maximum = match spec.kind {
            contract::Kind::SiteSources => 131_072,
            // Bounded projection: up to 1,000 observed source/target URL pairs,
            // plus the saved page sample and four rule-based groups.
            contract::Kind::Analysis | contract::Kind::Research | contract::Kind::Audit | contract::Kind::AuditInsights => 64 * 1024 * 1024,
            _ => 2 * 1024 * 1024,
        };
        if bytes.len() + chunk.len() > maximum {
            return unavailable();
        }
        bytes.extend_from_slice(&chunk);
    }
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return unavailable();
    };
    let Some(data) = payload
        .get("data")
        .and_then(|data| projection::sanitize(&spec, data))
    else {
        return unavailable();
    };
    (
        status,
        [
            ("cache-control", "no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        Json(json!({"data":data})),
    )
        .into_response()
}

fn unavailable() -> Response {
    failure(
        StatusCode::BAD_GATEWAY,
        "marketing_source_unavailable",
        "Marketing data is temporarily unavailable.",
    )
}
