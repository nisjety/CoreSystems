package billing

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

var (
	ErrNotFound            = errors.New("not found")
	ErrOrganizationDeleted = errors.New("billing organization is permanently deleted")
	ErrStaleAccountWrite   = errors.New("billing account changed since it was read")
	ErrUsageEventConflict  = errors.New("usage event_id is already bound to a different payload")
)

type Repository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *Repository {
	return &Repository{pool: pool}
}

// Ping verifies the database is reachable and this pool can authenticate, for
// the /health probe. A stale DB password surfaces here — which a port-only
// healthcheck silently misses while reads limp on stale pooled connections.
func (r *Repository) Ping(ctx context.Context) error {
	return r.pool.Ping(ctx)
}

// SaveAccountStateCAS creates a missing account or replaces an existing local
// state snapshot only when both its plan revision and updated_at version still
// match. Canonical Org plan revisions use ApplyOrganizationPlanRevision. This
// CAS prevents provider, checkout, hydration, and trial writers from applying
// a stale full snapshot over a newer canonical or same-revision write.
func (r *Repository) SaveAccountStateCAS(ctx context.Context, account Account) error {
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

	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin account upsert: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`, account.OrgID); err != nil {
		return fmt.Errorf("lock billing organization lifecycle: %w", err)
	}
	var tombstoned bool
	if err := tx.QueryRow(ctx,
		`SELECT EXISTS (SELECT 1 FROM billing_organization_tombstones WHERE org_id = $1)`,
		account.OrgID,
	).Scan(&tombstoned); err != nil {
		return fmt.Errorf("check billing organization tombstone: %w", err)
	}
	if tombstoned {
		return ErrOrganizationDeleted
	}

	var currentRevision int64
	var currentUpdatedAt time.Time
	err = tx.QueryRow(ctx, `
SELECT plan_revision, updated_at
FROM billing_accounts
WHERE org_id = $1
FOR UPDATE`, account.OrgID).Scan(&currentRevision, &currentUpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		_, err = tx.Exec(ctx, `
INSERT INTO billing_accounts (
  org_id, plan, plan_revision, subscription_state, credits,
  products, feature_flags, entitlements, quota_limits,
  provider_customer_id, metadata, trial_ends_at, updated_at
) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7::jsonb, $8::jsonb, $9::jsonb, $10::jsonb, $11::jsonb, $12, NOW())`,
			account.OrgID, account.Plan, account.PlanRevision, account.SubscriptionState, account.Credits,
			string(products), string(featureFlags), string(entitlements), string(quotaLimits),
			string(providers), string(metadata), account.TrialEndsAt,
		)
		if err != nil {
			return fmt.Errorf("create account state: %w", err)
		}
	} else if err != nil {
		return fmt.Errorf("read account state version: %w", err)
	} else {
		if account.UpdatedAt.IsZero() || account.PlanRevision != currentRevision || !account.UpdatedAt.Equal(currentUpdatedAt) {
			return ErrStaleAccountWrite
		}
		result, err := tx.Exec(ctx, `
UPDATE billing_accounts SET
  plan = $2,
  subscription_state = $3,
  credits = $4,
  products = $5::jsonb,
  feature_flags = $6::jsonb,
  entitlements = $7::jsonb,
  quota_limits = $8::jsonb,
  provider_customer_id = $9::jsonb,
  metadata = $10::jsonb,
  trial_ends_at = $11,
  updated_at = NOW()
WHERE org_id = $1 AND plan_revision = $12 AND updated_at = $13`,
			account.OrgID, account.Plan, account.SubscriptionState, account.Credits,
			string(products), string(featureFlags), string(entitlements), string(quotaLimits),
			string(providers), string(metadata), account.TrialEndsAt, account.PlanRevision, account.UpdatedAt,
		)
		if err != nil {
			return fmt.Errorf("update account state: %w", err)
		}
		if result.RowsAffected() != 1 {
			return ErrStaleAccountWrite
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit account state: %w", err)
	}
	return nil
}

// ApplyOrganizationPlanRevision atomically rejects stale/duplicate revisions,
// checks the permanent deletion tombstone under the lifecycle lock, and writes
// the derived account only when the incoming Org revision is newer.
func (r *Repository) ApplyOrganizationPlanRevision(ctx context.Context, account Account, revision int64) (bool, error) {
	if revision < 1 {
		return false, fmt.Errorf("plan revision must be positive")
	}
	products, err := json.Marshal(account.Products)
	if err != nil {
		return false, fmt.Errorf("marshal products: %w", err)
	}
	featureFlags, err := json.Marshal(account.FeatureFlags)
	if err != nil {
		return false, fmt.Errorf("marshal feature flags: %w", err)
	}
	entitlements, err := json.Marshal(account.Entitlements)
	if err != nil {
		return false, fmt.Errorf("marshal entitlements: %w", err)
	}
	quotaLimits, err := json.Marshal(account.QuotaLimits)
	if err != nil {
		return false, fmt.Errorf("marshal quota limits: %w", err)
	}
	providers, err := json.Marshal(account.ProviderCustomerID)
	if err != nil {
		return false, fmt.Errorf("marshal provider ids: %w", err)
	}
	metadata, err := json.Marshal(account.Metadata)
	if err != nil {
		return false, fmt.Errorf("marshal metadata: %w", err)
	}

	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return false, fmt.Errorf("begin revisioned plan apply: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`, account.OrgID); err != nil {
		return false, fmt.Errorf("lock billing organization lifecycle: %w", err)
	}
	var tombstoned bool
	if err := tx.QueryRow(ctx,
		`SELECT EXISTS (SELECT 1 FROM billing_organization_tombstones WHERE org_id = $1)`,
		account.OrgID,
	).Scan(&tombstoned); err != nil {
		return false, fmt.Errorf("check billing organization tombstone: %w", err)
	}
	if tombstoned {
		return false, ErrOrganizationDeleted
	}

	result, err := tx.Exec(ctx, `
INSERT INTO billing_accounts (
  org_id, plan, plan_revision, subscription_state, credits,
  products, feature_flags, entitlements, quota_limits,
  provider_customer_id, metadata, trial_ends_at, updated_at
) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7::jsonb, $8::jsonb, $9::jsonb, $10::jsonb, $11::jsonb, $12, NOW())
ON CONFLICT (org_id) DO UPDATE SET
  plan = EXCLUDED.plan,
  plan_revision = EXCLUDED.plan_revision,
  subscription_state = EXCLUDED.subscription_state,
  credits = EXCLUDED.credits,
  products = EXCLUDED.products,
  feature_flags = EXCLUDED.feature_flags,
  entitlements = EXCLUDED.entitlements,
  quota_limits = EXCLUDED.quota_limits,
  provider_customer_id = EXCLUDED.provider_customer_id,
  metadata = EXCLUDED.metadata,
  trial_ends_at = EXCLUDED.trial_ends_at,
  updated_at = NOW()
WHERE billing_accounts.plan_revision < EXCLUDED.plan_revision`,
		account.OrgID, account.Plan, revision, account.SubscriptionState, account.Credits,
		string(products), string(featureFlags), string(entitlements), string(quotaLimits),
		string(providers), string(metadata), account.TrialEndsAt,
	)
	if err != nil {
		return false, fmt.Errorf("apply revisioned organization plan: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return false, fmt.Errorf("commit revisioned organization plan: %w", err)
	}
	return result.RowsAffected() == 1, nil
}

func (r *Repository) IsOrganizationTombstoned(ctx context.Context, orgID string) (bool, error) {
	var tombstoned bool
	if err := r.pool.QueryRow(ctx,
		`SELECT EXISTS (SELECT 1 FROM billing_organization_tombstones WHERE org_id = $1)`,
		orgID,
	).Scan(&tombstoned); err != nil {
		return false, fmt.Errorf("check billing organization tombstone: %w", err)
	}
	return tombstoned, nil
}

func (r *Repository) TombstoneOrganization(ctx context.Context, orgID, reason string) error {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin organization deactivation: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`, orgID); err != nil {
		return fmt.Errorf("lock billing organization lifecycle: %w", err)
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO billing_organization_tombstones (org_id, reason)
VALUES ($1, $2)
ON CONFLICT (org_id) DO UPDATE SET reason = EXCLUDED.reason`, orgID, reason); err != nil {
		return fmt.Errorf("record billing organization tombstone: %w", err)
	}
	if _, err := tx.Exec(ctx, `
UPDATE billing_accounts
SET subscription_state = 'canceled',
    trial_ends_at = NULL,
    metadata = COALESCE(metadata, '{}'::jsonb) || jsonb_build_object(
      'deactivated_reason', $2::text,
      'deactivated_at', NOW()
    ),
    updated_at = NOW()
WHERE org_id = $1`, orgID, reason); err != nil {
		return fmt.Errorf("deactivate billing account: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit organization deactivation: %w", err)
	}
	return nil
}

func (r *Repository) GetAccount(ctx context.Context, orgID string) (Account, error) {
	query := `
		SELECT
			org_id, plan, plan_revision, subscription_state, credits,
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
		&account.PlanRevision,
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

// RecordUsage atomically reserves the caller-stable identity, appends the
// usage aggregate row, and creates the durable Lago delivery job. Repeated
// identical requests repair partial rows left by the pre-0007 implementation;
// conflicting reuse of an event id fails closed.
func (r *Repository) RecordUsage(ctx context.Context, event UsageEvent) (bool, error) {
	if err := ValidateUsageEvent(event); err != nil {
		return false, err
	}
	metadata, err := json.Marshal(event.Metadata)
	if err != nil {
		return false, fmt.Errorf("marshal usage metadata: %w", err)
	}
	payload := usageRetryPayload(event)
	payloadJSON, err := json.Marshal(payload)
	if err != nil {
		return false, fmt.Errorf("marshal usage retry payload: %w", err)
	}
	hash := sha256.Sum256(payloadJSON)
	payloadHash := hex.EncodeToString(hash[:])

	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return false, fmt.Errorf("begin usage record: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`, "billing-usage:"+event.EventID); err != nil {
		return false, fmt.Errorf("lock usage event: %w", err)
	}

	reserved, err := tx.Exec(ctx, `
INSERT INTO billing_usage_dedup (event_id, org_id, metric, occurred_at, payload_hash)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (event_id) DO NOTHING`, event.EventID, event.OrgID, event.Metric, event.OccurredAt, payloadHash)
	if err != nil {
		return false, fmt.Errorf("reserve usage event: %w", err)
	}
	if reserved.RowsAffected() == 0 {
		var sameIdentity bool
		var existingHash *string
		if err := tx.QueryRow(ctx, `
SELECT org_id = $2 AND metric = $3 AND occurred_at = $4, payload_hash
FROM billing_usage_dedup
WHERE event_id = $1`, event.EventID, event.OrgID, event.Metric, event.OccurredAt).Scan(
			&sameIdentity, &existingHash,
		); err != nil {
			return false, fmt.Errorf("read reserved usage event: %w", err)
		}
		if !sameIdentity || (existingHash != nil && *existingHash != payloadHash) {
			return false, ErrUsageEventConflict
		}
		if existingHash == nil {
			rows, err := tx.Query(ctx, `
SELECT id, quantity, source, metadata
FROM billing_usage_events
WHERE org_id = $1 AND metric = $2 AND occurred_at = $3 AND event_id IS NULL
ORDER BY id
LIMIT 2
FOR UPDATE`, event.OrgID, event.Metric, event.OccurredAt)
			if err != nil {
				return false, fmt.Errorf("read legacy usage aggregate: %w", err)
			}
			type legacyUsage struct {
				id       int64
				quantity float64
				source   string
				metadata []byte
			}
			legacy := make([]legacyUsage, 0, 2)
			for rows.Next() {
				var candidate legacyUsage
				if err := rows.Scan(&candidate.id, &candidate.quantity, &candidate.source, &candidate.metadata); err != nil {
					rows.Close()
					return false, fmt.Errorf("scan legacy usage aggregate: %w", err)
				}
				legacy = append(legacy, candidate)
			}
			if err := rows.Err(); err != nil {
				rows.Close()
				return false, fmt.Errorf("iterate legacy usage aggregate: %w", err)
			}
			rows.Close()
			if len(legacy) > 1 {
				return false, ErrUsageEventConflict
			}
			if len(legacy) == 1 {
				var legacyMetadata map[string]interface{}
				if err := json.Unmarshal(legacy[0].metadata, &legacyMetadata); err != nil {
					return false, fmt.Errorf("decode legacy usage metadata: %w", err)
				}
				legacyEvent := event
				legacyEvent.Quantity = legacy[0].quantity
				legacyEvent.Source = legacy[0].source
				legacyEvent.Metadata = legacyMetadata
				legacyPayload, err := json.Marshal(usageRetryPayload(legacyEvent))
				if err != nil {
					return false, fmt.Errorf("marshal legacy usage payload: %w", err)
				}
				legacyHash := sha256.Sum256(legacyPayload)
				if hex.EncodeToString(legacyHash[:]) != payloadHash {
					return false, ErrUsageEventConflict
				}
				if _, err := tx.Exec(ctx, `
UPDATE billing_usage_events SET event_id = $2 WHERE id = $1`, legacy[0].id, event.EventID); err != nil {
					return false, fmt.Errorf("bind legacy usage aggregate identity: %w", err)
				}
			}
			if _, err := tx.Exec(ctx, `
UPDATE billing_usage_dedup SET payload_hash = $2
WHERE event_id = $1 AND payload_hash IS NULL`, event.EventID, payloadHash); err != nil {
				return false, fmt.Errorf("bind legacy usage reservation payload: %w", err)
			}
		}
	}

	usageResult, err := tx.Exec(ctx, `
INSERT INTO billing_usage_events (event_id, org_id, metric, quantity, source, occurred_at, metadata)
VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)
ON CONFLICT DO NOTHING`,
		event.EventID, event.OrgID, event.Metric, event.Quantity, event.Source, event.OccurredAt, string(metadata),
	)
	if err != nil {
		return false, fmt.Errorf("insert usage event: %w", err)
	}

	dedupeKey := string(RetryJobKindLagoUsage) + ":" + event.EventID
	if _, err := tx.Exec(ctx, `
INSERT INTO billing_retry_jobs (kind, dedupe_key, payload, status, attempt_count, next_attempt_at)
VALUES ($1, $2, $3::jsonb, 'pending', 0, NOW())
ON CONFLICT (dedupe_key) DO NOTHING`, RetryJobKindLagoUsage, dedupeKey, string(payloadJSON)); err != nil {
		return false, fmt.Errorf("enqueue Lago usage delivery: %w", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return false, fmt.Errorf("commit usage record: %w", err)
	}
	return usageResult.RowsAffected() == 1, nil
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
	if limit <= 0 {
		return nil, fmt.Errorf("retry claim limit must be positive")
	}
	query := `
		WITH candidate AS (
			SELECT id
			FROM billing_retry_jobs
			WHERE (status = 'pending' AND next_attempt_at <= now())
			   OR (status = 'processing' AND updated_at <= now() - INTERVAL '5 minutes')
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
