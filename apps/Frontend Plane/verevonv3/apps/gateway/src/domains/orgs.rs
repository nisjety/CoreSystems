pub(crate) mod deletion;
mod info;
pub(crate) mod instructions;
pub(crate) mod members;
pub(crate) mod quotas;
mod roles;
pub(crate) mod settings;
mod shared;
pub(crate) mod switch;

use axum::{
    routing::{delete, get, patch, post, put},
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
            "/api/v1/orgs/invitations/{invitation_id}/accept",
            post(members::accept_invitation),
        )
        // Org info
        .route("/api/v1/orgs/{id}", get(info::get_org))
        .route(
            "/api/v1/orgs/{id}/entitlements",
            get(info::get_org_entitlements),
        )
        // Org-admin settings (interactive Zero-Data-Retention posture)
        .route(
            "/api/v1/orgs/{id}/settings",
            patch(settings::update_org_settings),
        )
        // ADR-0003 — org layer of the authored-instruction hierarchy (Convex,
        // not org-core, hence a separate route from `/settings` above).
        .route(
            "/api/v1/orgs/{id}/instructions",
            get(instructions::get_org_instructions).patch(instructions::update_org_instructions),
        )
        // Quotas (spend/token ceilings). Control Plane owns these; model-gateway
        // reads them for cost-core's budget check. Previously org-core had the
        // full settable API with no route or client reaching it, so an operator
        // could not set a ceiling the Model Plane was already enforcing.
        .route("/api/v1/orgs/{id}/quotas", get(quotas::list_quotas))
        .route("/api/v1/orgs/{id}/quotas/{key}", put(quotas::set_quota))
        // Members
        .route("/api/v1/orgs/{id}/members", get(members::list_members))
        .route(
            "/api/v1/orgs/{id}/members/invite",
            post(members::invite_member),
        )
        .route(
            "/api/v1/orgs/{id}/members/{user_id}",
            delete(members::remove_member),
        )
        .route(
            "/api/v1/orgs/{id}/members/{user_id}/role",
            patch(members::update_member_role),
        )
        // Roles
        // Custom role writes are intentionally not exposed: Auth Core owns
        // canonical memberships and the MVP supports its built-in roles only.
        .route("/api/v1/orgs/{id}/roles", get(roles::list_roles))
        // GDPR org deletion (Flow C — 30-day soft-delete grace window). See
        // `orgs::deletion` for the org-core contract and gating rationale.
        .route(
            "/api/v1/orgs/{id}/gdpr/soft-delete",
            delete(deletion::soft_delete),
        )
        .route("/api/v1/orgs/{id}/gdpr/restore", post(deletion::restore))
        .route(
            "/api/v1/orgs/{id}/gdpr/deletion/mark-exported",
            post(deletion::mark_exported),
        )
        .route(
            "/api/v1/orgs/{id}/gdpr/deletion/acknowledge",
            post(deletion::acknowledge),
        )
        .route(
            "/api/v1/orgs/{id}/gdpr/deletion/status",
            get(deletion::get_status),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
