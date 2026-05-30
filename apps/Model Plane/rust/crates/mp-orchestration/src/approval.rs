//! Approval records. Owned by `session-core`; gates risky actions in
//! `execution-core`.
//!
//! An `Approval` represents a pause in execution while a human confirms a
//! risky action (tool call, plan step, permission escalation, etc).
//!
//! State machine:
//! ```text
//!    requested ──grant──▶ granted
//!        │                    ┃
//!        ├──deny──▶ denied    ┃
//!        └──timeout──▶ timed_out
//!                             ┃
//!  Terminal from grant: granted; execution resumes on the run.
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{OrchestrationError, OrchestrationResult};

/// Category of approval — drives UI copy + audit semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    /// Plan approval (user approves a whole plan before execution).
    Plan,
    /// Tool-call approval (per-invocation gate).
    ToolCall,
    /// Permission escalation (e.g. granting broader scope temporarily).
    Permission,
    /// Destructive action (irreversible side effect).
    Destructive,
    /// Cost threshold crossed.
    Cost,
}

/// Lifecycle states for an approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    /// Awaiting decision.
    Requested,
    /// Approved by the gating user.
    Granted,
    /// Rejected by the gating user.
    Denied,
    /// Expired before any decision was made.
    TimedOut,
}

impl ApprovalState {
    /// Canonical wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Granted => "granted",
            Self::Denied => "denied",
            Self::TimedOut => "timed_out",
        }
    }

    /// True when no further transitions are allowed.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Requested)
    }
}

/// Approval record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    /// Approval ULID (prefix `appr_`).
    pub id: String,
    /// Run this approval gates.
    pub run_id: String,
    /// Step this approval gates (empty string if run-level).
    pub step_id: String,
    /// Kind of approval.
    pub kind: ApprovalKind,
    /// Current state.
    pub state: ApprovalState,
    /// Who is expected to approve (user id or group).
    pub requested_of: String,
    /// Who actually decided (populated on terminal transition).
    pub decided_by: String,
    /// Reason supplied with the decision.
    pub decision_reason: String,
    /// Free-form context for the approver (tool args, plan summary, etc).
    pub context: serde_json::Value,
    /// Wall-clock timestamp the approval was requested.
    pub requested_at: DateTime<Utc>,
    /// Decided-at, if terminal.
    #[serde(default)]
    pub decided_at: Option<DateTime<Utc>>,
    /// Explicit expiry time; `timeout` transition cannot fire before this.
    pub expires_at: DateTime<Utc>,
}

impl Approval {
    const KIND: &'static str = "approval";

    /// Construct a new approval in `Requested`.
    ///
    /// # Errors
    /// - `MissingField` when required string fields are empty.
    /// - `InvariantViolation` when `expires_at <= requested_at`.
    pub fn new(
        id: impl Into<String>,
        run_id: impl Into<String>,
        kind: ApprovalKind,
        requested_of: impl Into<String>,
        requested_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
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
        let requested_of = requested_of.into();
        if requested_of.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "requested_of",
            });
        }
        if expires_at <= requested_at {
            return Err(OrchestrationError::InvariantViolation {
                kind: Self::KIND,
                detail: "expires_at must be after requested_at".into(),
            });
        }
        Ok(Self {
            id,
            run_id,
            step_id: String::new(),
            kind,
            state: ApprovalState::Requested,
            requested_of,
            decided_by: String::new(),
            decision_reason: String::new(),
            context: serde_json::Value::Null,
            requested_at,
            decided_at: None,
            expires_at,
        })
    }

    /// Grant the approval. Records decider + reason.
    ///
    /// # Errors
    /// - `IllegalTransition` when already terminal.
    /// - `MissingField` when `decided_by` is empty.
    pub fn grant(
        &mut self,
        decided_by: impl Into<String>,
        reason: impl Into<String>,
        now: DateTime<Utc>,
    ) -> OrchestrationResult<()> {
        self.decide(ApprovalState::Granted, decided_by, reason, now)
    }

    /// Deny the approval.
    ///
    /// # Errors
    /// - `IllegalTransition` when already terminal.
    /// - `MissingField` when `decided_by` is empty.
    pub fn deny(
        &mut self,
        decided_by: impl Into<String>,
        reason: impl Into<String>,
        now: DateTime<Utc>,
    ) -> OrchestrationResult<()> {
        self.decide(ApprovalState::Denied, decided_by, reason, now)
    }

    /// Mark the approval as timed out. Only legal when `now >= expires_at`
    /// and the approval is still `Requested`.
    ///
    /// # Errors
    /// - `IllegalTransition` when already terminal.
    /// - `InvariantViolation` when `now < expires_at`.
    pub fn mark_timeout(&mut self, now: DateTime<Utc>) -> OrchestrationResult<()> {
        if self.state.is_terminal() {
            return Err(OrchestrationError::IllegalTransition {
                kind: Self::KIND,
                from: self.state.as_str().to_owned(),
                to: ApprovalState::TimedOut.as_str().to_owned(),
            });
        }
        if now < self.expires_at {
            return Err(OrchestrationError::InvariantViolation {
                kind: Self::KIND,
                detail: "cannot time out before expires_at".into(),
            });
        }
        self.state = ApprovalState::TimedOut;
        self.decided_at = Some(now);
        Ok(())
    }

    fn decide(
        &mut self,
        to: ApprovalState,
        decided_by: impl Into<String>,
        reason: impl Into<String>,
        now: DateTime<Utc>,
    ) -> OrchestrationResult<()> {
        if self.state != ApprovalState::Requested {
            return Err(OrchestrationError::IllegalTransition {
                kind: Self::KIND,
                from: self.state.as_str().to_owned(),
                to: to.as_str().to_owned(),
            });
        }
        let by = decided_by.into();
        if by.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "decided_by",
            });
        }
        self.state = to;
        self.decided_by = by;
        self.decision_reason = reason.into();
        self.decided_at = Some(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid epoch")
    }

    fn make() -> Approval {
        Approval::new(
            "appr_01",
            "run_01",
            ApprovalKind::ToolCall,
            "alice",
            now(),
            now() + Duration::minutes(10),
        )
        .expect("ctor")
    }

    #[test]
    fn starts_requested() {
        let a = make();
        assert_eq!(a.state, ApprovalState::Requested);
        assert!(!a.state.is_terminal());
    }

    #[test]
    fn grant_records_decider_and_reason() {
        let mut a = make();
        a.grant("bob", "looks safe", now() + Duration::seconds(30))
            .unwrap();
        assert_eq!(a.state, ApprovalState::Granted);
        assert_eq!(a.decided_by, "bob");
        assert_eq!(a.decision_reason, "looks safe");
        assert!(a.decided_at.is_some());
    }

    #[test]
    fn deny_records_decider_and_reason() {
        let mut a = make();
        a.deny("bob", "too risky", now() + Duration::seconds(30))
            .unwrap();
        assert_eq!(a.state, ApprovalState::Denied);
        assert_eq!(a.decided_by, "bob");
    }

    #[test]
    fn double_decision_rejected() {
        let mut a = make();
        a.grant("bob", "", now() + Duration::seconds(30)).unwrap();
        assert!(a.deny("carol", "", now() + Duration::seconds(60)).is_err());
    }

    #[test]
    fn timeout_requires_past_expiry() {
        let mut a = make();
        assert!(a.mark_timeout(now()).is_err()); // now == requested_at, expires 10min later
        let later = now() + Duration::minutes(11);
        a.mark_timeout(later).unwrap();
        assert_eq!(a.state, ApprovalState::TimedOut);
    }

    #[test]
    fn timeout_rejects_after_grant() {
        let mut a = make();
        a.grant("bob", "", now() + Duration::seconds(30)).unwrap();
        let later = now() + Duration::minutes(11);
        assert!(a.mark_timeout(later).is_err());
    }

    #[test]
    fn expires_at_must_be_after_requested_at() {
        let err = Approval::new(
            "appr_01",
            "run_01",
            ApprovalKind::ToolCall,
            "alice",
            now(),
            now(),
        )
        .unwrap_err();
        assert_eq!(err.code(), "INVARIANT_VIOLATION");
    }

    #[test]
    fn decide_rejects_empty_decider() {
        let mut a = make();
        assert!(a.grant("", "", now() + Duration::seconds(30)).is_err());
    }

    #[test]
    fn approval_roundtrips_json() {
        let mut a = make();
        a.grant("bob", "looks safe", now() + Duration::seconds(30))
            .unwrap();
        let s = serde_json::to_string(&a).unwrap();
        let back: Approval = serde_json::from_str(&s).unwrap();
        assert_eq!(back.state, ApprovalState::Granted);
        assert_eq!(back.decided_by, "bob");
    }
}
