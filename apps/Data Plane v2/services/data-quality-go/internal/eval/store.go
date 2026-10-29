package eval

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/shared/go/orgscope"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

var (
	ErrNotFound            = errors.New("evaluation not found")
	ErrInvalidTransition   = errors.New("invalid evaluation status transition")
	ErrIdempotencyConflict = errors.New("idempotency key is bound to another evaluation request")
)

type EvalStore interface {
	Create(context.Context, model.EvalRun) (*model.EvalRun, bool, error)
	Get(context.Context, string, string) (*model.EvalRun, error)
	Start(context.Context, string, string) (*model.EvalRun, error)
	Complete(context.Context, string, string, json.RawMessage) (*model.EvalRun, error)
	Fail(context.Context, string, string, string) (*model.EvalRun, error)
}

type recoverableEvalStore interface {
	EvalStore
	Recoverable(context.Context, time.Time, int) ([]model.EvalRun, error)
}

type PostgresEvalStore struct {
	pool *pgxpool.Pool
}

// Recoverable requeues evaluations whose execution lease expired and returns
// the pending backlog to execute.
//
// Phase 1 RLS: deliberately NOT wrapped in a scope. This is the cross-org
// durable-recovery sweep driven by cmd/main.go's background loop — it is
// exactly the "background worker draining a queue for every org" exception the
// helper's own "when NOT to use this" section names. Neither statement below
// takes an org_id, and none is available to take: the loop has no request and
// no tenant. Scoping it would silently narrow recovery to one organization
// while every other tenant's evaluations stayed stuck in 'running' forever,
// and nothing would report an error — the loop would simply look idle.
//
// Isolation is preserved downstream instead: this returns each row's own
// org_id, and Runner.Recover calls RunEval once per run with it, so every
// statement that acts on a recovered evaluation runs scoped to that single
// tenant.
func (s *PostgresEvalStore) Recoverable(ctx context.Context, staleBefore time.Time, limit int) ([]model.EvalRun, error) {
	if limit <= 0 || limit > 1000 {
		limit = 100
	}
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin eval recovery: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck -- commit owns the successful path.
	if _, err := tx.Exec(ctx, `
		UPDATE quality_eval_runs
		SET status = 'pending', started_at = NULL, updated_at = NOW(),
		    error_message = 'recovered after expired execution lease'
		WHERE status = 'running' AND updated_at < $1
	`, staleBefore); err != nil {
		return nil, fmt.Errorf("requeue stale evaluations: %w", err)
	}
	rows, err := tx.Query(ctx, `SELECT `+evalColumns+`
		FROM quality_eval_runs
		WHERE status = 'pending'
		ORDER BY created_at
		LIMIT $1
	`, limit)
	if err != nil {
		return nil, fmt.Errorf("list recoverable evaluations: %w", err)
	}
	defer rows.Close()
	runs := make([]model.EvalRun, 0, limit)
	for rows.Next() {
		run, err := scanEval(rows)
		if err != nil {
			return nil, fmt.Errorf("scan recoverable evaluation: %w", err)
		}
		runs = append(runs, *run)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate recoverable evaluations: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit eval recovery: %w", err)
	}
	return runs, nil
}

func NewPostgresEvalStore(pool *pgxpool.Pool) *PostgresEvalStore {
	return &PostgresEvalStore{pool: pool}
}

type rowScanner interface {
	Scan(...any) error
}

const evalColumns = `eval_id::text, org_id, strategy, status, corpus, scorecard,
       error_message, idempotency_key, created_at, started_at, finished_at, updated_at`

func scanEval(row rowScanner) (*model.EvalRun, error) {
	var run model.EvalRun
	var scorecard []byte
	if err := row.Scan(
		&run.EvalID, &run.OrgID, &run.Strategy, &run.Status, &run.Corpus, &scorecard,
		&run.Error, &run.IdempotencyKey, &run.CreatedAt, &run.StartedAt, &run.FinishedAt, &run.UpdatedAt,
	); err != nil {
		return nil, err
	}
	if len(scorecard) > 0 {
		run.Scorecard = append(json.RawMessage(nil), scorecard...)
	}
	return &run, nil
}

// createResult carries Create's two-value success out of the scoped callback.
type createResult struct {
	run        *model.EvalRun
	wasCreated bool
}

// Create inserts an evaluation, or returns the existing one for a replayed
// idempotency key.
//
// Phase 1 RLS: the scope replaces this function's own Begin rather than
// nesting a second transaction inside it — the insert and the idempotency
// read-back stay in one transaction exactly as before. run.OrgID is set by the
// handler from verified caller claims, never from the request body.
func (s *PostgresEvalStore) Create(ctx context.Context, run model.EvalRun) (*model.EvalRun, bool, error) {
	out, err := orgscope.InOrgScope(ctx, s.pool, run.OrgID,
		func(ctx context.Context, tx pgx.Tx) (createResult, error) {
			created, err := scanEval(tx.QueryRow(ctx, `
				INSERT INTO quality_eval_runs
				    (eval_id, org_id, strategy, status, corpus, idempotency_key, created_at, updated_at)
				VALUES ($1::uuid, $2, $3, $4, $5, $6, $7, $7)
				ON CONFLICT (org_id, idempotency_key) DO NOTHING
				RETURNING `+evalColumns,
				run.EvalID, run.OrgID, run.Strategy, run.Status, run.Corpus,
				run.IdempotencyKey, run.CreatedAt,
			))
			wasCreated := true
			if errors.Is(err, pgx.ErrNoRows) {
				wasCreated = false
				created, err = scanEval(tx.QueryRow(ctx, `SELECT `+evalColumns+`
					FROM quality_eval_runs WHERE org_id = $1 AND idempotency_key = $2`,
					run.OrgID, run.IdempotencyKey,
				))
			}
			if err != nil {
				return createResult{}, fmt.Errorf("create or load eval: %w", err)
			}
			if !wasCreated && !sameEvalIntent(created, &run) {
				return createResult{}, ErrIdempotencyConflict
			}
			return createResult{run: created, wasCreated: wasCreated}, nil
		})
	if err != nil {
		return nil, false, err
	}
	return out.run, out.wasCreated, nil
}

func sameEvalIntent(existing, requested *model.EvalRun) bool {
	return existing != nil && requested != nil &&
		existing.OrgID == requested.OrgID &&
		existing.Strategy == requested.Strategy &&
		existing.Corpus == requested.Corpus
}

// Get reads a single evaluation belonging to orgID.
//
// Phase 1 RLS: single-org read. eval_id is caller-supplied (a URL path param),
// so this is a classic IDOR shape — `org_id = $1` is the existing defence and
// the policy is its backstop.
func (s *PostgresEvalStore) Get(ctx context.Context, orgID, evalID string) (*model.EvalRun, error) {
	run, err := orgscope.InOrgScope(ctx, s.pool, orgID,
		func(ctx context.Context, tx pgx.Tx) (*model.EvalRun, error) {
			return scanEval(tx.QueryRow(ctx, `SELECT `+evalColumns+`
				FROM quality_eval_runs WHERE org_id = $1 AND eval_id = $2::uuid`, orgID, evalID))
		})
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get eval: %w", err)
	}
	return run, nil
}

func (s *PostgresEvalStore) Start(ctx context.Context, orgID, evalID string) (*model.EvalRun, error) {
	return s.transition(ctx, orgID, evalID, `
		UPDATE quality_eval_runs
		SET status = 'running', started_at = COALESCE(started_at, NOW()), updated_at = NOW()
		WHERE org_id = $1 AND eval_id = $2::uuid AND status = 'pending'
		RETURNING `+evalColumns, orgID, evalID)
}

func (s *PostgresEvalStore) Complete(ctx context.Context, orgID, evalID string, scorecard json.RawMessage) (*model.EvalRun, error) {
	return s.transition(ctx, orgID, evalID, `
		UPDATE quality_eval_runs
		SET status = 'completed', scorecard = $3::jsonb, error_message = NULL,
		    finished_at = NOW(), updated_at = NOW()
		WHERE org_id = $1 AND eval_id = $2::uuid AND status = 'running'
		RETURNING `+evalColumns, orgID, evalID, scorecard)
}

func (s *PostgresEvalStore) Fail(ctx context.Context, orgID, evalID, message string) (*model.EvalRun, error) {
	return s.transition(ctx, orgID, evalID, `
		UPDATE quality_eval_runs
		SET status = 'failed', scorecard = NULL, error_message = $3,
		    finished_at = NOW(), updated_at = NOW()
		WHERE org_id = $1 AND eval_id = $2::uuid AND status IN ('pending', 'running')
		RETURNING `+evalColumns, orgID, evalID, message)
}

// transition applies one conditional status UPDATE and classifies a no-op.
//
// Phase 1 RLS: every caller (Start/Complete/Fail) passes a single org, so the
// whole transition runs in one scope. The existence check that used to call
// s.Get now runs inline on the same tx: calling Get here would open a SECOND
// scope while this one still holds a pool connection, which the helper rejects
// as a nested scope. Same two queries, same classification, one transaction —
// so a concurrent write can no longer land between the UPDATE and the check
// and turn an invalid-transition into a spurious not-found.
func (s *PostgresEvalStore) transition(ctx context.Context, orgID, evalID, query string, args ...any) (*model.EvalRun, error) {
	return orgscope.InOrgScope(ctx, s.pool, orgID,
		func(ctx context.Context, tx pgx.Tx) (*model.EvalRun, error) {
			run, err := scanEval(tx.QueryRow(ctx, query, args...))
			if !errors.Is(err, pgx.ErrNoRows) {
				if err != nil {
					return nil, fmt.Errorf("update eval: %w", err)
				}
				return run, nil
			}
			// The conditional UPDATE matched nothing. A zero-row RETURNING is
			// not an error, so the transaction is still healthy here.
			if _, getErr := scanEval(tx.QueryRow(ctx, `SELECT `+evalColumns+`
				FROM quality_eval_runs WHERE org_id = $1 AND eval_id = $2::uuid`, orgID, evalID)); getErr != nil {
				if errors.Is(getErr, pgx.ErrNoRows) {
					return nil, ErrNotFound
				}
				return nil, fmt.Errorf("get eval: %w", getErr)
			}
			return nil, ErrInvalidTransition
		})
}
