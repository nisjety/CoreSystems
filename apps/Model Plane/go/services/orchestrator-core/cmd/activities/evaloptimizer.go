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

// ── Type definitions ─────────────────────────────────────────────────────────

// EvalOptimizerInput configures one evaluator-optimizer run. It carries the run
// identity (for lifecycle events + tracing) plus the generator/judge loop
// parameters. ZDR and OrgID propagate to every model invocation.
type EvalOptimizerInput struct {
	RunID    string
	ThreadID string
	OrgID    string
	UserID   string

	GeneratorModel  string
	GeneratorSystem string
	Task            string

	JudgeModel  string
	JudgeSystem string
	Rubric      string

	MaxRounds           int
	PassThreshold       float64
	PerRoundTokenBudget int
	TotalTokenBudget    int
	GeneratorMaxTokens  int
	JudgeMaxTokens      int
	Temperature         float32

	ZDR bool
}

// ToConfig maps the activity/workflow input onto the pure loop config. It is
// exported so the durable workflow driver builds its config from the exact
// same mapping the in-process activity uses.
func (in EvalOptimizerInput) ToConfig() evaloptimizer.Config {
	return evaloptimizer.Config{
		GeneratorModel:      in.GeneratorModel,
		GeneratorSystem:     in.GeneratorSystem,
		Task:                in.Task,
		JudgeModel:          in.JudgeModel,
		JudgeSystem:         in.JudgeSystem,
		Rubric:              in.Rubric,
		MaxRounds:           in.MaxRounds,
		PassThreshold:       in.PassThreshold,
		PerRoundTokenBudget: in.PerRoundTokenBudget,
		TotalTokenBudget:    in.TotalTokenBudget,
		GeneratorMaxTokens:  in.GeneratorMaxTokens,
		JudgeMaxTokens:      in.JudgeMaxTokens,
		Temperature:         in.Temperature,
		ZDR:                 in.ZDR,
		OrgID:               in.OrgID,
	}
}

// EvalOptimizerRound is one generator→judge round in the returned transcript.
type EvalOptimizerRound struct {
	Round             int     `json:"round"`
	Answer            string  `json:"answer"`
	Passed            bool    `json:"passed"`
	Score             float64 `json:"score"`
	Feedback          string  `json:"feedback"`
	GenTokens         int     `json:"gen_tokens"`
	JudgeTokens       int     `json:"judge_tokens"`
	VerdictParseError string  `json:"verdict_parse_error,omitempty"`
}

// EvalOptimizerOutput is the activity result: the best attempt plus the full
// round transcript and an observable terminal reason.
type EvalOptimizerOutput struct {
	Passed      bool                 `json:"passed"`
	StopReason  string               `json:"stop_reason"`
	BestAnswer  string               `json:"best_answer"`
	BestScore   float64              `json:"best_score"`
	BestRound   int                  `json:"best_round"`
	RoundsRun   int                  `json:"rounds_run"`
	TotalTokens int                  `json:"total_tokens"`
	Rounds      []EvalOptimizerRound `json:"rounds"`
}

// ── gRPC-backed model invoker ────────────────────────────────────────────────

// inferenceInvoker adapts inference-core's Infer RPC to the pure
// evaloptimizer.ModelInvoker. It reuses the existing InferenceCore client
// wired through grpcclient rather than standing up a parallel model path.
type inferenceInvoker struct {
	client mpv1.InferenceCoreClient
	runID  string
	seq    int
}

func (iv *inferenceInvoker) Invoke(ctx context.Context, req evaloptimizer.InvokeRequest) (evaloptimizer.InvokeResult, error) {
	iv.seq++
	resp, err := iv.client.Infer(ctx, toInferRequest(req, iv.runID, iv.seq))
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

// toInferRequest maps a provider-agnostic InvokeRequest onto inference-core's
// InferRequest. It is a pure function so the field mapping — notably ZDR and
// org scope, which must reach the provider boundary intact — is unit-testable
// without a live gRPC server.
func toInferRequest(req evaloptimizer.InvokeRequest, runID string, seq int) *mpv1.InferRequest {
	msgs := make([]*mpv1.ChatMessage, 0, len(req.Messages))
	for _, m := range req.Messages {
		msgs = append(msgs, &mpv1.ChatMessage{Role: m.Role, Content: m.Content})
	}
	return &mpv1.InferRequest{
		RequestId:              fmt.Sprintf("%s-%s-%d", runID, req.Call, seq),
		OrgId:                  req.OrgID,
		Model:                  req.Model,
		Messages:               msgs,
		Temperature:            req.Temperature,
		MaxTokens:              int32(req.MaxTokens),
		StructuredOutputSchema: req.StructuredOutputSchema,
		Zdr:                    req.ZDR,
	}
}

// ── Activity: EvaluatorOptimizerActivity ─────────────────────────────────────

// EvaluatorOptimizerActivity runs the ENTIRE evaluator-optimizer loop against
// inference-core inside one activity — a single crash domain: if the worker
// dies mid-loop, the activity retry restarts from round 0. It remains for
// cheap one-shot/in-process use; the durable, resume-safe driver is
// EvaluatorOptimizerWorkflow (cmd/workflows), which runs each leg as its own
// activity via InferModelActivity so completed legs replay from history.
//
// The generator and (distinct) judge invocations both flow through the shared
// InferenceCore client; the loop enforces the turn/token caps in code.
// Per-round observability is emitted through the existing run event publisher,
// and OTEL counters record the terminal outcome.
func (a *Activities) EvaluatorOptimizerActivity(ctx context.Context, input EvalOptimizerInput) (EvalOptimizerOutput, error) {
	if a.clients == nil || a.clients.InferenceCore == nil {
		return EvalOptimizerOutput{}, status.Error(codes.Unavailable, "inference-core unavailable")
	}

	inv := &inferenceInvoker{
		client: mpv1.NewInferenceCoreClient(a.clients.InferenceCore),
		runID:  input.RunID,
	}

	outcome, err := evaloptimizer.RunLoop(ctx, input.ToConfig(), inv)
	out := outcomeToOutput(outcome)

	if err != nil {
		telemetry.EvaluatorOptimizerRunsTotal.Add(ctx, 1, metric.WithAttributes(
			attribute.String("stop_reason", evaloptimizer.StopError),
			attribute.Bool("passed", false),
		))
		telemetry.EvaluatorOptimizerRoundsTotal.Add(ctx, int64(out.RoundsRun))
		a.publishOptimizerRounds(input, out)
		return out, err
	}

	telemetry.EvaluatorOptimizerRunsTotal.Add(ctx, 1, metric.WithAttributes(
		attribute.String("stop_reason", out.StopReason),
		attribute.Bool("passed", out.Passed),
	))
	telemetry.EvaluatorOptimizerRoundsTotal.Add(ctx, int64(out.RoundsRun))
	a.publishOptimizerRounds(input, out)
	return out, nil
}

// publishOptimizerRounds emits one run event per executed round through the
// existing NATS publisher (no-op when no publisher is wired), making the loop's
// progress and a did-not-pass outcome observable to downstream consumers.
func (a *Activities) publishOptimizerRounds(input EvalOptimizerInput, out EvalOptimizerOutput) {
	for _, r := range out.Rounds {
		a.publishRunEvent(input.RunID, input.OrgID, input.UserID,
			"EVALUATOR_OPTIMIZER_ROUND",
			fmt.Sprintf("eval-opt-round-%d", r.Round),
			map[string]any{
				"run_id":   input.RunID,
				"round":    r.Round,
				"passed":   r.Passed,
				"score":    r.Score,
				"feedback": r.Feedback,
			})
	}
}

func outcomeToOutput(o evaloptimizer.Outcome) EvalOptimizerOutput {
	rounds := make([]EvalOptimizerRound, 0, len(o.Rounds))
	for _, at := range o.Rounds {
		rounds = append(rounds, EvalOptimizerRound{
			Round:             at.Round,
			Answer:            at.Answer,
			Passed:            at.Verdict.Passed,
			Score:             at.Verdict.Score,
			Feedback:          at.Verdict.Feedback,
			GenTokens:         at.GenTokens,
			JudgeTokens:       at.JudgeTokens,
			VerdictParseError: at.VerdictParseError,
		})
	}
	return EvalOptimizerOutput{
		Passed:      o.Passed,
		StopReason:  o.StopReason,
		BestAnswer:  o.Best.Answer,
		BestScore:   o.Best.Verdict.Score,
		BestRound:   o.Best.Round,
		RoundsRun:   o.RoundsRun(),
		TotalTokens: o.TotalTokens,
		Rounds:      rounds,
	}
}
