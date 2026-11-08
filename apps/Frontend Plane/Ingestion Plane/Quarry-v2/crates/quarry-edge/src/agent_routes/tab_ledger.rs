//! WS8.F: the tab ledger and the attach registry of one live run.
//!
//! Who holds each browser tab, and which agent attachment (if any) the run has.
//! This is pure state with no I/O: the handlers in `attach.rs`, `tabs.rs` and
//! `step.rs` decide what to do with the answers. Every holder change is a
//! compare-and-set on the tab's epoch, and the epoch moves only on a real change of
//! holder, so a repeated request is idempotent and a stale one is refused.
//!
//! The rules (plan WS8.F, D8-7 implicit take-over):
//! - an agent never changes a tab's holder; it may act only on a tab it holds;
//! - a person's non-passive action on an agent-held tab takes the tab over, in the
//!   same step (the epoch moves once); a passive action (a read) does not;
//! - giving a tab back to the agent needs a live attachment;
//! - at most one live attachment per run.

use std::collections::BTreeMap;

use quarry_core::contracts::AgentAction;
use quarry_runtime::step_receipts::{BrowserAttachState, BrowserTabHolder};

use super::{LaneRef, LaneState, RunLive};

/// Who opened a tab. Recorded once, at open; never changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenedBy {
    Human,
    Agent,
}

/// Who acts on a request. The verified caller decides it (see `derive_initiator`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Human,
    Agent,
}

/// What the ledger keeps for one tab: ids and classes only, never a URL or a title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabMeta {
    pub opened_by: OpenedBy,
    pub holder: BrowserTabHolder,
    /// The attachment that opened the tab, when an agent opened it.
    pub attach_id: Option<String>,
    /// Moves by one on each real change of holder; starts at 0.
    pub epoch: u64,
}

/// What a holder change did. `Unchanged` carries the current epoch so a caller can
/// answer an idempotent request with the same epoch it would have written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabChange {
    Unchanged { epoch: u64 },
    Changed { epoch: u64 },
}

/// Why a tab transition was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabError {
    /// No such tab on this run (closed, or never opened here). A pinned delegate gets
    /// `pinned_tab_closed` for this, never a fallback to another tab.
    NotFound,
    /// The caller's expected epoch is not the tab's current epoch.
    Stale { current: u64 },
    /// An agent asked to act on a tab a person holds (`USER_TOOK_OVER`).
    HeldByHuman { tab_epoch: u64 },
    /// A hand-over to the agent with no live attachment (`no_attached_agent`).
    NoAttachedAgent,
}

/// The outcome of gating an admitted action on a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAdmit {
    Allowed,
    /// The person's action took the tab over in this step; the epoch is the new one.
    TakenOver { tab_epoch: u64 },
}

/// The tabs of one live run, by tab id. A `BTreeMap` keeps the listing stable.
#[derive(Debug, Default, Clone)]
pub struct TabLedger {
    tabs: BTreeMap<String, TabMeta>,
    /// W3 E3.2-e2: the per-origin consent of tabs offered to attach mode, by tab id. Kept apart
    /// from `TabMeta` so the holder rules and their callers do not change.
    consents: BTreeMap<String, TabConsent>,
    /// Attachment-scoped handoffs, retained only until the attachment ends.
    retained: BTreeMap<String, String>,
}

impl TabLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a tab the browser already had (the first tab, seeded lazily) or one a
    /// person opened. A tab already known is left as it is.
    pub fn seed(&mut self, tab_id: &str, opened_by: OpenedBy) {
        let holder = match opened_by {
            OpenedBy::Human => BrowserTabHolder::Human,
            OpenedBy::Agent => BrowserTabHolder::Agent,
        };
        self.tabs.entry(tab_id.to_owned()).or_insert(TabMeta {
            opened_by,
            holder,
            attach_id: None,
            epoch: 0,
        });
    }

    /// Record a tab an agent attachment opened: held by the agent from the start.
    pub fn open_agent_tab(&mut self, tab_id: &str, attach_id: &str) {
        self.tabs.insert(
            tab_id.to_owned(),
            TabMeta {
                opened_by: OpenedBy::Agent,
                holder: BrowserTabHolder::Agent,
                attach_id: Some(attach_id.to_owned()),
                epoch: 0,
            },
        );
    }

    pub fn get(&self, tab_id: &str) -> Option<&TabMeta> {
        self.tabs.get(tab_id)
    }

    /// Drop a tab that was closed. Returns what the ledger held for it.
    pub fn forget(&mut self, tab_id: &str) -> Option<TabMeta> {
        self.consents.remove(tab_id);
        self.retained.remove(tab_id);
        self.tabs.remove(tab_id)
    }

    /// Drop every tab the browser no longer has. `live` is the driver's list.
    pub fn retain_live(&mut self, live: &[&str]) {
        self.tabs.retain(|id, _| live.contains(&id.as_str()));
        self.consents.retain(|id, _| live.contains(&id.as_str()));
        self.retained.retain(|id, _| live.contains(&id.as_str()));
    }

    /// Mark only the tab belonging to this attachment for the owner to continue using.
    pub fn mark_for_handoff(&mut self, tab_id: &str, attach_id: &str) -> Result<(), TabError> {
        let tab = self.tabs.get(tab_id).ok_or(TabError::NotFound)?;
        if tab.attach_id.as_deref() != Some(attach_id) { return Err(TabError::NotFound); }
        // Prevent another delegate step after the marking step has finished.
        self.set_holder(tab_id, BrowserTabHolder::Human, None)?;
        self.retained.insert(tab_id.to_owned(), attach_id.to_owned());
        Ok(())
    }

    pub fn is_marked_for_handoff(&self, tab_id: &str, attach_id: &str) -> bool {
        self.retained.get(tab_id).is_some_and(|id| id == attach_id)
            && self.tabs.get(tab_id).is_some_and(|tab| tab.attach_id.as_deref() == Some(attach_id))
    }

    pub fn clear_handoff(&mut self, tab_id: &str) {
        self.retained.remove(tab_id);
    }

    /// Consume the one-use mark, transferring control only to the human owner.
    pub fn handoff_marked_tab(&mut self, tab_id: &str, attach_id: &str) -> bool {
        let marked = self.retained.remove(tab_id).as_deref() == Some(attach_id);
        if !marked || !self.tabs.get(tab_id).is_some_and(|tab| tab.attach_id.as_deref() == Some(attach_id)) {
            return false;
        }
        self.set_holder(tab_id, BrowserTabHolder::Human, None).is_ok()
    }

    /// Return a borrowed tab intact and clear its one-use attachment and consent.
    pub fn return_borrowed_tab(&mut self, tab_id: &str, attach_id: &str) {
        let Some(tab) = self.tabs.get_mut(tab_id) else { return; };
        if tab.attach_id.as_deref() != Some(attach_id) {
            return;
        }
        tab.attach_id = None;
        if tab.holder != BrowserTabHolder::Human {
            tab.holder = BrowserTabHolder::Human;
            tab.epoch += 1;
        }
        self.consents.remove(tab_id);
        self.retained.remove(tab_id);
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &TabMeta)> {
        self.tabs.iter()
    }

    /// A person's explicit holder change (`POST .../tabs/{id}/control`). `expected` is
    /// the epoch the caller read; a mismatch is `Stale`. Setting the holder it already
    /// has is `Unchanged` and moves nothing.
    pub fn set_holder(
        &mut self,
        tab_id: &str,
        holder: BrowserTabHolder,
        expected: Option<u64>,
    ) -> Result<TabChange, TabError> {
        let meta = self.tabs.get_mut(tab_id).ok_or(TabError::NotFound)?;
        if let Some(expected) = expected {
            if expected != meta.epoch {
                return Err(TabError::Stale { current: meta.epoch });
            }
        }
        if meta.holder == holder {
            return Ok(TabChange::Unchanged { epoch: meta.epoch });
        }
        meta.holder = holder;
        meta.epoch += 1;
        Ok(TabChange::Changed { epoch: meta.epoch })
    }

    /// Gate an admitted action on the tab it addresses.
    ///
    /// - An agent may act only on a tab the agent holds; it never changes the holder.
    /// - A person's passive action (a read) on an agent tab is allowed and changes
    ///   nothing.
    /// - A person's non-passive action on an agent tab takes the tab over, and the
    ///   epoch moves once, in this call.
    pub fn gate_action(
        &mut self,
        tab_id: &str,
        actor: Actor,
        passive: bool,
    ) -> Result<TabAdmit, TabError> {
        let meta = self.tabs.get_mut(tab_id).ok_or(TabError::NotFound)?;
        match (actor, meta.holder) {
            (Actor::Agent, BrowserTabHolder::Agent) => Ok(TabAdmit::Allowed),
            (Actor::Agent, BrowserTabHolder::Human) => Err(TabError::HeldByHuman {
                tab_epoch: meta.epoch,
            }),
            (Actor::Human, BrowserTabHolder::Human) => Ok(TabAdmit::Allowed),
            (Actor::Human, BrowserTabHolder::Agent) if passive => Ok(TabAdmit::Allowed),
            (Actor::Human, BrowserTabHolder::Agent) => {
                meta.holder = BrowserTabHolder::Human;
                meta.epoch += 1;
                Ok(TabAdmit::TakenOver {
                    tab_epoch: meta.epoch,
                })
            }
        }
    }
}

/// Whether an action only reads the page. A person's read on an agent tab does not
/// take the tab over (D8-7). Scroll and every input are not reads.
pub fn is_passive(action: &AgentAction) -> bool {
    matches!(
        action,
        AgentAction::Screenshot { .. }
            | AgentAction::Pdf
            | AgentAction::GetContent
            | AgentAction::Wait { .. }
            | AgentAction::WaitFor { .. }
            | AgentAction::WaitForRef { .. }
            | AgentAction::FrameWaitForRef { .. }
            | AgentAction::WaitForSemantic { .. }
    )
}

/// Hand a tab to the agent (`POST .../tabs/{id}/control {holder: "agent"}`). Needs a
/// live attachment that owns this tab; otherwise `NoAttachedAgent`.
pub fn hand_to_agent(
    ledger: &mut TabLedger,
    attaches: &AttachRegistry,
    tab_id: &str,
    expected: Option<u64>,
) -> Result<TabChange, TabError> {
    match attaches.live() {
        Some(live) if live.tab_id == tab_id => {
            ledger.set_holder(tab_id, BrowserTabHolder::Agent, expected)
        }
        _ => Err(TabError::NoAttachedAgent),
    }
}

// ---------------------------------------------------------------------------
// W3 E3.2-e2: per-origin consent for attach-mode tabs.
//
// Attach mode (WS8.G, the Model Plane attach driver) gives the agent the person's own visible
// tab instead of a background tab the attachment opened. That tab is on an origin the person
// chose, so before it is attached its origin must carry the person's consent: an
// origin-extension grant that passes the E3.2-e1 gate and covers the tab's host (the route half
// is `tabs::admit_attach_mode_tab`). The ledger keeps the result per tab id:
// - a denial is sticky for the tab id: a later approval does not lift it (deny beats approval);
// - an approval binds the host it was checked for, so a tab that navigated since needs a new check;
// - a tab without an approval for its current host is never attached.
// Consent records are dropped with the tab (`forget`, `retain_live`). Ids and closed labels
// only: the approved host stays in process memory and is never logged or returned.
// Dark: nothing outside the tests calls these until the attach driver's route does.
// ---------------------------------------------------------------------------

/// Why a tab's consent was denied, as a closed label (never a host).
pub type TabConsentReason = &'static str;

/// The consent state of one tab offered to attach mode.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TabConsent {
    /// The person's consent covers `host` through the origin-extension grant `grant_id`.
    Approved { host: String, grant_id: String },
    /// Refused; the tab id is never attached while the ledger knows it.
    Denied { reason: TabConsentReason },
}

/// Why an attach-mode tab could not be approved or attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TabConsentError {
    /// No such tab on this run.
    NotFound,
    /// The tab's consent was denied (sticky for the tab id).
    Denied { reason: TabConsentReason },
    /// No approval for the tab's current host: none yet, or the tab navigated since.
    ConsentRequired,
    /// Only a tab the person holds can be offered to attach mode.
    NotHumanHeld,
    /// The tab already belongs to another attachment, or was opened by one.
    AlreadyAttached,
    /// The run has no live attachment pinned to this tab.
    NoAttachedAgent,
    /// The caller's expected epoch is not the tab's current epoch.
    Stale { current: u64 },
}

#[allow(dead_code)]
impl TabConsentError {
    /// The closed label of the refusal (error details, counters).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "pinned_tab_closed",
            Self::Denied { .. } => "attach_tab_consent_denied",
            Self::ConsentRequired => "attach_tab_consent_required",
            Self::NotHumanHeld => "attach_tab_not_human_held",
            Self::AlreadyAttached => "attach_tab_already_attached",
            Self::NoAttachedAgent => "no_attached_agent",
            Self::Stale { .. } => "stale_tab_epoch",
        }
    }
}

#[allow(dead_code)]
impl TabLedger {
    /// The consent recorded for a tab, if any.
    pub fn consent(&self, tab_id: &str) -> Option<&TabConsent> {
        self.consents.get(tab_id)
    }

    /// Record a denial for a tab. Sticky: it replaces an approval and is never replaced by one;
    /// the first denial's reason is kept.
    pub fn deny_consent(
        &mut self,
        tab_id: &str,
        reason: TabConsentReason,
    ) -> Result<(), TabConsentError> {
        if !self.tabs.contains_key(tab_id) {
            return Err(TabConsentError::NotFound);
        }
        if !matches!(self.consents.get(tab_id), Some(TabConsent::Denied { .. })) {
            self.consents
                .insert(tab_id.to_owned(), TabConsent::Denied { reason });
        }
        Ok(())
    }

    /// Record an approval of `host` for a tab. Refused when the tab was denied (deny beats
    /// approval). A new approval replaces an older one (the tab navigated and was re-checked).
    pub fn approve_consent(
        &mut self,
        tab_id: &str,
        host: &str,
        grant_id: &str,
    ) -> Result<(), TabConsentError> {
        if !self.tabs.contains_key(tab_id) {
            return Err(TabConsentError::NotFound);
        }
        if let Some(TabConsent::Denied { reason }) = self.consents.get(tab_id) {
            return Err(TabConsentError::Denied { reason: *reason });
        }
        self.consents.insert(
            tab_id.to_owned(),
            TabConsent::Approved {
                host: host.to_owned(),
                grant_id: grant_id.to_owned(),
            },
        );
        Ok(())
    }

    /// Whether the tab's recorded consent approves `current_host`. Returns the approving grant.
    pub fn consent_covers(&self, tab_id: &str, current_host: &str) -> Result<&str, TabConsentError> {
        if !self.tabs.contains_key(tab_id) {
            return Err(TabConsentError::NotFound);
        }
        match self.consents.get(tab_id) {
            Some(TabConsent::Denied { reason }) => Err(TabConsentError::Denied { reason: *reason }),
            Some(TabConsent::Approved { host, grant_id }) if host == current_host => {
                Ok(grant_id.as_str())
            }
            _ => Err(TabConsentError::ConsentRequired),
        }
    }

    /// Attach the person's tab to the attachment `attach_id` (attach mode). Consent is checked
    /// first, against the host the tab is on now: a denied tab, or one with no approval for
    /// `current_host`, is never attached. Only a tab the person holds and no attachment owns can
    /// be attached; the holder becomes the agent and the epoch moves once. Repeating the same
    /// attach is `Unchanged`.
    pub fn attach_visible_tab(
        &mut self,
        tab_id: &str,
        attach_id: &str,
        current_host: &str,
        expected: Option<u64>,
    ) -> Result<TabChange, TabConsentError> {
        let (epoch, holder, owner) = {
            let meta = self.tabs.get(tab_id).ok_or(TabConsentError::NotFound)?;
            (meta.epoch, meta.holder, meta.attach_id.clone())
        };
        if let Some(expected) = expected {
            if expected != epoch {
                return Err(TabConsentError::Stale { current: epoch });
            }
        }
        self.consent_covers(tab_id, current_host)?;
        match owner.as_deref() {
            Some(existing) if existing == attach_id && holder == BrowserTabHolder::Agent => {
                return Ok(TabChange::Unchanged { epoch });
            }
            Some(_) => return Err(TabConsentError::AlreadyAttached),
            None => {}
        }
        if holder != BrowserTabHolder::Human {
            return Err(TabConsentError::NotHumanHeld);
        }
        let meta = self.tabs.get_mut(tab_id).ok_or(TabConsentError::NotFound)?;
        meta.attach_id = Some(attach_id.to_owned());
        meta.holder = BrowserTabHolder::Agent;
        meta.epoch += 1;
        Ok(TabChange::Changed { epoch: meta.epoch })
    }
}

/// Attach-mode hand-over of the person's tab to the live attachment pinned to it. Needs a live
/// attachment whose pin is this tab (`NoAttachedAgent` otherwise), then the consent and holder
/// rules of [`TabLedger::attach_visible_tab`].
#[allow(dead_code)]
pub fn attach_consented_tab(
    ledger: &mut TabLedger,
    attaches: &AttachRegistry,
    tab_id: &str,
    current_host: &str,
    expected: Option<u64>,
) -> Result<TabChange, TabConsentError> {
    match attaches.live() {
        Some(live) if live.tab_id == tab_id => {
            let attach_id = live.attach_id.clone();
            ledger.attach_visible_tab(tab_id, &attach_id, current_host, expected)
        }
        _ => Err(TabConsentError::NoAttachedAgent),
    }
}

/// The budget of one attachment. Only a delegate's steps are charged to it; a person's
/// steps on the same run are never counted here (R-8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachBudget {
    pub max_steps: u32,
    pub max_runtime_s: u64,
    pub steps_used: u32,
}

/// Why an attachment's budget refused a delegate step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachRefusal {
    /// `RATE_LIMITED` with reason `attach_steps`.
    Steps,
    /// `RATE_LIMITED` with reason `attach_runtime`.
    Runtime,
}

impl AttachBudget {
    pub fn new(max_steps: u32, max_runtime_s: u64) -> Self {
        Self {
            max_steps,
            max_runtime_s,
            steps_used: 0,
        }
    }

    /// Refuse before a delegate step runs when the attachment is out of steps or of
    /// runtime. `elapsed_s` is the attachment's age on the run clock. A budget of 0
    /// steps refuses the first step.
    pub fn admit(&self, elapsed_s: u64) -> Result<(), AttachRefusal> {
        if self.steps_used >= self.max_steps {
            return Err(AttachRefusal::Steps);
        }
        if elapsed_s >= self.max_runtime_s {
            return Err(AttachRefusal::Runtime);
        }
        Ok(())
    }

    /// Count one delegate step that was admitted.
    pub fn charge(&mut self) {
        self.steps_used = self.steps_used.saturating_add(1);
    }
}

/// One agent attachment to a live run. Process-local: a restart loses it (documented).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorrowedTab {
    pub origin: url::Origin,
    pub prior_egress: Option<quarry_browser::BrowserEgressPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachState {
    pub attach_id: String,
    /// The attach grant (a child of the run's grant, kind `attach`).
    pub grant_id: String,
    /// The service principal allowed to act as this attachment's delegate.
    pub delegate_actor: String,
    /// The one tab the delegate may address (the pin; there is no override).
    pub tab_id: String,
    /// The owner's original tab, which must be returned on every exit path.
    pub borrowed: Option<BorrowedTab>,
    /// The hosts the attach grant allows (1 to 8, canonical).
    pub allowed_domains: Vec<String>,
    /// An independently enforced, tighten-only restriction for this delegate.
    pub policy_ask: Option<quarry_core::web_policy::RequestAsk>,
    pub budget: AttachBudget,
    /// WS8.H: the delegate's own lane. Its steps never wait for the person's lane, and the
    /// person's steps never wait for it.
    pub lane: LaneRef,
    /// `Attached` while live; any other value is an ended attachment.
    pub phase: BrowserAttachState,
    pub started_ms: u64,
    pub last_activity_ms: u64,
}

impl AttachState {
    pub fn is_live(&self) -> bool {
        self.phase == BrowserAttachState::Attached
    }

    /// Whether the attachment has been idle for at least `idle_ttl_s` at `now_ms`.
    pub fn idle_expired(&self, now_ms: u64, idle_ttl_s: u64) -> bool {
        now_ms.saturating_sub(self.last_activity_ms) / 1000 >= idle_ttl_s
    }
}

/// Why an attachment could not begin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachError {
    /// The run already has a live attachment (`attach_exists`, 409).
    Exists,
}

/// The attachment of one live run: at most one live at a time.
#[derive(Debug, Default, Clone)]
pub struct AttachRegistry {
    live: Option<AttachState>,
}

impl AttachRegistry {
    pub fn begin(&mut self, state: AttachState) -> Result<(), AttachError> {
        if self.live.is_some() {
            return Err(AttachError::Exists);
        }
        self.live = Some(state);
        Ok(())
    }

    pub fn live(&self) -> Option<&AttachState> {
        self.live.as_ref().filter(|state| state.is_live())
    }

    pub fn live_mut(&mut self) -> Option<&mut AttachState> {
        self.live.as_mut().filter(|state| state.is_live())
    }

    /// End the live attachment if it is `attach_id`, recording how it ended. Returns the
    /// ended attachment; `None` when the id is not the live one (already ended, or a
    /// stale id). An ended attachment is not live any more.
    pub fn end(&mut self, attach_id: &str, phase: BrowserAttachState) -> Option<AttachState> {
        let matches = self
            .live
            .as_ref()
            .is_some_and(|state| state.attach_id == attach_id && state.is_live());
        if !matches {
            return None;
        }
        let mut ended = self.live.take()?;
        ended.phase = phase;
        Some(ended)
    }
}

/// Whether a verified caller may act as this attachment's delegate (the lane check
/// of `resolve_caller`). Every condition must hold; anything else is the existing 404.
pub fn delegate_matches(
    attach: &AttachState,
    run_org_id: &str,
    caller_org_id: &str,
    caller_actor_id: &str,
    caller_is_service: bool,
    body_grant_id: Option<&str>,
) -> bool {
    caller_is_service
        && attach.is_live()
        && run_org_id == caller_org_id
        && !caller_actor_id.trim().is_empty()
        && caller_actor_id == attach.delegate_actor
        && body_grant_id == Some(attach.grant_id.as_str())
}

impl RunLive {
    /// The run's tab ledger. Locked only for the duration of one read or change.
    pub fn tabs(&self) -> std::sync::MutexGuard<'_, TabLedger> {
        self.tabs.lock().expect("tab ledger mutex poisoned")
    }

    /// The run's attachment registry.
    pub fn attaches(&self) -> std::sync::MutexGuard<'_, AttachRegistry> {
        self.attaches.lock().expect("attach registry mutex poisoned")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger_with(tab: &str, opened_by: OpenedBy) -> TabLedger {
        let mut ledger = TabLedger::new();
        ledger.seed(tab, opened_by);
        ledger
    }

    fn attach(id: &str, tab: &str, budget: AttachBudget) -> AttachState {
        AttachState {
            attach_id: id.to_owned(),
            grant_id: "grant_attach".to_owned(),
            delegate_actor: "svc_execution_core".to_owned(),
            tab_id: tab.to_owned(),
            borrowed: None,
            allowed_domains: vec!["example.com".to_owned()],
            policy_ask: None,
            budget,
            lane: LaneRef::new(LaneState::default()),
            phase: BrowserAttachState::Attached,
            started_ms: 0,
            last_activity_ms: 0,
        }
    }

    #[test]
    fn tab_epoch_increments_only_on_change() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        assert_eq!(
            ledger.set_holder("t1", BrowserTabHolder::Human, None),
            Ok(TabChange::Unchanged { epoch: 0 }),
            "setting the holder it already has moves nothing"
        );
        assert_eq!(ledger.get("t1").unwrap().epoch, 0);
        assert_eq!(
            ledger.set_holder("t1", BrowserTabHolder::Agent, Some(0)),
            Ok(TabChange::Changed { epoch: 1 })
        );
        assert_eq!(
            ledger.set_holder("t1", BrowserTabHolder::Agent, None),
            Ok(TabChange::Unchanged { epoch: 1 }),
            "a repeat is idempotent"
        );
        assert_eq!(ledger.get("t1").unwrap().epoch, 1);
    }

    #[test]
    fn stale_expected_epoch_is_conflict() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        ledger
            .set_holder("t1", BrowserTabHolder::Agent, None)
            .unwrap();
        assert_eq!(
            ledger.set_holder("t1", BrowserTabHolder::Human, Some(0)),
            Err(TabError::Stale { current: 1 })
        );
        assert_eq!(ledger.get("t1").unwrap().holder, BrowserTabHolder::Agent);
    }

    #[test]
    fn agent_cannot_change_holder() {
        // The agent's only path is gate_action, which never sets the holder for an agent.
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        assert_eq!(
            ledger.gate_action("t1", Actor::Agent, false),
            Err(TabError::HeldByHuman { tab_epoch: 0 })
        );
        assert_eq!(ledger.get("t1").unwrap().holder, BrowserTabHolder::Human);
        assert_eq!(ledger.get("t1").unwrap().epoch, 0);
    }

    #[test]
    fn agent_step_refused_when_tab_holder_is_human() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        assert!(matches!(
            ledger.gate_action("t1", Actor::Agent, true),
            Err(TabError::HeldByHuman { .. })
        ));
    }

    #[test]
    fn human_non_passive_action_on_agent_tab_takes_over_atomically() {
        let mut ledger = TabLedger::new();
        ledger.open_agent_tab("t2", "att_1");
        assert_eq!(
            ledger.gate_action("t2", Actor::Human, false),
            Ok(TabAdmit::TakenOver { tab_epoch: 1 })
        );
        let meta = ledger.get("t2").unwrap();
        assert_eq!(meta.holder, BrowserTabHolder::Human);
        assert_eq!(meta.epoch, 1, "the epoch moves once, in the same call");
        assert_eq!(
            ledger.gate_action("t2", Actor::Agent, false),
            Err(TabError::HeldByHuman { tab_epoch: 1 }),
            "the agent's next step on the tab is refused"
        );
    }

    #[test]
    fn human_passive_action_on_agent_tab_does_not_take_over() {
        let mut ledger = TabLedger::new();
        ledger.open_agent_tab("t2", "att_1");
        assert_eq!(
            ledger.gate_action("t2", Actor::Human, true),
            Ok(TabAdmit::Allowed)
        );
        let meta = ledger.get("t2").unwrap();
        assert_eq!(meta.holder, BrowserTabHolder::Agent);
        assert_eq!(meta.epoch, 0);
    }

    #[test]
    fn give_back_requires_live_attach() {
        let mut ledger = TabLedger::new();
        ledger.open_agent_tab("t2", "att_1");
        ledger
            .gate_action("t2", Actor::Human, false)
            .expect("takeover");
        let no_attach = AttachRegistry::default();
        assert_eq!(
            hand_to_agent(&mut ledger, &no_attach, "t2", None),
            Err(TabError::NoAttachedAgent)
        );
        let mut attaches = AttachRegistry::default();
        attaches
            .begin(attach("att_1", "t2", AttachBudget::new(20, 120)))
            .unwrap();
        assert_eq!(
            hand_to_agent(&mut ledger, &attaches, "t2", Some(1)),
            Ok(TabChange::Changed { epoch: 2 })
        );
        assert_eq!(ledger.get("t2").unwrap().holder, BrowserTabHolder::Agent);
    }

    #[test]
    fn a_missing_tab_is_not_found_and_never_falls_back() {
        let mut ledger = TabLedger::new();
        assert_eq!(
            ledger.gate_action("gone", Actor::Agent, false),
            Err(TabError::NotFound)
        );
        assert_eq!(
            ledger.set_holder("gone", BrowserTabHolder::Human, None),
            Err(TabError::NotFound)
        );
    }

    #[test]
    fn attach_second_is_conflict() {
        let mut attaches = AttachRegistry::default();
        attaches
            .begin(attach("att_1", "t2", AttachBudget::new(20, 120)))
            .unwrap();
        assert_eq!(
            attaches.begin(attach("att_2", "t3", AttachBudget::new(20, 120))),
            Err(AttachError::Exists)
        );
        assert_eq!(attaches.live().unwrap().attach_id, "att_1");
    }

    #[test]
    fn ended_attach_is_no_longer_live_and_cannot_be_ended_twice() {
        let mut attaches = AttachRegistry::default();
        attaches
            .begin(attach("att_1", "t2", AttachBudget::new(20, 120)))
            .unwrap();
        let ended = attaches
            .end("att_1", BrowserAttachState::Revoked)
            .expect("the live attach ends");
        assert_eq!(ended.phase, BrowserAttachState::Revoked);
        assert!(attaches.live().is_none());
        assert!(attaches.end("att_1", BrowserAttachState::Released).is_none());
        // A new attach may begin once the old one has ended.
        attaches
            .begin(attach("att_2", "t3", AttachBudget::new(20, 120)))
            .unwrap();
    }

    #[test]
    fn attach_budget_is_charged_only_by_delegate_steps() {
        // The budget is charged by `charge`, which only the delegate path calls. A person's
        // step never reaches it, so the count stays where the delegate left it.
        let mut budget = AttachBudget::new(2, 120);
        assert_eq!(budget.admit(0), Ok(()));
        budget.charge();
        budget.charge();
        assert_eq!(budget.steps_used, 2);
        assert_eq!(budget.admit(0), Err(AttachRefusal::Steps));
    }

    #[test]
    fn attach_runtime_budget_is_enforced_at_admission() {
        let budget = AttachBudget::new(20, 120);
        assert_eq!(budget.admit(119), Ok(()));
        assert_eq!(budget.admit(120), Err(AttachRefusal::Runtime));
        assert_eq!(AttachBudget::new(0, 120).admit(0), Err(AttachRefusal::Steps));
    }

    #[test]
    fn delegate_requires_service_principal_and_matching_attach_grant() {
        let state = attach("att_1", "t2", AttachBudget::new(20, 120));
        let ok = |svc, org, actor, grant| {
            delegate_matches(&state, "org_a", org, actor, svc, grant)
        };
        assert!(ok(true, "org_a", "svc_execution_core", Some("grant_attach")));
        // A person's token is never a delegate, even with the right grant.
        assert!(!ok(false, "org_a", "svc_execution_core", Some("grant_attach")));
        // Another org, another actor, another or missing grant: refused.
        assert!(!ok(true, "org_b", "svc_execution_core", Some("grant_attach")));
        assert!(!ok(true, "org_a", "svc_other", Some("grant_attach")));
        assert!(!ok(true, "org_a", "svc_execution_core", Some("grant_other")));
        assert!(!ok(true, "org_a", "svc_execution_core", None));
        assert!(!ok(true, "org_a", "", Some("grant_attach")));
    }

    #[test]
    fn idle_attach_expires_on_the_run_clock() {
        let state = attach("att_1", "t2", AttachBudget::new(20, 120));
        assert!(!state.idle_expired(179_999, 180));
        assert!(state.idle_expired(180_000, 180));
    }

    // W3 E3.2-e2: consent for attach-mode tabs.

    #[test]
    fn a_denied_tab_is_never_attached() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        ledger
            .deny_consent("t1", "origin_extension_denied")
            .unwrap();
        assert_eq!(
            ledger.attach_visible_tab("t1", "att_1", "shop.example.com", None),
            Err(TabConsentError::Denied {
                reason: "origin_extension_denied"
            })
        );
        let meta = ledger.get("t1").unwrap();
        assert_eq!(meta.holder, BrowserTabHolder::Human);
        assert_eq!(meta.attach_id, None);
        assert_eq!(meta.epoch, 0);
    }

    #[test]
    fn a_tab_without_consent_is_never_attached() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        assert_eq!(
            ledger.attach_visible_tab("t1", "att_1", "shop.example.com", None),
            Err(TabConsentError::ConsentRequired)
        );
        assert_eq!(ledger.get("t1").unwrap().holder, BrowserTabHolder::Human);
    }

    #[test]
    fn deny_beats_a_later_approval_and_keeps_the_first_reason() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        ledger
            .approve_consent("t1", "shop.example.com", "grant_ext_1")
            .unwrap();
        ledger.deny_consent("t1", "org_origin_consent_disabled").unwrap();
        ledger.deny_consent("t1", "origin_extension_denied").unwrap();
        assert_eq!(
            ledger.approve_consent("t1", "shop.example.com", "grant_ext_2"),
            Err(TabConsentError::Denied {
                reason: "org_origin_consent_disabled"
            })
        );
        assert_eq!(
            ledger.consent("t1"),
            Some(&TabConsent::Denied {
                reason: "org_origin_consent_disabled"
            })
        );
    }

    #[test]
    fn an_approval_binds_the_host_it_was_checked_for() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        ledger
            .approve_consent("t1", "shop.example.com", "grant_ext_1")
            .unwrap();
        assert_eq!(
            ledger.consent_covers("t1", "shop.example.com"),
            Ok("grant_ext_1")
        );
        // The tab navigated to another host since the check: no attach without a new check.
        assert_eq!(
            ledger.attach_visible_tab("t1", "att_1", "other.example.net", None),
            Err(TabConsentError::ConsentRequired)
        );
        assert_eq!(ledger.get("t1").unwrap().holder, BrowserTabHolder::Human);
    }

    #[test]
    fn a_consented_tab_is_attached_once_and_a_repeat_is_idempotent() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        ledger
            .approve_consent("t1", "shop.example.com", "grant_ext_1")
            .unwrap();
        assert_eq!(
            ledger.attach_visible_tab("t1", "att_1", "shop.example.com", Some(0)),
            Ok(TabChange::Changed { epoch: 1 })
        );
        let meta = ledger.get("t1").unwrap();
        assert_eq!(meta.holder, BrowserTabHolder::Agent);
        assert_eq!(meta.attach_id.as_deref(), Some("att_1"));
        assert_eq!(meta.opened_by, OpenedBy::Human, "the opener never changes");
        assert_eq!(
            ledger.attach_visible_tab("t1", "att_1", "shop.example.com", None),
            Ok(TabChange::Unchanged { epoch: 1 })
        );
        assert_eq!(
            ledger.attach_visible_tab("t1", "att_2", "shop.example.com", None),
            Err(TabConsentError::AlreadyAttached)
        );
    }

    #[test]
    fn a_returned_tab_keeps_its_identity_but_needs_fresh_consent() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        ledger.approve_consent("t1", "shop.example.com", "grant-1").unwrap();
        ledger.attach_visible_tab("t1", "att_1", "shop.example.com", Some(0)).unwrap();

        ledger.return_borrowed_tab("t1", "other-attachment");
        assert_eq!(ledger.get("t1").unwrap().holder, BrowserTabHolder::Agent);
        ledger.return_borrowed_tab("t1", "att_1");
        let tab = ledger.get("t1").unwrap();
        assert_eq!(tab.opened_by, OpenedBy::Human);
        assert_eq!(tab.holder, BrowserTabHolder::Human);
        assert_eq!(tab.epoch, 2);
        assert!(tab.attach_id.is_none());
        assert_eq!(
            ledger.attach_visible_tab("t1", "att_2", "shop.example.com", Some(2)),
            Err(TabConsentError::ConsentRequired)
        );
    }

    #[test]
    fn a_stale_epoch_or_a_tab_the_person_does_not_hold_is_refused() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        ledger
            .approve_consent("t1", "shop.example.com", "grant_ext_1")
            .unwrap();
        ledger
            .set_holder("t1", BrowserTabHolder::Agent, None)
            .unwrap();
        assert_eq!(
            ledger.attach_visible_tab("t1", "att_1", "shop.example.com", Some(0)),
            Err(TabConsentError::Stale { current: 1 })
        );
        assert_eq!(
            ledger.attach_visible_tab("t1", "att_1", "shop.example.com", None),
            Err(TabConsentError::NotHumanHeld)
        );
        // A tab an attachment opened is not the person's to offer.
        ledger.open_agent_tab("t2", "att_0");
        ledger
            .approve_consent("t2", "shop.example.com", "grant_ext_1")
            .unwrap();
        assert_eq!(
            ledger.attach_visible_tab("t2", "att_1", "shop.example.com", None),
            Err(TabConsentError::AlreadyAttached)
        );
    }

    #[test]
    fn attach_mode_needs_a_live_attachment_pinned_to_the_tab() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        ledger
            .approve_consent("t1", "shop.example.com", "grant_ext_1")
            .unwrap();
        let none = AttachRegistry::default();
        assert_eq!(
            attach_consented_tab(&mut ledger, &none, "t1", "shop.example.com", None),
            Err(TabConsentError::NoAttachedAgent)
        );
        let mut other_pin = AttachRegistry::default();
        other_pin
            .begin(attach("att_1", "t9", AttachBudget::new(20, 120)))
            .unwrap();
        assert_eq!(
            attach_consented_tab(&mut ledger, &other_pin, "t1", "shop.example.com", None),
            Err(TabConsentError::NoAttachedAgent)
        );
        let mut pinned = AttachRegistry::default();
        pinned
            .begin(attach("att_1", "t1", AttachBudget::new(20, 120)))
            .unwrap();
        assert_eq!(
            attach_consented_tab(&mut ledger, &pinned, "t1", "shop.example.com", None),
            Ok(TabChange::Changed { epoch: 1 })
        );
        assert_eq!(ledger.get("t1").unwrap().attach_id.as_deref(), Some("att_1"));
    }

    #[test]
    fn consent_leaves_with_the_tab() {
        let mut ledger = ledger_with("t1", OpenedBy::Human);
        ledger.seed("t2", OpenedBy::Human);
        ledger.deny_consent("t1", "origin_extension_denied").unwrap();
        ledger
            .approve_consent("t2", "shop.example.com", "grant_ext_1")
            .unwrap();
        ledger.forget("t1");
        assert_eq!(ledger.consent("t1"), None);
        ledger.retain_live(&[]);
        assert_eq!(ledger.consent("t2"), None);
        assert_eq!(
            ledger.deny_consent("t1", "origin_extension_denied"),
            Err(TabConsentError::NotFound),
            "a closed tab cannot carry a record"
        );
        assert_eq!(
            ledger.approve_consent("gone", "shop.example.com", "grant_ext_1"),
            Err(TabConsentError::NotFound)
        );
    }

    #[test]
    fn passive_actions_are_reads_only() {
        assert!(is_passive(&AgentAction::GetContent));
        assert!(is_passive(&AgentAction::Pdf));
        assert!(is_passive(&AgentAction::Wait { ms: 0 }));
    }
}
