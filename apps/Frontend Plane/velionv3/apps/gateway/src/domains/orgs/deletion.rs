//! Flow C — 30-day organization soft-delete grace window.
//!
//! Proxies to org-core's `/orgs/:id/gdpr/*` surface (`internal/http/gdpr_handlers.go`).
//! org-core owns the precise authorization decision in every handler
//! (`authorizeOrgErasure` for soft-delete/restore is owner-only;
//! `authorizeDeletionSelfService` for the member self-service routes accepts
//! any active member or a platform admin) — the gateway checks below are a
//! broad first gate, matching the existing `update_org_settings` /
//! `orgs::info` pattern of scoping every call to the session's live active
//! membership before proxying.

use axum::{
    extract::{Extension, Path, State},
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, middleware::AuthenticatedUser, upstream::proxy_json};

use super::shared::{actor_for, require_active_org, require_org_admin, GatewayJsonResponse};

/// Build an org-core GDPR-deletion URL. org-core mounts these routes directly
/// on its root router (the "frontend proxy routes" block in `server.go`), so
/// the path is `/orgs/:id/gdpr/<suffix>` — NOT under `/api/v1/organizations`.
fn deletion_url(org_core_url: &str, id: &str, suffix: &str) -> String {
    format!(
        "{}/orgs/{}/gdpr/{}",
        org_core_url.trim_end_matches('/'),
        urlencoding::encode(id),
        suffix
    )
}

/// DELETE /api/v1/orgs/:id/gdpr/soft-delete
///
/// Body: `{ "confirm": true, "org_name": "<exact org name>" }`. Owner-gated;
/// org-core returns 400 if `confirm` is false or `org_name` does not exactly
/// match. On success this opens the 30-day grace window (ledger rows +
/// `velion.org.deletion.pending`).
pub(super) async fn soft_delete(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    let url = deletion_url(&state.org_core_url, &id, "soft-delete");
    proxy_json(
        &state,
        Method::DELETE,
        &url,
        Some(body),
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// POST /api/v1/orgs/:id/gdpr/restore
///
/// Reverses a pending soft-delete: clears `deleted_at`, wipes the deletion
/// ledger, publishes `velion.org.deletion.cancelled`. 409 if the organization
/// is not currently pending deletion. Same owner-gated pattern as `soft_delete`.
pub(super) async fn restore(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    let url = deletion_url(&state.org_core_url, &id, "restore");
    proxy_json(
        &state,
        Method::POST,
        &url,
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// POST /api/v1/orgs/:id/gdpr/deletion/mark-exported
///
/// Records that the calling member received their personal-data export ahead
/// of the org's scheduled purge. Any active member of the org may call this
/// for themselves — org-core's `authorizeDeletionSelfService` makes the final
/// self-or-platform-admin decision.
pub(super) async fn mark_exported(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_active_org(&user, &id) {
        return response;
    }
    let url = deletion_url(&state.org_core_url, &id, "deletion/mark-exported");
    proxy_json(
        &state,
        Method::POST,
        &url,
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// POST /api/v1/orgs/:id/gdpr/deletion/acknowledge
///
/// Records that the calling member acknowledged the org's pending-deletion
/// notice. Same gating as `mark_exported`.
pub(super) async fn acknowledge(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_active_org(&user, &id) {
        return response;
    }
    let url = deletion_url(&state.org_core_url, &id, "deletion/acknowledge");
    proxy_json(
        &state,
        Method::POST,
        &url,
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// GET /api/v1/orgs/:id/gdpr/deletion/status
///
/// Whether the org is inside its 30-day grace window, the caller's own
/// export/acknowledge checkpoint, and — for an owner/admin caller — every
/// member's ledger row. Same gating as `mark_exported`.
pub(super) async fn get_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_active_org(&user, &id) {
        return response;
    }
    let url = deletion_url(&state.org_core_url, &id, "deletion/status");
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_gdpr_deletion_urls_without_the_api_v1_organizations_prefix() {
        // org-core mounts these on its root router, not under /api/v1/organizations
        // (see `server.go`'s "Frontend proxy routes" block) — a regression here
        // would silently 404 every deletion call.
        assert_eq!(
            deletion_url("http://org-core:8080", "org_1", "soft-delete"),
            "http://org-core:8080/orgs/org_1/gdpr/soft-delete"
        );
        assert_eq!(
            deletion_url("http://org-core:8080", "org_1", "restore"),
            "http://org-core:8080/orgs/org_1/gdpr/restore"
        );
        assert_eq!(
            deletion_url("http://org-core:8080", "org_1", "deletion/mark-exported"),
            "http://org-core:8080/orgs/org_1/gdpr/deletion/mark-exported"
        );
        assert_eq!(
            deletion_url("http://org-core:8080", "org_1", "deletion/acknowledge"),
            "http://org-core:8080/orgs/org_1/gdpr/deletion/acknowledge"
        );
        assert_eq!(
            deletion_url("http://org-core:8080", "org_1", "deletion/status"),
            "http://org-core:8080/orgs/org_1/gdpr/deletion/status"
        );
    }

    #[test]
    fn url_encodes_the_org_id_segment() {
        assert_eq!(
            deletion_url("http://org-core:8080", "org with space", "restore"),
            "http://org-core:8080/orgs/org%20with%20space/gdpr/restore"
        );
    }

    #[test]
    fn trims_a_trailing_slash_on_the_configured_base_url() {
        assert_eq!(
            deletion_url("http://org-core:8080/", "org_1", "restore"),
            "http://org-core:8080/orgs/org_1/gdpr/restore"
        );
    }
}
