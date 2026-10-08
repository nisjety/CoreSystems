package eval

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

func recordedRun(scorecard string) *model.EvalRun {
	return &model.EvalRun{
		OrgID: "org-corpus-contextual", Strategy: "harness:contextual", Corpus: "dpv2-eval-corpus-2026-10@0123456789ab",
		Status: model.EvalCompleted, Scorecard: json.RawMessage(scorecard),
	}
}

func TestARecordedRunReplayedWithTheSameContentIsTheSameRun(t *testing.T) {
	stored := recordedRun(`{"strategy":"harness:contextual","queries_run":87,"mean_ndcg_at_10":0.7732}`)
	// Postgres' jsonb normalizes whitespace and key order: a replay must still match.
	replay := recordedRun(`{ "mean_ndcg_at_10": 0.7732,  "queries_run": 87, "strategy": "harness:contextual" }`)
	if !sameRecordedRun(stored, replay) {
		t.Fatal("a replay that differs only in formatting was treated as another run")
	}
}

func TestNumbersThatJsonbNormalizesAreTheSameNumber(t *testing.T) {
	stored := recordedRun(`{"strategy":"harness:contextual","mean_latency_ms":0.00001,"p95_latency_ms":109}`)
	replay := recordedRun(`{"strategy":"harness:contextual","mean_latency_ms":1e-05,"p95_latency_ms":109.0}`)
	if !sameRecordedRun(stored, replay) {
		t.Fatal("1e-05 against 0.00001 and 109.0 against 109 were treated as different numbers: a legitimate replay would conflict")
	}
}

func TestARecordedRunReplayedWithDifferentContentIsRefused(t *testing.T) {
	stored := recordedRun(`{"strategy":"harness:contextual","queries_run":87,"mean_ndcg_at_10":0.7732}`)
	cases := map[string]*model.EvalRun{
		"another metric": recordedRun(`{"strategy":"harness:contextual","queries_run":87,"mean_ndcg_at_10":0.7731}`),
		"another strategy": func() *model.EvalRun {
			r := recordedRun(`{"strategy":"harness:contextual","queries_run":87,"mean_ndcg_at_10":0.7732}`)
			r.Strategy = "harness:baseline"
			return r
		}(),
		"another corpus": func() *model.EvalRun {
			r := recordedRun(`{"strategy":"harness:contextual","queries_run":87,"mean_ndcg_at_10":0.7732}`)
			r.Corpus = "other@ffffffffffff"
			return r
		}(),
		"another organization": func() *model.EvalRun {
			r := recordedRun(`{"strategy":"harness:contextual","queries_run":87,"mean_ndcg_at_10":0.7732}`)
			r.OrgID = "org-corpus-baseline"
			return r
		}(),
		"an extra key": recordedRun(`{"strategy":"harness:contextual","queries_run":87,"mean_ndcg_at_10":0.7732,"note":"x"}`),
	}
	for name, replay := range cases {
		t.Run(name, func(t *testing.T) {
			if sameRecordedRun(stored, replay) {
				t.Fatalf("%s: history must be immutable, but the replay was accepted as the same run", name)
			}
		})
	}
}

func TestOnlyAFinishedRunWithAScorecardCanBeRecorded(t *testing.T) {
	store := &PostgresEvalStore{}
	for name, run := range map[string]model.EvalRun{
		"pending":      {OrgID: "o", Strategy: "harness:x", Status: model.EvalPending, Scorecard: json.RawMessage(`{}`)},
		"running":      {OrgID: "o", Strategy: "harness:x", Status: model.EvalRunning, Scorecard: json.RawMessage(`{}`)},
		"no scorecard": {OrgID: "o", Strategy: "harness:x", Status: model.EvalCompleted},
		"a scorecard that is not JSON": {OrgID: "o", Strategy: "harness:x", Status: model.EvalCompleted,
			Scorecard: json.RawMessage(`not json`)},
		"a strategy of the service's own runs": {OrgID: "o", Strategy: "hybrid", Status: model.EvalCompleted,
			Scorecard: json.RawMessage(`{}`)},
	} {
		t.Run(name, func(t *testing.T) {
			if _, _, err := store.Record(context.Background(), run); err == nil {
				t.Fatalf("%s was recorded", name)
			}
		})
	}
}
