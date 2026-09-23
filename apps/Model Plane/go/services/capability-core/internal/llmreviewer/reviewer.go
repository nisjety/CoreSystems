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
const reviewMaxTokens = 4096

const reviewTool = "submit_skill_review"
const reviewSchema = `{"type":"object","properties":{"skills":{"type":"array","maxItems":8,"items":{"type":"object","properties":{"name":{"type":"string","minLength":1,"maxLength":160},"description":{"type":"string","maxLength":1000},"content":{"type":"string","minLength":1,"maxLength":8000},"trigger_keywords":{"type":"array","maxItems":12,"items":{"type":"string","maxLength":120}},"confidence":{"type":"number","minimum":0,"maximum":1}},"required":["name","description","content","trigger_keywords","confidence"],"additionalProperties":false}}},"required":["skills"],"additionalProperties":false}`

// ResponseError exposes bounded, content-free diagnostics and retry policy.
// Never log a transcript, generated skill, provider message or raw response.
type ResponseError struct {
	Kind         string
	Model        string
	StopReason   string
	OutputTokens int32
}

func (e *ResponseError) Error() string {
	return fmt.Sprintf("llmreviewer: %s (model=%s stop=%s output_tokens=%d)", e.Kind, e.Model, e.StopReason, e.OutputTokens)
}
func responseError(kind string, resp *mpv1.InferResponse) error {
	// Only recognized metadata is reported; model IDs are bounded, not body text.
	model := resp.GetModelUsed()
	if len(model) > 100 {
		model = "unknown"
	}
	stop := resp.GetStopReason()
	switch stop {
	case "end_turn", "stop", "tool_use", "tool_calls", "max_tokens", "length", "refusal", "content_filter", "":
	default:
		stop = "unknown"
	}
	return &ResponseError{Kind: kind, Model: model, StopReason: stop, OutputTokens: resp.GetOutputTokens()}
}

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
		Tools:       []*mpv1.ToolDefinition{{Name: reviewTool, Description: "Submit the structured session review, or an empty skills array when nothing qualifies.", ParametersJson: reviewSchema}},
		ToolChoice:  reviewTool,
		// Not ZDR: HandleRunCompleted's retention gate (see
		// sessionreview.RetentionPosture.AllowsDerivedPersistence) already
		// refuses to reach this call at all unless the run's own envelope
		// declared an explicit `zdr: false` — i.e. the org/user already
		// consented to Verevon deriving and persisting artifacts from this
		// conversation. Requesting a ZDR-scoped inference call for it would
		// only add a requirement no deployment in most environments can
		// satisfy (inference-core's ZDR gate needs a real, evidence-bound
		// provider attestation — see rust/services/inference-core/src/
		// provider/zdr.rs) without protecting anything the upstream gate
		// hasn't already cleared.
		Zdr: false,
	}
	resp, err := r.client.Infer(ctx, req)
	if err != nil {
		return nil, fmt.Errorf("llmreviewer: infer call: %w", err)
	}
	if resp == nil {
		return nil, responseError("empty_response", resp)
	}
	switch resp.GetStopReason() {
	case "max_tokens", "length":
		return nil, responseError("truncated", resp)
	case "refusal", "content_filter":
		return nil, responseError("refused", resp)
	}
	raw := resp.GetContent()
	if calls := resp.GetToolCalls(); len(calls) > 0 {
		if len(calls) != 1 || calls[0].GetName() != reviewTool {
			return nil, responseError("unexpected_tool_response", resp)
		}
		raw = calls[0].GetArgumentsJson()
	}
	if strings.TrimSpace(raw) == "" {
		return nil, responseError("empty_response", resp)
	}
	candidates, err := learning.ParseReviewResponse(raw)
	if err != nil {
		return nil, responseError("invalid_schema", resp)
	}
	return candidates, nil
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
