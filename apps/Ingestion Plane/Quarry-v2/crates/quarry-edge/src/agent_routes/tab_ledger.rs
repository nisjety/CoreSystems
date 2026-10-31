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
        self.tabs.remove(tab_id)
    }

    /// Drop every tab the browser no longer has. `live` is the driver's list.
    pub fn retain_live(&mut self, live: &[&str]) {
        self.tabs.retain(|id, _| live.contains(&id.as_str()));
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
pub struct AttachState {
    pub attach_id: String,
    /// The attach grant (a child of the run's grant, kind `attach`).
    pub grant_id: String,
    /// The service principal allowed to act as this attachment's delegate.
    pub delegate_actor: String,
    /// The one tab the delegate may address (the pin; there is no override).
    pub tab_id: String,
    /// The hosts the attach grant allows (1 to 8, canonical).
    pub allowed_domains: Vec<String>,
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
            allowed_domains: vec!["example.com".to_owned()],
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

    #[test]
    fn passive_actions_are_reads_only() {
        assert!(is_passive(&AgentAction::GetContent));
        assert!(is_passive(&AgentAction::Pdf));
        assert!(is_passive(&AgentAction::Wait { ms: 0 }));
    }
}
