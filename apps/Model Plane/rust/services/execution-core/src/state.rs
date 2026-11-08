//! In-memory run state store for execution-core.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::{mapref::entry::Entry, DashMap};

use crate::browser_session::BrowserSession;
use crate::workspace_edit::WorkspaceLock;

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
    /// Whether this lease may host S4.2 background processes, as
    /// `AcquireLease` reported it.
    ///
    /// Cached rather than re-derived because the authority it reflects is not
    /// re-readable from here: sandbox-manager decided it once, from a Control
    /// decision this process no longer holds, and every later process RPC
    /// travels on execution-core's own service token with no decision in hand.
    /// So this is a copy of the row's answer, kept for one purpose — refusing
    /// `process_start` with the real reason instead of preparing a spawn the
    /// registry would deny. `RegisterProcess` checks the row again and that
    /// check, not this one, is authoritative.
    pub processes_permitted: bool,
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

/// What happened to a steer text offered to a live browser run (E3.3-f). The same arms as
/// model-gateway's `EnqueueOutcome` for a queued chat message, held to the same caps
/// (`mp_contracts::queued_limits`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteerOutcome {
    /// Accepted; `pending` counts everything now waiting, this text included.
    Queued { pending: usize },
    /// `MAX_QUEUED_MESSAGES` already waiting.
    Full { pending: usize },
    /// Longer than `MAX_QUEUED_CHARS` characters.
    TooLong { chars: usize },
    /// Nothing but whitespace.
    Empty,
}

/// One effect a scheduled Do run withheld instead of performing (WS20 S6). It keeps the
/// tool name, a closed reason and the digest of the arguments, never the arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithheldEffect {
    pub tool: String,
    pub reason_code: &'static str,
    pub args_sha256: String,
}

/// How long a cached thread lease is reused. It is shorter than sandbox-manager's lease TTL
/// (`sandbox_lease::LEASE_TTL`, one hour, slid by every acquire), so a lease this process still
/// reuses is one the server has not let expire.
const THREAD_LEASE_CACHE_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Default)]
pub struct StateStore {
    runs: Arc<DashMap<String, RunSnapshot>>,
    owners: Arc<DashMap<String, RunOwner>>,
    leases: Arc<DashMap<String, SandboxLease>>,
    workspaces: Arc<DashMap<String, HydratedWorkspace>>,
    /// The thread leases this process acquired, keyed by run and thread. The thread is part of the key so
    /// a run that names a second thread never reuses the first thread's lease.
    thread_leases: Arc<DashMap<(String, String), (SandboxLease, Instant)>>,
    /// One lock per thread with a call in flight here. A call holds it from hydrate to save.
    thread_locks: Arc<DashMap<String, WorkspaceLock>>,
    /// One lock per run with a workspace, shared by its file tools and dropped with the workspace.
    workspace_locks: Arc<DashMap<String, WorkspaceLock>>,
    /// Scheduled Do runs with no one attached to answer an approval. Each run's
    /// withheld effects accumulate here until its step takes them.
    unattended: Arc<DashMap<String, Arc<std::sync::Mutex<Vec<WithheldEffect>>>>>,
    /// The browser page a Do run has open, keyed by run (WS21-S5a). It holds ids only and is ended
    /// by the run's end, its cancellation, a close or the next open.
    browser_sessions: Arc<DashMap<String, BrowserSession>>,
    /// Texts a person sent to a live browser run, oldest first, waiting for the loop's next
    /// step boundary (E3.3-f). Dropped with the run's cancel.
    steers: Arc<DashMap<String, VecDeque<String>>>,
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

    /// The browser page the run has open, if any (WS21-S5a). A copy: the map keeps its own.
    #[must_use]
    pub(crate) fn browser_session(&self, run_id: &str) -> Option<BrowserSession> {
        self.browser_sessions
            .get(run_id)
            .map(|session| session.clone())
    }

    /// Record the run's browser page and return the one it replaces, which the caller must end.
    pub(crate) fn replace_browser_session(
        &self,
        run_id: &str,
        session: BrowserSession,
    ) -> Option<BrowserSession> {
        self.browser_sessions.insert(run_id.to_owned(), session)
    }

    /// Remove the run's browser page. Whoever takes it is the one that ends it, so two callers
    /// cannot both end the same grant.
    pub(crate) fn take_browser_session(&self, run_id: &str) -> Option<BrowserSession> {
        self.browser_sessions
            .remove(run_id)
            .map(|(_, session)| session)
    }

    /// Mark a run unattended: no one is present to answer its approvals, so each
    /// gated effect is withheld and recorded instead of paused (WS20 S6).
    pub(crate) fn begin_unattended(&self, run_id: &str) {
        self.unattended.insert(
            run_id.to_owned(),
            Arc::new(std::sync::Mutex::new(Vec::new())),
        );
    }

    /// Record one withheld effect of an unattended run. Returns `false` for an attended
    /// run, which keeps its own refusal path untouched.
    pub(crate) fn withhold(&self, run_id: &str, effect: WithheldEffect) -> bool {
        let Some(ledger) = self
            .unattended
            .get(run_id)
            .map(|entry| Arc::clone(entry.value()))
        else {
            return false;
        };
        ledger
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(effect);
        true
    }

    /// Whether the run is unattended, so its gated effects are withheld, not paused.
    pub(crate) fn is_unattended(&self, run_id: &str) -> bool {
        self.unattended.contains_key(run_id)
    }

    /// End an unattended run's ledger and return what it withheld, in call order.
    pub(crate) fn take_withheld(&self, run_id: &str) -> Vec<WithheldEffect> {
        let Some((_, ledger)) = self.unattended.remove(run_id) else {
            return Vec::new();
        };
        let mut effects = ledger
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *effects)
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

    /// Queue `text` for the run's next step boundary (E3.3-f). Trimmed, at most
    /// `MAX_QUEUED_CHARS` characters, at most `MAX_QUEUED_MESSAGES` waiting; a refused text is
    /// not queued, so a message that fails its bound never reaches the planner.
    pub fn enqueue_steer(&self, run_id: &str, text: &str) -> SteerOutcome {
        let text = text.trim();
        if text.is_empty() {
            return SteerOutcome::Empty;
        }
        let chars = text.chars().count();
        if chars > mp_contracts::queued_limits::MAX_QUEUED_CHARS {
            return SteerOutcome::TooLong { chars };
        }
        let mut queue = self.steers.entry(run_id.to_owned()).or_default();
        if queue.len() >= mp_contracts::queued_limits::MAX_QUEUED_MESSAGES {
            return SteerOutcome::Full {
                pending: queue.len(),
            };
        }
        queue.push_back(text.to_owned());
        SteerOutcome::Queued {
            pending: queue.len(),
        }
    }

    /// Take everything waiting for the run, oldest first (E3.3-f).
    #[must_use]
    pub fn take_steer(&self, run_id: &str) -> Vec<String> {
        self.steers
            .remove(run_id)
            .map(|(_, queue)| queue.into_iter().collect())
            .unwrap_or_default()
    }

    /// How many steer texts wait for the run.
    #[must_use]
    pub fn pending_steer(&self, run_id: &str) -> usize {
        self.steers.get(run_id).map_or(0, |queue| queue.len())
    }

    #[must_use]
    pub fn cancel(&self, run_id: &str, reason: Option<String>) -> bool {
        self.steers.remove(run_id);
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
        self.workspace_locks.remove(run_id);
        self.workspaces
            .remove(run_id)
            .map(|(_, workspace)| workspace)
    }

    /// The lock that serialises this run's workspace file tools: reads share it,
    /// edits and puts take it exclusively. Created on first use. It sits beside the
    /// workspace rather than on `HydratedWorkspace`, which `sandbox_lease` builds by literal.
    /// This run's cached lease on `thread_id`, while it is still fresh.
    #[must_use]
    pub fn thread_lease(&self, run_id: &str, thread_id: &str) -> Option<SandboxLease> {
        let entry = self
            .thread_leases
            .get(&(run_id.to_owned(), thread_id.to_owned()))?;
        let (lease, cached_at) = entry.value();
        (cached_at.elapsed() < THREAD_LEASE_CACHE_TTL).then(|| lease.clone())
    }

    /// Caches the lease this run acquired on `thread_id`. Entries past the cache TTL are dropped on each
    /// insert, so a long-lived process keeps none for the runs it stopped serving.
    pub fn cache_thread_lease(&self, run_id: &str, thread_id: &str, lease: SandboxLease) {
        self.thread_leases
            .retain(|_, entry| entry.1.elapsed() < THREAD_LEASE_CACHE_TTL);
        self.thread_leases.insert(
            (run_id.to_owned(), thread_id.to_owned()),
            (lease, Instant::now()),
        );
    }

    /// The lock that serialises one thread's workspace calls in this process. Locks no call holds
    /// any longer are dropped when the next one is made.
    #[must_use]
    pub fn thread_lock(&self, org_id: &str, thread_id: &str) -> WorkspaceLock {
        self.thread_locks
            .retain(|_, lock| Arc::strong_count(lock) > 1);
        self.thread_locks
            .entry(format!("{org_id}/{thread_id}"))
            .or_insert_with(|| Arc::new(tokio::sync::RwLock::new(())))
            .value()
            .clone()
    }

    #[must_use]
    pub fn workspace_lock(&self, run_id: &str) -> WorkspaceLock {
        self.workspace_locks
            .entry(run_id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::RwLock::new(())))
            .value()
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_lease_is_never_reused_for_another_thread_of_the_same_run() {
        let store = StateStore::new();
        store.cache_thread_lease(
            "run-1",
            "thread-a",
            SandboxLease {
                lease_id: "lease-a".to_owned(),
                backend_id: "backend-1".to_owned(),
                processes_permitted: false,
            },
        );
        assert_eq!(
            store
                .thread_lease("run-1", "thread-a")
                .map(|lease| lease.lease_id),
            Some("lease-a".to_owned())
        );
        assert!(store.thread_lease("run-1", "thread-b").is_none());
        assert!(store.thread_lease("run-2", "thread-a").is_none());
    }

    #[test]
    fn one_thread_shares_one_lock_and_another_thread_does_not() {
        let store = StateStore::new();
        let first = store.thread_lock("org-1", "thread-a");
        let again = store.thread_lock("org-1", "thread-a");
        let other = store.thread_lock("org-1", "thread-b");
        assert!(Arc::ptr_eq(&first, &again));
        assert!(!Arc::ptr_eq(&first, &other));
    }

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
            processes_permitted: false,
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
            processes_permitted: false,
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

    #[test]
    fn a_run_shares_one_workspace_lock_until_its_workspace_is_taken() {
        let store = StateStore::new();
        let first = store.workspace_lock("run-1");
        assert!(Arc::ptr_eq(&first, &store.workspace_lock("run-1")));
        assert!(!Arc::ptr_eq(&first, &store.workspace_lock("run-2")));
        store.cache_hydrated_workspace(
            "run-1",
            HydratedWorkspace {
                path: PathBuf::from("/tmp/ws-1"),
                baseline: HashMap::new(),
            },
        );
        assert!(store.take_hydrated_workspace("run-1").is_some());
        assert!(!Arc::ptr_eq(&first, &store.workspace_lock("run-1")));
    }

    #[test]
    fn steer_is_held_to_the_queued_input_caps_and_taken_oldest_first() {
        let store = StateStore::new();
        assert_eq!(store.enqueue_steer("run-1", "  \n "), SteerOutcome::Empty);
        let long = "x".repeat(mp_contracts::queued_limits::MAX_QUEUED_CHARS + 1);
        assert_eq!(
            store.enqueue_steer("run-1", &long),
            SteerOutcome::TooLong {
                chars: mp_contracts::queued_limits::MAX_QUEUED_CHARS + 1
            }
        );
        let exact = "y".repeat(mp_contracts::queued_limits::MAX_QUEUED_CHARS);
        assert_eq!(
            store.enqueue_steer("run-1", &exact),
            SteerOutcome::Queued { pending: 1 }
        );
        assert_eq!(store.enqueue_steer("run-1", "two"), SteerOutcome::Queued { pending: 2 });
        assert_eq!(store.enqueue_steer("run-1", " three "), SteerOutcome::Queued { pending: 3 });
        assert_eq!(store.enqueue_steer("run-1", "four"), SteerOutcome::Full { pending: 3 });
        // Another run has its own queue.
        assert_eq!(store.enqueue_steer("run-2", "other"), SteerOutcome::Queued { pending: 1 });
        assert_eq!(store.pending_steer("run-1"), 3);
        assert_eq!(store.take_steer("run-1"), vec![exact, "two".to_owned(), "three".to_owned()]);
        assert!(store.take_steer("run-1").is_empty(), "taken once");
        assert_eq!(store.pending_steer("run-2"), 1);
    }

    #[test]
    fn a_cancel_drops_the_runs_steer_queue() {
        let store = StateStore::new();
        let _ = store.enqueue_steer("run-1", "later");
        let _ = store.cancel("run-1", None);
        assert!(store.take_steer("run-1").is_empty());
    }

    #[test]
    fn steer_limits_are_the_gateways_queued_input_limits() {
        // One definition: model-gateway's constants re-export these.
        assert_eq!(mp_contracts::queued_limits::MAX_QUEUED_CHARS, 2_000);
        assert_eq!(mp_contracts::queued_limits::MAX_QUEUED_MESSAGES, 3);
    }
}
