package jobs

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/shared/go/orgscope"

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

	// ClaimNext leases the oldest runnable job for `owner` (plan P2-4).
	// Returns (nil, nil) when there is nothing to claim.
	ClaimNext(ctx context.Context, owner string, lease time.Duration, maxAttempts int) (*model.Job, error)
	// ExpireExhausted terminally fails jobs that have burned through
	// `maxAttempts`, so a permanently-failing job cannot be reclaimed forever.
	ExpireExhausted(ctx context.Context, maxAttempts int) (int64, error)
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

// createResult carries Create's two-value success out of the scoped callback.
type createResult struct {
	job        *model.Job
	wasCreated bool
}

// Create records job intent, or returns the existing job for a replayed
// idempotency key.
//
// Phase 1 RLS: unlike ClaimNext and ExpireExhausted below, this is a
// request-scoped path — job.OrgID is set by the HTTP handler from verified
// caller claims (authctx rejects any token without a non-empty org_id), never
// from the request body. The scope replaces this function's own Begin rather
// than nesting a second transaction inside it, so the insert and the
// idempotency read-back stay in one transaction exactly as before.
func (s *PostgresJobStore) Create(ctx context.Context, job model.Job) (*model.Job, bool, error) {
	documentIDs, err := json.Marshal(job.DocumentIDs)
	if err != nil {
		return nil, false, fmt.Errorf("encode job document ids: %w", err)
	}

	out, err := orgscope.InOrgScope(ctx, s.pool, job.OrgID,
		func(ctx context.Context, tx pgx.Tx) (createResult, error) {
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
				return createResult{}, fmt.Errorf("create or load job: %w", err)
			}
			if !wasCreated && !sameJobIntent(created, &job) {
				return createResult{}, ErrIdempotencyConflict
			}
			return createResult{job: created, wasCreated: wasCreated}, nil
		})
	if err != nil {
		return nil, false, err
	}
	return out.job, out.wasCreated, nil
}

func sameJobIntent(existing, requested *model.Job) bool {
	return existing != nil && requested != nil &&
		existing.OrgID == requested.OrgID &&
		existing.JobType == requested.JobType &&
		slices.Equal(existing.DocumentIDs, requested.DocumentIDs)
}

// Get reads a single job belonging to orgID.
//
// Phase 1 RLS: single-org read. jobID is caller-supplied (a URL path param), so
// this is a classic IDOR shape — `org_id = $1` is the existing defence and the
// policy is its backstop.
func (s *PostgresJobStore) Get(ctx context.Context, orgID, jobID string) (*model.Job, error) {
	job, err := orgscope.InOrgScope(ctx, s.pool, orgID,
		func(ctx context.Context, tx pgx.Tx) (*model.Job, error) {
			return scanJob(tx.QueryRow(ctx, `
				SELECT `+jobColumns+`
				FROM data_orchestrator_jobs
				WHERE org_id = $1 AND job_id = $2::uuid
			`, orgID, jobID))
		})
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

// SetProgress also RENEWS the lease (plan P2-4). A long reindex would otherwise
// outlive its lease and be reclaimed by another worker while still running,
// giving two workers the same job. Progress reporting is the natural liveness
// signal, so it doubles as the heartbeat.
func (s *PostgresJobStore) SetProgress(ctx context.Context, orgID, jobID string, progress int) (*model.Job, error) {
	return s.transition(ctx, orgID, jobID, `
		UPDATE data_orchestrator_jobs
		SET progress = $3,
		    lease_until = CASE
		        WHEN lease_until IS NULL THEN NULL
		        ELSE GREATEST(lease_until, NOW() + make_interval(secs => $4))
		    END,
		    updated_at = NOW()
		WHERE org_id = $1 AND job_id = $2::uuid AND status = 'running'
		  AND progress <= $3 AND $3 <= total
		RETURNING `+jobColumns, progress, LeaseSeconds)
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

// ClaimNext atomically leases the oldest runnable job (plan P2-4, closes D12).
//
// Runnable means either `pending`, or `running` with an **expired lease** — the
// signature of a worker that died mid-job. `FOR UPDATE SKIP LOCKED` is what makes
// this safe for multiple replicas: each claimant skips rows another has locked
// rather than blocking on them. Same idiom as `wiki_event_outbox`,
// `index_deletion_outbox` and `quickwit_admin_jobs`.
//
// `attempts` is incremented on claim, not on failure, so a worker that dies
// without reporting anything still burns an attempt. Otherwise a job that
// reliably crashes its worker would be reclaimed forever.
//
// `started_at` uses COALESCE so a reclaimed job keeps its original start time —
// it reports when the work first began, not when the last retry did.
//
// Phase 1 RLS: deliberately NOT wrapped in a scope, and this is the clearest
// such case in the service. `data_orchestrator_jobs` is a CROSS-ORG queue: this
// statement's whole job is to find the oldest runnable row belonging to ANY
// tenant. A scoped claim would see one organization's rows and silently stop
// running every other tenant's jobs — and it would not fail loudly, it would
// return (nil, nil) and look exactly like an idle queue. There is also no org
// to scope to: the worker has no request and no tenant, which is why no org_id
// appears in the WHERE clause.
//
// Isolation is preserved downstream instead: the claimed row carries its own
// org_id, and every statement the executor then runs against it
// (Start/SetProgress/Complete/Fail) goes through the scoped transition path.
func (s *PostgresJobStore) ClaimNext(
	ctx context.Context,
	owner string,
	lease time.Duration,
	maxAttempts int,
) (*model.Job, error) {
	job, err := scanJob(s.pool.QueryRow(ctx, `
		WITH candidate AS (
			-- Aliased to candidate_job_id, NOT job_id: the FROM clause below puts
			-- this CTE's columns in scope for RETURNING, and jobColumns selects an
			-- unqualified job_id. Naming it job_id here makes that reference
			-- ambiguous, and every claim fails with SQLSTATE 42702.
			SELECT job_id AS candidate_job_id
			FROM data_orchestrator_jobs
			WHERE attempts < $3
			  AND (
			        status = 'pending'
			     OR (status = 'running' AND lease_until IS NOT NULL AND lease_until <= NOW())
			  )
			ORDER BY created_at
			LIMIT 1
			FOR UPDATE SKIP LOCKED
		)
		UPDATE data_orchestrator_jobs AS j
		SET status = 'running',
		    started_at = COALESCE(j.started_at, NOW()),
		    lease_owner = $1,
		    lease_until = NOW() + make_interval(secs => $2),
		    attempts = j.attempts + 1,
		    updated_at = NOW()
		FROM candidate c
		WHERE j.job_id = c.candidate_job_id
		RETURNING `+jobColumns,
		owner, lease.Seconds(), maxAttempts,
	))
	if errors.Is(err, pgx.ErrNoRows) {
		// Nothing claimable — the common case on an idle tick.
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("claim next job: %w", err)
	}
	return job, nil
}

// ExpireExhausted terminally fails jobs that have used up `maxAttempts`.
//
// Without this a job that fails deterministically would sit at
// `attempts >= maxAttempts` forever: excluded from ClaimNext, but still
// `running` and never reported as failed. Writing `error_message` together with
// `completed_at` is required by `data_orchestrator_jobs_terminal_shape_check`.
//
// Phase 1 RLS: deliberately NOT wrapped in a scope, for the same reason as
// ClaimNext. This is the cross-org sweep half of the queue — it retires
// exhausted jobs for every tenant on each worker tick and takes no org_id.
// Scoping it would leave every other organization's exhausted jobs stuck in
// `running` forever while the returned count still read zero, which is
// indistinguishable from a healthy queue.
func (s *PostgresJobStore) ExpireExhausted(ctx context.Context, maxAttempts int) (int64, error) {
	tag, err := s.pool.Exec(ctx, `
		UPDATE data_orchestrator_jobs
		SET status = 'failed',
		    error_message = COALESCE(
		        NULLIF(error_message, ''),
		        'job abandoned after ' || attempts || ' attempts'
		    ),
		    completed_at = NOW(),
		    lease_owner = NULL,
		    lease_until = NULL,
		    updated_at = NOW()
		WHERE status IN ('pending', 'running')
		  AND attempts >= $1
	`, maxAttempts)
	if err != nil {
		return 0, fmt.Errorf("expire exhausted jobs: %w", err)
	}
	return tag.RowsAffected(), nil
}

// transition applies one conditional status UPDATE and classifies a no-op.
//
// Phase 1 RLS: every caller (Start/SetProgress/Complete/Fail) names a single
// organization — from verified claims on the HTTP path, and from the claimed
// row's own org_id when the durable worker executes a job — so the whole
// transition runs in one scope. The existence check that used to call s.Get now
// runs inline on the same tx: calling Get here would open a SECOND scope while
// this one still holds a pool connection, which the helper rejects as a nested
// scope. Same two queries, same classification, one transaction — so a
// concurrent write can no longer land between the UPDATE and the check and turn
// an invalid-transition into a spurious not-found.
func (s *PostgresJobStore) transition(ctx context.Context, orgID, jobID, query string, args ...any) (*model.Job, error) {
	params := []any{orgID, jobID}
	params = append(params, args...)
	return orgscope.InOrgScope(ctx, s.pool, orgID,
		func(ctx context.Context, tx pgx.Tx) (*model.Job, error) {
			job, err := scanJob(tx.QueryRow(ctx, query, params...))
			if err == nil {
				return job, nil
			}
			if !errors.Is(err, pgx.ErrNoRows) {
				return nil, fmt.Errorf("persist job transition: %w", err)
			}
			// The conditional UPDATE matched nothing. A zero-row RETURNING is
			// not an error, so the transaction is still healthy here.
			if _, getErr := scanJob(tx.QueryRow(ctx, `
				SELECT `+jobColumns+`
				FROM data_orchestrator_jobs
				WHERE org_id = $1 AND job_id = $2::uuid
			`, orgID, jobID)); getErr != nil {
				if errors.Is(getErr, pgx.ErrNoRows) {
					return nil, ErrJobNotFound
				}
				return nil, fmt.Errorf("get job: %w", getErr)
			}
			return nil, ErrInvalidJobTransition
		})
}
