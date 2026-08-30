//! W5 — Fleet / task orchestration.
//!
//! A fleet is a coordinated batch of agent runs that share a budget and
//! report to a single fleet-level view. Each agent run is still a fully
//! isolated "dedicated cloud computer" (W1) with its own lease, proxy
//! affinity, and receipts. The fleet adds a batch-level budget tracker
//! and a fan-out/fan-in shape the orchestrator Temporal workflow will
//! drive. This module is the in-process pool the runtime uses; durable
//! fleet state lives in `quarry-orchestrator/internal/fleet` on the Go side.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::RunKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FleetStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl Default for FleetStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetTask {
    pub fleet_id: String,
    pub org_id: String,
    /// Total USD budget shared across all member runs. None = uncapped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_usd: Option<f64>,
    pub max_parallel_runs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_domain_intel_id: Option<String>,
    #[serde(default)]
    pub member_run_ids: Vec<String>,
    #[serde(default)]
    pub status: FleetStatus,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

impl FleetTask {
    pub fn new(org_id: impl Into<String>, max_parallel_runs: u32) -> Self {
        Self {
            fleet_id: format!("fleet_{}", ulid::Ulid::new()),
            org_id: org_id.into(),
            budget_usd: None,
            max_parallel_runs: max_parallel_runs.max(1),
            shared_profile_id: None,
            shared_domain_intel_id: None,
            member_run_ids: Vec::new(),
            status: FleetStatus::Pending,
            created_at: Utc::now(),
            finished_at: None,
        }
    }

    pub fn with_budget(mut self, budget_usd: f64) -> Self {
        self.budget_usd = Some(budget_usd);
        self
    }

    pub fn add_member(&mut self, run_id: &RunKind) {
        let s = run_id.to_string();
        if !self.member_run_ids.contains(&s) {
            self.member_run_ids.push(s);
        }
    }

    pub fn finish(&mut self, status: FleetStatus) {
        self.status = status;
        self.finished_at = Some(Utc::now());
    }
}

/// Tracks spend across a fleet by summing `StepReceipt.cost_micro_usd`
/// from each member run. Mirrors `AgentLoop::over_budget` but at fleet
/// scope. The tracker is deliberately separate from the `FleetTask`
/// envelope so the orchestrator can update spend without taking a
/// write lock on the task itself while child workflows are in flight.
#[derive(Debug, Default)]
pub struct FleetBudgetTracker {
    budget_micro_usd: Option<u64>,
    spent_micro_usd: AtomicU64,
    per_run_micro_usd: Arc<RwLock<HashMap<String, u64>>>,
}

impl FleetBudgetTracker {
    pub fn new(budget_usd: Option<f64>) -> Self {
        let budget_micro_usd = budget_usd.map(|usd| (usd * 1_000_000.0).round() as u64);
        Self {
            budget_micro_usd,
            spent_micro_usd: AtomicU64::new(0),
            per_run_micro_usd: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn from_fleet(fleet: &FleetTask) -> Self {
        Self::new(fleet.budget_usd)
    }

    /// Record additional spend for `run_id`. Negative / NaN is ignored.
    pub async fn record(&self, run_id: &str, additional_usd: f64) {
        if !additional_usd.is_finite() || additional_usd < 0.0 {
            return;
        }
        let micro = (additional_usd * 1_000_000.0).round() as u64;
        self.spent_micro_usd.fetch_add(micro, Ordering::Relaxed);
        let mut per_run = self.per_run_micro_usd.write().await;
        *per_run.entry(run_id.to_string()).or_insert(0) += micro;
    }

    /// Record spend directly in micro-USD (receipt path). Mirrors
    /// `ReceiptBuilder::complete` which already stores `cost_micro_usd`.
    pub async fn record_micro(&self, run_id: &str, micro_usd: u64) {
        self.spent_micro_usd.fetch_add(micro_usd, Ordering::Relaxed);
        let mut per_run = self.per_run_micro_usd.write().await;
        *per_run.entry(run_id.to_string()).or_insert(0) += micro_usd;
    }

    pub fn spent_usd(&self) -> f64 {
        self.spent_micro_usd.load(Ordering::Relaxed) as f64 / 1_000_000.0
    }

    pub fn spent_micro_usd(&self) -> u64 {
        self.spent_micro_usd.load(Ordering::Relaxed)
    }

    pub fn budget_usd(&self) -> Option<f64> {
        self.budget_micro_usd.map(|m| m as f64 / 1_000_000.0)
    }

    pub fn over_budget(&self) -> Option<(f64, f64)> {
        let limit = self.budget_usd()?;
        let spent = self.spent_usd();
        if spent >= limit {
            Some((spent, limit))
        } else {
            None
        }
    }

    pub async fn per_run_spent_usd(&self, run_id: &str) -> f64 {
        let per_run = self.per_run_micro_usd.read().await;
        per_run.get(run_id).copied().unwrap_or(0) as f64 / 1_000_000.0
    }

    pub async fn snapshot(&self) -> HashMap<String, f64> {
        let per_run = self.per_run_micro_usd.read().await;
        per_run
            .iter()
            .map(|(k, v)| (k.clone(), *v as f64 / 1_000_000.0))
            .collect()
    }
}

/// In-process fleet registry. The orchestrator's durable fleet state lives in
/// Go (`quarry-orchestrator/internal/fleet`); this is the runtime's live view
/// for the duration of a `FleetOrchestrator` workflow branch.
#[derive(Debug, Default)]
pub struct FleetRegistry {
    fleets: Arc<RwLock<HashMap<String, FleetTask>>>,
    trackers: Arc<RwLock<HashMap<String, Arc<FleetBudgetTracker>>>>,
}

impl FleetRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create(&self, fleet: FleetTask) -> QuarryResult<Arc<FleetBudgetTracker>> {
        let fleet_id = fleet.fleet_id.clone();
        let tracker = Arc::new(FleetBudgetTracker::from_fleet(&fleet));
        let mut fleets = self.fleets.write().await;
        if fleets.contains_key(&fleet_id) {
            return Err(QuarryError::new(
                ErrorCode::Conflict,
                format!("fleet already exists: {fleet_id}"),
            ));
        }
        fleets.insert(fleet_id.clone(), fleet);
        self.trackers.write().await.insert(fleet_id, tracker.clone());
        Ok(tracker)
    }

    pub async fn get(&self, fleet_id: &str) -> Option<FleetTask> {
        self.fleets.read().await.get(fleet_id).cloned()
    }

    pub async fn tracker(&self, fleet_id: &str) -> Option<Arc<FleetBudgetTracker>> {
        self.trackers.read().await.get(fleet_id).cloned()
    }

    pub async fn update_status(&self, fleet_id: &str, status: FleetStatus) -> QuarryResult<()> {
        let mut fleets = self.fleets.write().await;
        let fleet = fleets.get_mut(fleet_id).ok_or_else(|| {
            QuarryError::new(ErrorCode::NotFound, format!("fleet not found: {fleet_id}"))
        })?;
        fleet.finish(status);
        Ok(())
    }

    pub async fn add_member(&self, fleet_id: &str, run_id: &RunKind) -> QuarryResult<()> {
        let mut fleets = self.fleets.write().await;
        let fleet = fleets.get_mut(fleet_id).ok_or_else(|| {
            QuarryError::new(ErrorCode::NotFound, format!("fleet not found: {fleet_id}"))
        })?;
        fleet.add_member(run_id);
        Ok(())
    }

    pub async fn len(&self) -> usize {
        self.fleets.read().await.len()
    }
}

/// NATS subject helpers for fleet-scoped events. Mirrors the existing
/// `quarry.run.<run_id>.<event>` convention. Fleet subjects carry the
/// same event envelope but are scoped to the batch so the App Shell can
/// subscribe once (`quarry.fleet.<fleet_id>.>`) and see every member run.
pub mod subjects {
    pub fn fleet_subject(fleet_id: &str, event: &str) -> String {
        format!("quarry.fleet.{fleet_id}.{event}")
    }

    pub fn fleet_wildcard(fleet_id: &str) -> String {
        format!("quarry.fleet.{fleet_id}.>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::ids::Id;

    fn run_id() -> RunKind {
        Id::new()
    }

    #[tokio::test]
    async fn fleet_task_add_member_is_idempotent() {
        let mut fleet = FleetTask::new("org_a", 3);
        let r = run_id();
        fleet.add_member(&r);
        fleet.add_member(&r);
        assert_eq!(fleet.member_run_ids.len(), 1);
    }

    #[tokio::test]
    async fn fleet_budget_tracker_aggregates_across_members() {
        let tracker = FleetBudgetTracker::new(Some(1.0));
        tracker.record("run_a", 0.30).await;
        tracker.record("run_b", 0.40).await;
        tracker.record("run_c", 0.35).await;
        // 0.30 + 0.40 + 0.35 = 1.05 >= 1.0 → over budget
        assert!(tracker.over_budget().is_some());
        let (spent, limit) = tracker.over_budget().unwrap();
        assert!((spent - 1.05).abs() < 1e-6);
        assert!((limit - 1.0).abs() < 1e-9);
        assert!((tracker.per_run_spent_usd("run_b").await - 0.40).abs() < 1e-6);
    }

    #[tokio::test]
    async fn fleet_budget_tracker_back_pressure_on_over_budget() {
        // Fleet budget 0.01 — a single screenshot ($0.005) should not trip,
        // but two should. This is the back-pressure signal the orchestrator
        // uses to stop fanning out new AgentRunWF children.
        let tracker = FleetBudgetTracker::new(Some(0.009));
        tracker.record_micro("run_a", 5_000).await; // screenshot
        assert!(tracker.over_budget().is_none());
        tracker.record_micro("run_b", 5_000).await;
        assert!(tracker.over_budget().is_some());
    }

    #[tokio::test]
    async fn fleet_registry_create_and_get() {
        let reg = FleetRegistry::new();
        let fleet = FleetTask::new("org_a", 2).with_budget(5.0);
        let fleet_id = fleet.fleet_id.clone();
        reg.create(fleet).await.unwrap();
        let got = reg.get(&fleet_id).await.unwrap();
        assert_eq!(got.org_id, "org_a");
        assert_eq!(got.max_parallel_runs, 2);
        assert!(reg.tracker(&fleet_id).await.is_some());
    }

    #[tokio::test]
    async fn fleet_registry_rejects_duplicate_fleet_id() {
        let reg = FleetRegistry::new();
        let fleet = FleetTask::new("org_a", 2);
        let dup = FleetTask {
            fleet_id: fleet.fleet_id.clone(),
            ..FleetTask::new("org_a", 2)
        };
        reg.create(fleet).await.unwrap();
        let err = reg.create(dup).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
    }

    #[tokio::test]
    async fn fleet_subject_helpers() {
        assert_eq!(
            subjects::fleet_subject("fleet_abc", "agent.started"),
            "quarry.fleet.fleet_abc.agent.started"
        );
        assert_eq!(
            subjects::fleet_wildcard("fleet_abc"),
            "quarry.fleet.fleet_abc.>"
        );
    }

    #[tokio::test]
    async fn fleet_task_finish_sets_status_and_timestamp() {
        let mut fleet = FleetTask::new("org_a", 1);
        assert_eq!(fleet.status, FleetStatus::Pending);
        fleet.finish(FleetStatus::Completed);
        assert_eq!(fleet.status, FleetStatus::Completed);
        assert!(fleet.finished_at.is_some());
    }

    #[test]
    fn fleet_task_new_clamps_max_parallel() {
        let fleet = FleetTask::new("org_a", 0);
        assert_eq!(fleet.max_parallel_runs, 1);
    }
}
