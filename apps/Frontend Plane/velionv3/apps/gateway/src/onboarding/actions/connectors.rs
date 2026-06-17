use axum::{extract::State, http::HeaderMap, response::IntoResponse, Json};
use reqwest::Method;
use serde_json::json;

use crate::{
    auth::actor_from_request,
    config::AppState,
    contracts::{
        ConnectSessionRequest, OrgActionRequest, SourceCleanupRequest, SourceDiscoveryRequest,
    },
    upstream::proxy_json,
    utils::{empty_to_none, trim_opt},
};

pub(crate) async fn start_connect_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ConnectSessionRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "organizationId": input.org_id.trim(),
        "workspaceId": input.org_id.trim(),
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
        Some(input.org_id.trim()),
        Some(&actor),
        Some("application/json"),
    )
    .await
}

pub(crate) async fn discover_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SourceDiscoveryRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "organizationId": input.org_id.trim(),
        "workspaceId": input.org_id.trim(),
        "connectorId": input.connector_id.trim(),
        "label": input.label,
        "provider": input.provider.trim(),
        "sources": input.sources,
    });

    proxy_json(
        &state,
        Method::POST,
        &format!(
            "{}/api/v1/onboarding/discover-source",
            state.integration_core_url
        ),
        Some(body),
        Some(input.org_id.trim()),
        Some(&actor),
        Some("application/json"),
    )
    .await
}

pub(crate) async fn cleanup_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SourceCleanupRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "organizationId": input.org_id.trim(),
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
        Some(input.org_id.trim()),
        Some(&actor),
        Some("application/json"),
    )
    .await
}

pub(crate) async fn warm_sharepoint_discovery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<OrgActionRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "organizationId": input.org_id.trim(),
        "workspaceId": input.org_id.trim(),
    });

    proxy_json(
        &state,
        Method::POST,
        &format!(
            "{}/api/v1/providers/microsoft/sharepoint/discovery/warm",
            state.integration_core_url
        ),
        Some(body),
        Some(input.org_id.trim()),
        Some(&actor),
        Some("application/json"),
    )
    .await
}

pub(crate) async fn start_integration_sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SourceDiscoveryRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "organizationId": input.org_id.trim(),
        "workspaceId": input.org_id.trim(),
        "connectorId": input.connector_id.trim(),
        "provider": input.provider.trim(),
        "sources": input.sources,
    });

    proxy_json(
        &state,
        Method::POST,
        &format!(
            "{}/api/v1/providers/{}/sync",
            state.integration_core_url,
            urlencoding::encode(input.provider.trim())
        ),
        Some(body),
        Some(input.org_id.trim()),
        Some(&actor),
        Some("application/json"),
    )
    .await
}
