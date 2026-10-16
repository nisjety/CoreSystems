use super::*;

use std::sync::atomic::{AtomicU64, Ordering};

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
/// A service principal is always an agent, whatever it sends. A user is a person
/// unless the hint says agent (the gateway sends that for actions the model
/// proposed through the user's own session). `System` is never derived from a
/// request.
pub fn derive_initiator(
    claims: &crate::auth::Claims,
    hint: Option<InitiatorHint>,
) -> StepInitiator {
    if claims.is_service() {
        return StepInitiator::Agent;
    }
    match hint {
        Some(InitiatorHint::Agent) => StepInitiator::Agent,
        _ => StepInitiator::Human,
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
            Ok(previous) => Ok((true, Self::unpack(previous).epoch + 1)),
            Err(current) => match refused_at {
                Some(epoch) => Err(epoch),
                None => Ok((false, Self::unpack(current).epoch)),
            },
        }
    }

    /// Put back a checkpointed state when a run is resumed.
    pub fn restore(&self, epoch: u64, mode: BrowserControlMode) {
        self.control.store(Self::pack(epoch, mode), Ordering::Release);
    }

    /// Whether the verified caller owns this run (same rule as
    /// `caller_owns_live_run`, without the run lock).
    pub fn owned_by(&self, claims: &crate::auth::Claims) -> bool {
        caller_owns_browser_run(&self.org_id, &self.actor_id, claims)
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
