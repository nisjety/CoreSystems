//! W5 — Fleet REST surface.
//!
//! A fleet is a batch of agent runs that share a budget. The edge keeps an
//! in-process `FleetRegistry` (mirroring the runtime's `quarry_runtime::fleet`)
//! for dev/single-node. Production wires the orchestrator's durable fleet
//! store via the control plane's `quarry_fleets` table — the same split the
//! existing `agent_routes` already use for receipts/checkpoints.
//!
//! Routes (all org-scoped via verified JWT, like every other agent route):
//! - `POST   /v1/fleets`              — create a fleet envelope
//! - `GET    /v1/fleets/:id`           — fetch a fleet
//! - `POST   /v1/fleets/:id/members`   — attach a run to a fleet
//! - `GET    /v1/fleets/:id/budget`    — fleet budget snapshot

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use quarry_runtime::fleet::{FleetRegistry, FleetStatus, FleetTask};

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateFleetRequest {
    #[serde(default)]
    pub budget_usd: Option<f64>,
    #[serde(default = "default_parallel")]
    pub max_parallel_runs: u32,
    #[serde(default)]
    pub shared_profile_id: Option<String>,
}

fn default_parallel() -> u32 {
    3
}

#[derive(Debug, Serialize)]
pub struct FleetResponse {
    pub fleet: FleetTask,
}

#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    pub run_id: String,
}

#[derive(Debug, Serialize)]
pub struct BudgetResponse {
    pub fleet_id: String,
    pub budget_usd: Option<f64>,
    pub spent_usd: f64,
    pub over_budget: bool,
    pub per_run_usd: std::collections::HashMap<String, f64>,
}

pub type FleetState = Arc<RwLock<FleetRegistry>>;

pub fn new_fleet_state() -> FleetState {
    Arc::new(RwLock::new(FleetRegistry::new()))
}

pub async fn create_fleet(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<CreateFleetRequest>,
) -> impl IntoResponse {
    let mut fleet = FleetTask::new(claims.org_id.clone(), req.max_parallel_runs);
    if let Some(b) = req.budget_usd {
        fleet = fleet.with_budget(b);
    }
    fleet.shared_profile_id = req.shared_profile_id.clone();

    let fleet_id = fleet.fleet_id.clone();
    {
        let reg = state.fleets.read().await;
        // FleetRegistry::create is async; clone out to avoid holding read lock across await
        drop(reg);
        let reg = state.fleets.write().await;
        if let Err(e) = reg.create(fleet.clone()).await {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": e.message, "code": format!("{:?}", e.code)})),
            )
                .into_response();
        }
    }

    // Convex-compat: the edge's fleet view is the source for the Home dashboard.
    // In production the orchestrator's durable fleet store is the source of truth.
    (
        StatusCode::CREATED,
        Json(FleetResponse { fleet: {
            // re-read to include any registry-normalized fields
            let reg = state.fleets.read().await;
            reg.get(&fleet_id).await.unwrap_or(fleet)
        }}),
    )
        .into_response()
}

pub async fn get_fleet(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(fleet_id): Path<String>,
) -> impl IntoResponse {
    let reg = state.fleets.read().await;
    let Some(fleet) = reg.get(&fleet_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "fleet not found", "code": "NOT_FOUND"})),
        )
            .into_response();
    };
    if fleet.org_id != claims.org_id {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "fleet not found", "code": "NOT_FOUND"})),
        )
            .into_response();
    }
    (StatusCode::OK, Json(FleetResponse { fleet })).into_response()
}

pub async fn add_member(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(fleet_id): Path<String>,
    Json(req): Json<AddMemberRequest>,
) -> impl IntoResponse {
    let run_id: quarry_core::ids::kinds::RunKind = match req.run_id.parse() {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.message, "code": "BAD_REQUEST"})),
            )
                .into_response();
        }
    };

    let reg = state.fleets.write().await;
    // Verify fleet exists and is owned by caller before mutating
    let fleet = match reg.get(&fleet_id).await {
        Some(f) if f.org_id == claims.org_id => f,
        _ => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "fleet not found", "code": "NOT_FOUND"})),
            )
                .into_response();
        }
    };
    drop(fleet);
    if let Err(e) = reg.add_member(&fleet_id, &run_id).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.message, "code": "INTERNAL"})),
        )
            .into_response();
    }
    let fleet = reg.get(&fleet_id).await.unwrap();
    (StatusCode::OK, Json(FleetResponse { fleet })).into_response()
}

pub async fn get_budget(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(fleet_id): Path<String>,
) -> impl IntoResponse {
    let reg = state.fleets.read().await;
    let Some(fleet) = reg.get(&fleet_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "fleet not found", "code": "NOT_FOUND"})),
        )
            .into_response();
    };
    if fleet.org_id != claims.org_id {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "fleet not found", "code": "NOT_FOUND"})),
        )
            .into_response();
    }
    let tracker = reg.tracker(&fleet_id).await.unwrap();
    let spent = tracker.spent_usd();
    let over = tracker.over_budget().is_some();
    let per_run = tracker.snapshot().await;
    (
        StatusCode::OK,
        Json(BudgetResponse {
            fleet_id: fleet.fleet_id.clone(),
            budget_usd: fleet.budget_usd,
            spent_usd: spent,
            over_budget: over,
            per_run_usd: per_run,
        }),
    )
        .into_response()
}

/// Fleet-level events stream reuses W3's `agent_events_stream` with
/// `?fleet_id=<id>` — the App Shell subscribes once
/// (`quarry.fleet.<fleet_id>.>`) and sees every member run. The
/// response advertises `X-Agent-Fleet-Stream` so the App Shell knows
/// the shape is available. No new SSE implementation needed; the
/// existing `EventSink` already carries `quarry.fleet.<fleet_id>.*`
/// subjects via `fleet::subjects::fleet_subject`.
pub fn fleet_router() -> axum::Router<AppState> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/v1/fleets", post(create_fleet))
        .route("/v1/fleets/:fleet_id", get(get_fleet))
        .route("/v1/fleets/:fleet_id/members", post(add_member))
        .route("/v1/fleets/:fleet_id/budget", get(get_budget))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::ids::Id;

    #[tokio::test]
    async fn fleet_budget_snapshot_aggregates_receipt_cost() {
        let reg = FleetRegistry::new();
        let fleet = FleetTask::new("org_a", 3).with_budget(1.0);
        let fleet_id = fleet.fleet_id.clone();
        let tracker = reg.create(fleet).await.unwrap();
        let r1: quarry_core::ids::kinds::RunKind = Id::new();
        let r2: quarry_core::ids::kinds::RunKind = Id::new();
        // simulate receipts: screenshot 0.005 + navigate 0.001 etc.
        tracker.record_micro(&r1.to_string(), 5_000).await;
        tracker.record_micro(&r2.to_string(), 5_000).await;
        assert!((tracker.spent_usd() - 0.01).abs() < 1e-9);
        // Back-pressure
        tracker.record(&r1.to_string(), 0.995).await;
        assert!(tracker.over_budget().is_some());
        // per-run
        let snap = tracker.snapshot().await;
        assert_eq!(snap.len(), 2);
        // registry view
        let reg_fleet = reg.get(&fleet_id).await.unwrap();
        assert_eq!(reg_fleet.budget_usd, Some(1.0));
    }
}
