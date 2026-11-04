pub(crate) mod deletion;
mod info;
pub(crate) mod instructions;
pub(crate) mod members;
pub(crate) mod model_entitlement;
pub(crate) mod quotas;
mod retention;
mod roles;
pub(crate) mod settings;
mod shared;
pub(crate) mod space_policy;
pub(crate) mod switch;
mod website;

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
        // The organisation's own website, recorded in org-core (the company facts owner)
        .route(
            "/api/v1/orgs/{id}/website",
            patch(website::update_org_website),
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
        // Org model entitlement (WS10-S5, ADR-0005): which chat models the org and
        // each role may use. org-core stores and explains it; inference-core
        // enforces it. Behind `AppState::model_entitlement_admin_enabled`.
        .route(
            "/api/v1/orgs/{id}/model-entitlement",
            get(model_entitlement::get_model_entitlement)
                .put(model_entitlement::put_model_entitlement),
        )
        .route(
            "/api/v1/orgs/{id}/model-entitlement/explain",
            get(model_entitlement::explain_model_entitlement),
        )
        // EP-9 C1: the retention window and legal holds. org-core stores them and
        // has no such route while ORG_RETENTION_POLICY_ENABLED is off, which these
        // answer as a typed 404.
        .route(
            "/api/v1/orgs/{id}/retention",
            get(retention::get_retention).put(retention::put_retention),
        )
        .route(
            "/api/v1/orgs/{id}/legal-holds",
            get(retention::list_legal_holds).post(retention::place_legal_hold),
        )
        .route(
            "/api/v1/orgs/{id}/legal-holds/{hold_id}/release",
            post(retention::release_legal_hold),
        )
        // WS03-S4: org-admin Space effect-policy console routes (Control owns the row).
        // Behind `SpaceFeatureFlags::policy_admin_api`; off answers 404.
        .route(
            "/api/v1/orgs/{id}/space-policy",
            get(space_policy::get_space_policy),
        )
        .route(
            "/api/v1/orgs/{id}/space-policy/entitlements",
            put(space_policy::put_space_policy_entitlements),
        )
        .route(
            "/api/v1/orgs/{id}/space-policy/events",
            get(space_policy::list_space_policy_events),
        )
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
