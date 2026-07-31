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

const (
	// SignalApproval is sent to approve a pending human-approval step.
	SignalApproval = "approval"
	// SignalCancel is sent to cancel the run gracefully.
	SignalCancel = "cancel"

	defaultMaxTurns = 10
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
