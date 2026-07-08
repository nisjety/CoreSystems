//! In-memory run state store for execution-core.

use std::sync::Arc;

use dashmap::DashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    /// User-initiated pause (Phase 2 B5) — distinct from `AwaitingApproval`
    /// (a HITL gate). Polled by an in-flight loop (e.g. the browser-agent
    /// loop) between steps; never aborts a step already in flight.
    Paused,
    AwaitingApproval,
    Completed,
    Failed,
    Cancelled,
}

impl RunStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSnapshot {
    pub run_id: String,
    pub step_index: u32,
    pub status: RunStatus,
    pub last_error: Option<String>,
}

impl RunSnapshot {
    #[must_use]
    pub fn new(run_id: String) -> Self {
        Self {
            run_id,
            step_index: 0,
            status: RunStatus::Running,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StateStore {
    runs: Arc<DashMap<String, RunSnapshot>>,
}

impl StateStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn get_or_create(&self, run_id: &str) -> RunSnapshot {
        if let Some(existing) = self.runs.get(run_id) {
            return existing.clone();
        }

        let snapshot = RunSnapshot::new(run_id.to_owned());
        self.runs.insert(run_id.to_owned(), snapshot.clone());
        snapshot
    }

    pub fn update(&self, snapshot: RunSnapshot) {
        self.runs.insert(snapshot.run_id.clone(), snapshot);
    }

    #[must_use]
    pub fn cancel(&self, run_id: &str, reason: Option<String>) -> bool {
        let mut next = self.get_or_create(run_id);
        next.status = RunStatus::Cancelled;
        next.last_error = reason;
        self.update(next);
        true
    }

    /// Mark a run paused (Phase 2 B5). Mirrors `cancel`. Idempotent — pausing
    /// an already-paused run is a no-op success.
    #[must_use]
    pub fn pause(&self, run_id: &str) -> bool {
        let mut next = self.get_or_create(run_id);
        next.status = RunStatus::Paused;
        self.update(next);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_run_defaults_to_running() {
        let store = StateStore::new();
        assert_eq!(store.get_or_create("run_1").status, RunStatus::Running);
    }

    #[test]
    fn pause_sets_paused_status() {
        let store = StateStore::new();
        assert!(store.pause("run_1"));
        assert_eq!(store.get_or_create("run_1").status, RunStatus::Paused);
    }

    #[test]
    fn pause_is_idempotent() {
        let store = StateStore::new();
        assert!(store.pause("run_1"));
        assert!(store.pause("run_1"));
        assert_eq!(store.get_or_create("run_1").status, RunStatus::Paused);
    }

    #[test]
    fn cancel_overrides_a_prior_pause() {
        let store = StateStore::new();
        assert!(store.pause("run_1"));
        assert!(store.cancel("run_1", Some("user_stop".to_owned())));
        let snapshot = store.get_or_create("run_1");
        assert_eq!(snapshot.status, RunStatus::Cancelled);
        assert_eq!(snapshot.last_error.as_deref(), Some("user_stop"));
    }
}
