//! Ingestions domain.
//!
//! Backs the Verevon v3 SPA's `/api/ingestions/*` surface (runs, schedules,
//! actions, sources, profiles, evidence) by proxying quarry-edge on the
//! Ingestion Plane — the JWT-authenticated face of Quarry-v2 that owns the
//! execution + profile endpoints and forwards the durable registry to
//! quarry-control internally. All routes sit behind `require_session`; org
//! scope is derived from the authenticated session (bearer audience token for
//! quarry, session-resolved org for cross-plane lookups) — never a client
//! header. See [`shared`] for the auth/scoping model and [`sources`] for the
//! cross-plane source aggregation.

mod evidence;
mod profiles;
mod runs;
mod schedules;
mod shared;
mod sources;

use axum::{
    routing::{delete, get, post},
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
            "/api/ingestions/schedules",
            get(schedules::list_schedules).post(schedules::create_schedule),
        )
        .route("/api/ingestions/actions", post(schedules::schedule_actions))
        .route(
            "/api/ingestions/sources",
            get(sources::list_sources).post(sources::create_source),
        )
        .route(
            "/api/ingestions/sources/:id",
            delete(sources::delete_source),
        )
        .route("/api/ingestions/profiles", get(profiles::list_profiles))
        .route("/api/ingestions/evidence", get(evidence::get_evidence))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
