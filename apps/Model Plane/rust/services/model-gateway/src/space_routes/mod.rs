//! Space room HTTP routes (build plan P-SC1).
//!
//! One file per feature lives under this module (`posts.rs`, `artifacts.rs`,
//! `timeline.rs`, ...), each exposing a `router()` that [`router`] merges, so
//! `http_routes.rs` (hotspot 16) keeps a single `.merge()` line for all of
//! them. No Space feature logic goes in `sse.rs` or `sse_events.rs`.
//!
//! The router is empty today: it adds no route, so the gateway surface is
//! unchanged. Every route added here is Control-decision-gated and ZDR
//! fail-closed per the plan, and authorised as a user principal.

use axum::Router;

use crate::state::AppState;

/// All Space room routes. Merged once into the authenticated router.
pub fn router() -> Router<AppState> {
    Router::new().merge(crate::space_sse::router())
}
