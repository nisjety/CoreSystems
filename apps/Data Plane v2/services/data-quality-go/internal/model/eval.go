package model

import (
	"encoding/json"
	"time"
)

type EvalStatus string

const (
	EvalPending   EvalStatus = "pending"
	EvalRunning   EvalStatus = "running"
	EvalCompleted EvalStatus = "completed"
	EvalFailed    EvalStatus = "failed"
)

type EvalRun struct {
	EvalID         string          `json:"eval_id"`
	OrgID          string          `json:"org_id"`
	Strategy       string          `json:"strategy"`
	Status         EvalStatus      `json:"status"`
	Corpus         string          `json:"corpus"`
	Scorecard      json.RawMessage `json:"scorecard,omitempty"`
	Error          *string         `json:"error,omitempty"`
	IdempotencyKey string          `json:"-"`
	CreatedAt      time.Time       `json:"created_at"`
	StartedAt      *time.Time      `json:"started_at,omitempty"`
	FinishedAt     *time.Time      `json:"finished_at,omitempty"`
	UpdatedAt      time.Time       `json:"updated_at"`
}

type Scorecard struct {
	Strategy    string        `json:"strategy"`
	QueriesRun  int           `json:"queries_run"`
	MeanRecall  float64       `json:"mean_recall_at_10"`
	MeanNDCG    float64       `json:"mean_ndcg_at_10"`
	MeanMRR     float64       `json:"mean_mrr"`
	MeanLatency float64       `json:"mean_latency_ms"`
	P95Latency  float64       `json:"p95_latency_ms"`
	Details     []QueryResult `json:"details,omitempty"`
}

type QueryResult struct {
	Query      string  `json:"query"`
	RecallAt10 float64 `json:"recall_at_10"`
	NDCGAt10   float64 `json:"ndcg_at_10"`
	MRR        float64 `json:"mrr"`
	LatencyMs  float64 `json:"latency_ms"`
	Candidates int     `json:"candidates_returned"`
}

type CreateEvalInput struct {
	OrgID          string `json:"org_id"`
	Strategy       string `json:"strategy"`
	Corpus         string `json:"corpus"`
	IdempotencyKey string `json:"-"`
}

type CompareEvalInput struct {
	OrgID          string `json:"org_id"`
	StrategyA      string `json:"strategy_a"`
	StrategyB      string `json:"strategy_b"`
	Corpus         string `json:"corpus"`
	IdempotencyKey string `json:"-"`
}

type CompareResult struct {
	ScorecardA Scorecard          `json:"scorecard_a"`
	ScorecardB Scorecard          `json:"scorecard_b"`
	Diffs      map[string]float64 `json:"diffs"`
	Winner     string             `json:"winner"`
}

type TrustScore struct {
	DocumentID     string  `json:"document_id"`
	Title          string  `json:"title"`
	Source         string  `json:"source"`
	AuthorityScore float64 `json:"authority_score"`
	FreshnessScore float64 `json:"freshness_score"`
	CompositeScore float64 `json:"composite_score"`
	AgeDays        int     `json:"age_days"`
}

type GateResult struct {
	Gate    string `json:"gate"`
	Passed  bool   `json:"passed"`
	Message string `json:"message"`
	Value   string `json:"value,omitempty"`
}

type GateReport struct {
	AllPassed bool         `json:"all_passed"`
	Gates     []GateResult `json:"gates"`
}
