//! Proto interop shim for orchestration events.
//!
//! Translates between the in-process `OrchestrationEvent` enum (with timestamp
//! and rich Rust enum payloads) and the wire-format `mp.v1` protobuf
//! `OrchestrationEvent` (envelope + oneof payload, integer enum codes).
//!
//! The protobuf envelope carries `at` once at the top level. Rust variants
//! carry `at` per-variant, so encode lifts the timestamp into the envelope
//! and decode injects it back into each variant.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    orchestration_event, ApprovalKind as ProtoApprovalKind, ApprovalState as ProtoApprovalState,
    OrchestrationEvent as ProtoOrchestrationEvent, PlanState as ProtoPlanState,
    SubagentRole as ProtoSubagentRole, TodoState as ProtoTodoState,
    VerificationResult as ProtoVerificationResult, VerificationStatus as ProtoVerificationStatus,
};
use orchestration_event::{
    ApprovalContinuationVerified as PbApprovalContinuationVerified,
    ApprovalStateChanged as PbApprovalStateChanged,
    BrowserActionApprovalRequired as PbBrowserActionApprovalRequired,
    BrowserActionDecided as PbBrowserActionDecided,
    BrowserActionDispatched as PbBrowserActionDispatched,
    BrowserObservationReceived as PbBrowserObservationReceived,
    BrowserRunPaused as PbBrowserRunPaused, BrowserRunResumed as PbBrowserRunResumed,
    Event as ProtoEvent, PlanTransitioned as PbPlanTransitioned,
    RunPausedForApproval as PbRunPausedForApproval,
    RunResumedAfterApproval as PbRunResumedAfterApproval, SubagentAttached as PbSubagentAttached,
    SubagentStopped as PbSubagentStopped, TodoTransitioned as PbTodoTransitioned,
};

use crate::{
    ApprovalKind, ApprovalState, OrchestrationEvent, PlanState, SubagentRole, TodoState,
    VerificationStatus,
};

/// Errors returned when decoding a wire-format orchestration event.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ShimError {
    /// The envelope did not include a timestamp.
    #[error("orchestration event envelope missing timestamp")]
    MissingTimestamp,
    /// The envelope did not include an event payload.
    #[error("orchestration event envelope missing payload")]
    MissingEvent,
    /// The envelope timestamp was not a valid `chrono::DateTime<Utc>`.
    #[error("orchestration event timestamp out of range")]
    InvalidTimestamp,
    /// An integer code did not map to a known `PlanState`.
    #[error("unknown PlanState code: {0}")]
    UnknownPlanState(i32),
    /// An integer code did not map to a known `TodoState`.
    #[error("unknown TodoState code: {0}")]
    UnknownTodoState(i32),
    /// An integer code did not map to a known `ApprovalKind`.
    #[error("unknown ApprovalKind code: {0}")]
    UnknownApprovalKind(i32),
    /// An integer code did not map to a known `ApprovalState`.
    #[error("unknown ApprovalState code: {0}")]
    UnknownApprovalState(i32),
    /// An integer code did not map to a known `SubagentRole`.
    #[error("unknown SubagentRole code: {0}")]
    UnknownSubagentRole(i32),
    /// An integer code did not map to a known, judged `VerificationStatus`
    /// (`Unspecified` is also rejected here — this event never carries an
    /// unjudged verification).
    #[error("unknown or unspecified VerificationStatus code: {0}")]
    UnknownVerificationStatus(i32),
    /// `ApprovalContinuationVerified` requires a `verification` field; an
    /// event missing it cannot round-trip (the recording handler never emits
    /// this event without one — see `orchestration_grpc.rs`).
    #[error("approval continuation verified event missing verification result")]
    MissingVerificationResult,
}

// ---------- timestamp helpers ----------

fn ts_from(dt: DateTime<Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: dt.timestamp(),
        nanos: i32::try_from(dt.timestamp_subsec_nanos()).unwrap_or(0),
    }
}

fn ts_to(ts: prost_types::Timestamp) -> Result<DateTime<Utc>, ShimError> {
    let nanos = u32::try_from(ts.nanos).map_err(|_| ShimError::InvalidTimestamp)?;
    DateTime::from_timestamp(ts.seconds, nanos).ok_or(ShimError::InvalidTimestamp)
}

// ---------- enum encode ----------

fn plan_state_to_proto(s: PlanState) -> i32 {
    match s {
        PlanState::Draft => ProtoPlanState::Draft as i32,
        PlanState::Proposed => ProtoPlanState::Proposed as i32,
        PlanState::Approved => ProtoPlanState::Approved as i32,
        PlanState::Rejected => ProtoPlanState::Rejected as i32,
        PlanState::Executing => ProtoPlanState::Executing as i32,
        PlanState::Completed => ProtoPlanState::Completed as i32,
        PlanState::Failed => ProtoPlanState::Failed as i32,
        PlanState::Superseded => ProtoPlanState::Superseded as i32,
        PlanState::Archived => ProtoPlanState::Archived as i32,
    }
}

fn todo_state_to_proto(s: TodoState) -> i32 {
    match s {
        TodoState::Pending => ProtoTodoState::Pending as i32,
        TodoState::InProgress => ProtoTodoState::InProgress as i32,
        TodoState::Blocked => ProtoTodoState::Blocked as i32,
        TodoState::Completed => ProtoTodoState::Completed as i32,
        TodoState::Cancelled => ProtoTodoState::Cancelled as i32,
    }
}

fn approval_kind_to_proto(k: ApprovalKind) -> i32 {
    match k {
        ApprovalKind::Plan => ProtoApprovalKind::Plan as i32,
        ApprovalKind::ToolCall => ProtoApprovalKind::ToolCall as i32,
        ApprovalKind::Permission => ProtoApprovalKind::Permission as i32,
        ApprovalKind::Destructive => ProtoApprovalKind::Destructive as i32,
        ApprovalKind::Cost => ProtoApprovalKind::Cost as i32,
    }
}

fn approval_state_to_proto(s: ApprovalState) -> i32 {
    match s {
        ApprovalState::Requested => ProtoApprovalState::Requested as i32,
        ApprovalState::Granted => ProtoApprovalState::Granted as i32,
        ApprovalState::Denied => ProtoApprovalState::Denied as i32,
        ApprovalState::TimedOut => ProtoApprovalState::TimedOut as i32,
    }
}

fn subagent_role_to_proto(r: SubagentRole) -> i32 {
    match r {
        SubagentRole::Coder => ProtoSubagentRole::Coder as i32,
        SubagentRole::Reviewer => ProtoSubagentRole::Reviewer as i32,
        SubagentRole::Researcher => ProtoSubagentRole::Researcher as i32,
        SubagentRole::Explorer => ProtoSubagentRole::Explorer as i32,
        SubagentRole::Generic => ProtoSubagentRole::Generic as i32,
    }
}

fn verification_status_to_proto(s: VerificationStatus) -> i32 {
    match s {
        VerificationStatus::Unknown => ProtoVerificationStatus::Unknown as i32,
        VerificationStatus::VerifiedSuccess => ProtoVerificationStatus::VerifiedSuccess as i32,
        VerificationStatus::VerifiedFailure => ProtoVerificationStatus::VerifiedFailure as i32,
        VerificationStatus::PartiallyVerified => ProtoVerificationStatus::PartiallyVerified as i32,
    }
}

// ---------- enum decode ----------

fn plan_state_from_proto(code: i32) -> Result<PlanState, ShimError> {
    let p = ProtoPlanState::try_from(code).map_err(|_| ShimError::UnknownPlanState(code))?;
    match p {
        ProtoPlanState::Unspecified => Err(ShimError::UnknownPlanState(code)),
        ProtoPlanState::Draft => Ok(PlanState::Draft),
        ProtoPlanState::Proposed => Ok(PlanState::Proposed),
        ProtoPlanState::Approved => Ok(PlanState::Approved),
        ProtoPlanState::Rejected => Ok(PlanState::Rejected),
        ProtoPlanState::Executing => Ok(PlanState::Executing),
        ProtoPlanState::Completed => Ok(PlanState::Completed),
        ProtoPlanState::Failed => Ok(PlanState::Failed),
        ProtoPlanState::Superseded => Ok(PlanState::Superseded),
        ProtoPlanState::Archived => Ok(PlanState::Archived),
    }
}

fn todo_state_from_proto(code: i32) -> Result<TodoState, ShimError> {
    let p = ProtoTodoState::try_from(code).map_err(|_| ShimError::UnknownTodoState(code))?;
    match p {
        ProtoTodoState::Unspecified => Err(ShimError::UnknownTodoState(code)),
        ProtoTodoState::Pending => Ok(TodoState::Pending),
        ProtoTodoState::InProgress => Ok(TodoState::InProgress),
        ProtoTodoState::Blocked => Ok(TodoState::Blocked),
        ProtoTodoState::Completed => Ok(TodoState::Completed),
        ProtoTodoState::Cancelled => Ok(TodoState::Cancelled),
    }
}

fn approval_kind_from_proto(code: i32) -> Result<ApprovalKind, ShimError> {
    let p = ProtoApprovalKind::try_from(code).map_err(|_| ShimError::UnknownApprovalKind(code))?;
    match p {
        ProtoApprovalKind::Unspecified => Err(ShimError::UnknownApprovalKind(code)),
        ProtoApprovalKind::Plan => Ok(ApprovalKind::Plan),
        ProtoApprovalKind::ToolCall => Ok(ApprovalKind::ToolCall),
        ProtoApprovalKind::Permission => Ok(ApprovalKind::Permission),
        ProtoApprovalKind::Destructive => Ok(ApprovalKind::Destructive),
        ProtoApprovalKind::Cost => Ok(ApprovalKind::Cost),
    }
}

fn approval_state_from_proto(code: i32) -> Result<ApprovalState, ShimError> {
    let p =
        ProtoApprovalState::try_from(code).map_err(|_| ShimError::UnknownApprovalState(code))?;
    match p {
        ProtoApprovalState::Unspecified => Err(ShimError::UnknownApprovalState(code)),
        ProtoApprovalState::Requested => Ok(ApprovalState::Requested),
        ProtoApprovalState::Granted => Ok(ApprovalState::Granted),
        ProtoApprovalState::Denied => Ok(ApprovalState::Denied),
        ProtoApprovalState::TimedOut => Ok(ApprovalState::TimedOut),
    }
}

fn subagent_role_from_proto(code: i32) -> Result<SubagentRole, ShimError> {
    let p = ProtoSubagentRole::try_from(code).map_err(|_| ShimError::UnknownSubagentRole(code))?;
    match p {
        ProtoSubagentRole::Unspecified => Err(ShimError::UnknownSubagentRole(code)),
        ProtoSubagentRole::Coder => Ok(SubagentRole::Coder),
        ProtoSubagentRole::Reviewer => Ok(SubagentRole::Reviewer),
        ProtoSubagentRole::Researcher => Ok(SubagentRole::Researcher),
        ProtoSubagentRole::Explorer => Ok(SubagentRole::Explorer),
        ProtoSubagentRole::Generic => Ok(SubagentRole::Generic),
    }
}

fn verification_status_from_proto(code: i32) -> Result<VerificationStatus, ShimError> {
    let p = ProtoVerificationStatus::try_from(code)
        .map_err(|_| ShimError::UnknownVerificationStatus(code))?;
    match p {
        ProtoVerificationStatus::Unspecified => Err(ShimError::UnknownVerificationStatus(code)),
        ProtoVerificationStatus::Unknown => Ok(VerificationStatus::Unknown),
        ProtoVerificationStatus::VerifiedSuccess => Ok(VerificationStatus::VerifiedSuccess),
        ProtoVerificationStatus::VerifiedFailure => Ok(VerificationStatus::VerifiedFailure),
        ProtoVerificationStatus::PartiallyVerified => Ok(VerificationStatus::PartiallyVerified),
    }
}

// ---------- envelope encode ----------

/// Wrap an encoded event variant in the proto envelope. `event_id` is left
/// empty — session-core assigns it at broadcast time.
fn envelope(at: DateTime<Utc>, event: ProtoEvent) -> ProtoOrchestrationEvent {
    ProtoOrchestrationEvent {
        event_id: String::new(),
        at: Some(ts_from(at)),
        event: Some(event),
    }
}

impl From<OrchestrationEvent> for ProtoOrchestrationEvent {
    // exhaustive event-variant mapping; splitting would obscure the 1:1 shim
    #[allow(clippy::too_many_lines)]
    fn from(ev: OrchestrationEvent) -> Self {
        match ev {
            OrchestrationEvent::PlanTransitioned {
                plan_id,
                run_id,
                from,
                to,
                at,
            } => envelope(
                at,
                ProtoEvent::PlanTransitioned(PbPlanTransitioned {
                    plan_id,
                    run_id,
                    from: plan_state_to_proto(from),
                    to: plan_state_to_proto(to),
                }),
            ),
            OrchestrationEvent::TodoTransitioned {
                todo_id,
                thread_id,
                from,
                to,
                at,
            } => envelope(
                at,
                ProtoEvent::TodoTransitioned(PbTodoTransitioned {
                    todo_id,
                    thread_id,
                    from: todo_state_to_proto(from),
                    to: todo_state_to_proto(to),
                }),
            ),
            OrchestrationEvent::ApprovalStateChanged {
                approval_id,
                run_id,
                approval_kind,
                to,
                decided_by,
                at,
            } => envelope(
                at,
                ProtoEvent::ApprovalStateChanged(PbApprovalStateChanged {
                    approval_id,
                    run_id,
                    approval_kind: approval_kind_to_proto(approval_kind),
                    to: approval_state_to_proto(to),
                    decided_by,
                }),
            ),
            OrchestrationEvent::SubagentAttached {
                parent_run_id,
                child_run_id,
                role,
                at,
            } => envelope(
                at,
                ProtoEvent::SubagentAttached(PbSubagentAttached {
                    parent_run_id,
                    child_run_id,
                    role: subagent_role_to_proto(role),
                }),
            ),
            OrchestrationEvent::SubagentStopped {
                child_run_id,
                status,
                at,
            } => envelope(
                at,
                ProtoEvent::SubagentStopped(PbSubagentStopped {
                    child_run_id,
                    status,
                }),
            ),
            OrchestrationEvent::RunPausedForApproval {
                run_id,
                approval_id,
                at,
            } => envelope(
                at,
                ProtoEvent::RunPausedForApproval(PbRunPausedForApproval {
                    run_id,
                    approval_id,
                }),
            ),
            OrchestrationEvent::RunResumedAfterApproval {
                run_id,
                approval_id,
                at,
            } => envelope(
                at,
                ProtoEvent::RunResumedAfterApproval(PbRunResumedAfterApproval {
                    run_id,
                    approval_id,
                }),
            ),
            OrchestrationEvent::BrowserActionDispatched {
                run_id,
                plan_id,
                action_id,
                action_type,
                url,
                at,
            } => envelope(
                at,
                ProtoEvent::BrowserActionDispatched(PbBrowserActionDispatched {
                    run_id,
                    plan_id,
                    action_id,
                    action_type,
                    url,
                    // The domain enum has no `reason` field — this direction is
                    // vestigial (browser_events.rs constructs the wire event
                    // directly and never round-trips through this domain enum).
                    reason: String::new(),
                }),
            ),
            OrchestrationEvent::BrowserObservationReceived {
                run_id,
                plan_id,
                action_id,
                status,
                page_url,
                page_title,
                at,
            } => envelope(
                at,
                ProtoEvent::BrowserObservationReceived(PbBrowserObservationReceived {
                    run_id,
                    plan_id,
                    action_id,
                    status,
                    page_url,
                    page_title,
                    // See note above: vestigial direction, no domain equivalent.
                    screenshot_ref: String::new(),
                    dom_snapshot_ref: String::new(),
                }),
            ),
            OrchestrationEvent::BrowserRunPaused {
                run_id,
                plan_id,
                at,
            } => envelope(
                at,
                ProtoEvent::BrowserRunPaused(PbBrowserRunPaused { run_id, plan_id }),
            ),
            OrchestrationEvent::BrowserRunResumed {
                run_id,
                plan_id,
                at,
            } => envelope(
                at,
                ProtoEvent::BrowserRunResumed(PbBrowserRunResumed { run_id, plan_id }),
            ),
            OrchestrationEvent::BrowserActionApprovalRequired {
                run_id,
                plan_id,
                action_id,
                action_type,
                url,
                selector,
                reason,
                risk_category,
                approval_id,
                at,
            } => envelope(
                at,
                ProtoEvent::BrowserActionApprovalRequired(PbBrowserActionApprovalRequired {
                    run_id,
                    plan_id,
                    action_id,
                    action_type,
                    url,
                    selector,
                    reason,
                    risk_category,
                    approval_id,
                }),
            ),
            OrchestrationEvent::BrowserActionDecided {
                run_id,
                plan_id,
                action_id,
                approval_id,
                decision,
                decided_by,
                at,
            } => envelope(
                at,
                ProtoEvent::BrowserActionDecided(PbBrowserActionDecided {
                    run_id,
                    plan_id,
                    action_id,
                    approval_id,
                    decision,
                    decided_by,
                }),
            ),
            OrchestrationEvent::ApprovalContinuationVerified {
                run_id,
                delivery_id,
                approval_id,
                receipt_id,
                verification_status,
                verification_method,
                verification_reason,
                at,
            } => envelope(
                at,
                ProtoEvent::ApprovalContinuationVerified(PbApprovalContinuationVerified {
                    run_id,
                    delivery_id,
                    approval_id,
                    receipt_id: receipt_id.clone(),
                    verification: Some(ProtoVerificationResult {
                        effect_id: receipt_id,
                        status: verification_status_to_proto(verification_status),
                        method: verification_method,
                        reason: verification_reason,
                        verified_at: Some(ts_from(at)),
                    }),
                }),
            ),
        }
    }
}

// ---------- envelope decode ----------

impl TryFrom<ProtoOrchestrationEvent> for OrchestrationEvent {
    type Error = ShimError;

    // One match arm per oneof variant keeps the mapping exhaustively visible
    // in a single place rather than splitting it across helper functions —
    // same tradeoff `model-gateway`'s `orchestration_event_to_step_update`
    // makes for the same reason.
    #[allow(clippy::too_many_lines)]
    fn try_from(pb: ProtoOrchestrationEvent) -> Result<Self, Self::Error> {
        let at = ts_to(pb.at.ok_or(ShimError::MissingTimestamp)?)?;
        let event = pb.event.ok_or(ShimError::MissingEvent)?;
        Ok(match event {
            ProtoEvent::PlanTransitioned(p) => OrchestrationEvent::PlanTransitioned {
                plan_id: p.plan_id,
                run_id: p.run_id,
                from: plan_state_from_proto(p.from)?,
                to: plan_state_from_proto(p.to)?,
                at,
            },
            ProtoEvent::TodoTransitioned(p) => OrchestrationEvent::TodoTransitioned {
                todo_id: p.todo_id,
                thread_id: p.thread_id,
                from: todo_state_from_proto(p.from)?,
                to: todo_state_from_proto(p.to)?,
                at,
            },
            ProtoEvent::ApprovalStateChanged(p) => OrchestrationEvent::ApprovalStateChanged {
                approval_id: p.approval_id,
                run_id: p.run_id,
                approval_kind: approval_kind_from_proto(p.approval_kind)?,
                to: approval_state_from_proto(p.to)?,
                decided_by: p.decided_by,
                at,
            },
            ProtoEvent::SubagentAttached(p) => OrchestrationEvent::SubagentAttached {
                parent_run_id: p.parent_run_id,
                child_run_id: p.child_run_id,
                role: subagent_role_from_proto(p.role)?,
                at,
            },
            ProtoEvent::SubagentStopped(p) => OrchestrationEvent::SubagentStopped {
                child_run_id: p.child_run_id,
                status: p.status,
                at,
            },
            ProtoEvent::RunPausedForApproval(p) => OrchestrationEvent::RunPausedForApproval {
                run_id: p.run_id,
                approval_id: p.approval_id,
                at,
            },
            ProtoEvent::RunResumedAfterApproval(p) => OrchestrationEvent::RunResumedAfterApproval {
                run_id: p.run_id,
                approval_id: p.approval_id,
                at,
            },
            ProtoEvent::BrowserActionDispatched(p) => OrchestrationEvent::BrowserActionDispatched {
                run_id: p.run_id,
                plan_id: p.plan_id,
                action_id: p.action_id,
                action_type: p.action_type,
                url: p.url,
                at,
            },
            ProtoEvent::BrowserObservationReceived(p) => {
                OrchestrationEvent::BrowserObservationReceived {
                    run_id: p.run_id,
                    plan_id: p.plan_id,
                    action_id: p.action_id,
                    status: p.status,
                    page_url: p.page_url,
                    page_title: p.page_title,
                    // `p.screenshot_ref`/`p.dom_snapshot_ref` have no domain
                    // equivalent (see the `From` direction's note) — dropped.
                    at,
                }
            }
            ProtoEvent::BrowserRunPaused(p) => OrchestrationEvent::BrowserRunPaused {
                run_id: p.run_id,
                plan_id: p.plan_id,
                at,
            },
            ProtoEvent::BrowserRunResumed(p) => OrchestrationEvent::BrowserRunResumed {
                run_id: p.run_id,
                plan_id: p.plan_id,
                at,
            },
            ProtoEvent::BrowserActionApprovalRequired(p) => {
                OrchestrationEvent::BrowserActionApprovalRequired {
                    run_id: p.run_id,
                    plan_id: p.plan_id,
                    action_id: p.action_id,
                    action_type: p.action_type,
                    url: p.url,
                    selector: p.selector,
                    reason: p.reason,
                    risk_category: p.risk_category,
                    approval_id: p.approval_id,
                    at,
                }
            }
            ProtoEvent::BrowserActionDecided(p) => OrchestrationEvent::BrowserActionDecided {
                run_id: p.run_id,
                plan_id: p.plan_id,
                action_id: p.action_id,
                approval_id: p.approval_id,
                decision: p.decision,
                decided_by: p.decided_by,
                at,
            },
            ProtoEvent::ApprovalContinuationVerified(p) => {
                let v = p.verification.ok_or(ShimError::MissingVerificationResult)?;
                OrchestrationEvent::ApprovalContinuationVerified {
                    run_id: p.run_id,
                    delivery_id: p.delivery_id,
                    approval_id: p.approval_id,
                    receipt_id: p.receipt_id,
                    verification_status: verification_status_from_proto(v.status)?,
                    verification_method: v.method,
                    verification_reason: v.reason,
                    at,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid epoch")
    }

    fn round_trip(ev: &OrchestrationEvent) {
        let pb: ProtoOrchestrationEvent = ev.clone().into();
        let back: OrchestrationEvent = pb.try_into().expect("decode");
        assert_eq!(*ev, back);
    }

    #[test]
    fn round_trip_plan_transitioned() {
        round_trip(&OrchestrationEvent::PlanTransitioned {
            plan_id: "plan-1".into(),
            run_id: "run-1".into(),
            from: PlanState::Draft,
            to: PlanState::Proposed,
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_todo_transitioned() {
        round_trip(&OrchestrationEvent::TodoTransitioned {
            todo_id: "todo-1".into(),
            thread_id: "thread-1".into(),
            from: TodoState::Pending,
            to: TodoState::InProgress,
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_browser_action_dispatched() {
        round_trip(&OrchestrationEvent::BrowserActionDispatched {
            run_id: "run-1".into(),
            plan_id: "plan-1".into(),
            action_id: "act_0001".into(),
            action_type: "goto".into(),
            url: "https://example.com".into(),
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_browser_observation_received() {
        round_trip(&OrchestrationEvent::BrowserObservationReceived {
            run_id: "run-1".into(),
            plan_id: "plan-1".into(),
            action_id: "act_0001".into(),
            status: "success".into(),
            page_url: "https://example.com/landing".into(),
            page_title: "Example Domain".into(),
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_browser_run_paused() {
        round_trip(&OrchestrationEvent::BrowserRunPaused {
            run_id: "run-1".into(),
            plan_id: "plan-1".into(),
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_browser_run_resumed() {
        round_trip(&OrchestrationEvent::BrowserRunResumed {
            run_id: "run-1".into(),
            plan_id: "plan-1".into(),
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_browser_action_approval_required() {
        round_trip(&OrchestrationEvent::BrowserActionApprovalRequired {
            run_id: "run-1".into(),
            plan_id: "plan-1".into(),
            action_id: "act_0002".into(),
            action_type: "click".into(),
            url: "https://example.com/checkout".into(),
            selector: "button.place-order".into(),
            reason: "action appears to interact with a checkout/payment flow".into(),
            risk_category: "checkout".into(),
            approval_id: "appr-1".into(),
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_browser_action_decided() {
        round_trip(&OrchestrationEvent::BrowserActionDecided {
            run_id: "run-1".into(),
            plan_id: "plan-1".into(),
            action_id: "act_0002".into(),
            approval_id: "appr-1".into(),
            decision: "granted".into(),
            decided_by: "user@example.com".into(),
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_approval_continuation_verified() {
        round_trip(&OrchestrationEvent::ApprovalContinuationVerified {
            run_id: "run-1".into(),
            delivery_id: "delivery-1".into(),
            approval_id: "appr-1".into(),
            receipt_id: "receipt-1".into(),
            verification_status: VerificationStatus::VerifiedSuccess,
            verification_method: "structural".into(),
            verification_reason: "provider returned authoritative receipt id booking-1".into(),
            at: fixture_at(),
        });
    }

    #[test]
    fn decode_rejects_unspecified_verification_status() {
        let mut pb: ProtoOrchestrationEvent = OrchestrationEvent::ApprovalContinuationVerified {
            run_id: "run-1".into(),
            delivery_id: "delivery-1".into(),
            approval_id: "appr-1".into(),
            receipt_id: "receipt-1".into(),
            verification_status: VerificationStatus::Unknown,
            verification_method: "structural".into(),
            verification_reason: String::new(),
            at: fixture_at(),
        }
        .into();
        if let Some(ProtoEvent::ApprovalContinuationVerified(ref mut e)) = pb.event {
            e.verification.as_mut().expect("verification present").status =
                ProtoVerificationStatus::Unspecified as i32;
        } else {
            panic!("expected ApprovalContinuationVerified");
        }
        let decoded: Result<OrchestrationEvent, ShimError> = pb.try_into();
        assert_eq!(
            decoded,
            Err(ShimError::UnknownVerificationStatus(
                ProtoVerificationStatus::Unspecified as i32
            ))
        );
    }

    #[test]
    fn decode_rejects_a_missing_verification_result() {
        let mut pb: ProtoOrchestrationEvent = OrchestrationEvent::ApprovalContinuationVerified {
            run_id: "run-1".into(),
            delivery_id: "delivery-1".into(),
            approval_id: "appr-1".into(),
            receipt_id: "receipt-1".into(),
            verification_status: VerificationStatus::Unknown,
            verification_method: "structural".into(),
            verification_reason: String::new(),
            at: fixture_at(),
        }
        .into();
        if let Some(ProtoEvent::ApprovalContinuationVerified(ref mut e)) = pb.event {
            e.verification = None;
        } else {
            panic!("expected ApprovalContinuationVerified");
        }
        let decoded: Result<OrchestrationEvent, ShimError> = pb.try_into();
        assert_eq!(decoded, Err(ShimError::MissingVerificationResult));
    }

    #[test]
    fn round_trip_approval_state_changed() {
        round_trip(&OrchestrationEvent::ApprovalStateChanged {
            approval_id: "appr-1".into(),
            run_id: "run-1".into(),
            approval_kind: ApprovalKind::ToolCall,
            to: ApprovalState::Granted,
            decided_by: "user@example.com".into(),
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_subagent_attached() {
        round_trip(&OrchestrationEvent::SubagentAttached {
            parent_run_id: "run-parent".into(),
            child_run_id: "run-child".into(),
            role: SubagentRole::Reviewer,
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_subagent_stopped() {
        round_trip(&OrchestrationEvent::SubagentStopped {
            child_run_id: "run-child".into(),
            status: "completed".into(),
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_run_paused_for_approval() {
        round_trip(&OrchestrationEvent::RunPausedForApproval {
            run_id: "run-1".into(),
            approval_id: "appr-1".into(),
            at: fixture_at(),
        });
    }

    #[test]
    fn round_trip_run_resumed_after_approval() {
        round_trip(&OrchestrationEvent::RunResumedAfterApproval {
            run_id: "run-1".into(),
            approval_id: "appr-1".into(),
            at: fixture_at(),
        });
    }

    #[test]
    fn decode_rejects_missing_timestamp() {
        let pb = ProtoOrchestrationEvent {
            event_id: String::new(),
            at: None,
            event: Some(ProtoEvent::RunPausedForApproval(PbRunPausedForApproval {
                run_id: "r".into(),
                approval_id: "a".into(),
            })),
        };
        assert_eq!(
            OrchestrationEvent::try_from(pb),
            Err(ShimError::MissingTimestamp)
        );
    }

    #[test]
    fn decode_rejects_missing_event() {
        let pb = ProtoOrchestrationEvent {
            event_id: String::new(),
            at: Some(ts_from(fixture_at())),
            event: None,
        };
        assert_eq!(
            OrchestrationEvent::try_from(pb),
            Err(ShimError::MissingEvent)
        );
    }

    #[test]
    fn decode_rejects_unspecified_plan_state() {
        let pb = ProtoOrchestrationEvent {
            event_id: String::new(),
            at: Some(ts_from(fixture_at())),
            event: Some(ProtoEvent::PlanTransitioned(PbPlanTransitioned {
                plan_id: "p".into(),
                run_id: "r".into(),
                from: 0,
                to: 1,
            })),
        };
        assert_eq!(
            OrchestrationEvent::try_from(pb),
            Err(ShimError::UnknownPlanState(0))
        );
    }
}
