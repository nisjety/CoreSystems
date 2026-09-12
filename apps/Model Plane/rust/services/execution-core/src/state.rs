//! In-memory run state store for execution-core.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use dashmap::{mapref::entry::Entry, DashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunOwner {
    org_id: String,
    user_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// A sandbox-manager lease acquired for one run's Space-scoped
/// `code_interpreter` calls. `AcquireLease` mints a fresh lease (and lease
/// id) on every call — it is not idempotent by scope — so a run's FIRST
/// Space-scoped step caches its result here and every later step in the same
/// run reuses it, mirroring `owners`' own "insert-once, read-many" idiom.
/// See `sandbox_lease::ensure_sandbox_lease`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxLease {
    pub lease_id: String,
    /// The backend this lease is pinned to; `SnapshotSandbox`/`ReleaseLease`
    /// must present the same value back (S3.2's own backend-pin invariant).
    pub backend_id: String,
}

/// A run's Space-scoped `code_interpreter` workspace, hydrated once onto
/// local disk and reused across every call in the run. `baseline` is
/// `hydrate`'s own `path -> content_hash` return value, captured at hydrate
/// time — kept here so the run's eventual `diff_and_upload` (at release
/// time) compares against what THIS run actually observed, not whatever
/// the Space's rows say by then. See `sandbox_lease::ensure_hydrated_workspace`.
#[derive(Debug, Clone)]
pub struct HydratedWorkspace {
    pub path: PathBuf,
    pub baseline: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct StateStore {
    runs: Arc<DashMap<String, RunSnapshot>>,
    owners: Arc<DashMap<String, RunOwner>>,
    leases: Arc<DashMap<String, SandboxLease>>,
    workspaces: Arc<DashMap<String, HydratedWorkspace>>,
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

    fn resume_if_status(&self, run_id: &str, expected: RunStatus) -> Option<u32> {
        let Entry::Occupied(mut entry) = self.runs.entry(run_id.to_owned()) else {
            return None;
        };
        if entry.get().status != expected {
            return None;
        }
        let current = entry.get().clone();
        entry.insert(RunSnapshot {
            status: RunStatus::Running,
            ..current.clone()
        });
        Some(current.step_index)
    }

    /// Atomically resume only a user-paused run. Approval-gated runs require
    /// the distinct durable-approval transition below.
    #[must_use]
    pub fn resume_paused(&self, run_id: &str) -> Option<u32> {
        self.resume_if_status(run_id, RunStatus::Paused)
    }

    /// Atomically resume only a run waiting on approval. The caller must first
    /// verify the durable granted approval and its run/tenant binding.
    #[must_use]
    pub fn resume_approved(&self, run_id: &str) -> Option<u32> {
        self.resume_if_status(run_id, RunStatus::AwaitingApproval)
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

    #[must_use]
    pub fn sandbox_lease(&self, run_id: &str) -> Option<SandboxLease> {
        self.leases.get(run_id).map(|entry| entry.clone())
    }

    /// Cache the lease this run acquired for its first Space-scoped
    /// `code_interpreter` step. Unconditional insert-or-replace: a caller that
    /// already checked `sandbox_lease` and found nothing is the only caller
    /// that should reach this, so there is no existing value worth preserving
    /// over a fresh one (unlike `cache_verified_owner`, which must never let a
    /// second principal silently rebind an owner).
    pub fn cache_sandbox_lease(&self, run_id: &str, lease: SandboxLease) {
        self.leases.insert(run_id.to_owned(), lease);
    }

    /// Remove and return this run's sandbox lease, if it ever acquired one.
    /// Used only at a run's actual end (cancel or terminal completion) —
    /// removing rather than merely reading means a second release attempt for
    /// the same `run_id` sees `None` and is a no-op, and the map does not grow
    /// forever for runs that ever used `code_interpreter` in a Space.
    #[must_use]
    pub fn take_sandbox_lease(&self, run_id: &str) -> Option<SandboxLease> {
        self.leases.remove(run_id).map(|(_, lease)| lease)
    }

    /// Read this run's already-hydrated Space workspace, if any.
    #[must_use]
    pub fn hydrated_workspace(&self, run_id: &str) -> Option<HydratedWorkspace> {
        self.workspaces.get(run_id).map(|entry| entry.clone())
    }

    /// Cache the workspace this run hydrated for its first Space-scoped
    /// `code_interpreter` call. Unconditional insert-or-replace, mirroring
    /// `cache_sandbox_lease`: only a caller that already checked
    /// `hydrated_workspace` and found nothing reaches here.
    pub fn cache_hydrated_workspace(&self, run_id: &str, workspace: HydratedWorkspace) {
        self.workspaces.insert(run_id.to_owned(), workspace);
    }

    /// Remove and return this run's hydrated workspace, if it ever created
    /// one. Used only at a run's actual end, alongside `take_sandbox_lease` —
    /// removing rather than reading means a second release attempt finds
    /// nothing and does not try to re-upload or re-delete the directory.
    #[must_use]
    pub fn take_hydrated_workspace(&self, run_id: &str) -> Option<HydratedWorkspace> {
        self.workspaces
            .remove(run_id)
            .map(|(_, workspace)| workspace)
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

            assert_eq!(store.resume_approved("run_approval"), Some(4));
            let mut finished = store.get_or_create("run_approval");
            finished.status = terminal;
            store.update(finished);

            assert_eq!(store.resume_approved("run_approval"), None);
            assert_eq!(store.get_or_create("run_approval").status, terminal);
        }
    }

    #[test]
    fn manual_resume_cannot_bypass_an_approval_gate() {
        let store = StateStore::new();
        store.update(RunSnapshot {
            run_id: "run_approval".to_owned(),
            step_index: 4,
            status: RunStatus::AwaitingApproval,
            last_error: None,
        });

        assert_eq!(store.resume_paused("run_approval"), None);
        assert_eq!(
            store.get_or_create("run_approval").status,
            RunStatus::AwaitingApproval
        );
    }

    #[test]
    fn durable_approval_delivery_and_manual_pause_use_distinct_transitions() {
        let store = StateStore::new();
        store.update(RunSnapshot {
            run_id: "run_approval".to_owned(),
            step_index: 7,
            status: RunStatus::AwaitingApproval,
            last_error: None,
        });
        store.update(RunSnapshot {
            run_id: "run_paused".to_owned(),
            step_index: 2,
            status: RunStatus::Paused,
            last_error: None,
        });

        assert_eq!(store.resume_approved("run_paused"), None);
        assert_eq!(store.resume_paused("run_paused"), Some(2));
        assert_eq!(store.resume_approved("run_approval"), Some(7));
    }

    #[test]
    fn a_run_with_no_lease_yet_reads_as_absent() {
        let store = StateStore::new();
        assert_eq!(store.sandbox_lease("run-1"), None);
    }

    #[test]
    fn a_cached_lease_is_reused_by_later_reads_for_the_same_run() {
        let store = StateStore::new();
        let lease = SandboxLease {
            lease_id: "lease-1".to_owned(),
            backend_id: "backend-1".to_owned(),
        };
        store.cache_sandbox_lease("run-1", lease.clone());
        assert_eq!(store.sandbox_lease("run-1"), Some(lease));
        assert_eq!(store.sandbox_lease("run-2"), None);
    }

    #[test]
    fn taking_a_lease_removes_it_so_a_second_release_is_a_no_op() {
        let store = StateStore::new();
        let lease = SandboxLease {
            lease_id: "lease-1".to_owned(),
            backend_id: "backend-1".to_owned(),
        };
        store.cache_sandbox_lease("run-1", lease.clone());
        assert_eq!(store.take_sandbox_lease("run-1"), Some(lease));
        assert_eq!(store.take_sandbox_lease("run-1"), None);
        assert_eq!(store.sandbox_lease("run-1"), None);
    }

    #[test]
    fn a_run_with_no_hydrated_workspace_yet_reads_as_absent() {
        let store = StateStore::new();
        assert!(store.hydrated_workspace("run-1").is_none());
    }

    #[test]
    fn a_cached_hydrated_workspace_is_reused_by_later_reads_for_the_same_run() {
        let store = StateStore::new();
        let workspace = HydratedWorkspace {
            path: PathBuf::from("/tmp/ws-1"),
            baseline: HashMap::from([("a.txt".to_owned(), "sha256:aaa".to_owned())]),
        };
        store.cache_hydrated_workspace("run-1", workspace.clone());
        let read = store.hydrated_workspace("run-1").expect("cached workspace");
        assert_eq!(read.path, workspace.path);
        assert_eq!(read.baseline, workspace.baseline);
        assert!(store.hydrated_workspace("run-2").is_none());
    }

    #[test]
    fn taking_a_hydrated_workspace_removes_it_so_a_second_release_is_a_no_op() {
        let store = StateStore::new();
        let workspace = HydratedWorkspace {
            path: PathBuf::from("/tmp/ws-1"),
            baseline: HashMap::new(),
        };
        store.cache_hydrated_workspace("run-1", workspace);
        assert!(store.take_hydrated_workspace("run-1").is_some());
        assert!(store.take_hydrated_workspace("run-1").is_none());
        assert!(store.hydrated_workspace("run-1").is_none());
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
