use super::*;

use crate::budget::RunClass;
use crate::limits::RunCapOverride;
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
    /// INNH-10: starts of an API key admitted but not committed yet, by `key_book`.
    pending_key: HashMap<String, usize>,
    /// D1-021: run ids a resume has claimed but not committed yet, so two resumes of the
    /// same id racing each other cannot both get past the "is it live" check.
    pending_resume_ids: std::collections::HashSet<String>,
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

    /// W3-E3.4-2/3: whether `run_id` is live in this process, or a resume of it is being
    /// committed (D1-021's claim). The sweeper asks before it marks a released run's
    /// checkpoint `expired`/`suspended`, so a resume that won the race keeps its `active` one.
    pub fn is_live_or_resuming(&self, run_id: &str) -> bool {
        self.runs.contains_key(run_id) || self.pending_resume_ids.contains(run_id)
    }

    /// Tests only: no API-key start is pending any more (every slot was committed or dropped).
    #[cfg(test)]
    pub fn inner_pending_key_is_empty_for_tests(&self) -> bool {
        self.pending_key.is_empty()
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

/// INNH-10: the registry's book key for one API key's runs: its organization and its actor
/// (`apikey:<key_id>`). The key id is unique across organizations, the org is kept as well so
/// a runaway id in one tenant can never count against another.
fn key_book(org_id: &str, key_actor: &str) -> String {
    format!("{org_id}\u{1f}{key_actor}")
}

/// Why a start was refused. The reasons are the closed list in
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

/// Why a start or resume was not admitted: the caps said no, or the run a resume names is
/// already live (or is being resumed by another request right now).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReserveDenied {
    Capacity(CapacityDenied),
    ResumeLive,
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
    /// No plan bound: see [`try_reserve_with`](Self::try_reserve_with).
    pub fn try_reserve(
        self: &Arc<Self>,
        org_id: &str,
        class: RunClass,
    ) -> Result<RunSlot, CapacityDenied> {
        self.try_reserve_with(org_id, class, RunCapOverride::NONE)
    }

    /// As [`try_reserve`](Self::try_reserve), with the organization's plan bounds
    /// (WS6.G). The global cap is checked first and always. A class the plan
    /// bounds then answers to that bound alone (counting the organization's live
    /// and pending runs of that class), refused as `org_plan_cap`; the static
    /// per-organization caps do not apply to it, so a plan can raise an
    /// organization above the floor, but never above the global cap. A class the
    /// plan does not bound is checked against the static caps as before.
    pub fn try_reserve_with(
        self: &Arc<Self>,
        org_id: &str,
        class: RunClass,
        plan: RunCapOverride,
    ) -> Result<RunSlot, CapacityDenied> {
        match self.try_reserve_for(org_id, class, plan, None) {
            Ok(slot) => Ok(slot),
            Err(ReserveDenied::Capacity(denied)) => Err(denied),
            // No resume id was given, so the run-live refusal cannot happen.
            Err(ReserveDenied::ResumeLive) => unreachable!("no resume id was named"),
        }
    }

    /// As [`try_reserve_with`](Self::try_reserve_with), and for a resume (D1-021) the run
    /// id it names is checked under the same lock that counts the caps: a run that is
    /// already live, or that another resume is committing, is refused (`ResumeLive`)
    /// instead of being replaced. Before this, `commit` inserted over the live entry, so a
    /// second session was started for the id and the first one, with its action possibly
    /// running, was dropped from the registry without being released.
    pub fn try_reserve_for(
        self: &Arc<Self>,
        org_id: &str,
        class: RunClass,
        plan: RunCapOverride,
        resume_run_id: Option<&str>,
    ) -> Result<RunSlot, ReserveDenied> {
        self.try_reserve_keyed(org_id, class, plan, resume_run_id, None)
    }

    /// As [`try_reserve_for`](Self::try_reserve_for), and for an API key's run (INNH-10)
    /// `key_actor` (`apikey:<key_id>`) is counted too: the key may have at most
    /// `RunLimits::max_runs_per_key` live runs (live plus pending, in the run's organization),
    /// refused as `per_key_run_cap` with scope `key`. It is checked right after the global cap,
    /// so a key at its cap hears so rather than hearing about the organization.
    pub fn try_reserve_keyed(
        self: &Arc<Self>,
        org_id: &str,
        class: RunClass,
        plan: RunCapOverride,
        resume_run_id: Option<&str>,
        key_actor: Option<&str>,
    ) -> Result<RunSlot, ReserveDenied> {
        let limits = &self.limits;
        let mut inner = self.inner.lock().expect("agent_runs mutex poisoned");
        if let Some(run_id) = resume_run_id {
            if inner.runs.contains_key(run_id) || inner.pending_resume_ids.contains(run_id) {
                return Err(ReserveDenied::ResumeLive);
            }
        }
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
        let denied = |reason, scope, limit, active| {
            ReserveDenied::Capacity(CapacityDenied {
                reason,
                scope,
                limit,
                active,
                retry_after_s: limits.cap_retry_after_s,
            })
        };
        if limits.max_concurrent_runs > 0 && global >= limits.max_concurrent_runs as usize {
            return Err(denied(
                "global_run_cap",
                "global",
                limits.max_concurrent_runs,
                global,
            ));
        }
        if let Some(actor) = key_actor {
            if limits.max_runs_per_key > 0 {
                let live = inner
                    .runs
                    .values()
                    .filter(|handle| {
                        handle.live.org_id() == org_id && handle.live.actor_id() == actor
                    })
                    .count();
                let pending = inner
                    .pending_key
                    .get(&key_book(org_id, actor))
                    .copied()
                    .unwrap_or(0);
                if live + pending >= limits.max_runs_per_key as usize {
                    return Err(denied(
                        "per_key_run_cap",
                        "key",
                        limits.max_runs_per_key,
                        live + pending,
                    ));
                }
            }
        }
        if let Some(bound) = plan.bound_for(class, limits.max_concurrent_runs) {
            let in_class = match class {
                RunClass::Interactive => interactive_in_org,
                RunClass::Agent => in_org.saturating_sub(interactive_in_org),
            };
            if in_class >= bound as usize {
                return Err(denied("org_plan_cap", "org", bound, in_class));
            }
        } else {
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
        }
        inner.pending_global += 1;
        if let Some(run_id) = resume_run_id {
            inner.pending_resume_ids.insert(run_id.to_owned());
        }
        *inner.pending_org.entry(org_id.to_owned()).or_default() += 1;
        if class == RunClass::Interactive {
            *inner.pending_interactive.entry(org_id.to_owned()).or_default() += 1;
        }
        if let Some(actor) = key_actor {
            *inner.pending_key.entry(key_book(org_id, actor)).or_default() += 1;
        }
        Ok(RunSlot {
            registry: self.clone(),
            org_id: org_id.to_owned(),
            class,
            resume_run_id: resume_run_id.map(str::to_owned),
            key_actor: key_actor.map(str::to_owned),
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
    /// The id a resume claimed (D1-021), released with the slot.
    resume_run_id: Option<String>,
    /// INNH-10: the API key actor (`apikey:<key_id>`) whose pending count this slot holds.
    key_actor: Option<String>,
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
        if let Some(run_id) = self.resume_run_id.as_deref() {
            inner.pending_resume_ids.remove(run_id);
        }
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
        if let Some(actor) = self.key_actor.as_deref() {
            let book = key_book(&self.org_id, actor);
            if let Some(count) = inner.pending_key.get_mut(&book) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    inner.pending_key.remove(&book);
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

#[cfg(test)]
mod key_cap_tests {
    use super::*;
    use RunClass::Agent;

    const KEY_A: &str = "apikey:key-a";
    const KEY_B: &str = "apikey:key-b";

    fn registry(per_key: u32) -> Arc<RunRegistry> {
        Arc::new(RunRegistry::new(RunLimits {
            max_runs_per_key: per_key,
            ..RunLimits::unlimited()
        }))
    }

    fn reserve(runs: &Arc<RunRegistry>, org: &str, key: Option<&str>) -> Result<RunSlot, CapacityDenied> {
        match runs.try_reserve_keyed(org, Agent, RunCapOverride::NONE, None, key) {
            Ok(slot) => Ok(slot),
            Err(ReserveDenied::Capacity(denied)) => Err(denied),
            Err(ReserveDenied::ResumeLive) => unreachable!("no resume id was named"),
        }
    }

    #[test]
    fn a_third_pending_run_of_one_key_is_refused_and_another_key_is_not() {
        let runs = registry(2);
        let first = reserve(&runs, "org_a", Some(KEY_A)).unwrap();
        let _second = reserve(&runs, "org_a", Some(KEY_A)).unwrap();
        let denied = reserve(&runs, "org_a", Some(KEY_A)).unwrap_err();
        assert_eq!(denied.reason, "per_key_run_cap");
        assert_eq!(denied.scope, "key");
        assert_eq!(denied.limit, 2);
        assert_eq!(denied.active, 2);
        assert_eq!(denied.retry_after_s, RunLimits::default().cap_retry_after_s);
        // Another key of the same org, a person's start and a service's start are unaffected.
        assert!(reserve(&runs, "org_a", Some(KEY_B)).is_ok());
        assert!(reserve(&runs, "org_a", None).is_ok());
        // The same key in another organization is a different book.
        assert!(reserve(&runs, "org_b", Some(KEY_A)).is_ok());
        // A start that fails before it commits gives the slot back.
        drop(first);
        assert!(reserve(&runs, "org_a", Some(KEY_A)).is_ok());
    }

    #[tokio::test]
    async fn live_runs_of_the_key_count_and_closing_one_frees_a_slot() {
        let runs = registry(2);
        let state = crate::test_support::test_state(crate::test_support::StubDriver::ok());
        let mut claims = crate::test_support::claims_for_org("org_a");
        claims.user_id = KEY_A.into();
        let mut live = Vec::new();
        for _ in 0..2 {
            let handle = crate::test_support::insert_live_run(&state, &claims);
            let run_id = handle.entry.try_lock().expect("idle").run_id.to_string();
            runs.lock().unwrap().insert(run_id.clone(), handle);
            live.push(run_id);
        }
        let denied = reserve(&runs, "org_a", Some(KEY_A)).unwrap_err();
        assert_eq!((denied.reason, denied.active), ("per_key_run_cap", 2));
        assert!(reserve(&runs, "org_a", Some(KEY_B)).is_ok(), "another key is not counted");
        runs.lock().unwrap().remove(&live[0]);
        assert!(reserve(&runs, "org_a", Some(KEY_A)).is_ok(), "closing a run frees its slot");
    }

    #[tokio::test]
    async fn a_committed_slot_moves_from_pending_to_live_without_double_counting() {
        let runs = registry(2);
        let state = crate::test_support::test_state(crate::test_support::StubDriver::ok());
        let mut claims = crate::test_support::claims_for_org("org_a");
        claims.user_id = KEY_A.into();
        let slot = reserve(&runs, "org_a", Some(KEY_A)).unwrap();
        let handle = crate::test_support::insert_live_run(&state, &claims);
        slot.commit("run_x".to_owned(), handle);
        // One live, nothing pending: one more fits, the next does not.
        let one_more = reserve(&runs, "org_a", Some(KEY_A)).unwrap();
        assert!(reserve(&runs, "org_a", Some(KEY_A)).is_err());
        drop(one_more);
        assert!(runs.lock().unwrap().inner_pending_key_is_empty_for_tests());
    }

    #[test]
    fn zero_means_no_per_key_cap() {
        let runs = registry(0);
        let held: Vec<_> = (0..10)
            .map(|_| reserve(&runs, "org_a", Some(KEY_A)).unwrap())
            .collect();
        assert_eq!(held.len(), 10);
    }

    #[test]
    fn the_global_cap_is_checked_before_the_key_cap() {
        let runs = Arc::new(RunRegistry::new(RunLimits {
            max_concurrent_runs: 1,
            max_runs_per_key: 2,
            ..RunLimits::unlimited()
        }));
        let _held = reserve(&runs, "org_a", Some(KEY_A)).unwrap();
        let denied = reserve(&runs, "org_a", Some(KEY_A)).unwrap_err();
        assert_eq!(denied.reason, "global_run_cap");
    }
}

#[cfg(test)]
mod resume_claim_tests {
    use super::*;
    use RunClass::Agent;

    fn registry() -> Arc<RunRegistry> {
        Arc::new(RunRegistry::new(RunLimits::unlimited()))
    }

    #[test]
    fn a_second_resume_of_the_same_id_is_refused_while_the_first_is_pending() {
        let runs = registry();
        let first = runs
            .try_reserve_for("org_a", Agent, RunCapOverride::NONE, Some("run_1"))
            .expect("the first resume claims the id");
        let second = runs.try_reserve_for("org_a", Agent, RunCapOverride::NONE, Some("run_1"));
        assert_eq!(second.unwrap_err(), ReserveDenied::ResumeLive);
        // Another id is not affected, and neither is a start that names none.
        assert!(runs
            .try_reserve_for("org_a", Agent, RunCapOverride::NONE, Some("run_2"))
            .is_ok());
        assert!(runs
            .try_reserve_for("org_a", Agent, RunCapOverride::NONE, None)
            .is_ok());
        // A resume that fails before it commits gives the id back.
        drop(first);
        assert!(runs
            .try_reserve_for("org_a", Agent, RunCapOverride::NONE, Some("run_1"))
            .is_ok());
    }

    #[test]
    fn a_pending_resume_counts_as_live_for_the_sweeper_until_it_is_dropped() {
        let runs = registry();
        assert!(!runs.lock().unwrap().is_live_or_resuming("run_1"));
        let slot = runs
            .try_reserve_for("org_a", Agent, RunCapOverride::NONE, Some("run_1"))
            .expect("the resume claims the id");
        assert!(runs.lock().unwrap().is_live_or_resuming("run_1"));
        assert!(!runs.lock().unwrap().is_live_or_resuming("run_2"));
        drop(slot);
        assert!(!runs.lock().unwrap().is_live_or_resuming("run_1"));
    }

    #[tokio::test]
    async fn a_resume_of_a_run_that_is_live_is_refused_and_the_live_entry_is_not_replaced() {
        let runs = registry();
        let claims = crate::test_support::claims_for_org("org_a");
        let state = crate::test_support::test_state(crate::test_support::StubDriver::ok());
        let live = crate::test_support::insert_live_run(&state, &claims);
        let run_id = live
            .entry
            .try_lock()
            .expect("nothing holds the run yet")
            .run_id
            .to_string();
        runs.lock()
            .unwrap()
            .insert(run_id.clone(), live.clone());
        let refused = runs.try_reserve_for("org_a", Agent, RunCapOverride::NONE, Some(&run_id));
        assert_eq!(refused.unwrap_err(), ReserveDenied::ResumeLive);
        let held = runs.lock().unwrap().get(&run_id).cloned().expect("still registered");
        assert!(
            Arc::ptr_eq(&held.live, &live.live),
            "the live run is the one that was there before the refused resume"
        );
        // Once the run has left the registry (closed), the id can be resumed.
        runs.lock().unwrap().remove(&run_id);
        assert!(runs
            .try_reserve_for("org_a", Agent, RunCapOverride::NONE, Some(&run_id))
            .is_ok());
    }
}

#[cfg(test)]
mod plan_cap_tests {
    use super::*;
    use RunClass::{Agent, Interactive};

    fn registry(max_concurrent: u32, per_org: u32, interactive_per_org: u32) -> Arc<RunRegistry> {
        Arc::new(RunRegistry::new(RunLimits {
            max_concurrent_runs: max_concurrent,
            max_runs_per_org: per_org,
            max_interactive_runs_per_org: interactive_per_org,
            ..RunLimits::default()
        }))
    }

    fn agents(n: u32) -> RunCapOverride {
        RunCapOverride {
            agent_runs: Some(n),
            interactive_runs: None,
        }
    }

    #[test]
    fn a_plan_bound_refuses_the_next_run_of_its_class_as_org_plan_cap() {
        let runs = registry(0, 0, 0);
        let _first = runs.try_reserve_with("org_a", Agent, agents(1)).unwrap();
        let denied = runs.try_reserve_with("org_a", Agent, agents(1)).unwrap_err();
        assert_eq!(denied.reason, "org_plan_cap");
        assert_eq!(denied.scope, "org");
        assert_eq!((denied.limit, denied.active), (1, 1));
        // Another organization is unaffected by it.
        assert!(runs.try_reserve_with("org_b", Agent, agents(1)).is_ok());
    }

    #[test]
    fn a_plan_can_raise_an_organization_above_the_static_floor_but_not_above_the_global_cap() {
        // Floor: two runs per organization; global: four. The plan says three.
        let runs = registry(4, 2, 0);
        let _slots: Vec<_> = (0..3)
            .map(|_| runs.try_reserve_with("org_a", Agent, agents(3)).unwrap())
            .collect();
        let denied = runs.try_reserve_with("org_a", Agent, agents(3)).unwrap_err();
        assert_eq!(denied.reason, "org_plan_cap", "the plan bound is the org's own");
        // Another organization takes the fourth global slot, then the global cap answers.
        let _other = runs.try_reserve_with("org_b", Agent, agents(3)).unwrap();
        let denied = runs.try_reserve_with("org_b", Agent, agents(3)).unwrap_err();
        assert_eq!(denied.reason, "global_run_cap");
    }

    #[test]
    fn a_plan_value_above_the_global_cap_is_clamped_to_it() {
        let runs = registry(2, 0, 0);
        let _a = runs.try_reserve_with("org_a", Agent, agents(50)).unwrap();
        let _b = runs.try_reserve_with("org_a", Agent, agents(50)).unwrap();
        // The global check runs first, so the refusal names the global cap.
        let denied = runs.try_reserve_with("org_a", Agent, agents(50)).unwrap_err();
        assert_eq!(denied.reason, "global_run_cap");
    }

    #[test]
    fn a_class_the_plan_does_not_bound_keeps_the_static_caps() {
        let runs = registry(0, 0, 1);
        let _agent = runs.try_reserve_with("org_a", Agent, agents(5)).unwrap();
        let _person = runs.try_reserve_with("org_a", Interactive, agents(5)).unwrap();
        let denied = runs
            .try_reserve_with("org_a", Interactive, agents(5))
            .unwrap_err();
        assert_eq!(denied.reason, "org_interactive_run_cap");
    }

    #[test]
    fn the_agent_bound_counts_agent_runs_only() {
        let runs = registry(0, 0, 0);
        let _people: Vec<_> = (0..3)
            .map(|_| runs.try_reserve_with("org_a", Interactive, agents(1)).unwrap())
            .collect();
        assert!(runs.try_reserve_with("org_a", Agent, agents(1)).is_ok());
    }

    #[test]
    fn an_explicit_zero_refuses_every_start_of_the_class() {
        let runs = registry(0, 0, 0);
        let denied = runs.try_reserve_with("org_a", Agent, agents(0)).unwrap_err();
        assert_eq!(denied.reason, "org_plan_cap");
        assert_eq!((denied.limit, denied.active), (0, 0));
    }

    #[test]
    fn a_dropped_slot_gives_the_plan_capacity_back() {
        let runs = registry(0, 0, 0);
        let slot = runs.try_reserve_with("org_a", Agent, agents(1)).unwrap();
        assert!(runs.try_reserve_with("org_a", Agent, agents(1)).is_err());
        drop(slot);
        assert!(runs.try_reserve_with("org_a", Agent, agents(1)).is_ok());
    }

    #[test]
    fn no_plan_bound_is_the_static_behaviour() {
        let runs = registry(0, 1, 0);
        let _first = runs.try_reserve_with("org_a", Agent, RunCapOverride::NONE).unwrap();
        let denied = runs
            .try_reserve_with("org_a", Agent, RunCapOverride::NONE)
            .unwrap_err();
        assert_eq!(denied.reason, "org_run_cap");
    }
}
