package eval

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

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

func (s *PostgresEvalStore) Create(ctx context.Context, run model.EvalRun) (*model.EvalRun, bool, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, false, fmt.Errorf("begin create eval: %w", err)
	}
	defer tx.Rollback(ctx)

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
		return nil, false, fmt.Errorf("create or load eval: %w", err)
	}
	if !wasCreated && !sameEvalIntent(created, &run) {
		return nil, false, ErrIdempotencyConflict
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, false, fmt.Errorf("commit create eval: %w", err)
	}
	return created, wasCreated, nil
}

func sameEvalIntent(existing, requested *model.EvalRun) bool {
	return existing != nil && requested != nil &&
		existing.OrgID == requested.OrgID &&
		existing.Strategy == requested.Strategy &&
		existing.Corpus == requested.Corpus
}

func (s *PostgresEvalStore) Get(ctx context.Context, orgID, evalID string) (*model.EvalRun, error) {
	run, err := scanEval(s.pool.QueryRow(ctx, `SELECT `+evalColumns+`
		FROM quality_eval_runs WHERE org_id = $1 AND eval_id = $2::uuid`, orgID, evalID))
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

func (s *PostgresEvalStore) transition(ctx context.Context, orgID, evalID, query string, args ...any) (*model.EvalRun, error) {
	run, err := scanEval(s.pool.QueryRow(ctx, query, args...))
	if !errors.Is(err, pgx.ErrNoRows) {
		if err != nil {
			return nil, fmt.Errorf("update eval: %w", err)
		}
		return run, nil
	}
	if _, getErr := s.Get(ctx, orgID, evalID); errors.Is(getErr, ErrNotFound) {
		return nil, ErrNotFound
	} else if getErr != nil {
		return nil, getErr
	}
	return nil, ErrInvalidTransition
}
