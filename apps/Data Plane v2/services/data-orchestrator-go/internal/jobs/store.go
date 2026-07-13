package jobs

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

var (
	ErrJobNotFound          = errors.New("job not found")
	ErrInvalidJobTransition = errors.New("invalid job status transition")
	ErrIdempotencyConflict  = errors.New("idempotency key is bound to another job request")
)

type JobStore interface {
	Create(context.Context, model.Job) (*model.Job, bool, error)
	Get(context.Context, string, string) (*model.Job, error)
	Start(context.Context, string, string) (*model.Job, error)
	SetProgress(context.Context, string, string, int) (*model.Job, error)
	Complete(context.Context, string, string, json.RawMessage) (*model.Job, error)
	Fail(context.Context, string, string, string) (*model.Job, error)
}

type PostgresJobStore struct {
	pool *pgxpool.Pool
}

func NewPostgresJobStore(pool *pgxpool.Pool) *PostgresJobStore {
	return &PostgresJobStore{pool: pool}
}

const jobColumns = `
	job_id::text, org_id, job_type, status, document_ids, result,
	error_message, progress, total, idempotency_key, created_at,
	started_at, completed_at, updated_at`

type rowScanner interface {
	Scan(...any) error
}

func scanJob(row rowScanner) (*model.Job, error) {
	var (
		job         model.Job
		documentIDs []byte
		result      []byte
	)
	if err := row.Scan(
		&job.JobID, &job.OrgID, &job.JobType, &job.Status, &documentIDs,
		&result, &job.ErrorMessage, &job.Progress, &job.Total,
		&job.IdempotencyKey, &job.CreatedAt, &job.StartedAt,
		&job.CompletedAt, &job.UpdatedAt,
	); err != nil {
		return nil, err
	}
	if err := json.Unmarshal(documentIDs, &job.DocumentIDs); err != nil {
		return nil, fmt.Errorf("decode job document ids: %w", err)
	}
	job.Result = append(json.RawMessage(nil), result...)
	return &job, nil
}

func (s *PostgresJobStore) Create(ctx context.Context, job model.Job) (*model.Job, bool, error) {
	documentIDs, err := json.Marshal(job.DocumentIDs)
	if err != nil {
		return nil, false, fmt.Errorf("encode job document ids: %w", err)
	}

	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, false, fmt.Errorf("begin create job: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck -- commit below owns the successful path.

	created, err := scanJob(tx.QueryRow(ctx, `
		INSERT INTO data_orchestrator_jobs (
			job_id, org_id, job_type, status, document_ids, progress, total,
			idempotency_key, created_at, updated_at
		)
		VALUES ($1::uuid, $2, $3, $4, $5::jsonb, $6, $7, $8, $9, $9)
		ON CONFLICT (org_id, idempotency_key) DO NOTHING
		RETURNING `+jobColumns,
		job.JobID, job.OrgID, job.JobType, job.Status, string(documentIDs),
		job.Progress, job.Total, job.IdempotencyKey, job.CreatedAt,
	))
	wasCreated := true
	if errors.Is(err, pgx.ErrNoRows) {
		wasCreated = false
		created, err = scanJob(tx.QueryRow(ctx, `
			SELECT `+jobColumns+`
			FROM data_orchestrator_jobs
			WHERE org_id = $1 AND idempotency_key = $2
		`, job.OrgID, job.IdempotencyKey))
	}
	if err != nil {
		return nil, false, fmt.Errorf("create or load job: %w", err)
	}
	if !wasCreated && !sameJobIntent(created, &job) {
		return nil, false, ErrIdempotencyConflict
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, false, fmt.Errorf("commit create job: %w", err)
	}
	return created, wasCreated, nil
}

func sameJobIntent(existing, requested *model.Job) bool {
	return existing != nil && requested != nil &&
		existing.OrgID == requested.OrgID &&
		existing.JobType == requested.JobType &&
		slices.Equal(existing.DocumentIDs, requested.DocumentIDs)
}

func (s *PostgresJobStore) Get(ctx context.Context, orgID, jobID string) (*model.Job, error) {
	job, err := scanJob(s.pool.QueryRow(ctx, `
		SELECT `+jobColumns+`
		FROM data_orchestrator_jobs
		WHERE org_id = $1 AND job_id = $2::uuid
	`, orgID, jobID))
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrJobNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get job: %w", err)
	}
	return job, nil
}

func (s *PostgresJobStore) Start(ctx context.Context, orgID, jobID string) (*model.Job, error) {
	return s.transition(ctx, orgID, jobID, `
		UPDATE data_orchestrator_jobs
		SET status = 'running', started_at = NOW(), updated_at = NOW()
		WHERE org_id = $1 AND job_id = $2::uuid AND status = 'pending'
		RETURNING `+jobColumns)
}

func (s *PostgresJobStore) SetProgress(ctx context.Context, orgID, jobID string, progress int) (*model.Job, error) {
	return s.transition(ctx, orgID, jobID, `
		UPDATE data_orchestrator_jobs
		SET progress = $3, updated_at = NOW()
		WHERE org_id = $1 AND job_id = $2::uuid AND status = 'running'
		  AND progress <= $3 AND $3 <= total
		RETURNING `+jobColumns, progress)
}

func (s *PostgresJobStore) Complete(ctx context.Context, orgID, jobID string, result json.RawMessage) (*model.Job, error) {
	if len(result) == 0 {
		result = json.RawMessage(`{}`)
	}
	return s.transition(ctx, orgID, jobID, `
		UPDATE data_orchestrator_jobs
		SET status = 'completed', progress = total, result = $3::jsonb,
			completed_at = NOW(), updated_at = NOW()
		WHERE org_id = $1 AND job_id = $2::uuid AND status = 'running'
		RETURNING `+jobColumns, string(result))
}

func (s *PostgresJobStore) Fail(ctx context.Context, orgID, jobID, message string) (*model.Job, error) {
	return s.transition(ctx, orgID, jobID, `
		UPDATE data_orchestrator_jobs
		SET status = 'failed', error_message = $3, completed_at = NOW(), updated_at = NOW()
		WHERE org_id = $1 AND job_id = $2::uuid AND status IN ('pending', 'running')
		RETURNING `+jobColumns, message)
}

func (s *PostgresJobStore) transition(ctx context.Context, orgID, jobID, query string, args ...any) (*model.Job, error) {
	params := []any{orgID, jobID}
	params = append(params, args...)
	job, err := scanJob(s.pool.QueryRow(ctx, query, params...))
	if err == nil {
		return job, nil
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return nil, fmt.Errorf("persist job transition: %w", err)
	}
	if _, getErr := s.Get(ctx, orgID, jobID); errors.Is(getErr, ErrJobNotFound) {
		return nil, ErrJobNotFound
	} else if getErr != nil {
		return nil, getErr
	}
	return nil, ErrInvalidJobTransition
}
