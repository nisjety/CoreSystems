//! In-memory run state store for execution-core.

use std::sync::Arc;

use dashmap::DashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    Running,
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
}
