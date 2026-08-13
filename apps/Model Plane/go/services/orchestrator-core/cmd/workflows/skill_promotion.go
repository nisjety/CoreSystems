package workflows

import (
	"errors"
	"fmt"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// ErrRegistryUpdateNotSupported is returned by SkillPromotionWorkflow's step 3
// (see its doc comment). capability-core's PromoteSkill RPC — the only thing
// that could durably persist a scope change — was removed per
// QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md SKILL-2: it always failed in
// production, and even a working version would have mutated Capability.Scope,
// a rollout/routing label capability-core's policy engine never reads when
// deciding whether a capability may run. Broadening a capability's actual
// reach is capability-core's ScopeStore grant API, which this workflow does
// not call. Exported so a caller (e.g. FeedbackPromotionWorkflow, or a direct
// StartWorkflow caller) can match on it with errors.Is rather than string
// matching.
var ErrRegistryUpdateNotSupported = errors.New(
	"orchestrator-core: durable skill-scope registry update is not supported; " +
		"capability-core's PromoteSkill RPC was removed (SKILL-2) because " +
		"Capability.Scope has no runtime-authorization effect — see " +
		"capability-core's PromoteSkill doc comment and ScopeStore.Grant",
)

// SkillPromotionInput is the input for the skill promotion workflow.
type SkillPromotionInput struct {
	SkillID   string `json:"skill_id"`
	FromScope string `json:"from_scope"`
	ToScope   string `json:"to_scope"`
	// OrgID is the tenant the promotion belongs to.
	//
	// capability-core's promotion RPCs carry no org_id of their own — a skill id
	// and two scopes is the whole message — so the tenant reaches capability-core
	// only through the caller's credential. The activities need it to mint an
	// org-bound token; without it every promotion RPC is Unauthenticated.
	OrgID string `json:"org_id"`
}

// SkillPromotionOutput contains the result of the promotion process.
type SkillPromotionOutput struct {
	Promoted bool     `json:"promoted"`
	Checks   []string `json:"checks"`
	Reason   string   `json:"reason"`
}

// SkillPromotionWorkflow validates and gate-checks a proposed skill-scope
// promotion via capability-core.
//
// Steps:
//  1. Validate the skill bundle via capability-core.
//  2. Run promotion gate checks.
//  3. Durably persist the new scope — NOT IMPLEMENTED. See
//     ErrRegistryUpdateNotSupported: capability-core has no working RPC for
//     this, and the field this workflow used to write has no effect on
//     capability availability anyway. Steps 1-2 still run for real, so a
//     caller learns whether the skill WOULD be eligible; step 3 refuses to
//     claim an action it cannot perform, immediately and without retrying.
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
		OrgID:   input.OrgID,
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
		OrgID:     input.OrgID,
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

	// Step 3: durably persist the promotion — deliberately not attempted.
	// See ErrRegistryUpdateNotSupported and the function doc comment: no
	// activity is invoked here (there is nothing left that could succeed),
	// so this fails immediately instead of burning the 3-attempt exponential
	// backoff (~7s minimum) UpdateRegistryActivity used to retry against an
	// RPC that always failed.
	logger.Error("SkillPromotionWorkflow: registry update not supported",
		"skill_id", input.SkillID,
		"from", input.FromScope,
		"to", input.ToScope,
		"gate_checks", gateResult.Checks,
	)
	return SkillPromotionOutput{
		Promoted: false,
		Checks:   gateResult.Checks,
		Reason:   fmt.Sprintf("gate passed but registry update is not supported: %v", ErrRegistryUpdateNotSupported),
	}, fmt.Errorf("update registry: %w", ErrRegistryUpdateNotSupported)
}
