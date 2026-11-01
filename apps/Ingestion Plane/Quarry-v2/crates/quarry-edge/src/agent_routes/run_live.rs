use super::*;

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::budget::RunClass;
use crate::run_limits::{RunLimits, AGENT_MAX_TTL_CEILING_S};

/// Milliseconds since the Unix epoch. The sweeper takes `now` as a parameter so a
/// test can move the clock; production passes this.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// WS6.K: how long a run has been running, in seconds, on the run clock. A clock that
/// reads earlier than the start gives 0, never a negative or wrapped value.
pub fn run_duration_seconds(started_ms: u64, now_ms: u64) -> f64 {
    now_ms.saturating_sub(started_ms) as f64 / 1000.0
}

/// WS6.K: records `quarry_agent_run_duration_seconds{class}` once, when a run ends.
/// The only label is the closed `interactive|agent` set: no run id, org, actor or URL.
/// The macro is a no-op until the recorder is installed (`metrics.rs`).
pub fn record_run_duration(live: &RunLive, now_ms: u64) {
    metrics::histogram!("quarry_agent_run_duration_seconds", "class" => live.class.label())
        .record(run_duration_seconds(live.started_ms, now_ms));
}

/// Why a run is released by the sweeper (its `agent.expired` `reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpireReason {
    /// No activity for the class's idle limit.
    Idle,
    /// An agent run reached its absolute lifetime.
    Ttl,
    /// A person held the browser and then went quiet.
    TakeoverIdle,
    /// A person's session reached the absolute backstop (even with a viewer open).
    MaxAge,
}

impl ExpireReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Ttl => "ttl",
            Self::TakeoverIdle => "takeover_idle",
            Self::MaxAge => "max_age",
        }
    }
}

/// A run the sweeper would release, and how long it sat idle and lived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpireVerdict {
    pub reason: ExpireReason,
    pub idle_s: u64,
    pub age_s: u64,
}

/// Who is driving an action: a person at the keyboard, an agent proposing it, or
/// the edge itself (the run sweeper, later). The same three values the audit
/// trail already uses for control transitions.
pub type StepInitiator = BrowserTimelineInitiator;

/// What a client says about who initiated an action. It can only *tighten*:
/// a hint of `agent` makes a person's own token count as an agent for this call,
/// and nothing a client sends can make an agent count as a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitiatorHint {
    Human,
    Agent,
}

/// The initiator of an action, from the verified token and the optional hint.
///
/// A service principal is always an agent, whatever it sends, and so is an API key (INNH-10:
/// there is never a person behind a key). A user is a person
/// unless the hint says agent (the gateway sends that for actions the model
/// proposed through the user's own session). `System` is never derived from a
/// request.
pub fn derive_initiator(
    claims: &crate::auth::Claims,
    hint: Option<InitiatorHint>,
) -> StepInitiator {
    if claims.is_service() || claims.is_api_key() {
        return StepInitiator::Agent;
    }
    match hint {
        Some(InitiatorHint::Agent) => StepInitiator::Agent,
        _ => StepInitiator::Human,
    }
}

/// The longest advisory value kept.
const ADVISORY_MAX_LEN: usize = 64;

/// What the caller said about *which* agent it is, in fields it chose: the AI run
/// it acts for and a short label. Advisory only. Nothing here is verified, so no
/// decision (takeover gate, budget class, ownership) may read it; it is recorded
/// next to the verified identity and marked as the caller's word.
///
/// Both are identifier-shaped (`[A-Za-z0-9._:-]`, 1 to 64 characters). Anything
/// else is dropped, not rejected: a bad label must not fail a browser call, and an
/// identifier-shaped value cannot carry a URL or a page title into a ZDR-bound
/// record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdvisoryAgent {
    pub ai_run_id: Option<String>,
    pub label: Option<String>,
}

impl AdvisoryAgent {
    #[must_use]
    pub fn new(ai_run_id: Option<&str>, label: Option<&str>) -> Self {
        Self {
            ai_run_id: advisory_identifier(ai_run_id),
            label: advisory_identifier(label),
        }
    }
}

fn advisory_identifier(raw: Option<&str>) -> Option<String> {
    let value = raw?.trim();
    let shaped = !value.is_empty()
        && value.len() <= ADVISORY_MAX_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'));
    shaped.then(|| value.to_owned())
}

/// Who drove an operation: the class, and for an agent which one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Initiator {
    pub class: StepInitiator,
    /// Present only when `class` is `Agent` and the call came through
    /// [`derive_initiator_ref`].
    pub agent: Option<AgentIdentity>,
}

impl From<StepInitiator> for Initiator {
    fn from(class: StepInitiator) -> Self {
        Self { class, agent: None }
    }
}

/// [`derive_initiator`] plus which agent it was.
///
/// - A service principal's token: an agent, whatever it sends. The identity is
///   `ServicePrincipal` and its `principal_id` is the token's signed service id:
///   the only verified part. It cannot be claimed or changed from the request.
/// - An API key's token (INNH-10): an agent, whatever it sends. The identity is `ApiKey` and
///   its `principal_id` is the token's signed actor, `apikey:<key_id>`: verified, like a
///   service id. The key id is the only verified part; the owner is not on the receipt.
/// - A person's token with the `agent` hint: an agent (the hint only tightens),
///   identity `UserSession` with no `principal_id`: the token proves a person,
///   not an agent.
/// - A person's token otherwise: a person, no agent.
///
/// `advisory` rides along on an agent's identity and is dropped for a person.
#[must_use]
pub fn derive_initiator_ref(
    claims: &crate::auth::Claims,
    hint: Option<InitiatorHint>,
    advisory: &AdvisoryAgent,
) -> Initiator {
    let class = derive_initiator(claims, hint);
    if class != StepInitiator::Agent {
        return Initiator { class, agent: None };
    }
    let service = claims.is_service();
    let api_key = claims.is_api_key();
    Initiator {
        class,
        agent: Some(AgentIdentity {
            basis: if service {
                AgentIdentityBasis::ServicePrincipal
            } else if api_key {
                AgentIdentityBasis::ApiKey
            } else {
                AgentIdentityBasis::UserSession
            },
            principal_id: if service {
                Some(
                    claims
                        .service_id
                        .clone()
                        .unwrap_or_else(|| claims.actor_id().to_owned()),
                )
            } else if api_key {
                Some(claims.actor_id().to_owned())
            } else {
                None
            },
            ai_run_id: advisory.ai_run_id.clone(),
            label: advisory.label.clone(),
        }),
    }
}

/// A consistent reading of who holds the browser and for which epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlSnapshot {
    pub mode: BrowserControlMode,
    /// Counts real mode changes; it never goes down and never repeats for one run.
    pub epoch: u64,
}

/// The part of a run that must be readable and changeable without waiting for
/// the run's lock: who owns it and who holds the browser. A step holds the run
/// lock for as long as its action takes, and a person taking over must not queue
/// behind it.
///
/// `control` packs `epoch << 1 | takeover_bit` into one atomic so the mode and
/// its epoch are always read together.
pub struct RunLive {
    org_id: String,
    actor_id: String,
    control: AtomicU64,
    /// When the current takeover began (ms since the epoch); 0 when none is open or the
    /// start is not known (a run resumed into a takeover). Written after the control
    /// word changes, so a reader can see the new mode a moment before the time.
    taken_over_ms: AtomicU64,
    class: RunClass,
    started_ms: u64,
    last_activity_ms: AtomicU64,
    /// Open WebSocket, frame stream or event stream: a person is watching.
    viewers: AtomicU32,
    /// Requests currently running an action on this run (or waiting to).
    in_flight: AtomicU32,
    /// The lifetime the caller asked for (`constraints.max_runtime_s`), if it asked.
    explicit_runtime_s: Option<u64>,
    /// WS7.D: the run's web-policy decision, resolved at start (see `agent_policy_gate`).
    /// `None` only for a run built outside `start_run`.
    policy: Option<crate::policy_baseline::Decision>,
    /// D2-051: the `policy` ask the run was started with (merged with the ask of every
    /// earlier segment), kept so each checkpoint stores it and a resume re-applies it.
    policy_ask: Option<quarry_core::web_policy::RequestAsk>,
    /// D2-061: the run's event trace, read for the next seq each checkpoint stores.
    trace: Option<(quarry_runtime::EventSink, RunKind)>,
    /// WS8.F: who holds each tab, and the run's attach. Locked for one read or change only.
    pub(super) tabs: StdMutex<TabLedger>,
    pub(super) attaches: StdMutex<AttachRegistry>,
    /// WS8.H: the next step key. Shared by every lane, so the events and receipts keyed by a
    /// step are unique across the run. Reserved once an action is admitted (a refusal takes none).
    steps_reserved: AtomicU32,
}

impl RunLive {
    pub fn new(
        org_id: impl Into<String>,
        actor_id: impl Into<String>,
        mode: BrowserControlMode,
        epoch: u64,
    ) -> Self {
        Self {
            org_id: org_id.into(),
            actor_id: actor_id.into(),
            control: AtomicU64::new(Self::pack(epoch, mode)),
            taken_over_ms: AtomicU64::new(0),
            class: RunClass::Interactive,
            started_ms: now_ms(),
            last_activity_ms: AtomicU64::new(now_ms()),
            viewers: AtomicU32::new(0),
            in_flight: AtomicU32::new(0),
            explicit_runtime_s: None,
            policy: None,
            policy_ask: None,
            trace: None,
            tabs: StdMutex::new(TabLedger::new()),
            attaches: StdMutex::new(AttachRegistry::default()),
            steps_reserved: AtomicU32::new(0),
        }
    }

    /// WS8.H: start the step keys of a resumed run at the first key its earlier segments
    /// did not use (`lifecycle::resume_next_step`, D2), not at its count of completed steps:
    /// a failed step also used a key.
    pub fn with_resume_step(mut self, next_step: u32) -> Self {
        *self.steps_reserved.get_mut() = next_step;
        self
    }

    /// WS8.H: reserve the next step key for an admitted action (any lane). Reserved while the
    /// entry lock is held, after admission, so a refusal takes none.
    pub fn reserve_step(&self) -> u32 {
        self.steps_reserved.fetch_add(1, Ordering::AcqRel)
    }

    /// D2: the step key the run hands out next (one past the highest reserved). Every
    /// checkpoint stores it, so a resume never hands a used key out again. Read while the
    /// entry lock is held, which is also when keys are reserved.
    pub fn next_step(&self) -> u32 {
        self.steps_reserved.load(Ordering::Acquire)
    }

    /// Say what kind of run this is and how long its caller asked for.
    pub fn with_run(mut self, class: RunClass, explicit_runtime_s: Option<u64>) -> Self {
        self.class = class;
        self.explicit_runtime_s = explicit_runtime_s.filter(|seconds| *seconds > 0);
        self
    }

    /// WS7.D: attach the decision `start_run` resolved for this run.
    pub fn with_policy(mut self, decision: crate::policy_baseline::Decision) -> Self {
        self.policy = Some(decision);
        self
    }

    /// D2-051: keep the ask the run's policy was resolved with, for its checkpoints.
    pub fn with_policy_ask(mut self, ask: Option<quarry_core::web_policy::RequestAsk>) -> Self {
        self.policy_ask = ask;
        self
    }

    /// D2-051: the ask a checkpoint stores, or `None` when the run has none.
    pub fn policy_ask(&self) -> Option<&quarry_core::web_policy::RequestAsk> {
        self.policy_ask.as_ref()
    }

    /// D2-061: attach the run's event trace, so a checkpoint can store its next seq.
    pub fn with_trace(mut self, sink: quarry_runtime::EventSink, run_id: RunKind) -> Self {
        self.trace = Some((sink, run_id));
        self
    }

    /// D2-061: the seq the run's trace mints next, or 0 when it has no open trace (a run
    /// built outside `start_run`, or one whose trace is closed): a checkpoint then stores
    /// nothing, and a resume starts at 1 as before.
    pub fn trace_seq(&self) -> u64 {
        self.trace
            .as_ref()
            .and_then(|(sink, run_id)| sink.next_seq(run_id))
            .unwrap_or(0)
    }

    /// WS7.D: the run's web-policy decision, or `None` for a run built outside `start_run`.
    pub fn policy(&self) -> Option<&crate::policy_baseline::Decision> {
        self.policy.as_ref()
    }

    /// Start the clocks at `now_ms` (tests).
    pub fn with_clock(mut self, started_ms: u64) -> Self {
        self.started_ms = started_ms;
        *self.last_activity_ms.get_mut() = started_ms;
        self
    }

    pub fn class(&self) -> RunClass {
        self.class
    }

    /// When the run started, in ms since the epoch (the clock `new` set).
    pub fn started_ms(&self) -> u64 {
        self.started_ms
    }

    pub fn org_id(&self) -> &str {
        &self.org_id
    }

    /// The signed actor that started the run (`apikey:<key_id>` for an API key's run).
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    /// Record activity by the run's owner.
    pub fn touch(&self) {
        self.touch_at(now_ms());
    }

    pub fn touch_at(&self, now_ms: u64) {
        self.last_activity_ms.fetch_max(now_ms, Ordering::AcqRel);
    }

    pub fn viewers(&self) -> u32 {
        self.viewers.load(Ordering::Acquire)
    }

    pub fn in_flight(&self) -> u32 {
        self.in_flight.load(Ordering::Acquire)
    }

    /// WS6.J: how long the run sat idle and how long it lived at `now_ms`, in whole seconds
    /// (the `idle_s` and `age_s` of an `agent.expired` event with `reason: "shutdown"`).
    pub fn idle_and_age_s(&self, now_ms: u64) -> (u64, u64) {
        let idle_s = now_ms.saturating_sub(self.last_activity_ms.load(Ordering::Acquire)) / 1000;
        let age_s = now_ms.saturating_sub(self.started_ms) / 1000;
        (idle_s, age_s)
    }

    /// Hold while a person is watching (a WebSocket, a frame or event stream).
    pub fn viewer_guard(self: &Arc<Self>) -> ViewerGuard {
        self.viewers.fetch_add(1, Ordering::AcqRel);
        self.touch();
        ViewerGuard(self.clone())
    }

    /// Hold while an action runs, from before it waits for the run lock until it
    /// finishes: a run with an action in flight is never expired.
    pub fn in_flight_guard(self: &Arc<Self>) -> InFlightGuard {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        InFlightGuard(self.clone())
    }

    /// Whether the sweeper should release this run at `now_ms`, and why.
    ///
    /// - An action in flight is never expired.
    /// - An agent run lives its explicit `max_runtime_s` plus a grace, or the agent
    ///   default when it gave none (the 120 s a UI session carries is not an agent's
    ///   request); it expires idle after the agent idle limit; viewers do not matter.
    /// - A person's session ignores the gateway's 120 s; an open viewer blocks the
    ///   idle limit but not the absolute backstop; a takeover has a longer idle limit.
    pub fn verdict(&self, now_ms: u64, limits: &RunLimits) -> Option<ExpireVerdict> {
        if self.in_flight() > 0 {
            return None;
        }
        let age_s = now_ms.saturating_sub(self.started_ms) / 1000;
        let idle_s = now_ms.saturating_sub(self.last_activity_ms.load(Ordering::Acquire)) / 1000;
        let found = |reason| Some(ExpireVerdict { reason, idle_s, age_s });
        match self.class {
            RunClass::Agent => {
                let absolute = match self.explicit_runtime_s {
                    Some(seconds) => (seconds + limits.agent_ttl_grace_s).min(AGENT_MAX_TTL_CEILING_S),
                    None => limits.agent_max_ttl_s,
                };
                if age_s >= absolute {
                    found(ExpireReason::Ttl)
                } else if idle_s >= limits.agent_idle_ttl_s {
                    found(ExpireReason::Idle)
                } else {
                    None
                }
            }
            RunClass::Interactive => {
                if age_s >= limits.interactive_max_ttl_s {
                    return found(ExpireReason::MaxAge);
                }
                if self.viewers() > 0 {
                    return None;
                }
                if self.snapshot().mode == BrowserControlMode::HumanTakeover {
                    (idle_s >= limits.takeover_idle_ttl_s).then_some(ExpireVerdict {
                        reason: ExpireReason::TakeoverIdle,
                        idle_s,
                        age_s,
                    })
                } else {
                    (idle_s >= limits.interactive_idle_ttl_s).then_some(ExpireVerdict {
                        reason: ExpireReason::Idle,
                        idle_s,
                        age_s,
                    })
                }
            }
        }
    }

    fn pack(epoch: u64, mode: BrowserControlMode) -> u64 {
        (epoch << 1) | u64::from(mode == BrowserControlMode::HumanTakeover)
    }

    fn unpack(raw: u64) -> ControlSnapshot {
        ControlSnapshot {
            mode: if raw & 1 == 1 {
                BrowserControlMode::HumanTakeover
            } else {
                BrowserControlMode::AgentControl
            },
            epoch: raw >> 1,
        }
    }

    pub fn snapshot(&self) -> ControlSnapshot {
        Self::unpack(self.control.load(Ordering::Acquire))
    }

    /// Switch the control mode. The epoch advances only when the mode really
    /// changes, so a repeated request is idempotent. With `expected_epoch` the
    /// switch is a compare-and-set: it is refused, and the current epoch
    /// returned, when the run has moved on.
    ///
    /// Returns `(changed, epoch)` on success.
    pub fn set_mode(
        &self,
        mode: BrowserControlMode,
        expected_epoch: Option<u64>,
    ) -> Result<(bool, u64), u64> {
        let mut refused_at = None;
        let result = self
            .control
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                let snapshot = Self::unpack(current);
                if expected_epoch.is_some_and(|expected| expected != snapshot.epoch) {
                    refused_at = Some(snapshot.epoch);
                    return None;
                }
                if snapshot.mode == mode {
                    return None;
                }
                Some(Self::pack(snapshot.epoch + 1, mode))
            });
        match result {
            Ok(previous) => {
                let at = if mode == BrowserControlMode::HumanTakeover { now_ms() } else { 0 };
                self.taken_over_ms.store(at, Ordering::Release);
                Ok((true, Self::unpack(previous).epoch + 1))
            }
            Err(current) => match refused_at {
                Some(epoch) => Err(epoch),
                None => Ok((false, Self::unpack(current).epoch)),
            },
        }
    }

    /// Put back a checkpointed state when a run is resumed.
    pub fn restore(&self, epoch: u64, mode: BrowserControlMode) {
        self.control.store(Self::pack(epoch, mode), Ordering::Release);
        // The checkpoint does not carry when the takeover began.
        self.taken_over_ms.store(0, Ordering::Release);
    }

    /// When the open takeover began, if one is open and its start is known.
    pub fn taken_over_at_ms(&self) -> Option<u64> {
        if self.snapshot().mode != BrowserControlMode::HumanTakeover {
            return None;
        }
        Some(self.taken_over_ms.load(Ordering::Acquire)).filter(|ms| *ms > 0)
    }

    /// Whether the verified caller owns this run (same rule as
    /// `caller_owns_live_run`, without the run lock).
    pub fn owned_by(&self, claims: &crate::auth::Claims) -> bool {
        caller_owns_browser_run(&self.org_id, &self.actor_id, claims)
    }
}

/// Counts a person watching the run; released on drop.
pub struct ViewerGuard(Arc<RunLive>);

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        self.0.viewers.fetch_sub(1, Ordering::AcqRel);
        self.0.touch();
    }
}

/// Counts an action in flight; released on drop.
pub struct InFlightGuard(Arc<RunLive>);

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::AcqRel);
        self.0.touch();
    }
}

/// What the registry holds for a live run: the run itself behind its lock, and
/// the lock-free part beside it.
#[derive(Clone)]
pub struct RunHandle {
    pub entry: Arc<TokioMutex<RunEntry>>,
    pub live: Arc<RunLive>,
}

impl RunHandle {
    pub fn new(entry: RunEntry, live: RunLive) -> Self {
        Self {
            entry: Arc::new(TokioMutex::new(entry)),
            live: Arc::new(live),
        }
    }
}

/// Find a live run in the registry. The map lock is held only for the lookup.
pub(super) fn lookup_run(state: &AppState, run_id: &str) -> Option<RunHandle> {
    state
        .agent_runs
        .lock()
        .expect("agent_runs mutex poisoned")
        .get(run_id)
        .cloned()
}

#[cfg(test)]
mod run_duration_tests {
    use super::run_duration_seconds;

    #[test]
    fn duration_is_measured_on_the_run_clock_in_seconds() {
        assert_eq!(run_duration_seconds(1_000, 3_500), 2.5);
        assert_eq!(run_duration_seconds(1_000, 1_000), 0.0);
    }

    #[test]
    fn a_clock_that_reads_earlier_than_the_start_gives_zero() {
        assert_eq!(run_duration_seconds(5_000, 4_000), 0.0);
    }
}
