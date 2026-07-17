package activities

import (
	"context"
	"fmt"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/evaloptimizer"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/telemetry"
)

// ── Per-leg activities for the durable evaluator-optimizer driver ────────────
//
// EvaluatorOptimizerWorkflow owns the loop control flow and dispatches each
// model call (generator leg, judge leg) as one InferModelActivity. Every leg
// is therefore a durable Temporal checkpoint: a worker crash replays completed
// legs from event history and resumes at the interrupted leg, instead of
// restarting the loop from round 0 (and re-spending its tokens).

// InferInput is one durable model-call leg. RunID and Seq are supplied by the
// workflow deterministically (Seq = round*2 for the generator leg, round*2+1
// for the judge leg), so activity RETRIES reuse the same InferRequest
// request_id and inference-core can dedupe/serve from its prompt cache instead
// of re-charging the provider.
type InferInput struct {
	RunID string
	Seq   int
	Req   evaloptimizer.InvokeRequest
}

// InferModelActivity performs a single model invocation against
// inference-core. ZDR and org scope arrive on the InvokeRequest and are
// carried through to the provider boundary by toInferRequest.
func (a *Activities) InferModelActivity(ctx context.Context, in InferInput) (evaloptimizer.InvokeResult, error) {
	if a.clients == nil || a.clients.InferenceCore == nil {
		return evaloptimizer.InvokeResult{}, status.Error(codes.Unavailable, "inference-core unavailable")
	}
	resp, err := mpv1.NewInferenceCoreClient(a.clients.InferenceCore).Infer(ctx, toInferRequest(in.Req, in.RunID, in.Seq))
	if err != nil {
		return evaloptimizer.InvokeResult{}, err
	}
	return evaloptimizer.InvokeResult{
		Content:      resp.GetContent(),
		InputTokens:  int(resp.GetInputTokens()),
		OutputTokens: int(resp.GetOutputTokens()),
		ModelUsed:    resp.GetModelUsed(),
	}, nil
}

// EvalRoundEvent is the per-round observability record the durable workflow
// emits after each generator→judge round completes.
type EvalRoundEvent struct {
	RunID    string
	OrgID    string
	UserID   string
	Round    int
	Passed   bool
	Score    float64
	Feedback string
}

// PublishEvalRoundActivity emits one round's outcome as a run event and counts
// the round. Publishing is best-effort (a nil publisher is a no-op); this
// activity never fails the loop over observability.
func (a *Activities) PublishEvalRoundActivity(ctx context.Context, ev EvalRoundEvent) error {
	a.publishRunEvent(ev.RunID, ev.OrgID, ev.UserID,
		"EVALUATOR_OPTIMIZER_ROUND",
		fmt.Sprintf("eval-opt-round-%d", ev.Round),
		map[string]any{
			"run_id":   ev.RunID,
			"round":    ev.Round,
			"passed":   ev.Passed,
			"score":    ev.Score,
			"feedback": ev.Feedback,
		})
	telemetry.EvaluatorOptimizerRoundsTotal.Add(ctx, 1)
	return nil
}

// EvalOutcomeEvent is the terminal record for one durable evaluator-optimizer
// run, making a did-not-pass outcome observable downstream.
type EvalOutcomeEvent struct {
	RunID       string
	OrgID       string
	UserID      string
	StopReason  string
	Passed      bool
	RoundsRun   int
	TotalTokens int
	BestScore   float64
}

// RecordEvalOutcomeActivity publishes the terminal outcome event and counts
// the run by stop reason.
func (a *Activities) RecordEvalOutcomeActivity(ctx context.Context, ev EvalOutcomeEvent) error {
	a.publishRunEvent(ev.RunID, ev.OrgID, ev.UserID,
		"EVALUATOR_OPTIMIZER_OUTCOME",
		"eval-opt-outcome",
		map[string]any{
			"run_id":       ev.RunID,
			"stop_reason":  ev.StopReason,
			"passed":       ev.Passed,
			"rounds_run":   ev.RoundsRun,
			"total_tokens": ev.TotalTokens,
			"best_score":   ev.BestScore,
		})
	telemetry.EvaluatorOptimizerRunsTotal.Add(ctx, 1, metric.WithAttributes(
		attribute.String("stop_reason", ev.StopReason),
		attribute.Bool("passed", ev.Passed),
	))
	return nil
}
