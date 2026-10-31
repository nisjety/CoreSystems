//! Ingestions domain.
//!
//! Backs the Verevon v3 SPA's `/api/ingestions/*` surface (runs, schedules,
//! actions, sources, profiles, evidence, change webhooks, document reads) by proxying quarry-edge on the
//! Ingestion Plane — the JWT-authenticated face of Quarry-v2 that owns the
//! execution + profile endpoints and forwards the durable registry to
//! quarry-control internally. All routes sit behind `require_session`; org
//! scope is derived from the authenticated session (bearer audience token for
//! quarry, session-resolved org for cross-plane lookups) — never a client
//! header. See [`shared`] for the auth/scoping model and [`sources`] for the
//! cross-plane source aggregation.

mod evidence;
pub(crate) mod jobs;
mod parse;
mod profiles;
pub(crate) mod runs;
pub(crate) mod schedules;
mod shared;
pub(crate) mod sources;
mod webhooks;

use axum::{
    routing::{delete, get, patch, post},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/ingestions/runs",
            get(runs::list_runs).post(runs::create_run),
        )
        .route(
            "/api/ingestions/schedules/capabilities",
            get(schedules::schedule_capabilities),
        )
        .route(
            "/api/ingestions/schedules",
            get(schedules::list_schedules).post(schedules::create_schedule),
        )
        .route("/api/ingestions/actions", post(schedules::schedule_actions))
        .route(
            "/api/ingestions/sources",
            get(sources::list_sources).post(sources::create_source),
        )
        .route(
            "/api/ingestions/sources/{id}",
            delete(sources::delete_source),
        )
        .route("/api/ingestions/jobs/{id}", get(jobs::get_job))
        .route("/api/ingestions/jobs/{id}/errors", get(jobs::get_job_errors))
        .route("/api/ingestions/jobs/{id}/results", get(jobs::get_job_results))
        .route("/api/ingestions/artifacts/{id}", get(jobs::get_artifact_text))
        .route("/api/ingestions/jobs/{id}/control", post(jobs::job_control))
        .route("/api/ingestions/usage", get(jobs::get_usage))
        .route("/api/ingestions/profiles", get(profiles::list_profiles))
        .route("/api/ingestions/evidence", get(evidence::get_evidence))
        .route("/api/ingestions/parse", post(parse::parse_document))
        .route(
            "/api/ingestions/webhooks",
            get(webhooks::list_webhooks).post(webhooks::create_webhook),
        )
        .route(
            "/api/ingestions/webhooks/{id}",
            patch(webhooks::update_webhook).delete(webhooks::delete_webhook),
        )
        .route(
            "/api/ingestions/webhooks/{id}/rotate-secret",
            post(webhooks::rotate_webhook_secret),
        )
        .route(
            "/api/ingestions/webhooks/{id}/deliveries",
            get(webhooks::list_webhook_deliveries),
        )
        .route(
            "/api/ingestions/webhooks/{id}/deliveries/{delivery_id}/retry",
            post(webhooks::retry_webhook_delivery),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
