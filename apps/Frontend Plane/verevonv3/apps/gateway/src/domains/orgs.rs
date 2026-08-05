mod deletion;
mod info;
mod members;
mod roles;
mod settings;
mod shared;
mod switch;

use axum::{
    routing::{delete, get, patch, post},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/orgs", get(switch::list_orgs))
        // Static "switch-active" must be registered before the ":id" param routes
        .route(
            "/api/v1/orgs/switch-active",
            post(switch::switch_active_org),
        )
        .route(
            "/api/v1/orgs/invitations/:invitation_id/accept",
            post(members::accept_invitation),
        )
        // Org info
        .route("/api/v1/orgs/:id", get(info::get_org))
        .route(
            "/api/v1/orgs/:id/entitlements",
            get(info::get_org_entitlements),
        )
        // Org-admin settings (interactive Zero-Data-Retention posture)
        .route(
            "/api/v1/orgs/:id/settings",
            patch(settings::update_org_settings),
        )
        // Members
        .route("/api/v1/orgs/:id/members", get(members::list_members))
        .route(
            "/api/v1/orgs/:id/members/invite",
            post(members::invite_member),
        )
        .route(
            "/api/v1/orgs/:id/members/:user_id",
            delete(members::remove_member),
        )
        .route(
            "/api/v1/orgs/:id/members/:user_id/role",
            patch(members::update_member_role),
        )
        // Roles
        // Custom role writes are intentionally not exposed: Auth Core owns
        // canonical memberships and the MVP supports its built-in roles only.
        .route("/api/v1/orgs/:id/roles", get(roles::list_roles))
        // GDPR org deletion (Flow C — 30-day soft-delete grace window). See
        // `orgs::deletion` for the org-core contract and gating rationale.
        .route(
            "/api/v1/orgs/:id/gdpr/soft-delete",
            delete(deletion::soft_delete),
        )
        .route("/api/v1/orgs/:id/gdpr/restore", post(deletion::restore))
        .route(
            "/api/v1/orgs/:id/gdpr/deletion/mark-exported",
            post(deletion::mark_exported),
        )
        .route(
            "/api/v1/orgs/:id/gdpr/deletion/acknowledge",
            post(deletion::acknowledge),
        )
        .route(
            "/api/v1/orgs/:id/gdpr/deletion/status",
            get(deletion::get_status),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
