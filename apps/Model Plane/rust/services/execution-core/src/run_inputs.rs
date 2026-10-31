//! The files a run's steps may read, held in memory for the length of the call
//! that carries them (build plan WS08-S5).
//!
//! `ExecuteStepRequest.attachments` brings the bytes of the files a person
//! attached. A tool call is dispatched deep inside the runtime loop, whose
//! signatures this feature does not widen, so the request registers its files
//! here under `(org_id, run_id)` and the `code_interpreter` arm reads them back
//! by the same key. The registry is the only holder, and it is built to hold
//! customer bytes no longer than the work needs:
//!
//! * a registration is an RAII guard: parallel steps of one run share a single
//!   registration, and the last guard to drop removes the entry and overwrites
//!   the bytes with zeros;
//! * the registry is bounded in entries and in total bytes, so a flood of runs
//!   refuses new registrations instead of growing without limit;
//! * a registration whose guard never ran (a leaked task) is evicted by a TTL
//!   sweep on the next registration;
//! * nothing here logs content: counts and sizes only.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1::RunAttachment;

/// Most runs that may hold inputs at once.
const DEFAULT_MAX_ENTRIES: usize = 256;
/// Most bytes the registry holds across every run.
const DEFAULT_MAX_BYTES: usize = 64 * 1024 * 1024;
/// Longer than any single tool call (`EXECUTION_CODE_TIMEOUT_SECS` is minutes at
/// most), so a live registration is never swept from under its own call.
const DEFAULT_TTL: Duration = Duration::from_secs(15 * 60);
/// Longest a registration may live, whatever its run's deadline says. A run with no time limit
/// still has a leaked guard swept within a day.
const MAX_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// One validated attachment, ready to be written into a workspace.
pub struct StagedFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The files of one registration. Shared by every step of the run; emptied and
/// zeroed when the last guard drops.
pub struct InputSet {
    files: Mutex<Vec<StagedFile>>,
}

impl std::fmt::Debug for InputSet {
    // Names and counts only: this type holds customer bytes.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputSet")
            .field("files", &lock(&self.files).len())
            .finish()
    }
}

impl PartialEq for InputSet {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for InputSet {}

impl InputSet {
    /// Runs `read` over the files while holding them, and gives back its result.
    /// After the registration is released the slice is empty, so a late reader
    /// stages nothing rather than stale bytes.
    pub fn with_files<T>(&self, read: impl FnOnce(&[StagedFile]) -> T) -> T {
        read(&lock(&self.files))
    }

    fn wipe(&self) {
        let mut files = lock(&self.files);
        for file in files.iter_mut() {
            file.bytes.fill(0);
            std::hint::black_box(&file.bytes);
        }
        files.clear();
    }

    /// Whether the bytes have been released. Used by tests.
    #[cfg(test)]
    fn is_wiped(&self) -> bool {
        lock(&self.files).is_empty()
    }
}

/// Why a registration was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterError {
    /// The registry already holds the most runs it allows.
    EntryCap,
    /// The registry already holds the most bytes it allows.
    ByteCap,
}

struct Entry {
    id: u64,
    set: Arc<InputSet>,
    refs: usize,
    bytes: usize,
    registered_at: Instant,
    /// How long this registration may live, measured from `registered_at`.
    ttl: Duration,
}

#[derive(Default)]
struct State {
    entries: HashMap<(String, String), Entry>,
    bytes: usize,
    next_id: u64,
}

impl State {
    fn remove(&mut self, key: &(String, String)) {
        if let Some(entry) = self.entries.remove(key) {
            self.bytes = self.bytes.saturating_sub(entry.bytes);
            entry.set.wipe();
        }
    }
}

/// The registry. One per process ([`global`]); tests build their own.
pub struct RunInputs {
    state: Arc<Mutex<State>>,
    max_entries: usize,
    max_bytes: usize,
    ttl: Duration,
}

/// Keeps a registration alive. Dropping the last guard of a run removes the
/// entry and zeroes its bytes.
#[must_use = "dropping the guard releases the files at once"]
pub struct RunInputsGuard {
    state: Arc<Mutex<State>>,
    key: (String, String),
    id: u64,
}

impl std::fmt::Debug for RunInputsGuard {
    // The run key is an identifier, not content, but nothing else is worth printing.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunInputsGuard").finish_non_exhaustive()
    }
}

impl Drop for RunInputsGuard {
    fn drop(&mut self) {
        let mut state = lock(&self.state);
        let Some(entry) = state.entries.get_mut(&self.key) else {
            return;
        };
        // A guard whose entry was swept and replaced by a newer registration of
        // the same run must not release the newer one.
        if entry.id != self.id {
            return;
        }
        entry.refs = entry.refs.saturating_sub(1);
        if entry.refs == 0 {
            state.remove(&self.key);
        }
    }
}

impl Default for RunInputs {
    fn default() -> Self {
        Self::with_limits(DEFAULT_MAX_ENTRIES, DEFAULT_MAX_BYTES, DEFAULT_TTL)
    }
}

impl RunInputs {
    #[must_use]
    pub fn with_limits(max_entries: usize, max_bytes: usize, ttl: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            max_entries,
            max_bytes,
            ttl,
        }
    }

    /// Holds `attachments` for `(org_id, run_id)` until the returned guard (and
    /// every other guard of the run) drops. An empty list registers nothing and
    /// returns `None`. The list must already have passed
    /// `mp_contracts::attachments::validate`.
    ///
    /// A run that is already registered shares its existing registration; the
    /// steps of one run carry the same list, so the first one wins.
    ///
    /// # Errors
    ///
    /// [`RegisterError`] when the registry is at its entry or byte bound.
    pub fn register(
        &self,
        org_id: &str,
        run_id: &str,
        attachments: &[RunAttachment],
    ) -> Result<Option<RunInputsGuard>, RegisterError> {
        self.register_until(org_id, run_id, attachments, self.ttl)
    }

    /// [`register`](Self::register) with the registration's own lifetime: the sweep evicts it once
    /// it is `ttl` old, whatever else is registered. A run that outlives the default asks for its
    /// own. A registration that already exists keeps the longer of the two lifetimes, so a shorter
    /// one of the same run never cuts a longer one short.
    ///
    /// # Errors
    ///
    /// [`RegisterError`] when the registry is at its entry or byte bound.
    pub fn register_until(
        &self,
        org_id: &str,
        run_id: &str,
        attachments: &[RunAttachment],
        ttl: Duration,
    ) -> Result<Option<RunInputsGuard>, RegisterError> {
        if attachments.is_empty() {
            return Ok(None);
        }
        let key = (org_id.to_owned(), run_id.to_owned());
        let mut state = lock(&self.state);
        Self::sweep_locked(&mut state, Instant::now());
        if let Some(entry) = state.entries.get_mut(&key) {
            entry.refs += 1;
            entry.ttl = entry.ttl.max(ttl);
            return Ok(Some(RunInputsGuard {
                state: Arc::clone(&self.state),
                key,
                id: entry.id,
            }));
        }
        let bytes: usize = attachments.iter().map(|a| a.content.len()).sum();
        if state.entries.len() >= self.max_entries {
            return Err(RegisterError::EntryCap);
        }
        if state.bytes.saturating_add(bytes) > self.max_bytes {
            return Err(RegisterError::ByteCap);
        }
        let id = state.next_id;
        state.next_id += 1;
        let set = Arc::new(InputSet {
            files: Mutex::new(
                attachments
                    .iter()
                    .map(|a| StagedFile {
                        name: a.name.clone(),
                        bytes: a.content.clone(),
                    })
                    .collect(),
            ),
        });
        state.bytes += bytes;
        state.entries.insert(
            key.clone(),
            Entry {
                id,
                set,
                refs: 1,
                bytes,
                registered_at: Instant::now(),
                ttl,
            },
        );
        tracing::debug!(
            attachments = attachments.len(),
            bytes,
            "run inputs registered"
        );
        Ok(Some(RunInputsGuard {
            state: Arc::clone(&self.state),
            key,
            id,
        }))
    }

    /// The files registered for `(org_id, run_id)`, if any.
    #[must_use]
    pub fn get(&self, org_id: &str, run_id: &str) -> Option<Arc<InputSet>> {
        lock(&self.state)
            .entries
            .get(&(org_id.to_owned(), run_id.to_owned()))
            .map(|entry| Arc::clone(&entry.set))
    }

    /// Evicts every registration older than its own lifetime as of `now`; returns how many.
    pub fn sweep_at(&self, now: Instant) -> usize {
        Self::sweep_locked(&mut lock(&self.state), now)
    }

    fn sweep_locked(state: &mut State, now: Instant) -> usize {
        let stale: Vec<(String, String)> = state
            .entries
            .iter()
            .filter(|(_, entry)| now.saturating_duration_since(entry.registered_at) >= entry.ttl)
            .map(|(key, _)| key.clone())
            .collect();
        for key in &stale {
            state.remove(key);
        }
        stale.len()
    }

    /// Total bytes held now. Used by tests.
    #[cfg(test)]
    fn held_bytes(&self) -> usize {
        lock(&self.state).bytes
    }
}

/// The process-wide registry the gRPC handler registers into and the
/// `code_interpreter` arm reads from.
#[must_use]
pub fn global() -> &'static RunInputs {
    static GLOBAL: OnceLock<RunInputs> = OnceLock::new();
    GLOBAL.get_or_init(RunInputs::default)
}

/// The lifetime of a registration for a run with `remaining` of its time budget left (`None` when
/// the run has no limit): the run's own time, never less than [`DEFAULT_TTL`] and never more than
/// [`MAX_TTL`]. A Do run longer than the default keeps its files for the whole run.
#[must_use]
pub fn ttl_for_run(remaining: Option<Duration>) -> Duration {
    remaining.map_or(MAX_TTL, |left| left.clamp(DEFAULT_TTL, MAX_TTL))
}

/// A poisoned lock is still released: the data is bytes to zero, and refusing to
/// would keep them alive.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::attachments::sha256_hex;
    use mp_contracts::model_plane::v1::RunAttachmentKind;

    fn attachment(name: &str, content: &[u8]) -> RunAttachment {
        RunAttachment {
            id: name.to_owned(),
            name: name.to_owned(),
            mime: "text/csv".to_owned(),
            size_bytes: content.len() as u64,
            sha256: sha256_hex(content),
            content: content.to_vec(),
            kind: RunAttachmentKind::File as i32,
        }
    }

    fn registry() -> RunInputs {
        RunInputs::with_limits(4, 1024, Duration::from_secs(60))
    }

    #[test]
    fn an_empty_list_registers_nothing() {
        let registry = registry();
        assert!(registry.register("org", "run", &[]).expect("ok").is_none());
        assert!(registry.get("org", "run").is_none());
    }

    #[test]
    fn last_guard_drop_removes_and_zeroes() {
        let registry = registry();
        let guard = registry
            .register("org", "run", &[attachment("a.csv", b"secret")])
            .expect("registers")
            .expect("guard");
        let set = registry.get("org", "run").expect("registered");
        set.with_files(|files| assert_eq!(files[0].bytes, b"secret"));
        assert_eq!(registry.held_bytes(), 6);

        drop(guard);

        assert!(registry.get("org", "run").is_none(), "entry removed");
        assert_eq!(registry.held_bytes(), 0);
        // The reader still holds the set; its bytes are already gone.
        assert!(set.is_wiped());
    }

    #[test]
    fn parallel_steps_share_one_registration() {
        let registry = registry();
        let first = registry
            .register("org", "run", &[attachment("a.csv", b"one")])
            .expect("registers")
            .expect("guard");
        let second = registry
            .register("org", "run", &[attachment("a.csv", b"one")])
            .expect("shares")
            .expect("guard");
        assert_eq!(registry.held_bytes(), 3, "one copy, not two");
        let set = registry.get("org", "run").expect("registered");

        drop(first);
        assert!(registry.get("org", "run").is_some(), "second step still reads");
        assert!(!set.is_wiped());

        drop(second);
        assert!(registry.get("org", "run").is_none());
        assert!(set.is_wiped());
    }

    #[test]
    fn registrations_are_keyed_by_org_and_run() {
        let registry = registry();
        let _guard = registry
            .register("org-a", "run", &[attachment("a.csv", b"a")])
            .expect("registers");
        assert!(registry.get("org-b", "run").is_none(), "another org sees nothing");
        assert!(registry.get("org-a", "other-run").is_none());
    }

    #[test]
    fn ttl_sweep_evicts_orphans() {
        let registry = registry();
        let orphan = registry
            .register("org", "run", &[attachment("a.csv", b"leaked")])
            .expect("registers")
            .expect("guard");
        let set = registry.get("org", "run").expect("registered");
        // A leaked task never drops its guard.
        std::mem::forget(orphan);

        assert_eq!(registry.sweep_at(Instant::now()), 0, "a live entry stays");
        assert_eq!(registry.sweep_at(Instant::now() + Duration::from_secs(61)), 1);
        assert!(registry.get("org", "run").is_none());
        assert_eq!(registry.held_bytes(), 0);
        assert!(set.is_wiped());
    }

    #[test]
    fn a_swept_guard_does_not_release_a_newer_registration() {
        let registry = RunInputs::with_limits(4, 1024, Duration::ZERO);
        let stale = registry
            .register("org", "run", &[attachment("a.csv", b"old")])
            .expect("registers")
            .expect("guard");
        // The zero TTL sweeps the stale entry as the next registration arrives.
        let newer = registry
            .register("org", "run", &[attachment("a.csv", b"new")])
            .expect("registers")
            .expect("guard");
        drop(stale);
        let set = registry.get("org", "run").expect("the newer entry survives");
        set.with_files(|files| assert_eq!(files[0].bytes, b"new"));
        drop(newer);
        assert!(registry.get("org", "run").is_none());
    }

    #[test]
    fn byte_cap_rejects_registration() {
        let registry = registry();
        let _held = registry
            .register("org", "run-1", &[attachment("a.bin", &[1u8; 800])])
            .expect("registers");
        let refused = registry.register("org", "run-2", &[attachment("b.bin", &[2u8; 300])]);
        assert_eq!(refused.err(), Some(RegisterError::ByteCap));
        assert!(registry.get("org", "run-2").is_none());
    }

    #[test]
    fn entry_cap_rejects_registration() {
        let registry = RunInputs::with_limits(1, 1024, Duration::from_secs(60));
        let _held = registry
            .register("org", "run-1", &[attachment("a.bin", b"1")])
            .expect("registers");
        let refused = registry.register("org", "run-2", &[attachment("b.bin", b"2")]);
        assert_eq!(refused.err(), Some(RegisterError::EntryCap));
    }

    #[test]
    fn a_registration_with_its_own_lifetime_outlives_the_default_sweep() {
        let registry = RunInputs::with_limits(4, 1024, DEFAULT_TTL);
        let _long = registry
            .register_until(
                "org",
                "long-run",
                &[attachment("a.csv", b"long")],
                Duration::from_secs(60 * 60),
            )
            .expect("registers")
            .expect("guard");
        let _default = registry
            .register("org", "default-run", &[attachment("b.csv", b"short")])
            .expect("registers")
            .expect("guard");
        // Both guards are held, so only each registration's own lifetime decides the sweep.
        let later = Instant::now() + Duration::from_secs(16 * 60);
        assert_eq!(registry.sweep_at(later), 1, "only the default-lifetime entry goes");
        assert!(registry.get("org", "long-run").is_some(), "the longer run keeps its files");
        assert!(registry.get("org", "default-run").is_none());
    }

    #[test]
    fn a_shared_registration_keeps_the_longest_lifetime_asked_for() {
        let registry = RunInputs::with_limits(4, 1024, DEFAULT_TTL);
        let _short = registry
            .register("org", "run", &[attachment("a.csv", b"one")])
            .expect("registers")
            .expect("guard");
        let _long = registry
            .register_until(
                "org",
                "run",
                &[attachment("a.csv", b"one")],
                Duration::from_secs(2 * 60 * 60),
            )
            .expect("shares")
            .expect("guard");
        let later = Instant::now() + Duration::from_secs(16 * 60);
        assert_eq!(registry.sweep_at(later), 0, "the longest guard's lifetime holds");
        assert!(registry.get("org", "run").is_some());
    }

    #[test]
    fn a_run_lives_as_long_as_its_deadline_within_the_bounds() {
        assert_eq!(ttl_for_run(None), MAX_TTL, "no limit: the ceiling");
        assert_eq!(
            ttl_for_run(Some(Duration::from_secs(60))),
            DEFAULT_TTL,
            "never less than the default"
        );
        let two_hours = Duration::from_secs(2 * 60 * 60);
        assert_eq!(ttl_for_run(Some(two_hours)), two_hours);
        assert_eq!(
            ttl_for_run(Some(Duration::from_secs(100 * 60 * 60))),
            MAX_TTL,
            "never more than the ceiling"
        );
    }

    #[test]
    fn debug_never_prints_bytes() {
        let registry = registry();
        let _guard = registry
            .register("org", "run", &[attachment("a.csv", b"TOPSECRET")])
            .expect("registers");
        let set = registry.get("org", "run").expect("registered");
        assert!(!format!("{set:?}").contains("TOPSECRET"));
    }
}
