package billing

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

var ErrNotFound = errors.New("not found")

type Repository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *Repository {
	return &Repository{pool: pool}
}

func (r *Repository) UpsertAccount(ctx context.Context, account Account) error {
	products, err := json.Marshal(account.Products)
	if err != nil {
		return fmt.Errorf("marshal products: %w", err)
	}
	featureFlags, err := json.Marshal(account.FeatureFlags)
	if err != nil {
		return fmt.Errorf("marshal feature flags: %w", err)
	}
	entitlements, err := json.Marshal(account.Entitlements)
	if err != nil {
		return fmt.Errorf("marshal entitlements: %w", err)
	}
	quotaLimits, err := json.Marshal(account.QuotaLimits)
	if err != nil {
		return fmt.Errorf("marshal quota limits: %w", err)
	}
	providers, err := json.Marshal(account.ProviderCustomerID)
	if err != nil {
		return fmt.Errorf("marshal provider ids: %w", err)
	}
	metadata, err := json.Marshal(account.Metadata)
	if err != nil {
		return fmt.Errorf("marshal metadata: %w", err)
	}

	query := `
		INSERT INTO billing_accounts (
			org_id, plan, subscription_state, credits,
			products, feature_flags, entitlements, quota_limits,
			provider_customer_id, metadata, trial_ends_at, updated_at
		)
		VALUES ($1, $2, $3, $4, $5::jsonb, $6::jsonb, $7::jsonb, $8::jsonb, $9::jsonb, $10::jsonb, $11, now())
		ON CONFLICT (org_id)
		DO UPDATE SET
			plan = EXCLUDED.plan,
			subscription_state = EXCLUDED.subscription_state,
			credits = EXCLUDED.credits,
			products = EXCLUDED.products,
			feature_flags = EXCLUDED.feature_flags,
			entitlements = EXCLUDED.entitlements,
			quota_limits = EXCLUDED.quota_limits,
			provider_customer_id = EXCLUDED.provider_customer_id,
			metadata = EXCLUDED.metadata,
			trial_ends_at = EXCLUDED.trial_ends_at,
			updated_at = now()
	`

	_, err = r.pool.Exec(
		ctx,
		query,
		account.OrgID,
		account.Plan,
		account.SubscriptionState,
		account.Credits,
		string(products),
		string(featureFlags),
		string(entitlements),
		string(quotaLimits),
		string(providers),
		string(metadata),
		account.TrialEndsAt,
	)
	if err != nil {
		return fmt.Errorf("upsert account: %w", err)
	}

	return nil
}

func (r *Repository) GetAccount(ctx context.Context, orgID string) (Account, error) {
	query := `
		SELECT
			org_id, plan, subscription_state, credits,
			products, feature_flags, entitlements, quota_limits,
			provider_customer_id, metadata, trial_ends_at, created_at, updated_at
		FROM billing_accounts
		WHERE org_id = $1
	`

	var account Account
	var products, featureFlags, entitlements, quotaLimits, providers, metadata []byte
	if err := r.pool.QueryRow(ctx, query, orgID).Scan(
		&account.OrgID,
		&account.Plan,
		&account.SubscriptionState,
		&account.Credits,
		&products,
		&featureFlags,
		&entitlements,
		&quotaLimits,
		&providers,
		&metadata,
		&account.TrialEndsAt,
		&account.CreatedAt,
		&account.UpdatedAt,
	); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return Account{}, ErrNotFound
		}
		return Account{}, fmt.Errorf("query account: %w", err)
	}

	if err := json.Unmarshal(products, &account.Products); err != nil {
		return Account{}, fmt.Errorf("decode products: %w", err)
	}
	if err := json.Unmarshal(featureFlags, &account.FeatureFlags); err != nil {
		return Account{}, fmt.Errorf("decode feature flags: %w", err)
	}
	if err := json.Unmarshal(entitlements, &account.Entitlements); err != nil {
		return Account{}, fmt.Errorf("decode entitlements: %w", err)
	}
	if err := json.Unmarshal(quotaLimits, &account.QuotaLimits); err != nil {
		return Account{}, fmt.Errorf("decode quota limits: %w", err)
	}
	if err := json.Unmarshal(providers, &account.ProviderCustomerID); err != nil {
		return Account{}, fmt.Errorf("decode provider ids: %w", err)
	}
	if err := json.Unmarshal(metadata, &account.Metadata); err != nil {
		return Account{}, fmt.Errorf("decode metadata: %w", err)
	}

	return account, nil
}

// ListExpiredTrials returns org ids whose trial window has elapsed and that
// are still in the trialing state — the input to the trial-expiry sweep.
func (r *Repository) ListExpiredTrials(ctx context.Context, now time.Time, limit int) ([]string, error) {
	query := `
		SELECT org_id
		FROM billing_accounts
		WHERE subscription_state = 'trialing'
		  AND trial_ends_at IS NOT NULL
		  AND trial_ends_at <= $1
		ORDER BY trial_ends_at ASC
		LIMIT $2
	`

	rows, err := r.pool.Query(ctx, query, now, limit)
	if err != nil {
		return nil, fmt.Errorf("list expired trials: %w", err)
	}
	defer rows.Close()

	orgIDs := make([]string, 0, limit)
	for rows.Next() {
		var orgID string
		if err := rows.Scan(&orgID); err != nil {
			return nil, fmt.Errorf("scan expired trial: %w", err)
		}
		orgIDs = append(orgIDs, orgID)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate expired trials: %w", err)
	}

	return orgIDs, nil
}

func (r *Repository) SaveUsage(ctx context.Context, event UsageEvent) error {
	metadata, err := json.Marshal(event.Metadata)
	if err != nil {
		return fmt.Errorf("marshal usage metadata: %w", err)
	}

	query := `
		INSERT INTO billing_usage_events (org_id, metric, quantity, source, occurred_at, metadata)
		VALUES ($1, $2, $3, $4, $5, $6::jsonb)
	`

	_, err = r.pool.Exec(ctx, query, event.OrgID, event.Metric, event.Quantity, event.Source, event.OccurredAt, string(metadata))
	if err != nil {
		return fmt.Errorf("insert usage event: %w", err)
	}

	return nil
}

func (r *Repository) ReserveUsageEvent(ctx context.Context, event UsageEvent) (bool, error) {
	query := `
		INSERT INTO billing_usage_dedup (event_id, org_id, metric, occurred_at)
		VALUES ($1, $2, $3, $4)
		ON CONFLICT (event_id) DO NOTHING
	`

	result, err := r.pool.Exec(ctx, query, event.EventID, event.OrgID, event.Metric, event.OccurredAt)
	if err != nil {
		return false, fmt.Errorf("reserve usage event: %w", err)
	}

	return result.RowsAffected() > 0, nil
}

func (r *Repository) GetMetricUsage(ctx context.Context, orgID, metric string) (float64, error) {
	query := `
		SELECT COALESCE(SUM(quantity), 0)
		FROM billing_usage_events
		WHERE org_id = $1
		AND metric = $2
	`

	var used float64
	if err := r.pool.QueryRow(ctx, query, orgID, metric).Scan(&used); err != nil {
		return 0, fmt.Errorf("get metric usage: %w", err)
	}

	return used, nil
}

func (r *Repository) SaveInvoice(ctx context.Context, invoice Invoice) error {
	metadata, err := json.Marshal(invoice.Metadata)
	if err != nil {
		return fmt.Errorf("marshal invoice metadata: %w", err)
	}

	query := `
		INSERT INTO billing_invoices (
			invoice_id, org_id, provider, amount_cents, currency,
			status, issued_at, due_at, metadata, last_modified
		)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::jsonb, now())
	`

	_, err = r.pool.Exec(
		ctx,
		query,
		invoice.InvoiceID,
		invoice.OrgID,
		invoice.Provider,
		invoice.AmountCents,
		invoice.Currency,
		invoice.Status,
		invoice.IssuedAt,
		invoice.DueAt,
		string(metadata),
	)
	if err != nil {
		return fmt.Errorf("save invoice: %w", err)
	}

	return nil
}

func (r *Repository) EnqueueRetryJob(ctx context.Context, kind RetryJobKind, dedupeKey string, payload map[string]interface{}, nextAttemptAt time.Time) error {
	payloadJSON, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("marshal retry payload: %w", err)
	}

	query := `
		INSERT INTO billing_retry_jobs (kind, dedupe_key, payload, status, attempt_count, next_attempt_at)
		VALUES ($1, $2, $3::jsonb, 'pending', 0, $4)
		ON CONFLICT (dedupe_key) DO NOTHING
	`

	_, err = r.pool.Exec(ctx, query, kind, dedupeKey, string(payloadJSON), nextAttemptAt)
	if err != nil {
		return fmt.Errorf("enqueue retry job: %w", err)
	}

	return nil
}

func (r *Repository) ClaimDueRetryJobs(ctx context.Context, limit int) ([]RetryJob, error) {
	query := `
		WITH candidate AS (
			SELECT id
			FROM billing_retry_jobs
			WHERE status = 'pending'
			  AND next_attempt_at <= now()
			ORDER BY next_attempt_at ASC
			LIMIT $1
			FOR UPDATE SKIP LOCKED
		)
		UPDATE billing_retry_jobs j
		SET status = 'processing', updated_at = now()
		FROM candidate c
		WHERE j.id = c.id
		RETURNING j.id, j.kind, j.dedupe_key, j.payload, j.status, j.attempt_count,
		          j.next_attempt_at, COALESCE(j.last_error, ''), j.created_at, j.updated_at
	`

	rows, err := r.pool.Query(ctx, query, limit)
	if err != nil {
		return nil, fmt.Errorf("claim retry jobs: %w", err)
	}
	defer rows.Close()

	jobs := make([]RetryJob, 0, limit)
	for rows.Next() {
		var job RetryJob
		var payloadBytes []byte
		var kind string
		var status string
		if err := rows.Scan(
			&job.ID,
			&kind,
			&job.DedupeKey,
			&payloadBytes,
			&status,
			&job.AttemptCount,
			&job.NextAttemptAt,
			&job.LastError,
			&job.CreatedAt,
			&job.UpdatedAt,
		); err != nil {
			return nil, fmt.Errorf("scan retry job: %w", err)
		}
		job.Kind = RetryJobKind(kind)
		job.Status = RetryJobStatus(status)
		if err := json.Unmarshal(payloadBytes, &job.Payload); err != nil {
			return nil, fmt.Errorf("decode retry payload: %w", err)
		}
		jobs = append(jobs, job)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate retry jobs: %w", err)
	}

	return jobs, nil
}

func (r *Repository) MarkRetryJobSucceeded(ctx context.Context, jobID int64) error {
	query := `
		UPDATE billing_retry_jobs
		SET status = 'succeeded', updated_at = now(), last_error = NULL
		WHERE id = $1
	`
	if _, err := r.pool.Exec(ctx, query, jobID); err != nil {
		return fmt.Errorf("mark retry job succeeded: %w", err)
	}
	return nil
}

func (r *Repository) MarkRetryJobPending(ctx context.Context, jobID int64, attemptCount int, lastError string, nextAttemptAt time.Time) error {
	query := `
		UPDATE billing_retry_jobs
		SET status = 'pending',
		    attempt_count = $2,
		    last_error = $3,
		    next_attempt_at = $4,
		    updated_at = now()
		WHERE id = $1
	`
	if _, err := r.pool.Exec(ctx, query, jobID, attemptCount, lastError, nextAttemptAt); err != nil {
		return fmt.Errorf("mark retry job pending: %w", err)
	}
	return nil
}

func (r *Repository) MarkRetryJobDeadLetter(ctx context.Context, jobID int64, attemptCount int, lastError string) error {
	query := `
		UPDATE billing_retry_jobs
		SET status = 'dead_letter',
		    attempt_count = $2,
		    last_error = $3,
		    updated_at = now()
		WHERE id = $1
	`
	if _, err := r.pool.Exec(ctx, query, jobID, attemptCount, lastError); err != nil {
		return fmt.Errorf("mark retry job dead-letter: %w", err)
	}
	return nil
}

func NewDefaultAccount(orgID string) Account {
	return Account{
		OrgID:              orgID,
		Plan:               "free",
		SubscriptionState:  SubscriptionStateActive,
		Credits:            0,
		Products:           map[string]bool{},
		FeatureFlags:       map[string]bool{},
		Entitlements:       defaultEntitlementsForPlan("free"),
		QuotaLimits:        defaultQuotaLimitsForPlan("free"),
		ProviderCustomerID: map[string]string{},
		Metadata:           map[string]interface{}{},
		CreatedAt:          time.Now().UTC(),
		UpdatedAt:          time.Now().UTC(),
	}
}
