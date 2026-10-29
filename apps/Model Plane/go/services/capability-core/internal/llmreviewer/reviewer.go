// Package llmreviewer is the concrete learning.Reviewer (matrix §G7): it calls
// inference-core's InferenceCore.Infer with the review prompt + session
// transcript and parses the model's reply into skill candidates via
// learning.ParseReviewResponse. The learning package stays pure (no gRPC, no
// model); this adapter bridges it to inference-core — mirroring how
// internal/skillsink bridges the Sink side to session-core.
//
// Scope: this is the full LLM-call glue and is unit-tested against a fake
// inferer (no live model needed). It is deliberately decoupled from the
// trigger — *when* a review runs (e.g. orchestrator-core's Temporal session-end
// activity) is a scheduling concern owned elsewhere; this owns only the call.
// So G7's reviewer half is complete in logic; only the live-LLM e2e and the
// trigger registration remain (the latter needs Temporal, absent here).
package llmreviewer

import (
	"context"
	"fmt"
	"strings"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"google.golang.org/grpc"
)

// DefaultModel is used when NewReviewer is given an empty model, so the model
// choice isn't pinned in code — the caller (config/env) overrides it.
const DefaultModel = "claude-sonnet-4-20250514"

// reviewMaxTokens caps the review reply. Skill candidates are short JSON; this
// is generous without inviting a runaway generation.
const reviewMaxTokens = 2048

// inferer is the single method of the inference-core client this reviewer
// needs. Interface segregation keeps it unit-testable with a tiny fake and
// avoids depending on the full generated client surface (same pattern as
// skillsink.skillUpserter). The generated *mpv1.InferenceCoreClient satisfies
// it structurally.
type inferer interface {
	Infer(ctx context.Context, in *mpv1.InferRequest, opts ...grpc.CallOption) (*mpv1.InferResponse, error)
}

// Reviewer implements learning.Reviewer by calling inference-core. It is
// org-scoped: it carries the org_id (the Reviewer interface doesn't pass one),
// supplied when the learning loop runs for a given session — the same scoping
// pattern as skillsink.SessionCoreSink.
type Reviewer struct {
	client inferer
	model  string
	orgID  string
}

// NewReviewer builds an inference-backed reviewer for one org. An empty model
// falls back to DefaultModel.
func NewReviewer(client inferer, model, orgID string) *Reviewer {
	if strings.TrimSpace(model) == "" {
		model = DefaultModel
	}
	return &Reviewer{client: client, model: model, orgID: orgID}
}

// Review calls inference-core with the review prompt (system) + the transcript
// and existing-skill list (user), then parses the reply. The reply is UNTRUSTED
// model text; learning.ParseReviewResponse tolerates fences/prose and forces
// Origin=background_review, so a hostile reply can never mint a "user" skill.
func (r *Reviewer) Review(
	ctx context.Context,
	transcript string,
	existing []learning.ExistingSkill,
	prompt string,
) ([]learning.SkillCandidate, error) {
	if r.client == nil {
		return nil, fmt.Errorf("llmreviewer: inference client is nil")
	}
	req := &mpv1.InferRequest{
		OrgId: r.orgID,
		Model: r.model,
		Messages: []*mpv1.ChatMessage{
			{Role: "system", Content: prompt},
			{Role: "user", Content: buildUserMessage(transcript, existing)},
		},
		// Deterministic extraction — this is structured parsing, not creative.
		Temperature: 0,
		MaxTokens:   reviewMaxTokens,
		// Don't durably cache/log the transcript — it may carry sensitive data.
		Zdr: true,
	}
	resp, err := r.client.Infer(ctx, req)
	if err != nil {
		return nil, fmt.Errorf("llmreviewer: infer call: %w", err)
	}
	if resp == nil || strings.TrimSpace(resp.GetContent()) == "" {
		return nil, fmt.Errorf("llmreviewer: empty inference response")
	}
	return learning.ParseReviewResponse(resp.GetContent())
}

// buildUserMessage assembles the transcript plus the names of already-registered
// skills so the model avoids re-proposing them. This is only a hint — the real
// dedup/provenance enforcement is server-side (learning.SelectForPersistence +
// session-core's ON CONFLICT guard); the prompt hint just reduces churn.
func buildUserMessage(transcript string, existing []learning.ExistingSkill) string {
	var b strings.Builder
	b.WriteString("SESSION TRANSCRIPT:\n")
	b.WriteString(transcript)
	if len(existing) > 0 {
		b.WriteString("\n\nALREADY-REGISTERED SKILLS (do not re-propose these):\n")
		for _, e := range existing {
			b.WriteString("- ")
			b.WriteString(e.Name)
			b.WriteString("\n")
		}
	}
	return b.String()
}
