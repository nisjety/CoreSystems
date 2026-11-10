// Package workflows contains Temporal workflow definitions for orchestrator-core.
package workflows

import (
	"fmt"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// InteractiveRunInput is the input for the interactive run supervision workflow.
//
// Retention is server-owned like OrgID/UserID: the start handler overwrites
// whatever the client sent with the caller's signed posture, so a client cannot
// claim durability for a Zero Data Retention run. It is threaded to the
// lifecycle activities so RUN_COMPLETED / RUN_FAILED can declare the posture
// instead of dropping it.
type InteractiveRunInput struct {
	RunID     string               `json:"run_id"`
	ThreadID  string               `json:"thread_id"`
	Goal      string               `json:"goal"`
	Policy    string               `json:"policy"`
	OrgID     string               `json:"org_id"`
	UserID    string               `json:"user_id"`
	Retention activities.Retention `json:"retention"`
}

// ScheduledRunInput is the non-secret handoff from Capability Core after it
// consumed Control's one-fire preparation decision with Session Core. The
// immutable owner/schedule facts let the first activity request fresh execution
// authority; no Control bearer is ever persisted in Temporal history.
type ScheduledRunInput struct {
	RunID          string `json:"run_id"`
	ThreadID       string `json:"thread_id"`
	Goal           string `json:"goal"`
	Policy         string `json:"policy"`
	OrgID          string `json:"org_id"`
	SpaceRef       string `json:"space_ref"`
	SubjectID      string `json:"subject_id"`
	ScheduleID     string `json:"schedule_id"`
	FireKey        string `json:"fire_key"`
	TemplateDigest string `json:"template_digest"`
	PolicyDigest   string `json:"policy_digest"`
	IdempotencyKey string `json:"idempotency_key"`
	// The stored-consent contract the digest-verified template commits to (WS20
	// S5). All three are empty for a schedule without a contract.
	ContractJSON      string               `json:"contract_json,omitempty"`
	ContractDigest    string               `json:"contract_digest,omitempty"`
	ContractExpiresAt string               `json:"contract_expires_at,omitempty"`
	Retention         activities.Retention `json:"retention"`
	// GoalID is the thread goal a goal-bound schedule continues (EP-7 S6), read from the
	// digest-verified template. Set only while ORCHESTRATOR_GOAL_JUDGE_ACTIVITY_ENABLED is on;
	// empty, the fire is not judged and its history is what it was before the field existed.
	GoalID string `json:"goal_id,omitempty"`
}

// GoalJudgeTurnInput is the input of JudgeGoalTurnActivity (EP-7 S6). The type lives in the
// activities package, which this package already imports, so the workflow and the activity share
// one definition. It carries ids only: Temporal history must never hold the objective, the criteria
// or the answer, so the judge reads them itself from their owners.
type GoalJudgeTurnInput = activities.GoalJudgeTurnInput

const (
	// SignalApproval is sent to approve a pending human-approval step.
	SignalApproval = "approval"
	// SignalCancel is sent to cancel the run gracefully.
	SignalCancel = "cancel"

	defaultMaxTurns = 10

	// scheduledStepLaneChange keeps already-running scheduled workflows
	// replayable while new fires use the dedicated service-owned step activity.
	scheduledStepLaneChange  = "scheduled-step-lane"
	scheduledStepLaneVersion = 1

	// scheduledAgentLaneChange gates the bounded agent loop for a fire under a stored-consent
	// contract (WS20 S6). Histories that predate it keep the step loops below.
	scheduledAgentLaneChange  = "scheduled-agent-lane"
	scheduledAgentLaneVersion = 1

	// goalJudgeTurnChange gates the judge of a goal-bound fire (EP-7 S6). Histories that predate it
	// complete without the activity.
	goalJudgeTurnChange  = "goal-judge-turn"
	goalJudgeTurnVersion = 1
)

// InteractiveRunSupervision orchestrates an interactive agent run.
//
// Steps:
//  1. Start the run in session-core.
//  2. Execute the step loop in execution-core (respecting policy max turns).
//  3. Handle approval/cancel signals from the user.
//  4. On completion, mark the run as completed.
//  5. On failure, record the failure with compensation.
func InteractiveRunSupervision(ctx workflow.Context, input InteractiveRunInput) error {
	logger := workflow.GetLogger(ctx)
	logger.Info("InteractiveRunSupervision started",
		"run_id", input.RunID,
		"thread_id", input.ThreadID,
		"goal", input.Goal,
	)

	activityOpts := workflow.ActivityOptions{
		StartToCloseTimeout: 2 * time.Minute,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumAttempts:    3,
		},
	}
	actCtx := workflow.WithActivityOptions(ctx, activityOpts)

	// Track cancellation signal.
	cancelled := false
	cancelCh := workflow.GetSignalChannel(ctx, SignalCancel)
	workflow.Go(ctx, func(gCtx workflow.Context) {
		cancelCh.Receive(gCtx, nil)
		cancelled = true
		logger.Info("cancel signal received", "run_id", input.RunID)
	})

	// Step 1: Start the run in session-core.
	var runMeta activities.RunMetadata
	err := workflow.ExecuteActivity(actCtx,
		"StartRunActivity",
		input.RunID, input.ThreadID, input.OrgID, input.UserID,
	).Get(ctx, &runMeta)
	if err != nil {
		return handleFailure(ctx, input.RunID, input.OrgID, input.UserID, input.Retention, fmt.Sprintf("start run: %v", err))
	}

	if cancelled {
		return handleFailure(ctx, input.RunID, input.OrgID, input.UserID, input.Retention, "cancelled before step loop")
	}

	// Step 2: Execute step loop.
	stepInput := activities.StepLoopInput{
		RunID:    input.RunID,
		ThreadID: input.ThreadID,
		Goal:     input.Goal,
		Policy:   input.Policy,
		MaxTurns: defaultMaxTurns,
		OrgID:    input.OrgID,
		UserID:   input.UserID,
	}

	stepOutput, err := runStepLoop(actCtx, stepInput)
	if err != nil {
		return handleFailure(ctx, input.RunID, input.OrgID, input.UserID, input.Retention, fmt.Sprintf("step loop: %v", err))
	}

	// Step 3: If approval is needed, block until the approval signal arrives.
	if len(stepOutput.Steps) > 0 && stepOutput.Steps[len(stepOutput.Steps)-1].NeedsApproval {
		logger.Info("waiting for human approval", "run_id", input.RunID)

		approvalCh := workflow.GetSignalChannel(ctx, SignalApproval)
		selector := workflow.NewSelector(ctx)

		approved := false
		selector.AddReceive(approvalCh, func(c workflow.ReceiveChannel, more bool) {
			c.Receive(ctx, nil)
			approved = true
		})

		selector.Select(ctx)

		if cancelled || !approved {
			return handleFailure(ctx, input.RunID, input.OrgID, input.UserID, input.Retention, "cancelled during approval wait")
		}

		// Resume step loop after approval.
		stepOutput, err = runStepLoop(actCtx, stepInput)
		if err != nil {
			return handleFailure(ctx, input.RunID, input.OrgID, input.UserID, input.Retention, fmt.Sprintf("resumed step loop: %v", err))
		}
	}

	if cancelled {
		return handleFailure(ctx, input.RunID, input.OrgID, input.UserID, input.Retention, "cancelled after step loop")
	}

	// Step 4: Complete the run. The retention posture rides along so the
	// RUN_COMPLETED envelope can declare it — a ZDR run's envelope is then
	// suppressed outright, and the summary never leaves this workflow unless an
	// issuer explicitly attested the run is retainable.
	completionInput := activities.CompletionInput{
		RunID:     input.RunID,
		OrgID:     input.OrgID,
		UserID:    input.UserID,
		Summary:   stepOutput.Summary,
		Retention: input.Retention,
	}
	err = workflow.ExecuteActivity(actCtx,
		"CompleteRunActivity",
		completionInput,
	).Get(ctx, nil)
	if err != nil {
		return handleFailure(ctx, input.RunID, input.OrgID, input.UserID, input.Retention, fmt.Sprintf("complete run: %v", err))
	}

	logger.Info("InteractiveRunSupervision completed", "run_id", input.RunID)
	return nil
}

// ScheduledRunSupervision is the one owner-only lane for a Space cron fire.
// The short-lived Control decision is deliberately absent: Capability Core used
// it only to prepare the exact service-owned thread. This workflow receives
// that non-secret result and asks Session Core's dedicated RPC to create/reuse
// the deterministic task-backed run before it executes the ordinary step loop.
func ScheduledRunSupervision(ctx workflow.Context, input ScheduledRunInput) error {
	logger := workflow.GetLogger(ctx)
	logger.Info("ScheduledRunSupervision started",
		"run_id", input.RunID,
		"thread_id", input.ThreadID,
		"schedule_id", input.ScheduleID,
		"fire_key", input.FireKey,
	)

	activityOpts := workflow.ActivityOptions{
		StartToCloseTimeout: 2 * time.Minute,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumAttempts:    3,
		},
	}
	actCtx := workflow.WithActivityOptions(ctx, activityOpts)

	cancelled := false
	cancelCh := workflow.GetSignalChannel(ctx, SignalCancel)
	workflow.Go(ctx, func(gCtx workflow.Context) {
		cancelCh.Receive(gCtx, nil)
		cancelled = true
		logger.Info("cancel signal received", "run_id", input.RunID)
	})

	var runMeta activities.RunMetadata
	err := workflow.ExecuteActivity(actCtx,
		"StartScheduledRunActivity",
		input.RunID, input.ThreadID, input.OrgID, input.SpaceRef, input.SubjectID,
		input.ScheduleID, input.FireKey, input.TemplateDigest, input.IdempotencyKey, input.Goal, "execute",
		input.ContractJSON, input.ContractDigest, input.ContractExpiresAt,
	).Get(ctx, &runMeta)
	if err != nil {
		return handleFailure(ctx, input.RunID, input.OrgID, "", input.Retention, fmt.Sprintf("start scheduled run: %v", err))
	}
	if cancelled {
		return handleFailure(ctx, input.RunID, input.OrgID, "", input.Retention, "cancelled before step loop")
	}

	var stepOutput activities.StepLoopOutput
	// The agent lane is decided once, from the answer the start recorded. A fire without a
	// contract never takes it, and a history that predates it replays with the marker absent.
	agentLane := input.ContractJSON != "" && runMeta.AgentLane &&
		workflow.GetVersion(ctx, scheduledAgentLaneChange, workflow.DefaultVersion, scheduledAgentLaneVersion) == scheduledAgentLaneVersion
	switch {
	case agentLane:
		stepOutput, err = executeScheduledAgentStep(actCtx, input)
	case workflow.GetVersion(ctx, scheduledStepLaneChange, workflow.DefaultVersion, scheduledStepLaneVersion) == workflow.DefaultVersion:
		// Replay-only compatibility for histories created before the dedicated
		// service-owned lane existed. New fires must never take this branch.
		stepOutput, err = runStepLoop(actCtx, activities.StepLoopInput{
			RunID: input.RunID, ThreadID: input.ThreadID, Goal: input.Goal,
			Policy: input.Policy, MaxTurns: defaultMaxTurns, OrgID: input.OrgID,
		})
	default:
		stepOutput, err = executeScheduledStepLoop(actCtx, input)
	}
	if err != nil {
		return handleFailure(ctx, input.RunID, input.OrgID, "", input.Retention, fmt.Sprintf("step loop: %v", err))
	}
	if len(stepOutput.Steps) > 0 && stepOutput.Steps[len(stepOutput.Steps)-1].NeedsApproval {
		return handleFailure(ctx, input.RunID, input.OrgID, "", input.Retention, "scheduled runs cannot continue without a separately authorized approval")
	}
	if cancelled {
		return handleFailure(ctx, input.RunID, input.OrgID, "", input.Retention, "cancelled")
	}
	summary := stepOutput.Summary
	if agentLane {
		summary = scheduledAgentOutcomeSummary(input.RunID, stepOutput)
	}
	completionInput := activities.CompletionInput{
		RunID:     input.RunID,
		OrgID:     input.OrgID,
		Summary:   summary,
		Retention: input.Retention,
	}
	if err := workflow.ExecuteActivity(actCtx, "CompleteRunActivity", completionInput).Get(ctx, nil); err != nil {
		return handleFailure(ctx, input.RunID, input.OrgID, "", input.Retention, fmt.Sprintf("complete scheduled run: %v", err))
	}
	if input.GoalID != "" &&
		workflow.GetVersion(ctx, goalJudgeTurnChange, workflow.DefaultVersion, goalJudgeTurnVersion) == goalJudgeTurnVersion {
		judgeGoalTurn(ctx, actCtx, input)
	}
	logger.Info("ScheduledRunSupervision completed", "run_id", input.RunID)
	return nil
}

// judgeGoalTurn asks for the verdict on the goal turn a completed fire spent (EP-7 S6). The run is
// already complete and its turn already ledgered, so a judge that fails only leaves the turn
// unjudged: the goal's caps still stop it, and the next fire is judged afresh.
func judgeGoalTurn(ctx, actCtx workflow.Context, input ScheduledRunInput) {
	judge := GoalJudgeTurnInput{
		GoalID: input.GoalID, RunID: input.RunID, OrgID: input.OrgID,
		ScheduleID: input.ScheduleID, FireKey: input.FireKey,
	}
	if err := workflow.ExecuteActivity(actCtx, "JudgeGoalTurnActivity", judge).Get(ctx, nil); err != nil {
		workflow.GetLogger(ctx).Warn("goal turn not judged", "run_id", input.RunID, "error", err)
	}
}

// handleFailure records run failure via FailRunActivity and returns the error.
//
// The compensation activity runs on a disconnected context so it executes
// even when the workflow context has already been cancelled (e.g. native
// Temporal workflow cancellation propagated down from the parent). This is
// the standard SAGA compensation pattern: reversal work must not inherit
// the cancellation that triggered it.
func handleFailure(ctx workflow.Context, runID, orgID, userID string, retention activities.Retention, reason string) error {
	logger := workflow.GetLogger(ctx)
	logger.Error("run failed", "run_id", runID, "reason", reason)

	failInput := activities.FailureInput{
		RunID:     runID,
		OrgID:     orgID,
		UserID:    userID,
		Reason:    reason,
		Retention: retention,
	}

	// Use a disconnected context so compensation runs even when the outer
	// context has been cancelled (native Temporal cancellation). For plain
	// activity-failure paths the context is still active; either way the
	// FailRunActivity must execute.
	disconnectedCtx, _ := workflow.NewDisconnectedContext(ctx)
	disconnectedCtx = workflow.WithActivityOptions(disconnectedCtx, workflow.ActivityOptions{
		StartToCloseTimeout: 2 * time.Minute,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumAttempts:    3,
		},
	})

	_ = workflow.ExecuteActivity(disconnectedCtx,
		"FailRunActivity",
		failInput,
	).Get(disconnectedCtx, nil)

	return fmt.Errorf("interactive run %s failed: %s", runID, reason)
}
