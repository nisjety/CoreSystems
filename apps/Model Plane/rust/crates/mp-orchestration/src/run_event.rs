//! Typed orchestration event payloads.
//!
//! These are the payloads carried by `Event::payload` for orchestration-shell
//! event types (plan lifecycle, todo lifecycle, approval decisions, etc).
//!
//! Keeping them in one tagged enum lets SSE consumers + orchestrator recovery
//! pattern-match deterministically instead of de-duplicating JSON field checks
//! across services.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::approval::{ApprovalKind, ApprovalState};
use crate::plan::PlanState;
use crate::subagent::SubagentRole;
use crate::todo::TodoState;

/// Tag discriminating an orchestration event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationEventKind {
    /// A plan's state changed.
    PlanTransitioned,
    /// A todo's state changed.
    TodoTransitioned,
    /// An approval was requested or decided.
    ApprovalStateChanged,
    /// A subagent was attached to its parent.
    SubagentAttached,
    /// A subagent's run completed (success or failure).
    SubagentStopped,
    /// Run paused for an approval checkpoint.
    RunPausedForApproval,
    /// Run resumed after an approval checkpoint.
    RunResumedAfterApproval,
    /// The browser agent dispatched an action to the browser executor.
    BrowserActionDispatched,
    /// The browser agent received an observation back from the executor.
    BrowserObservationReceived,
    /// A user paused a running browser-agent loop (Phase 2 B5).
    BrowserRunPaused,
    /// A user resumed a paused browser-agent loop (Phase 2 B5).
    BrowserRunResumed,
    /// A browser action was classified risky and paused for approval (Phase 5).
    BrowserActionApprovalRequired,
    /// A pending browser-action approval was decided (Phase 5).
    BrowserActionDecided,
    /// A resumed continuation's independent verification was recorded
    /// (Verified Outcome Foundation, verevon-roadmap.md §3b).
    ApprovalContinuationVerified,
}

/// Independent judgment on whether a claimed outcome actually happened
/// (Verified Outcome Foundation). Mirrors `model_plane.v1.VerificationStatus`
/// minus `Unspecified` — an event never carries an unjudged verification (the
/// recording handler only fires this event when one was actually produced).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    /// No verification could be performed or its result was inconclusive.
    Unknown,
    /// Independently judged to have actually happened.
    VerifiedSuccess,
    /// Independently judged to have NOT happened, or to have failed.
    VerifiedFailure,
    /// Some but not all of the effect's expected consequences were confirmed.
    PartiallyVerified,
}

/// Typed orchestration event payload. Each variant carries the minimum fields
/// a consumer needs to react without fetching the owning record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OrchestrationEvent {
    /// A plan's state changed.
    PlanTransitioned {
        /// Plan id.
        plan_id: String,
        /// Run id this plan belongs to.
        run_id: String,
        /// Previous state.
        from: PlanState,
        /// New state.
        to: PlanState,
        /// When the transition happened.
        at: DateTime<Utc>,
    },
    /// A todo's state changed.
    TodoTransitioned {
        /// Todo id.
        todo_id: String,
        /// Thread id this todo belongs to.
        thread_id: String,
        /// Previous state.
        from: TodoState,
        /// New state.
        to: TodoState,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// An approval's state changed.
    ApprovalStateChanged {
        /// Approval id.
        approval_id: String,
        /// Run id this approval gates.
        run_id: String,
        /// Approval category.
        approval_kind: ApprovalKind,
        /// New state.
        to: ApprovalState,
        /// Who decided (empty for state == Requested).
        decided_by: String,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// A subagent was attached.
    SubagentAttached {
        /// Parent run id.
        parent_run_id: String,
        /// Child run id.
        child_run_id: String,
        /// Child's role.
        role: SubagentRole,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// A subagent's run stopped.
    SubagentStopped {
        /// Child run id that stopped.
        child_run_id: String,
        /// Terminal status ("completed", "failed", "cancelled").
        status: String,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// Run paused for an approval.
    RunPausedForApproval {
        /// Run id.
        run_id: String,
        /// Approval id that is gating.
        approval_id: String,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// Run resumed after approval.
    RunResumedAfterApproval {
        /// Run id.
        run_id: String,
        /// Approval id that unblocked.
        approval_id: String,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// The browser agent dispatched an action to the browser executor (B4).
    BrowserActionDispatched {
        /// Run this browser session belongs to.
        run_id: String,
        /// Browser-agent plan id driving this session.
        plan_id: String,
        /// Per-action id (e.g. `act_0001`).
        action_id: String,
        /// Action type slug: goto|click|type|extract|observe|scroll|wait.
        action_type: String,
        /// Target URL for navigations. Empty for non-goto actions.
        url: String,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// The browser agent received an observation back from the executor (B4).
    BrowserObservationReceived {
        /// Run this browser session belongs to.
        run_id: String,
        /// Browser-agent plan id driving this session.
        plan_id: String,
        /// Id of the action that produced this observation.
        action_id: String,
        /// Observation status slug: success|failed|timeout|blocked.
        status: String,
        /// Page URL after the action settled. Empty when unavailable.
        page_url: String,
        /// Page title after the action settled. Empty when unavailable.
        page_title: String,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// A user paused a running browser-agent loop (Phase 2 B5).
    BrowserRunPaused {
        /// Run this browser session belongs to.
        run_id: String,
        /// Browser-agent plan id driving this session.
        plan_id: String,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// A user resumed a paused browser-agent loop (Phase 2 B5).
    BrowserRunResumed {
        /// Run this browser session belongs to.
        run_id: String,
        /// Browser-agent plan id driving this session.
        plan_id: String,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// A browser action was classified risky and paused for approval (Phase 5).
    BrowserActionApprovalRequired {
        /// Run this browser session belongs to.
        run_id: String,
        /// Browser-agent plan id driving this session.
        plan_id: String,
        /// Id of the gated action. Empty for a run-level gate.
        action_id: String,
        /// Action type slug, or a synthetic slug for a run-level gate.
        action_type: String,
        /// Target URL, when known.
        url: String,
        /// CSS selector, when known.
        selector: String,
        /// Human-readable reason the action was classified risky.
        reason: String,
        /// `login|checkout|posting_form|destructive|cross_domain_navigation|persistent_cookie_use`
        risk_category: String,
        /// Durable approval id this event accompanies.
        approval_id: String,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// A pending browser-action approval was decided (Phase 5).
    BrowserActionDecided {
        /// Run this browser session belongs to.
        run_id: String,
        /// Browser-agent plan id driving this session.
        plan_id: String,
        /// Id of the gated action.
        action_id: String,
        /// Durable approval id.
        approval_id: String,
        /// `granted|denied|timed_out`
        decision: String,
        /// Principal who decided, when known.
        decided_by: String,
        /// Timestamp.
        at: DateTime<Utc>,
    },
    /// A resumed continuation's independent verification was recorded
    /// (Verified Outcome Foundation, verevon-roadmap.md §3b).
    ApprovalContinuationVerified {
        /// Run this continuation belongs to.
        run_id: String,
        /// Delivery this continuation resumed.
        delivery_id: String,
        /// Durable approval id that authorized the continuation.
        approval_id: String,
        /// The immutable execution receipt this verification judges.
        receipt_id: String,
        /// The independent judgment.
        verification_status: VerificationStatus,
        /// How the judgment was reached, e.g. `"structural"`.
        verification_method: String,
        /// Short, human-readable justification.
        verification_reason: String,
        /// Timestamp.
        at: DateTime<Utc>,
    },
}

impl OrchestrationEvent {
    /// Discriminator.
    #[must_use]
    pub fn kind(&self) -> OrchestrationEventKind {
        match self {
            Self::PlanTransitioned { .. } => OrchestrationEventKind::PlanTransitioned,
            Self::TodoTransitioned { .. } => OrchestrationEventKind::TodoTransitioned,
            Self::ApprovalStateChanged { .. } => OrchestrationEventKind::ApprovalStateChanged,
            Self::SubagentAttached { .. } => OrchestrationEventKind::SubagentAttached,
            Self::SubagentStopped { .. } => OrchestrationEventKind::SubagentStopped,
            Self::RunPausedForApproval { .. } => OrchestrationEventKind::RunPausedForApproval,
            Self::RunResumedAfterApproval { .. } => OrchestrationEventKind::RunResumedAfterApproval,
            Self::BrowserActionDispatched { .. } => OrchestrationEventKind::BrowserActionDispatched,
            Self::BrowserObservationReceived { .. } => {
                OrchestrationEventKind::BrowserObservationReceived
            }
            Self::BrowserRunPaused { .. } => OrchestrationEventKind::BrowserRunPaused,
            Self::BrowserRunResumed { .. } => OrchestrationEventKind::BrowserRunResumed,
            Self::BrowserActionApprovalRequired { .. } => {
                OrchestrationEventKind::BrowserActionApprovalRequired
            }
            Self::BrowserActionDecided { .. } => OrchestrationEventKind::BrowserActionDecided,
            Self::ApprovalContinuationVerified { .. } => {
                OrchestrationEventKind::ApprovalContinuationVerified
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid epoch")
    }

    #[test]
    fn kind_matches_variant() {
        let e = OrchestrationEvent::PlanTransitioned {
            plan_id: "plan_01".into(),
            run_id: "run_01".into(),
            from: PlanState::Draft,
            to: PlanState::Proposed,
            at: now(),
        };
        assert_eq!(e.kind(), OrchestrationEventKind::PlanTransitioned);
    }

    #[test]
    fn event_serializes_with_tagged_kind() {
        let e = OrchestrationEvent::TodoTransitioned {
            todo_id: "todo_01".into(),
            thread_id: "thread_01".into(),
            from: TodoState::Pending,
            to: TodoState::InProgress,
            at: now(),
        };
        let s = serde_json::to_string(&e).unwrap();
        assert!(
            s.contains(r#""kind":"todo_transitioned""#),
            "missing tag in {s}"
        );
    }

    #[test]
    fn event_roundtrips_json() {
        let e = OrchestrationEvent::ApprovalStateChanged {
            approval_id: "appr_01".into(),
            run_id: "run_01".into(),
            approval_kind: ApprovalKind::ToolCall,
            to: ApprovalState::Granted,
            decided_by: "bob".into(),
            at: now(),
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: OrchestrationEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(back.kind(), OrchestrationEventKind::ApprovalStateChanged);
    }

    #[test]
    fn all_kinds_exhaustive_in_match() {
        // Compile-time check: new variants force updates everywhere.
        let kinds = [
            OrchestrationEventKind::PlanTransitioned,
            OrchestrationEventKind::TodoTransitioned,
            OrchestrationEventKind::ApprovalStateChanged,
            OrchestrationEventKind::SubagentAttached,
            OrchestrationEventKind::SubagentStopped,
            OrchestrationEventKind::RunPausedForApproval,
            OrchestrationEventKind::RunResumedAfterApproval,
            OrchestrationEventKind::BrowserActionDispatched,
            OrchestrationEventKind::BrowserObservationReceived,
            OrchestrationEventKind::BrowserRunPaused,
            OrchestrationEventKind::BrowserRunResumed,
            OrchestrationEventKind::BrowserActionApprovalRequired,
            OrchestrationEventKind::BrowserActionDecided,
            OrchestrationEventKind::ApprovalContinuationVerified,
        ];
        assert_eq!(kinds.len(), 14);
    }
}
