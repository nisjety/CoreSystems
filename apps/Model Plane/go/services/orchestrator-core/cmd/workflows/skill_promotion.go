package workflows

import (
	"fmt"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// SkillPromotionInput is the input for the skill promotion workflow.
type SkillPromotionInput struct {
	SkillID   string `json:"skill_id"`
	FromScope string `json:"from_scope"`
	ToScope   string `json:"to_scope"`
}

// SkillPromotionOutput contains the result of the promotion process.
type SkillPromotionOutput struct {
	Promoted bool     `json:"promoted"`
	Checks   []string `json:"checks"`
	Reason   string   `json:"reason"`
}

// SkillPromotionWorkflow promotes a skill bundle from one scope to another.
//
// Steps:
//  1. Validate the skill bundle via capability-core.
//  2. Run promotion gate checks.
//  3. Update the registry with the new scope.
func SkillPromotionWorkflow(ctx workflow.Context, input SkillPromotionInput) (SkillPromotionOutput, error) {
	logger := workflow.GetLogger(ctx)
	logger.Info("SkillPromotionWorkflow started",
		"skill_id", input.SkillID,
		"from", input.FromScope,
		"to", input.ToScope,
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

	// Step 1: Validate skill bundle.
	validationInput := activities.SkillValidationInput{
		SkillID: input.SkillID,
	}

	var validationResult activities.SkillValidationOutput
	err := workflow.ExecuteActivity(actCtx,
		"ValidateSkillBundleActivity",
		validationInput,
	).Get(ctx, &validationResult)
	if err != nil {
		return SkillPromotionOutput{
			Promoted: false,
			Reason:   fmt.Sprintf("validation activity failed: %v", err),
		}, fmt.Errorf("validate skill bundle: %w", err)
	}

	if !validationResult.Valid {
		return SkillPromotionOutput{
			Promoted: false,
			Reason:   fmt.Sprintf("skill validation failed: %v", validationResult.Errors),
		}, nil
	}

	// Step 2: Run promotion gate checks.
	gateInput := activities.PromotionGateInput{
		SkillID:   input.SkillID,
		FromScope: input.FromScope,
		ToScope:   input.ToScope,
	}

	var gateResult activities.PromotionGateOutput
	err = workflow.ExecuteActivity(actCtx,
		"RunPromotionGateActivity",
		gateInput,
	).Get(ctx, &gateResult)
	if err != nil {
		return SkillPromotionOutput{
			Promoted: false,
			Reason:   fmt.Sprintf("promotion gate activity failed: %v", err),
		}, fmt.Errorf("promotion gate check: %w", err)
	}

	if !gateResult.Passed {
		return SkillPromotionOutput{
			Promoted: false,
			Checks:   gateResult.Checks,
			Reason:   "promotion gate checks failed",
		}, nil
	}

	// Step 3: Update registry.
	registryInput := activities.RegistryUpdateInput{
		SkillID:   input.SkillID,
		FromScope: input.FromScope,
		NewScope:  input.ToScope,
	}

	err = workflow.ExecuteActivity(actCtx,
		"UpdateRegistryActivity",
		registryInput,
	).Get(ctx, nil)
	if err != nil {
		return SkillPromotionOutput{
			Promoted: false,
			Checks:   gateResult.Checks,
			Reason:   fmt.Sprintf("registry update failed: %v", err),
		}, fmt.Errorf("update registry: %w", err)
	}

	logger.Info("SkillPromotionWorkflow completed",
		"skill_id", input.SkillID,
		"to", input.ToScope,
	)

	return SkillPromotionOutput{
		Promoted: true,
		Checks:   gateResult.Checks,
		Reason:   fmt.Sprintf("promoted from %s to %s", input.FromScope, input.ToScope),
	}, nil
}
