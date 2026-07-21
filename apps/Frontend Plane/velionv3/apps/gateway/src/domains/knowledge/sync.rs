//! Knowledge workspace sync + SharePoint source registration.
//!
//! Rust port of velionv2's `syncKnowledgeWorkspace` and
//! `createSharePointKnowledgeSource`. `POST /api/v1/knowledge/sync` triggers a
//! re-sync of every active integration connection (integration-core) and finspo
//! SharePoint source; `POST /api/v1/knowledge/sharepoint` registers a new finspo
//! source and kicks off its first sync.

use std::time::Duration;

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use futures_util::future::join_all;
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, domains::knowledge::shared, envelope::error, middleware::AuthenticatedUser,
};

const SYNC_TIMEOUT: Duration = Duration::from_secs(6);
const LIST_TIMEOUT: Duration = Duration::from_secs(4);

pub(super) async fn sync_workspace(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let org = crate::upstream::authorized_org_id(&state, &user).await;
    if org.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("no_active_org", "No active organization found.")),
        )
            .into_response();
    }
    let actor = shared::actor_for(&user);

    let connections_url = format!(
        "{}/api/v1/connections?organizationId={}",
        state.integration_core_url,
        urlencoding::encode(&org)
    );
    let finspo_sources_url = format!("{}/api/v1/sources", state.finspo_core_url);
    let (connections_payload, finspo_payload) = tokio::join!(
        shared::fetch_json(
            &state,
            Method::GET,
            &connections_url,
            None,
            Some(&org),
            &actor,
            LIST_TIMEOUT
        ),
        shared::fetch_json(
            &state,
            Method::GET,
            &finspo_sources_url,
            None,
            Some(&org),
            &actor,
            LIST_TIMEOUT
        ),
    );

    let connections = extract_list(connections_payload.as_ref(), "connections")
        .into_iter()
        .filter(|c| str_any(c, &["deletedAt", "deleted_at"]).is_empty())
        .filter_map(|c| {
            let id = str_any(&c, &["id", "connectionId", "connection_id"]);
            if id.is_empty() {
                return None;
            }
            let label = first_non_empty([
                str_any(&c, &["displayName", "display_name"]),
                str_any(&c, &["providerLabel", "provider_label"]),
                str_any(&c, &["providerKey", "provider_key"]),
                id.clone(),
            ]);
            Some((id, label))
        })
        .collect::<Vec<_>>();

    let finspo_sources = extract_list(finspo_payload.as_ref(), "sources")
        .into_iter()
        .filter_map(|s| {
            let id = str_any(&s, &["id"]);
            if id.is_empty() {
                return None;
            }
            let label = first_non_empty([
                str_any(&s, &["drive_name", "driveName"]),
                str_any(&s, &["site_id", "siteId"]),
                id.clone(),
            ]);
            Some((id, label))
        })
        .collect::<Vec<_>>();

    let integration_results = join_all(connections.into_iter().map(|(id, label)| {
        let state = &state;
        let actor = &actor;
        let org = &org;
        async move {
            let url = format!(
                "{}/api/v1/connections/{}/sync",
                state.integration_core_url,
                urlencoding::encode(&id)
            );
            if shared::request_ok(
                state,
                Method::POST,
                &url,
                None,
                Some(org),
                actor,
                SYNC_TIMEOUT,
            )
            .await
            {
                Ok(())
            } else {
                Err(label)
            }
        }
    }))
    .await;

    let finspo_results = join_all(finspo_sources.into_iter().map(|(id, label)| {
        let state = &state;
        let actor = &actor;
        let org = &org;
        async move {
            let url = format!(
                "{}/api/v1/sources/{}/sync",
                state.finspo_core_url,
                urlencoding::encode(&id)
            );
            if shared::request_ok(
                state,
                Method::POST,
                &url,
                None,
                Some(org),
                actor,
                SYNC_TIMEOUT,
            )
            .await
            {
                Ok(())
            } else {
                Err(label)
            }
        }
    }))
    .await;

    let integration_failures: Vec<String> = integration_results
        .iter()
        .filter_map(|r| r.as_ref().err().cloned())
        .collect();
    let finspo_failures: Vec<String> = finspo_results
        .iter()
        .filter_map(|r| r.as_ref().err().cloned())
        .collect();
    let integration_started = integration_results.len() - integration_failures.len();
    let finspo_started = finspo_results.len() - finspo_failures.len();

    Json(json!({
        "integrationStarted": integration_started,
        "integrationFailures": integration_failures,
        "finspoStarted": finspo_started,
        "finspoFailures": finspo_failures,
    }))
    .into_response()
}

/// `GET /api/v1/knowledge/sharepoint/sites` — list the org's SharePoint sites
/// (via finspo's Graph browser) so the Add-source UI can offer a pick-a-site
/// step instead of asking the user to hand-enter a raw composite Site ID.
/// Read-only; requires the org's Microsoft integration to be connected (finspo
/// returns `503 not_configured` otherwise, surfaced verbatim).
pub(super) async fn list_sharepoint_sites(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let org = crate::upstream::authorized_org_id(&state, &user).await;
    if org.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("no_active_org", "No active organization found.")),
        )
            .into_response();
    }
    let actor = shared::actor_for(&user);
    let url = format!("{}/api/v1/sharepoint/sites", state.finspo_core_url);
    match shared::fetch_json(&state, Method::GET, &url, None, Some(&org), &actor, LIST_TIMEOUT).await
    {
        Some(payload) => (StatusCode::OK, Json(payload)).into_response(),
        None => (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "sharepoint_sites_unavailable",
                "Could not list SharePoint sites. Connect a Microsoft 365 integration first.",
            )),
        )
            .into_response(),
    }
}

/// `GET /api/v1/knowledge/sharepoint/sites/:site_id/drives` — list a site's
/// document libraries (drives) so the UI can offer a pick-a-library step that
/// auto-fills the Drive ID / name / type on the register form.
pub(super) async fn list_sharepoint_drives(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(site_id): Path<String>,
) -> Response {
    let org = crate::upstream::authorized_org_id(&state, &user).await;
    if org.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("no_active_org", "No active organization found.")),
        )
            .into_response();
    }
    let site = site_id.trim();
    if site.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("site_id_required", "A SharePoint site is required.")),
        )
            .into_response();
    }
    let actor = shared::actor_for(&user);
    // finspo's route param is a Graph site id; percent-encode it so a composite
    // id containing commas/slashes survives the upstream path.
    let encoded = urlencoding::encode(site);
    let url = format!(
        "{}/api/v1/sharepoint/sites/{}/drives",
        state.finspo_core_url, encoded
    );
    match shared::fetch_json(&state, Method::GET, &url, None, Some(&org), &actor, LIST_TIMEOUT).await
    {
        Some(payload) => (StatusCode::OK, Json(payload)).into_response(),
        None => (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "sharepoint_drives_unavailable",
                "Could not list document libraries for this site.",
            )),
        )
            .into_response(),
    }
}

pub(super) async fn register_sharepoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(input): Json<Value>,
) -> Response {
    let org = crate::upstream::authorized_org_id(&state, &user).await;
    if org.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("no_active_org", "No active organization found.")),
        )
            .into_response();
    }
    let actor = shared::actor_for(&user);

    let create_body = json!({
        "tenant_id": trimmed(&input, "tenantId"),
        "site_id": trimmed(&input, "siteId"),
        "site_web_url": trimmed(&input, "siteWebUrl"),
        "drive_id": trimmed(&input, "driveId"),
        "drive_name": trimmed(&input, "driveName"),
        "drive_type": trimmed(&input, "driveType"),
        "enabled": true,
    });
    let create_url = format!("{}/api/v1/sources", state.finspo_core_url);
    let created = shared::fetch_json(
        &state,
        Method::POST,
        &create_url,
        Some(create_body),
        Some(&org),
        &actor,
        SYNC_TIMEOUT,
    )
    .await;

    let source_id = created
        .as_ref()
        .and_then(|p| {
            let scope = p.get("data").filter(|d| d.is_object()).unwrap_or(p);
            scope.get("id").and_then(Value::as_str).map(str::to_owned)
        })
        .filter(|s| !s.is_empty());

    let Some(source_id) = source_id else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "sharepoint_register_failed",
                "SharePoint source could not be registered.",
            )),
        )
            .into_response();
    };

    let sync_url = format!(
        "{}/api/v1/sources/{}/sync",
        state.finspo_core_url,
        urlencoding::encode(&source_id)
    );
    let sync_started = shared::request_ok(
        &state,
        Method::POST,
        &sync_url,
        None,
        Some(&org),
        &actor,
        SYNC_TIMEOUT,
    )
    .await;

    Json(json!({ "id": source_id, "syncStarted": sync_started })).into_response()
}

// ── Value extraction ──────────────────────────────────────────────────────

fn extract_list(payload: Option<&Value>, key: &str) -> Vec<Value> {
    let Some(payload) = payload else {
        return vec![];
    };
    let scope = payload
        .get("data")
        .filter(|d| d.is_object())
        .unwrap_or(payload);
    scope
        .get(key)
        .and_then(Value::as_array)
        .map(|arr| arr.to_vec())
        .unwrap_or_default()
}

fn str_any(value: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(found) = value.get(key).and_then(Value::as_str) {
            let trimmed = found.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }
    }
    String::new()
}

fn first_non_empty<const N: usize>(candidates: [String; N]) -> String {
    candidates
        .into_iter()
        .find(|c| !c.trim().is_empty())
        .unwrap_or_default()
}

fn trimmed(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}
