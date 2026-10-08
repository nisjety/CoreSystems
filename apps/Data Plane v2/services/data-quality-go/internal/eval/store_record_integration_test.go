package eval

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/google/uuid"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

// Postgres-gated (TEST_DATABASE_URL, like the lifecycle test of the same store): Record runs through the
// real orgscope helper, which drops to the runtime role, against the real table and its CHECKs.

func harnessRun(org, key, scorecard string) model.EvalRun {
	now := time.Now().UTC()
	return model.EvalRun{
		EvalID: uuid.NewString(), OrgID: org, Strategy: "harness:contextual",
		Status: model.EvalCompleted, Corpus: "dpv2-eval-corpus-2026-10@0123456789ab",
		Scorecard: json.RawMessage(scorecard), IdempotencyKey: key, CreatedAt: now, UpdatedAt: now,
	}
}

func TestPostgresEvalStoreRecordsAHarnessResultOnceAndKeepsItPrivateToItsOrganization(t *testing.T) {
	pool := qualityIntegrationPool(t)
	store := NewPostgresEvalStore(pool)
	ctx := context.Background()

	first := harnessRun("org-record-a", "harness-run-1-contextual",
		`{"strategy":"harness:contextual","queries_run":87,"golden_queries":87,"mean_ndcg_at_10":0.7732,"mean_mrr":0.7394,"mean_recall_at_10":0.8736}`)
	stored, created, err := store.Record(ctx, first)
	if err != nil || !created {
		t.Fatalf("record: created=%v err=%v", created, err)
	}
	if stored.Status != model.EvalCompleted || stored.StartedAt == nil || stored.FinishedAt == nil || stored.Strategy != "harness:contextual" {
		t.Fatalf("the recorded run is not a finished one: %+v", stored)
	}

	// The in-service reader shows a recorded run like any other, scoped to its organization.
	read, err := store.Get(ctx, "org-record-a", stored.EvalID)
	if err != nil || !sameJSON(read.Scorecard, first.Scorecard) {
		t.Fatalf("get = %+v err=%v: the scorecard that comes back is not the one that was recorded", read, err)
	}
	if _, err := store.Get(ctx, "org-record-b", stored.EvalID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("another organization read the run: err=%v, want ErrNotFound", err)
	}

	// Replay: the same key and the same content is the same run, whatever the formatting.
	replay := first
	replay.EvalID = uuid.NewString()
	replay.Scorecard = json.RawMessage(`{ "mean_recall_at_10": 0.8736, "mean_mrr": 0.7394, "mean_ndcg_at_10": 0.7732, "golden_queries": 87, "queries_run": 87, "strategy": "harness:contextual" }`)
	again, replayCreated, err := store.Record(ctx, replay)
	if err != nil || replayCreated || again.EvalID != stored.EvalID {
		t.Fatalf("replay = %+v created=%v err=%v, want the stored run and created=false", again, replayCreated, err)
	}

	// History is immutable: the same key with other numbers is a conflict, never an overwrite.
	changed := first
	changed.EvalID = uuid.NewString()
	changed.Scorecard = json.RawMessage(`{"strategy":"harness:contextual","queries_run":87,"golden_queries":87,"mean_ndcg_at_10":0.9,"mean_mrr":0.7394,"mean_recall_at_10":0.8736}`)
	if _, _, err := store.Record(ctx, changed); !errors.Is(err, ErrIdempotencyConflict) {
		t.Fatalf("a replay with other numbers: err=%v, want ErrIdempotencyConflict", err)
	}
	after, err := store.Get(ctx, "org-record-a", stored.EvalID)
	if err != nil || !sameJSON(after.Scorecard, first.Scorecard) {
		t.Fatalf("the stored scorecard changed after a refused replay: %+v err=%v", after, err)
	}

	// Keys are local to an organization.
	other := harnessRun("org-record-b", "harness-run-1-contextual", string(first.Scorecard))
	if _, otherCreated, err := store.Record(ctx, other); err != nil || !otherCreated {
		t.Fatalf("tenant-local key: created=%v err=%v", otherCreated, err)
	}

	// History is a query: the harness rows are told apart from the service's own runs by their strategy.
	var harnessRows int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM quality_eval_runs WHERE strategy LIKE 'harness:%' AND status = 'completed'`).Scan(&harnessRows); err != nil {
		t.Fatalf("history query: %v", err)
	}
	if harnessRows != 2 {
		t.Fatalf("harness rows = %d, want 2", harnessRows)
	}
}

func TestPostgresEvalStoreRefusesWhatTheTableWouldNotHold(t *testing.T) {
	pool := qualityIntegrationPool(t)
	store := NewPostgresEvalStore(pool)
	run := harnessRun("org-record-c", "short", `{"strategy":"harness:contextual"}`)
	if _, _, err := store.Record(context.Background(), run); err == nil {
		t.Fatal("an idempotency key under 8 characters passed the table's CHECK")
	}
}
