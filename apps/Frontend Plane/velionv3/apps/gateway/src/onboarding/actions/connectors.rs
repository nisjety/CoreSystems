use axum::{
    extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse, Extension, Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    auth::actor_from_request,
    config::AppState,
    contracts::{
        ActionActor, ConnectSessionRequest, OrgActionRequest, SourceCleanupRequest,
        SourceDiscoveryRequest,
    },
    envelope::error,
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json},
    utils::{empty_to_none, trim_opt},
};

async fn exact_authorized_org(
    state: &AppState,
    user: &AuthenticatedUser,
    requested_org_id: &str,
) -> Result<String, (StatusCode, Json<Value>)> {
    let authorized = authorized_org_id(state, user).await;
    if authorized.is_empty() || requested_org_id.trim() != authorized {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "The requested organization is not authorized for this session.",
            )),
        ));
    }
    Ok(authorized)
}

/// Resolve the org's connection id for a provider via integration-core's
/// connections list (newest active connection wins). The onboarding SPA only
/// knows the provider key it just connected; integration-core v2's discovery
/// and sync endpoints are per-connection.
async fn resolve_connection_id(
    state: &AppState,
    actor: &ActionActor,
    org_id: &str,
    provider: &str,
) -> Result<String, (StatusCode, Json<Value>)> {
    let (status, Json(body)) = proxy_json(
        state,
        Method::GET,
        &format!(
            "{}/api/v1/connections?organizationId={}&providerKey={}",
            state.integration_core_url,
            urlencoding::encode(org_id),
            urlencoding::encode(provider)
        ),
        None,
        Some(org_id),
        Some(actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err((status, Json(body)));
    }
    let connections = body
        .pointer("/data/connections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pick = connections
        .iter()
        .find(|c| c.get("status").and_then(Value::as_str) == Some("active"))
        .or_else(|| connections.first());
    match pick.and_then(|c| c.get("id")).and_then(Value::as_str) {
        Some(id) if !id.trim().is_empty() => Ok(id.to_string()),
        _ => Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "code": "connection_not_found",
                    "message": format!("No {provider} connection exists for this organization yet."),
                }
            })),
        )),
    }
}

pub(crate) async fn start_connect_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<ConnectSessionRequest>,
) -> impl IntoResponse {
    let org_id = match exact_authorized_org(&state, &user, &input.org_id).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "organizationId": org_id,
        "workspaceId": org_id,
        "userId": actor.user_id,
        "userEmail": empty_to_none(&actor.user_email),
        "selectedSources": input.selected_sources,
        "bundles": input.bundles.unwrap_or_else(|| vec!["onboarding".into()]),
        "providerContext": input.provider_context,
        "shop": trim_opt(input.shop),
    });

    proxy_json(
        &state,
        Method::POST,
        &format!(
            "{}/api/v1/providers/{}/connect-session",
            state.integration_core_url,
            urlencoding::encode(input.provider.trim())
        ),
        Some(body),
        Some(&org_id),
        Some(&actor),
        Some("application/json"),
    )
    .await
}

// integration-core v2 has no /api/v1/onboarding/discover-source (that was a
// v1 path — proxying it produced the 404 the SPA surfaced as "synkronisering
// kan ha feilet"). v2 discovery is GET /api/v1/connections/{id}/discovery, so
// resolve the freshly-created connection first.
pub(crate) async fn discover_source(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<SourceDiscoveryRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let org_id = match exact_authorized_org(&state, &user, &input.org_id).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let provider = input.provider.trim().to_string();

    let connection_id = match resolve_connection_id(&state, &actor, &org_id, &provider).await {
        Ok(id) => id,
        Err(err) => return err,
    };

    let (status, Json(body)) = proxy_json(
        &state,
        Method::GET,
        &format!(
            "{}/api/v1/connections/{}/discovery",
            state.integration_core_url,
            urlencoding::encode(&connection_id)
        ),
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(body));
    }
    // The SPA contract is `{ id?, discovered? }`; keep the discovery snapshot
    // alongside for richer consumers.
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "data": {
                "id": connection_id,
                "discovered": true,
                "discovery": body.pointer("/data").cloned().unwrap_or(Value::Null),
            }
        })),
    )
}

pub(crate) async fn cleanup_source(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<SourceCleanupRequest>,
) -> impl IntoResponse {
    let org_id = match exact_authorized_org(&state, &user, &input.org_id).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "organizationId": org_id,
        "sourceId": input.source_id.trim(),
    });

    proxy_json(
        &state,
        Method::POST,
        &format!(
            "{}/api/v1/onboarding/cleanup-source",
            state.integration_core_url
        ),
        Some(body),
        Some(&org_id),
        Some(&actor),
        Some("application/json"),
    )
    .await
}

pub(crate) async fn warm_sharepoint_discovery(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<OrgActionRequest>,
) -> impl IntoResponse {
    let org_id = match exact_authorized_org(&state, &user, &input.org_id).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "organizationId": org_id,
        "workspaceId": org_id,
    });

    proxy_json(
        &state,
        Method::POST,
        &format!(
            "{}/api/v1/providers/microsoft/sharepoint/discovery/warm",
            state.integration_core_url
        ),
        Some(body),
        Some(&org_id),
        Some(&actor),
        Some("application/json"),
    )
    .await
}

// integration-core v2 has no /api/v1/providers/{provider}/sync (v1 path — the
// second 404 behind "synkronisering kan ha feilet"). v2 queues sync jobs per
// connection: POST /api/v1/connections/{id}/sync.
pub(crate) async fn start_integration_sync(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<SourceDiscoveryRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let org_id = match exact_authorized_org(&state, &user, &input.org_id).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let provider = input.provider.trim().to_string();

    let connection_id = match resolve_connection_id(&state, &actor, &org_id, &provider).await {
        Ok(id) => id,
        Err(err) => return err,
    };

    let (status, Json(body)) = proxy_json(
        &state,
        Method::POST,
        &format!(
            "{}/api/v1/connections/{}/sync",
            state.integration_core_url,
            urlencoding::encode(&connection_id)
        ),
        Some(json!({})),
        Some(&org_id),
        Some(&actor),
        Some("application/json"),
    )
    .await;
    if !status.is_success() {
        return (status, Json(body));
    }
    let job_id = body
        .pointer("/data/syncJob/id")
        .and_then(Value::as_str)
        .unwrap_or(&connection_id)
        .to_string();
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "data": {
                "id": job_id,
                "started": true,
                "connectionId": connection_id,
            }
        })),
    )
}
