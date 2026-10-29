// Package orchestration contains constructors, NATS subject mapping, and
// gRPC handler scaffolding for the OrchestrationCoreService surface.
package orchestration

import (
	"google.golang.org/protobuf/types/known/timestamppb"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// Event-type identifiers used in the canonical envelope.
const (
	EventTypePlanTransitioned        = "plan.transitioned"
	EventTypeTodoTransitioned        = "todo.transitioned"
	EventTypeApprovalStateChanged    = "approval.state_changed"
	EventTypeSubagentAttached        = "subagent.attached"
	EventTypeSubagentStopped         = "subagent.stopped"
	EventTypeRunPausedForApproval    = "run.paused_for_approval"
	EventTypeRunResumedAfterApproval = "run.resumed_after_approval"
)

// NATS subjects under the mp.v1.orchestration.* prefix.
const (
	SubjectPlan     = "mp.v1.orchestration.plan"
	SubjectTodo     = "mp.v1.orchestration.todo"
	SubjectApproval = "mp.v1.orchestration.approval"
	SubjectSubagent = "mp.v1.orchestration.subagent"
	SubjectRun      = "mp.v1.orchestration.run"
)

// Producer identifier stamped in envelopes emitted by orchestrator-core.
const Producer = "orchestrator-core"

// NewPlanTransitioned builds a PlanTransitioned event with the current timestamp.
func NewPlanTransitioned(planID, runID string, from, to mpv1.PlanState) *mpv1.OrchestrationEvent {
	return &mpv1.OrchestrationEvent{
		At: timestamppb.Now(),
		Event: &mpv1.OrchestrationEvent_PlanTransitioned_{
			PlanTransitioned: &mpv1.OrchestrationEvent_PlanTransitioned{
				PlanId: planID,
				RunId:  runID,
				From:   from,
				To:     to,
			},
		},
	}
}

// NewTodoTransitioned builds a TodoTransitioned event.
func NewTodoTransitioned(todoID, threadID string, from, to mpv1.TodoState) *mpv1.OrchestrationEvent {
	return &mpv1.OrchestrationEvent{
		At: timestamppb.Now(),
		Event: &mpv1.OrchestrationEvent_TodoTransitioned_{
			TodoTransitioned: &mpv1.OrchestrationEvent_TodoTransitioned{
				TodoId:   todoID,
				ThreadId: threadID,
				From:     from,
				To:       to,
			},
		},
	}
}

// NewApprovalStateChanged builds an ApprovalStateChanged event.
func NewApprovalStateChanged(approvalID, runID string, kind mpv1.ApprovalKind, to mpv1.ApprovalState, decidedBy string) *mpv1.OrchestrationEvent {
	return &mpv1.OrchestrationEvent{
		At: timestamppb.Now(),
		Event: &mpv1.OrchestrationEvent_ApprovalStateChanged_{
			ApprovalStateChanged: &mpv1.OrchestrationEvent_ApprovalStateChanged{
				ApprovalId:   approvalID,
				RunId:        runID,
				ApprovalKind: kind,
				To:           to,
				DecidedBy:    decidedBy,
			},
		},
	}
}

// NewSubagentAttached builds a SubagentAttached event.
func NewSubagentAttached(parentRunID, childRunID string, role mpv1.SubagentRole) *mpv1.OrchestrationEvent {
	return &mpv1.OrchestrationEvent{
		At: timestamppb.Now(),
		Event: &mpv1.OrchestrationEvent_SubagentAttached_{
			SubagentAttached: &mpv1.OrchestrationEvent_SubagentAttached{
				ParentRunId: parentRunID,
				ChildRunId:  childRunID,
				Role:        role,
			},
		},
	}
}

// NewSubagentStopped builds a SubagentStopped event.
func NewSubagentStopped(childRunID, status string) *mpv1.OrchestrationEvent {
	return &mpv1.OrchestrationEvent{
		At: timestamppb.Now(),
		Event: &mpv1.OrchestrationEvent_SubagentStopped_{
			SubagentStopped: &mpv1.OrchestrationEvent_SubagentStopped{
				ChildRunId: childRunID,
				Status:     status,
			},
		},
	}
}

// NewRunPausedForApproval builds a RunPausedForApproval event.
func NewRunPausedForApproval(runID, approvalID string) *mpv1.OrchestrationEvent {
	return &mpv1.OrchestrationEvent{
		At: timestamppb.Now(),
		Event: &mpv1.OrchestrationEvent_RunPausedForApproval_{
			RunPausedForApproval: &mpv1.OrchestrationEvent_RunPausedForApproval{
				RunId:      runID,
				ApprovalId: approvalID,
			},
		},
	}
}

// NewRunResumedAfterApproval builds a RunResumedAfterApproval event.
func NewRunResumedAfterApproval(runID, approvalID string) *mpv1.OrchestrationEvent {
	return &mpv1.OrchestrationEvent{
		At: timestamppb.Now(),
		Event: &mpv1.OrchestrationEvent_RunResumedAfterApproval_{
			RunResumedAfterApproval: &mpv1.OrchestrationEvent_RunResumedAfterApproval{
				RunId:      runID,
				ApprovalId: approvalID,
			},
		},
	}
}

// EventTypeOf returns the canonical event_type string for an OrchestrationEvent.
// Returns empty string when the variant is unset or unknown.
func EventTypeOf(ev *mpv1.OrchestrationEvent) string {
	if ev == nil {
		return ""
	}
	switch ev.Event.(type) {
	case *mpv1.OrchestrationEvent_PlanTransitioned_:
		return EventTypePlanTransitioned
	case *mpv1.OrchestrationEvent_TodoTransitioned_:
		return EventTypeTodoTransitioned
	case *mpv1.OrchestrationEvent_ApprovalStateChanged_:
		return EventTypeApprovalStateChanged
	case *mpv1.OrchestrationEvent_SubagentAttached_:
		return EventTypeSubagentAttached
	case *mpv1.OrchestrationEvent_SubagentStopped_:
		return EventTypeSubagentStopped
	case *mpv1.OrchestrationEvent_RunPausedForApproval_:
		return EventTypeRunPausedForApproval
	case *mpv1.OrchestrationEvent_RunResumedAfterApproval_:
		return EventTypeRunResumedAfterApproval
	default:
		return ""
	}
}

// SubjectOf returns the NATS subject for an OrchestrationEvent.
// Returns empty string when the variant is unset or unknown.
func SubjectOf(ev *mpv1.OrchestrationEvent) string {
	if ev == nil {
		return ""
	}
	switch ev.Event.(type) {
	case *mpv1.OrchestrationEvent_PlanTransitioned_:
		return SubjectPlan
	case *mpv1.OrchestrationEvent_TodoTransitioned_:
		return SubjectTodo
	case *mpv1.OrchestrationEvent_ApprovalStateChanged_:
		return SubjectApproval
	case *mpv1.OrchestrationEvent_SubagentAttached_,
		*mpv1.OrchestrationEvent_SubagentStopped_:
		return SubjectSubagent
	case *mpv1.OrchestrationEvent_RunPausedForApproval_,
		*mpv1.OrchestrationEvent_RunResumedAfterApproval_:
		return SubjectRun
	default:
		return ""
	}
}
