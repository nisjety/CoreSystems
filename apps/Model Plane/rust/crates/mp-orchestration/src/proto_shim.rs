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
};
use orchestration_event::{
    ApprovalStateChanged as PbApprovalStateChanged, Event as ProtoEvent,
    PlanTransitioned as PbPlanTransitioned, RunPausedForApproval as PbRunPausedForApproval,
    RunResumedAfterApproval as PbRunResumedAfterApproval, SubagentAttached as PbSubagentAttached,
    SubagentStopped as PbSubagentStopped, TodoTransitioned as PbTodoTransitioned,
};

use crate::{ApprovalKind, ApprovalState, OrchestrationEvent, PlanState, SubagentRole, TodoState};

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
        }
    }
}

// ---------- envelope decode ----------

impl TryFrom<ProtoOrchestrationEvent> for OrchestrationEvent {
    type Error = ShimError;

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
