//! Space room event stream (build plan P-SC1 stub; filled by WS02-S3).
//!
//! `GET /v1/spaces/:space_id/events` and its wake-guard and history helpers
//! land here, never in `sse.rs`. The stub registers no route.

use axum::Router;

use crate::state::AppState;

/// The Space event stream routes. Empty until WS02-S3.
pub fn router() -> Router<AppState> {
    Router::new()
}
