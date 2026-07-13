//! In-memory run state store for execution-core.

use std::sync::Arc;

use dashmap::{mapref::entry::Entry, DashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunOwner {
    org_id: String,
    user_id: String,
}

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
    owners: Arc<DashMap<String, RunOwner>>,
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

    /// Atomically resume only a genuinely gated run.
    ///
    /// Returning `None` for running, unknown, and terminal runs makes an
    /// approval retry fail closed: a stale decision can never revive a run
    /// that completed, failed, or was cancelled after its first resume.
    #[must_use]
    pub fn resume(&self, run_id: &str) -> Option<u32> {
        let Entry::Occupied(mut entry) = self.runs.entry(run_id.to_owned()) else {
            return None;
        };
        if !matches!(
            entry.get().status,
            RunStatus::AwaitingApproval | RunStatus::Paused
        ) {
            return None;
        }
        let step_index = entry.get().step_index;
        entry.get_mut().status = RunStatus::Running;
        Some(step_index)
    }

    #[must_use]
    pub fn snapshot(&self, run_id: &str) -> Option<RunSnapshot> {
        self.runs.get(run_id).map(|snapshot| snapshot.clone())
    }

    /// Cache an owner only after Session Core has verified it against the
    /// durable runs table. Subsequent calls are idempotent only for that exact
    /// owner; a run id can never be rebound by another principal.
    #[must_use]
    pub(crate) fn cache_verified_owner(&self, run_id: &str, org_id: &str, user_id: &str) -> bool {
        if run_id.trim().is_empty() || org_id.trim().is_empty() || user_id.trim().is_empty() {
            return false;
        }
        let requested = RunOwner {
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
        };
        match self.owners.entry(run_id.to_owned()) {
            Entry::Occupied(existing) => existing.get() == &requested,
            Entry::Vacant(vacant) => {
                vacant.insert(requested);
                true
            }
        }
    }

    /// Return whether an already-bound run belongs to this exact verified
    /// principal. Unknown runs fail closed so control RPCs cannot manufacture
    /// state for guessed identifiers.
    #[must_use]
    pub fn authorizes(&self, run_id: &str, org_id: &str, user_id: &str) -> bool {
        self.owners
            .get(run_id)
            .is_some_and(|owner| owner.org_id == org_id && owner.user_id == user_id)
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

    #[test]
    fn approval_resume_replay_cannot_revive_terminal_runs() {
        for terminal in [RunStatus::Completed, RunStatus::Cancelled] {
            let store = StateStore::new();
            store.update(RunSnapshot {
                run_id: "run_approval".to_owned(),
                step_index: 4,
                status: RunStatus::AwaitingApproval,
                last_error: None,
            });

            assert_eq!(store.resume("run_approval"), Some(4));
            let mut finished = store.get_or_create("run_approval");
            finished.status = terminal.clone();
            store.update(finished);

            assert_eq!(store.resume("run_approval"), None);
            assert_eq!(store.get_or_create("run_approval").status, terminal);
        }
    }

    #[test]
    fn run_ownership_is_immutable_and_cross_tenant_controls_fail_closed() {
        let store = StateStore::new();
        assert!(!store.authorizes("unknown", "org-a", "user-a"));
        assert!(store.cache_verified_owner("run-1", "org-a", "user-a"));
        assert!(store.authorizes("run-1", "org-a", "user-a"));
        assert!(!store.authorizes("run-1", "org-b", "user-a"));
        assert!(!store.authorizes("run-1", "org-a", "user-b"));
        assert!(!store.cache_verified_owner("run-1", "org-b", "user-b"));
        assert!(store.authorizes("run-1", "org-a", "user-a"));
    }
}
