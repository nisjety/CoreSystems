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
// [RunReviewOutcome] returns the full [ReviewOutcome] instead.
func RunReview(
	ctx context.Context,
	transcript string,
	existing []ExistingSkill,
	reviewer Reviewer,
	sink Sink,
) (int, error) {
	outcome, err := RunReviewOutcome(ctx, transcript, existing, reviewer, sink)
	return outcome.Persisted, err
}

// SkipReason says why a review never reached the model. Empty means it did.
type SkipReason string

const (
	// SkipEmptyTranscript: the session left no conversation text to review.
	SkipEmptyTranscript SkipReason = "empty_transcript"
	// SkipConversationScope: the thread runs in conversation-only scope, whose
	// content stays out of workspace learning, so it is never reviewed.
	SkipConversationScope SkipReason = "conversation_scope"
)

// ReviewOutcome is what one review did, in counts only — never content — so a
// caller can report a skipped review apart from a completed one, and a
// completed one by how far its candidates got.
type ReviewOutcome struct {
	Skipped    SkipReason
	Candidates int // proposed by the reviewer
	Selected   int // cleared SelectForPersistence
	Persisted  int // written by the sink
}

// RunReviewOutcome is [RunReview] returning the full [ReviewOutcome]. On error
// the outcome keeps the counts reached before the failure, and Persisted is 0.
func RunReviewOutcome(
	ctx context.Context,
	transcript string,
	existing []ExistingSkill,
	reviewer Reviewer,
	sink Sink,
) (ReviewOutcome, error) {
	if strings.TrimSpace(transcript) == "" {
		return ReviewOutcome{Skipped: SkipEmptyTranscript}, nil // nothing to learn from
	}
	if reviewer == nil || sink == nil {
		return ReviewOutcome{}, fmt.Errorf("learning: reviewer and sink are required")
	}

	candidates, err := reviewer.Review(ctx, transcript, existing, ReviewPrompt)
	if err != nil {
		return ReviewOutcome{}, fmt.Errorf("learning: skill review failed: %w", err)
	}
	outcome := ReviewOutcome{Candidates: len(candidates)}

	selected := SelectForPersistence(candidates, existing)
	outcome.Selected = len(selected)
	if len(selected) == 0 {
		return outcome, nil // reviewed, but nothing cleared the retention policy
	}

	persisted, err := sink.Persist(ctx, selected)
	if err != nil {
		return outcome, fmt.Errorf("learning: persist %d skill(s): %w", len(selected), err)
	}
	outcome.Persisted = persisted
	return outcome, nil
}
