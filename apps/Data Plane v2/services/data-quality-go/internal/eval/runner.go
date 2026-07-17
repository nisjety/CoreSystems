package eval

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"sort"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

type Runner struct {
	store  EvalStore
	traces TraceSource
	golden GoldenSource
}

// Recover claims persisted pending evaluations and requeues executions whose
// lease expired. Start's conditional transition makes this safe across replicas.
func (r *Runner) Recover(ctx context.Context, staleAfter time.Duration) error {
	store, ok := r.store.(recoverableEvalStore)
	if !ok {
		return fmt.Errorf("evaluation recovery store is unavailable")
	}
	if staleAfter <= 0 {
		staleAfter = 5 * time.Minute
	}
	runs, err := store.Recoverable(ctx, time.Now().UTC().Add(-staleAfter), 100)
	if err != nil {
		return err
	}
	var recoveryErr error
	for _, run := range runs {
		if err := r.RunEval(ctx, run.OrgID, run.EvalID); err != nil && !errors.Is(err, ErrInvalidTransition) {
			recoveryErr = errors.Join(recoveryErr, err)
		}
	}
	return recoveryErr
}

func NewRunner(pool *pgxpool.Pool) *Runner {
	return NewRunnerWithAllStores(
		NewPostgresEvalStore(pool),
		NewPostgresTraceSource(pool),
		NewPostgresGoldenStore(pool),
	)
}

// NewRunnerWithStores keeps the legacy two-store construction: every query
// scores via the (labeled) proxy path.
func NewRunnerWithStores(store EvalStore, traces TraceSource) *Runner {
	return NewRunnerWithAllStores(store, traces, noGolden{})
}

func NewRunnerWithAllStores(store EvalStore, traces TraceSource, golden GoldenSource) *Runner {
	return &Runner{store: store, traces: traces, golden: golden}
}

func (r *Runner) CreateEval(ctx context.Context, input model.CreateEvalInput) (*model.EvalRun, bool, error) {
	now := time.Now().UTC()
	run := model.EvalRun{
		EvalID:         uuid.NewString(),
		OrgID:          input.OrgID,
		Strategy:       input.Strategy,
		Status:         model.EvalPending,
		Corpus:         input.Corpus,
		IdempotencyKey: input.IdempotencyKey,
		CreatedAt:      now,
		UpdatedAt:      now,
	}
	return r.store.Create(ctx, run)
}

func (r *Runner) RunEval(ctx context.Context, orgID, evalID string) error {
	run, err := r.store.Start(ctx, orgID, evalID)
	if err != nil {
		return fmt.Errorf("start eval: %w", err)
	}

	traces, err := r.traces.Recent(ctx, orgID, 100)
	if err != nil {
		message := "fetch traces: " + err.Error()
		if _, persistErr := r.store.Fail(ctx, orgID, evalID, message); persistErr != nil {
			return fmt.Errorf("%s; persist failure: %w", message, persistErr)
		}
		return fmt.Errorf("%s", message)
	}

	if len(traces) == 0 {
		_, err = r.store.Fail(ctx, orgID, evalID, "no retrieval traces found for evaluation")
		if err != nil {
			return fmt.Errorf("persist empty evaluation: %w", err)
		}
		return nil
	}

	golden, err := r.golden.Load(ctx, orgID)
	if err != nil {
		message := "fetch golden judgments: " + err.Error()
		if _, persistErr := r.store.Fail(ctx, orgID, evalID, message); persistErr != nil {
			return fmt.Errorf("%s; persist failure: %w", message, persistErr)
		}
		return fmt.Errorf("%s", message)
	}

	scorecard := score(run.Strategy, traces, golden)
	data, err := json.Marshal(scorecard)
	if err != nil {
		message := "encode scorecard: " + err.Error()
		_, _ = r.store.Fail(ctx, orgID, evalID, message)
		return fmt.Errorf("%s", message)
	}
	if _, err := r.store.Complete(ctx, orgID, evalID, data); err != nil {
		return fmt.Errorf("persist completed eval: %w", err)
	}
	return nil
}

func (r *Runner) GetEval(ctx context.Context, orgID, evalID string) (*model.EvalRun, error) {
	return r.store.Get(ctx, orgID, evalID)
}

func (r *Runner) RunCompare(ctx context.Context, input model.CompareEvalInput) (*model.CompareResult, error) {
	baseKey := input.IdempotencyKey
	if baseKey == "" {
		baseKey = "compare-" + uuid.NewString()
	}
	evalA, createdA, err := r.CreateEval(ctx, model.CreateEvalInput{
		OrgID: input.OrgID, Strategy: input.StrategyA, Corpus: input.Corpus, IdempotencyKey: baseKey + "-a",
	})
	if err != nil {
		return nil, fmt.Errorf("create eval A: %w", err)
	}
	if createdA {
		if err := r.RunEval(ctx, input.OrgID, evalA.EvalID); err != nil {
			return nil, fmt.Errorf("run eval A: %w", err)
		}
	}

	evalB, createdB, err := r.CreateEval(ctx, model.CreateEvalInput{
		OrgID: input.OrgID, Strategy: input.StrategyB, Corpus: input.Corpus, IdempotencyKey: baseKey + "-b",
	})
	if err != nil {
		return nil, fmt.Errorf("create eval B: %w", err)
	}
	if createdB {
		if err := r.RunEval(ctx, input.OrgID, evalB.EvalID); err != nil {
			return nil, fmt.Errorf("run eval B: %w", err)
		}
	}

	evalA, err = r.GetEval(ctx, input.OrgID, evalA.EvalID)
	if err != nil {
		return nil, fmt.Errorf("load eval A: %w", err)
	}
	evalB, err = r.GetEval(ctx, input.OrgID, evalB.EvalID)
	if err != nil {
		return nil, fmt.Errorf("load eval B: %w", err)
	}
	if evalA.Status != model.EvalCompleted || evalB.Status != model.EvalCompleted {
		return nil, fmt.Errorf("comparison evaluations are not completed")
	}

	var scA, scB model.Scorecard
	if err := json.Unmarshal(evalA.Scorecard, &scA); err != nil {
		return nil, fmt.Errorf("decode eval A scorecard: %w", err)
	}
	if err := json.Unmarshal(evalB.Scorecard, &scB); err != nil {
		return nil, fmt.Errorf("decode eval B scorecard: %w", err)
	}

	diffs := map[string]float64{
		"recall_at_10":    scA.MeanRecall - scB.MeanRecall,
		"ndcg_at_10":      scA.MeanNDCG - scB.MeanNDCG,
		"mrr":             scA.MeanMRR - scB.MeanMRR,
		"mean_latency_ms": scA.MeanLatency - scB.MeanLatency,
		"p95_latency_ms":  scA.P95Latency - scB.P95Latency,
	}

	winner := input.StrategyA
	scoreA := scA.MeanRecall + scA.MeanNDCG + scA.MeanMRR
	scoreB := scB.MeanRecall + scB.MeanNDCG + scB.MeanMRR
	if scoreB > scoreA {
		winner = input.StrategyB
	} else if math.Abs(scoreA-scoreB) < 0.01 {
		winner = "tie"
	}

	return &model.CompareResult{ScorecardA: scA, ScorecardB: scB, Diffs: diffs, Winner: winner}, nil
}

// score computes the scorecard. Queries with a golden judgment score REAL
// recall@10/nDCG@10/MRR against the trace's persisted top-10 candidates;
// unjudged queries keep the candidate-count proxy — each row is labeled
// (`metric_source`: "golden" | "proxy") so aggregates are never mistaken for
// judged quality when no golden set exists.
func score(strategy string, traces []RetrievalTrace, golden map[string][]string) model.Scorecard {
	results := make([]model.QueryResult, 0, len(traces))
	var sumRecall, sumNDCG, sumMRR, sumLatency float64
	goldenQueries := 0
	latencies := make([]float64, 0, len(traces))
	for _, trace := range traces {
		var recall, ndcg, mrr float64
		source := model.MetricSourceProxy
		if relevant, judged := golden[NormalizeQuery(trace.Query)]; judged {
			recall, ndcg, mrr = goldenMetrics(trace.Retrieved, relevant)
			source = model.MetricSourceGolden
			goldenQueries++
		} else {
			recall = math.Min(float64(trace.Candidates)/10.0, 1.0)
			ndcg = recall * 0.9
			if trace.Candidates > 0 {
				mrr = 1.0
			}
		}
		result := model.QueryResult{
			Query: trace.Query, RecallAt10: recall, NDCGAt10: ndcg, MRR: mrr,
			LatencyMs: float64(trace.TotalMS), Candidates: trace.Candidates,
			MetricSource: source,
		}
		results = append(results, result)
		sumRecall += recall
		sumNDCG += ndcg
		sumMRR += mrr
		sumLatency += result.LatencyMs
		latencies = append(latencies, result.LatencyMs)
	}
	sort.Float64s(latencies)
	p95Idx := int(math.Ceil(0.95*float64(len(latencies)))) - 1
	n := float64(len(results))
	return model.Scorecard{
		Strategy: strategy, QueriesRun: len(results), GoldenQueries: goldenQueries,
		MeanRecall: sumRecall / n,
		MeanNDCG:   sumNDCG / n, MeanMRR: sumMRR / n, MeanLatency: sumLatency / n,
		P95Latency: latencies[p95Idx], Details: results,
	}
}
