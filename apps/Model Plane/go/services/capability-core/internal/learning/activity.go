package learning

import (
	"context"
	"fmt"
	"strings"
)

// Reviewer abstracts the LLM review call. The production implementation
// (orchestrator-core's Temporal activity) calls inference-core with
// [ReviewPrompt]; tests inject a fake. Kept as an interface so the
// learning-loop control flow is testable without a model.
type Reviewer interface {
	// Review returns the skill candidates the model proposes from a session
	// transcript, given the skills already registered.
	Review(ctx context.Context, transcript string, existing []ExistingSkill, prompt string) ([]SkillCandidate, error)
}

// Sink abstracts persisting accepted candidates into capability-core's skills
// registry. The production implementation writes `models.Capability` rows
// (Kind="skill"); tests inject a fake. Per matrix §4.5 the registry is the
// single skills store — this is a write-through, not a parallel store.
type Sink interface {
	// Persist stores the accepted candidates and returns how many were written.
	Persist(ctx context.Context, skills []SkillCandidate) (int, error)
}

// RunReview is the post-session learning-loop control flow (matrix §G7):
// review the transcript, apply the provenance-aware retention policy
// ([SelectForPersistence]), and persist the survivors. It is pure control
// flow over injected dependencies — the Temporal activity in orchestrator-core
// supplies the concrete [Reviewer] (inference-core) and [Sink] (registry), so
// this logic is unit-tested here without a model, workflow engine, or DB.
//
// Returns the number of skills persisted. A session that yields nothing
// retainable persists zero — that is a valid, common outcome, not an error.
func RunReview(
	ctx context.Context,
	transcript string,
	existing []ExistingSkill,
	reviewer Reviewer,
	sink Sink,
) (int, error) {
	if strings.TrimSpace(transcript) == "" {
		return 0, nil // nothing to learn from
	}
	if reviewer == nil || sink == nil {
		return 0, fmt.Errorf("learning: reviewer and sink are required")
	}

	candidates, err := reviewer.Review(ctx, transcript, existing, ReviewPrompt)
	if err != nil {
		return 0, fmt.Errorf("learning: skill review failed: %w", err)
	}

	selected := SelectForPersistence(candidates, existing)
	if len(selected) == 0 {
		return 0, nil // reviewed, but nothing cleared the retention policy
	}

	persisted, err := sink.Persist(ctx, selected)
	if err != nil {
		return 0, fmt.Errorf("learning: persist %d skill(s): %w", len(selected), err)
	}
	return persisted, nil
}
