package workflows

import (
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// FeedbackPromotionInput configures one feedback-driven promotion sweep.
type FeedbackPromotionInput struct {
	// Minimum ratings a skill needs before it's eligible. 0 → activity default.
	MinSamples int `json:"min_samples"`
	// Good-ratio required to promote (0..1). 0 → activity default.
	PromoteThreshold float64 `json:"promote_threshold"`
}

// FeedbackPromotionOutput summarizes the sweep.
type FeedbackPromotionOutput struct {
	Evaluated int      `json:"evaluated"`
	Promoted  []string `json:"promoted"`
	Skipped   []string `json:"skipped"`
}

// FeedbackPromotionWorkflow closes the feedback → skill-promotion loop
// (HARNESS_PHASE1 §6). It is the nightly "D5" job:
//
//  1. AggregateFeedbackActivity reads accumulated operator ratings and returns
//     the skills whose good-ratio crossed the promotion bar.
//  2. For each candidate it runs SkillPromotionWorkflow as a child workflow
//     (validate → gate → registry update), so the existing promotion gates
//     still apply — feedback only *nominates*, the gates still *decide*.
//
// Operator ratings (good/acceptable/poor) thus drive which skills graduate
// scope, with the human-in-the-loop signal as the trigger and the capability
// gates as the guardrail.
func FeedbackPromotionWorkflow(ctx workflow.Context, input FeedbackPromotionInput) (FeedbackPromotionOutput, error) {
	logger := workflow.GetLogger(ctx)
	logger.Info("FeedbackPromotionWorkflow started",
		"min_samples", input.MinSamples,
		"threshold", input.PromoteThreshold,
	)

	activityOpts := workflow.ActivityOptions{
		StartToCloseTimeout: 1 * time.Minute,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumAttempts:    3,
		},
	}
	actCtx := workflow.WithActivityOptions(ctx, activityOpts)

	var agg activities.FeedbackAggregateOutput
	if err := workflow.ExecuteActivity(actCtx,
		"AggregateFeedbackActivity",
		activities.FeedbackAggregateInput{
			MinSamples:       input.MinSamples,
			PromoteThreshold: input.PromoteThreshold,
		},
	).Get(ctx, &agg); err != nil {
		return FeedbackPromotionOutput{}, err
	}

	out := FeedbackPromotionOutput{Evaluated: len(agg.Candidates)}

	for _, cand := range agg.Candidates {
		// Each promotion is its own child workflow so one failing gate doesn't
		// abort the sweep, and each shows up independently in Temporal history.
		childOpts := workflow.ChildWorkflowOptions{
			WorkflowID: workflow.GetInfo(ctx).WorkflowExecution.ID + ":promote:" + cand.SkillID,
		}
		childCtx := workflow.WithChildOptions(ctx, childOpts)

		var result SkillPromotionOutput
		err := workflow.ExecuteChildWorkflow(childCtx, SkillPromotionWorkflow, SkillPromotionInput{
			SkillID:   cand.SkillID,
			FromScope: cand.FromScope,
			ToScope:   cand.ToScope,
		}).Get(ctx, &result)

		if err == nil && result.Promoted {
			out.Promoted = append(out.Promoted, cand.SkillID)
		} else {
			out.Skipped = append(out.Skipped, cand.SkillID)
		}
	}

	logger.Info("FeedbackPromotionWorkflow completed",
		"evaluated", out.Evaluated,
		"promoted", len(out.Promoted),
		"skipped", len(out.Skipped),
	)
	return out, nil
}
