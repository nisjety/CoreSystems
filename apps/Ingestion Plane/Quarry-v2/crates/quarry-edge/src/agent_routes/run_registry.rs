use super::*;

use crate::budget::RunClass;
use crate::run_limits::RunLimits;

/// The live-run state under one lock: the runs, and the starts that were admitted
/// but have not been committed yet (they count against the caps so two requests
/// racing for the last slot cannot both get it).
#[derive(Default)]
pub struct Inner {
    runs: HashMap<String, RunHandle>,
    pending_global: usize,
    pending_org: HashMap<String, usize>,
    pending_interactive: HashMap<String, usize>,
}

impl Inner {
    pub fn get(&self, run_id: &str) -> Option<&RunHandle> {
        self.runs.get(run_id)
    }

    pub fn insert(&mut self, run_id: String, handle: RunHandle) -> Option<RunHandle> {
        self.runs.insert(run_id, handle)
    }

    pub fn remove(&mut self, run_id: &str) -> Option<RunHandle> {
        self.runs.remove(run_id)
    }

    pub fn drain(&mut self) -> std::collections::hash_map::Drain<'_, String, RunHandle> {
        self.runs.drain()
    }

    pub fn len(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// A copy of every handle, for the sweeper (the lock is not held while it works).
    pub fn snapshot(&self) -> Vec<(String, RunHandle)> {
        self.runs
            .iter()
            .map(|(run_id, handle)| (run_id.clone(), handle.clone()))
            .collect()
    }
}

/// Why a start was refused. The three reasons are the closed list in
/// `docs/ERROR_REASONS.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacityDenied {
    pub reason: &'static str,
    pub scope: &'static str,
    pub limit: u32,
    pub active: usize,
    pub retry_after_s: u64,
}

impl CapacityDenied {
    /// The typed 429: retryable, naming the cap and the numbers.
    pub fn into_error(self) -> QuarryError {
        QuarryError::new(
            ErrorCode::RateLimited,
            "browser capacity is full; try again shortly",
        )
        .with_details(serde_json::json!({
            "reason": self.reason,
            "scope": self.scope,
            "limit": self.limit,
            "active": self.active,
            "retry_after_seconds": self.retry_after_s,
        }))
    }
}

/// What the registry reports about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RegistryStats {
    pub live: usize,
    pub interactive: usize,
    pub agent: usize,
    pub pending: usize,
}

/// The live runs, with admission by cap.
pub struct RunRegistry {
    inner: StdMutex<Inner>,
    limits: RunLimits,
}

impl Default for RunRegistry {
    /// No caps and no sweeper.
    fn default() -> Self {
        Self::new(RunLimits::unlimited())
    }
}

impl RunRegistry {
    pub fn new(limits: RunLimits) -> Self {
        Self {
            inner: StdMutex::new(Inner::default()),
            limits,
        }
    }

    pub fn limits(&self) -> &RunLimits {
        &self.limits
    }

    /// The registry's map. Held only for O(1) work, never across an await.
    pub fn lock(&self) -> std::sync::LockResult<std::sync::MutexGuard<'_, Inner>> {
        self.inner.lock()
    }

    pub fn stats(&self) -> RegistryStats {
        let inner = self.inner.lock().expect("agent_runs mutex poisoned");
        let interactive = inner
            .runs
            .values()
            .filter(|handle| handle.live.class() == RunClass::Interactive)
            .count();
        RegistryStats {
            live: inner.runs.len(),
            interactive,
            agent: inner.runs.len() - interactive,
            pending: inner.pending_global,
        }
    }

    /// Admit a start against the caps, in order: global, organization, and (for a
    /// person's own session) the organization's interactive sub-cap. The returned
    /// slot counts until it is committed with the run or dropped. `0` means no cap.
    pub fn try_reserve(
        self: &Arc<Self>,
        org_id: &str,
        class: RunClass,
    ) -> Result<RunSlot, CapacityDenied> {
        let limits = &self.limits;
        let mut inner = self.inner.lock().expect("agent_runs mutex poisoned");
        let global = inner.runs.len() + inner.pending_global;
        let in_org = inner
            .runs
            .values()
            .filter(|handle| handle.live.org_id() == org_id)
            .count()
            + inner.pending_org.get(org_id).copied().unwrap_or(0);
        let interactive_in_org = inner
            .runs
            .values()
            .filter(|handle| {
                handle.live.org_id() == org_id && handle.live.class() == RunClass::Interactive
            })
            .count()
            + inner.pending_interactive.get(org_id).copied().unwrap_or(0);
        let denied = |reason, scope, limit, active| CapacityDenied {
            reason,
            scope,
            limit,
            active,
            retry_after_s: limits.cap_retry_after_s,
        };
        if limits.max_concurrent_runs > 0 && global >= limits.max_concurrent_runs as usize {
            return Err(denied(
                "global_run_cap",
                "global",
                limits.max_concurrent_runs,
                global,
            ));
        }
        if limits.max_runs_per_org > 0 && in_org >= limits.max_runs_per_org as usize {
            return Err(denied("org_run_cap", "org", limits.max_runs_per_org, in_org));
        }
        if class == RunClass::Interactive
            && limits.max_interactive_runs_per_org > 0
            && interactive_in_org >= limits.max_interactive_runs_per_org as usize
        {
            return Err(denied(
                "org_interactive_run_cap",
                "org",
                limits.max_interactive_runs_per_org,
                interactive_in_org,
            ));
        }
        inner.pending_global += 1;
        *inner.pending_org.entry(org_id.to_owned()).or_default() += 1;
        if class == RunClass::Interactive {
            *inner.pending_interactive.entry(org_id.to_owned()).or_default() += 1;
        }
        Ok(RunSlot {
            registry: self.clone(),
            org_id: org_id.to_owned(),
            class,
            committed: false,
        })
    }
}

/// A start the caps admitted. Dropping it (every early return of a failed start)
/// frees the capacity; `commit` turns it into the live run.
pub struct RunSlot {
    registry: Arc<RunRegistry>,
    org_id: String,
    class: RunClass,
    committed: bool,
}

impl std::fmt::Debug for RunSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunSlot")
            .field("org_id", &self.org_id)
            .field("class", &self.class)
            .field("committed", &self.committed)
            .finish()
    }
}

impl RunSlot {
    fn release_pending(&self, inner: &mut Inner) {
        inner.pending_global = inner.pending_global.saturating_sub(1);
        if let Some(count) = inner.pending_org.get_mut(&self.org_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                inner.pending_org.remove(&self.org_id);
            }
        }
        if self.class == RunClass::Interactive {
            if let Some(count) = inner.pending_interactive.get_mut(&self.org_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    inner.pending_interactive.remove(&self.org_id);
                }
            }
        }
    }

    /// Register the run; its slot stops being pending in the same step, so the
    /// caps never see it counted twice or not at all.
    pub fn commit(mut self, run_id: String, handle: RunHandle) {
        let registry = self.registry.clone();
        let mut inner = registry.inner.lock().expect("agent_runs mutex poisoned");
        self.release_pending(&mut inner);
        inner.runs.insert(run_id, handle);
        self.committed = true;
    }
}

impl Drop for RunSlot {
    fn drop(&mut self) {
        if !self.committed {
            // WS6.I: admitted, then failed before the run was registered.
            crate::metrics::record_run_start(self.class.label(), "aborted");
            let registry = self.registry.clone();
            let guard = registry.inner.lock();
            if let Ok(mut inner) = guard {
                self.release_pending(&mut inner);
            }
        }
    }
}
