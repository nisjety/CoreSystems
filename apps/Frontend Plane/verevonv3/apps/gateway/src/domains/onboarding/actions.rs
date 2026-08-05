use axum::{routing::post, Router};

use crate::{
    config::AppState,
    onboarding::actions::{
        cleanup_source, confirm_checkout, create_organization, discover_source, set_plan,
        start_checkout, start_connect_session, start_integration_sync, start_website_ingest,
        warm_sharepoint_discovery,
    },
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/onboarding/actions/create-organization",
            post(create_organization),
        )
        .route("/api/v1/onboarding/actions/set-plan", post(set_plan))
        .route(
            "/api/v1/onboarding/actions/start-checkout",
            post(start_checkout),
        )
        .route(
            "/api/v1/onboarding/actions/confirm-checkout",
            post(confirm_checkout),
        )
        .route(
            "/api/v1/onboarding/actions/start-website-ingest",
            post(start_website_ingest),
        )
        .route(
            "/api/v1/onboarding/actions/start-connect-session",
            post(start_connect_session),
        )
        .route(
            "/api/v1/onboarding/actions/discover-source",
            post(discover_source),
        )
        .route(
            "/api/v1/onboarding/actions/cleanup-source",
            post(cleanup_source),
        )
        .route(
            "/api/v1/onboarding/actions/warm-sharepoint-discovery",
            post(warm_sharepoint_discovery),
        )
        .route(
            "/api/v1/onboarding/actions/start-integration-sync",
            post(start_integration_sync),
        )
}
