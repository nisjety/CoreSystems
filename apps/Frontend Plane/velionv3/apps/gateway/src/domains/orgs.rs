mod info;
mod members;
mod roles;
mod shared;
mod switch;

use axum::{
    routing::{delete, get, patch, post},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        // Static "switch-active" must be registered before the ":id" param routes
        .route(
            "/api/v1/orgs/switch-active",
            post(switch::switch_active_org),
        )
        // Org info
        .route("/api/v1/orgs/:id", get(info::get_org))
        .route(
            "/api/v1/orgs/:id/entitlements",
            get(info::get_org_entitlements),
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
        .route("/api/v1/orgs/:id/roles", get(roles::list_roles))
        .route("/api/v1/orgs/:id/roles", post(roles::create_role))
        .route(
            "/api/v1/orgs/:id/roles/:role_name",
            patch(roles::update_role),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
