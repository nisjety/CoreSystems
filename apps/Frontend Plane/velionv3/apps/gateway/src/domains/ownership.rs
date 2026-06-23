//! Per-user ownership honesty-gate signal (PR-4/PR-6).
//!
//! THE GATE. The velionv3 SPA must render NO per-user privacy affordance —
//! Private/Shared badge, "Private" visibility option, or ShareDialog — unless
//! BOTH are true:
//!   1. `CONTROL_PLANE_ENFORCEMENT == "strict"` (retrieval + documents-api are
//!      enforcing ownership in the same release), and
//!   2. viewer identity is live (a validated, token-verified session user).
//!
//! Shipping a "Private" affordance while either is false would be a FALSE-PRIVACY
//! guarantee — worse than nothing. So `gate_open` is computed SERVER-SIDE here
//! (the single source of truth) from the same `CONTROL_PLANE_ENFORCEMENT` value
//! retrieval-engine reads; the client only obeys it, it can never decide it.
//!
//! Pure, local read of `AppState` + the session — no upstream call.

use axum::{
    extract::{Extension, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde_json::json;

use crate::{
    config::AppState,
    envelope::ok,
    middleware::{require_session, AuthenticatedUser},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/ownership/status", get(ownership_status))
        // require_session — identity must be live to even ask; an unauthenticated
        // caller can never observe gate_open=true.
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// GET /api/v1/ownership/status →
/// `{ data: { enforcement, identity_live, gate_open } }`.
///
/// `gate_open` is the ONLY flag the frontend should branch on for rendering any
/// privacy affordance. It is `enforcement == "strict" && identity_live`.
async fn ownership_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let enforcement = state.enforcement_mode.clone();
    let identity_live = !user.user_id.trim().is_empty();
    let gate_open = enforcement == "strict" && identity_live;
    Json(ok(json!({
        "enforcement": enforcement,
        "identity_live": identity_live,
        "gate_open": gate_open,
    })))
}
