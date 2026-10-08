package eval

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"reflect"
	"strings"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/dataplane/shared/go/orgscope"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

// ErrNotRecordable is returned for a run that cannot be stored as a finished, externally measured run.
var ErrNotRecordable = errors.New("only a completed run with a scorecard of a harness strategy can be recorded")

// ResultRecorder stores a run that was measured outside this service (the retrieval-quality harness,
// scripts/eval-gate.py). The handler depends on this, not on the Postgres store.
type ResultRecorder interface {
	Record(context.Context, model.EvalRun) (*model.EvalRun, bool, error)
}

// Record stores a COMPLETED run whose numbers were measured outside this service, so that a regression
// of the retrieval quality is a query over quality_eval_runs and not archaeology in CI artifacts. The
// harness strategy ("harness:<cell>") keeps these rows apart from the runs this service scores itself
// from its own traces ("hybrid", "dense", ...): `WHERE strategy LIKE 'harness:%'`.
//
// It is idempotent per (organization, idempotency key), and history is IMMUTABLE: a replay with the same
// content returns the stored run (created=false); a replay of the same key with different content is
// ErrIdempotencyConflict, never an overwrite. The organization is the caller's verified one; the handler
// sets run.OrgID from the token and never from the body.
//
// Like Create, the insert and the read-back of the replay share one scoped transaction, so row-level
// security applies to both and a tenant can neither see nor collide with another tenant's keys.
func (s *PostgresEvalStore) Record(ctx context.Context, run model.EvalRun) (*model.EvalRun, bool, error) {
	if err := recordable(run); err != nil {
		return nil, false, err
	}
	out, err := orgscope.InOrgScope(ctx, s.pool, run.OrgID,
		func(ctx context.Context, tx pgx.Tx) (createResult, error) {
			stored, err := scanEval(tx.QueryRow(ctx, `
				INSERT INTO quality_eval_runs
				    (eval_id, org_id, strategy, status, corpus, scorecard, idempotency_key,
				     created_at, started_at, finished_at, updated_at)
				VALUES ($1::uuid, $2, $3, 'completed', $4, $5::jsonb, $6, $7, $7, $7, $7)
				ON CONFLICT (org_id, idempotency_key) DO NOTHING
				RETURNING `+evalColumns,
				run.EvalID, run.OrgID, run.Strategy, run.Corpus, run.Scorecard, run.IdempotencyKey, run.CreatedAt,
			))
			wasCreated := true
			if errors.Is(err, pgx.ErrNoRows) {
				wasCreated = false
				stored, err = scanEval(tx.QueryRow(ctx, `SELECT `+evalColumns+`
					FROM quality_eval_runs WHERE org_id = $1 AND idempotency_key = $2`,
					run.OrgID, run.IdempotencyKey,
				))
			}
			if err != nil {
				return createResult{}, fmt.Errorf("record or load eval result: %w", err)
			}
			if !wasCreated && !sameRecordedRun(stored, &run) {
				return createResult{}, ErrIdempotencyConflict
			}
			return createResult{run: stored, wasCreated: wasCreated}, nil
		})
	if err != nil {
		return nil, false, err
	}
	return out.run, out.wasCreated, nil
}

func recordable(run model.EvalRun) error {
	if run.Status != model.EvalCompleted || len(run.Scorecard) == 0 || !json.Valid(run.Scorecard) ||
		!strings.HasPrefix(run.Strategy, harnessStrategy) {
		return ErrNotRecordable
	}
	return nil
}

// sameRecordedRun reports whether a replayed record is the run already stored under its key: the same
// organization, strategy, corpus and status, and the same scorecard CONTENT (jsonb normalizes whitespace
// and key order, so the comparison is on decoded values, not on bytes).
func sameRecordedRun(stored, replay *model.EvalRun) bool {
	if stored == nil || replay == nil {
		return false
	}
	if stored.OrgID != replay.OrgID || stored.Strategy != replay.Strategy || stored.Corpus != replay.Corpus ||
		stored.Status != model.EvalCompleted {
		return false
	}
	return sameJSON(stored.Scorecard, replay.Scorecard)
}

func sameJSON(a, b []byte) bool {
	var left, right any
	if err := decodeUseNumber(a, &left); err != nil {
		return false
	}
	if err := decodeUseNumber(b, &right); err != nil {
		return false
	}
	return equalValues(left, right)
}

// decodeUseNumber keeps numbers exact (not rounded to float64), so that 0.7732 and 0.7731999999 are
// different values while 1e-05 and 0.00001 (which jsonb normalizes into each other) are the same.
func decodeUseNumber(raw []byte, into any) error {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	return decoder.Decode(into)
}

func equalValues(a, b any) bool {
	switch left := a.(type) {
	case map[string]any:
		right, ok := b.(map[string]any)
		if !ok || len(left) != len(right) {
			return false
		}
		for key, value := range left {
			other, present := right[key]
			if !present || !equalValues(value, other) {
				return false
			}
		}
		return true
	case []any:
		right, ok := b.([]any)
		if !ok || len(left) != len(right) {
			return false
		}
		for i := range left {
			if !equalValues(left[i], right[i]) {
				return false
			}
		}
		return true
	case json.Number:
		right, ok := b.(json.Number)
		if !ok {
			return false
		}
		x, okX := new(big.Rat).SetString(left.String())
		y, okY := new(big.Rat).SetString(right.String())
		return okX && okY && x.Cmp(y) == 0
	default:
		return reflect.DeepEqual(a, b)
	}
}
