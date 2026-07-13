package eval

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

type memoryEvalStore struct {
	runs      map[string]*model.EvalRun
	createNew bool
}

func (s *memoryEvalStore) Create(_ context.Context, run model.EvalRun) (*model.EvalRun, bool, error) {
	if s.runs == nil {
		s.runs = make(map[string]*model.EvalRun)
	}
	key := run.OrgID + "/" + run.IdempotencyKey
	if existing := s.runs[key]; existing != nil {
		copy := *existing
		return &copy, false, nil
	}
	copy := run
	s.runs[key] = &copy
	s.runs[run.OrgID+"/"+run.EvalID] = &copy
	return &copy, true, nil
}

func (s *memoryEvalStore) Get(_ context.Context, orgID, evalID string) (*model.EvalRun, error) {
	run := s.runs[orgID+"/"+evalID]
	if run == nil {
		return nil, ErrNotFound
	}
	copy := *run
	return &copy, nil
}

func (s *memoryEvalStore) Start(_ context.Context, orgID, evalID string) (*model.EvalRun, error) {
	run := s.runs[orgID+"/"+evalID]
	if run == nil || run.Status != model.EvalPending {
		return nil, ErrInvalidTransition
	}
	now := time.Now()
	run.Status = model.EvalRunning
	run.StartedAt = &now
	copy := *run
	return &copy, nil
}

func (s *memoryEvalStore) Complete(_ context.Context, orgID, evalID string, scorecard json.RawMessage) (*model.EvalRun, error) {
	run := s.runs[orgID+"/"+evalID]
	if run == nil || run.Status != model.EvalRunning {
		return nil, ErrInvalidTransition
	}
	now := time.Now()
	run.Status = model.EvalCompleted
	run.Scorecard = append(json.RawMessage(nil), scorecard...)
	run.FinishedAt = &now
	copy := *run
	return &copy, nil
}

func (s *memoryEvalStore) Fail(_ context.Context, orgID, evalID, message string) (*model.EvalRun, error) {
	run := s.runs[orgID+"/"+evalID]
	if run == nil || (run.Status != model.EvalPending && run.Status != model.EvalRunning) {
		return nil, ErrInvalidTransition
	}
	now := time.Now()
	run.Status = model.EvalFailed
	run.Error = &message
	run.FinishedAt = &now
	copy := *run
	return &copy, nil
}

func (s *memoryEvalStore) Recoverable(_ context.Context, _ time.Time, _ int) ([]model.EvalRun, error) {
	runs := make([]model.EvalRun, 0)
	seen := make(map[string]bool)
	for _, run := range s.runs {
		if seen[run.EvalID] || run.Status != model.EvalPending {
			continue
		}
		seen[run.EvalID] = true
		runs = append(runs, *run)
	}
	return runs, nil
}

type staticTraceSource struct {
	traces []RetrievalTrace
	err    error
}

func (s staticTraceSource) Recent(context.Context, string, int) ([]RetrievalTrace, error) {
	return append([]RetrievalTrace(nil), s.traces...), s.err
}

func TestRunnerPersistsLifecycleAndScorecard(t *testing.T) {
	store := &memoryEvalStore{}
	runner := NewRunnerWithStores(store, staticTraceSource{traces: []RetrievalTrace{
		{Query: "first", TotalMS: 20, Candidates: 10},
		{Query: "second", TotalMS: 40, Candidates: 5},
	}})
	run, created, err := runner.CreateEval(context.Background(), model.CreateEvalInput{
		OrgID: "org-a", Strategy: "hybrid", Corpus: "recent", IdempotencyKey: "eval-request-1",
	})
	if err != nil || !created {
		t.Fatalf("CreateEval = (%v, %v), want newly created; err=%v", run, created, err)
	}
	if err := runner.RunEval(context.Background(), run.OrgID, run.EvalID); err != nil {
		t.Fatalf("RunEval: %v", err)
	}
	persisted, err := runner.GetEval(context.Background(), "org-a", run.EvalID)
	if err != nil {
		t.Fatalf("GetEval: %v", err)
	}
	if persisted.Status != model.EvalCompleted || persisted.StartedAt == nil || persisted.FinishedAt == nil {
		t.Fatalf("persisted lifecycle = %+v", persisted)
	}
	var scorecard model.Scorecard
	if err := json.Unmarshal(persisted.Scorecard, &scorecard); err != nil {
		t.Fatalf("decode scorecard: %v", err)
	}
	if scorecard.QueriesRun != 2 || scorecard.MeanRecall != 0.75 {
		t.Fatalf("scorecard = %+v", scorecard)
	}
}

func TestRunnerIdempotencyReturnsExistingWithoutSecondRun(t *testing.T) {
	store := &memoryEvalStore{}
	runner := NewRunnerWithStores(store, staticTraceSource{})
	input := model.CreateEvalInput{OrgID: "org-a", Strategy: "hybrid", IdempotencyKey: "same-request"}
	first, firstCreated, err := runner.CreateEval(context.Background(), input)
	if err != nil || !firstCreated {
		t.Fatalf("first create: created=%v err=%v", firstCreated, err)
	}
	second, secondCreated, err := runner.CreateEval(context.Background(), input)
	if err != nil || secondCreated {
		t.Fatalf("second create: created=%v err=%v", secondCreated, err)
	}
	if first.EvalID != second.EvalID {
		t.Fatalf("eval IDs differ: %s != %s", first.EvalID, second.EvalID)
	}
}

func TestRunnerPersistsFailureAndTenantScopesLookup(t *testing.T) {
	store := &memoryEvalStore{}
	runner := NewRunnerWithStores(store, staticTraceSource{err: errors.New("trace store unavailable")})
	run, _, err := runner.CreateEval(context.Background(), model.CreateEvalInput{
		OrgID: "org-a", Strategy: "hybrid", IdempotencyKey: "failed-request",
	})
	if err != nil {
		t.Fatalf("CreateEval: %v", err)
	}
	if err := runner.RunEval(context.Background(), "org-a", run.EvalID); err == nil {
		t.Fatal("RunEval error = nil, want trace error")
	}
	persisted, err := runner.GetEval(context.Background(), "org-a", run.EvalID)
	if err != nil || persisted.Status != model.EvalFailed || persisted.Error == nil {
		t.Fatalf("failed eval = %+v, err=%v", persisted, err)
	}
	if _, err := runner.GetEval(context.Background(), "org-b", run.EvalID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("cross-tenant lookup error = %v, want ErrNotFound", err)
	}
}

func TestRunnerRecoversPersistedPendingEvaluation(t *testing.T) {
	store := &memoryEvalStore{}
	runner := NewRunnerWithStores(store, staticTraceSource{traces: []RetrievalTrace{{Query: "recovered", TotalMS: 5, Candidates: 2}}})
	run, _, err := runner.CreateEval(context.Background(), model.CreateEvalInput{
		OrgID: "org-a", Strategy: "hybrid", IdempotencyKey: "recover-request",
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := runner.Recover(context.Background(), time.Minute); err != nil {
		t.Fatalf("Recover: %v", err)
	}
	persisted, err := runner.GetEval(context.Background(), "org-a", run.EvalID)
	if err != nil || persisted.Status != model.EvalCompleted {
		t.Fatalf("recovered eval=%+v err=%v", persisted, err)
	}
}
