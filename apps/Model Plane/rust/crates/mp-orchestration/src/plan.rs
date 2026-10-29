//! Plan records. Owned by `session-core`.
//!
//! A `Plan` is a structured proposal authored by an agent in plan mode (or
//! reactive mode). Users can review, amend, approve, reject, or archive a plan
//! before execution proceeds.
//!
//! State machine:
//! ```text
//!         draft ──submit──▶ proposed ──approve──▶ approved ──execute──▶ executing
//!           │                   │                       │                    │
//!           │                   ├──reject──▶ rejected   │                    │
//!           │                   ├──amend──▶ draft       │                    │
//!           │                                           └──superseded──▶ superseded
//!           └──archive──▶ archived
//!
//!   executing ──complete──▶ completed
//!             └──fail────▶ failed
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{OrchestrationError, OrchestrationResult};

/// Lifecycle states for a plan. Serialized as `snake_case`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanState {
    /// Authored but not yet submitted for review.
    Draft,
    /// Submitted to the user; awaiting decision.
    Proposed,
    /// User approved; eligible for execution.
    Approved,
    /// User rejected.
    Rejected,
    /// Currently executing.
    Executing,
    /// Executed successfully.
    Completed,
    /// Execution failed.
    Failed,
    /// Replaced by a newer plan before execution.
    Superseded,
    /// Archived by the operator.
    Archived,
}

impl PlanState {
    /// Canonical wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Executing => "executing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Superseded => "superseded",
            Self::Archived => "archived",
        }
    }

    /// True when no further transitions are allowed.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Rejected | Self::Completed | Self::Failed | Self::Superseded | Self::Archived
        )
    }
}

/// State of one step inside a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepState {
    /// Declared but not yet run.
    Pending,
    /// Currently running.
    Running,
    /// Completed successfully.
    Done,
    /// Skipped (e.g. conditional not met).
    Skipped,
    /// Failed; run failure semantics apply.
    Failed,
}

/// One concrete step inside a plan. Order within `Plan::steps` is meaningful.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// Step ULID (prefix `step_`).
    pub id: String,
    /// Human-readable step title.
    pub title: String,
    /// Structured operation (tool name + args) or free-form description.
    pub operation: Option<String>,
    /// Current step state.
    pub state: PlanStepState,
    /// Step created-at.
    pub created_at: DateTime<Utc>,
    /// Step last-updated-at.
    pub updated_at: DateTime<Utc>,
}

/// A plan describing the agent's proposed action(s).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Plan ULID (prefix `plan_`).
    pub id: String,
    /// Run this plan belongs to.
    pub run_id: String,
    /// Thread this plan belongs to.
    pub thread_id: String,
    /// Authoring user (if any) or "agent" for agent-authored plans.
    pub author: String,
    /// Current lifecycle state.
    pub state: PlanState,
    /// Summary shown to the user.
    pub summary: String,
    /// Ordered steps in the plan.
    pub steps: Vec<PlanStep>,
    /// Plan this plan superseded (empty string if none).
    pub supersedes: String,
    /// Arbitrary metadata.
    #[serde(default)]
    pub metadata: serde_json::Value,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Last updated at.
    pub updated_at: DateTime<Utc>,
}

impl Plan {
    const KIND: &'static str = "plan";

    /// Construct a new plan in `Draft` state. Returns `MissingField` if any
    /// required id / `run_id` / `thread_id` is empty.
    ///
    /// # Errors
    /// - `MissingField("id" | "run_id" | "thread_id" | "author")`
    pub fn new(
        id: impl Into<String>,
        run_id: impl Into<String>,
        thread_id: impl Into<String>,
        author: impl Into<String>,
        summary: impl Into<String>,
        now: DateTime<Utc>,
    ) -> OrchestrationResult<Self> {
        let id = id.into();
        if id.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "id",
            });
        }
        let run_id = run_id.into();
        if run_id.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "run_id",
            });
        }
        let thread_id = thread_id.into();
        if thread_id.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "thread_id",
            });
        }
        let author = author.into();
        if author.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "author",
            });
        }
        Ok(Self {
            id,
            run_id,
            thread_id,
            author,
            state: PlanState::Draft,
            summary: summary.into(),
            steps: Vec::new(),
            supersedes: String::new(),
            metadata: serde_json::Value::Null,
            created_at: now,
            updated_at: now,
        })
    }

    /// Attempt a state transition. Returns `IllegalTransition` if not allowed.
    ///
    /// # Errors
    /// - `IllegalTransition` when `(self.state, next)` is not in the allowed set.
    pub fn transition(&mut self, next: PlanState, now: DateTime<Utc>) -> OrchestrationResult<()> {
        if !Self::is_allowed(self.state, next) {
            return Err(OrchestrationError::IllegalTransition {
                kind: Self::KIND,
                from: self.state.as_str().to_owned(),
                to: next.as_str().to_owned(),
            });
        }
        self.state = next;
        self.updated_at = now;
        Ok(())
    }

    /// Append a step. Only allowed in `Draft`.
    ///
    /// # Errors
    /// - `InvariantViolation` when the plan is not in `Draft`.
    /// - `MissingField` for empty step id / title.
    pub fn add_step(&mut self, step: PlanStep, now: DateTime<Utc>) -> OrchestrationResult<()> {
        if self.state != PlanState::Draft {
            return Err(OrchestrationError::InvariantViolation {
                kind: Self::KIND,
                detail: format!("cannot add step while in {}", self.state.as_str()),
            });
        }
        if step.id.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: "plan_step",
                field: "id",
            });
        }
        if step.title.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: "plan_step",
                field: "title",
            });
        }
        self.steps.push(step);
        self.updated_at = now;
        Ok(())
    }

    /// Whether a transition is in the allowed set.
    #[must_use]
    pub fn is_allowed(from: PlanState, to: PlanState) -> bool {
        use PlanState::{
            Approved, Archived, Completed, Draft, Executing, Failed, Proposed, Rejected, Superseded,
        };
        matches!(
            (from, to),
            (Draft, Proposed | Archived)
                | (Proposed, Approved | Rejected | Draft | Superseded)
                | (Approved, Executing | Superseded)
                | (Executing, Completed | Failed)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid epoch")
    }

    fn make_plan() -> Plan {
        Plan::new(
            "plan_01",
            "run_01",
            "thread_01",
            "alice",
            "deploy the thing",
            now(),
        )
        .expect("ctor")
    }

    #[test]
    fn plan_starts_in_draft() {
        let p = make_plan();
        assert_eq!(p.state, PlanState::Draft);
        assert!(!p.state.is_terminal());
    }

    #[test]
    fn missing_fields_rejected() {
        assert!(matches!(
            Plan::new("", "r", "t", "a", "s", now()),
            Err(OrchestrationError::MissingField { field: "id", .. })
        ));
        assert!(matches!(
            Plan::new("p", "", "t", "a", "s", now()),
            Err(OrchestrationError::MissingField {
                field: "run_id",
                ..
            })
        ));
        assert!(matches!(
            Plan::new("p", "r", "", "a", "s", now()),
            Err(OrchestrationError::MissingField {
                field: "thread_id",
                ..
            })
        ));
        assert!(matches!(
            Plan::new("p", "r", "t", "", "s", now()),
            Err(OrchestrationError::MissingField {
                field: "author",
                ..
            })
        ));
    }

    #[test]
    fn draft_to_proposed_to_approved_to_executing_to_completed() {
        let mut p = make_plan();
        p.transition(PlanState::Proposed, now()).unwrap();
        p.transition(PlanState::Approved, now()).unwrap();
        p.transition(PlanState::Executing, now()).unwrap();
        p.transition(PlanState::Completed, now()).unwrap();
        assert!(p.state.is_terminal());
    }

    #[test]
    fn reject_from_proposed_is_terminal() {
        let mut p = make_plan();
        p.transition(PlanState::Proposed, now()).unwrap();
        p.transition(PlanState::Rejected, now()).unwrap();
        assert!(p.state.is_terminal());
    }

    #[test]
    fn cannot_approve_draft_directly() {
        let mut p = make_plan();
        let err = p.transition(PlanState::Approved, now()).unwrap_err();
        assert_eq!(err.code(), "ILLEGAL_TRANSITION");
    }

    #[test]
    fn cannot_go_from_completed() {
        let mut p = make_plan();
        p.transition(PlanState::Proposed, now()).unwrap();
        p.transition(PlanState::Approved, now()).unwrap();
        p.transition(PlanState::Executing, now()).unwrap();
        p.transition(PlanState::Completed, now()).unwrap();
        assert!(p.transition(PlanState::Executing, now()).is_err());
    }

    #[test]
    fn add_step_requires_draft() {
        let mut p = make_plan();
        let step = PlanStep {
            id: "step_01".into(),
            title: "do thing".into(),
            operation: None,
            state: PlanStepState::Pending,
            created_at: now(),
            updated_at: now(),
        };
        p.add_step(step.clone(), now()).unwrap();
        p.transition(PlanState::Proposed, now()).unwrap();
        assert!(matches!(
            p.add_step(step, now()),
            Err(OrchestrationError::InvariantViolation { .. })
        ));
    }

    #[test]
    fn amend_from_proposed_back_to_draft() {
        let mut p = make_plan();
        p.transition(PlanState::Proposed, now()).unwrap();
        p.transition(PlanState::Draft, now()).unwrap();
        assert_eq!(p.state, PlanState::Draft);
    }

    #[test]
    fn plan_roundtrips_json() {
        let p = make_plan();
        let s = serde_json::to_string(&p).unwrap();
        let back: Plan = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, p.id);
        assert_eq!(back.state, p.state);
    }
}
