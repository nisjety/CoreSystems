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
	// OrgID is the tenant whose skills this sweep promotes. It is threaded to
	// each child SkillPromotionWorkflow because capability-core's promotion RPCs
	// carry no org of their own — see SkillPromotionInput.OrgID.
	OrgID string `json:"org_id"`
}

// FeedbackPromotionOutput summarizes the sweep.
type FeedbackPromotionOutput struct {
	Evaluated int      `json:"evaluated"`
	Promoted  []string `json:"promoted"`
	Skipped   []string `json:"skipped"`
	// Quarantined are skills the quality policy stopped injecting this sweep.
	Quarantined []string `json:"quarantined"`
	// QuarantineWithheld is how many further quarantines the per-sweep cap held
	// back. Surfaced in the output, not just the log: a non-zero value means more
	// skills were below the bar than one sweep may stop, which is the signature
	// of a systemic problem rather than a few bad skills.
	QuarantineWithheld int `json:"quarantine_withheld"`
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
			OrgID:     input.OrgID,
		}).Get(ctx, &result)

		if err == nil && result.Promoted {
			out.Promoted = append(out.Promoted, cand.SkillID)
		} else {
			out.Skipped = append(out.Skipped, cand.SkillID)
		}
	}

	// Demotion runs in the same sweep as promotion, AFTER it.
	//
	// Same sweep because the two read the same evidence and splitting them into
	// separate schedules would let a skill be promoted by one job while the other
	// was about to quarantine it. After, because promotion is the reversible,
	// lower-consequence half: if the demotion pass fails, the sweep still did
	// something useful rather than nothing.
	var quarantine activities.QuarantineSweepOutput
	if qerr := workflow.ExecuteActivity(actCtx, "QuarantineSweepActivity").Get(ctx, &quarantine); qerr != nil {
		// Never fatal to the workflow. Failing the whole sweep because the
		// demotion half could not reach session-core would also discard the
		// promotions that already succeeded.
		logger.Error("quarantine sweep failed", "error", qerr)
	} else if quarantine.Evaluated > 0 {
		logger.Info("quarantine sweep completed",
			"evaluated", quarantine.Evaluated,
			"quarantined", len(quarantine.Quarantined),
			"failed", len(quarantine.Failed),
			"withheld", quarantine.Withheld)
	}
	out.Quarantined = quarantine.Quarantined
	out.QuarantineWithheld = quarantine.Withheld

	logger.Info("FeedbackPromotionWorkflow completed",
		"evaluated", out.Evaluated,
		"promoted", len(out.Promoted),
		"skipped", len(out.Skipped),
	)
	return out, nil
}
