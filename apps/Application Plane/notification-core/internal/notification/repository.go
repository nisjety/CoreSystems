package notification

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

const notificationRequestColumns = `
	id,
	organization_id,
	COALESCE(idempotency_key, ''),
	COALESCE(request_sha256, ''),
	retention_mode,
	recipient_kind,
	recipient_id,
	type,
	payload,
	source,
	status,
	provider,
	COALESCE(provider_request_id, ''),
	COALESCE(error_message, ''),
	created_at,
	updated_at,
	submitted_at,
	failed_at`

const deliveryAttemptColumns = `
	id,
	notification_id,
	attempt_number,
	status,
	worker_id,
	lease_expires_at,
	provider_request_id,
	provider_receipt_digest,
	error_code,
	created_at,
	updated_at,
	submitted_at,
	acknowledged_at`

const deliveryAttemptColumnsQualified = `
	attempt.id,
	attempt.notification_id,
	attempt.attempt_number,
	attempt.status,
	attempt.worker_id,
	attempt.lease_expires_at,
	attempt.provider_request_id,
	attempt.provider_receipt_digest,
	attempt.error_code,
	attempt.created_at,
	attempt.updated_at,
	attempt.submitted_at,
	attempt.acknowledged_at`

const feedProjectionColumns = `
	id,
	attempt_id,
	notification_id,
	provider_request_id,
	provider_receipt_digest,
	status,
	delivery_status,
	worker_id,
	lease_expires_at,
	error_code,
	created_at,
	updated_at,
	submitted_at,
	delivered_at,
	next_attempt_at`

const feedProjectionColumnsQualified = `
	projection.id,
	projection.attempt_id,
	projection.notification_id,
	projection.provider_request_id,
	projection.provider_receipt_digest,
	projection.status,
	projection.delivery_status,
	projection.worker_id,
	projection.lease_expires_at,
	projection.error_code,
	projection.created_at,
	projection.updated_at,
	projection.submitted_at,
	projection.delivered_at,
	projection.next_attempt_at`

type rowScanner interface {
	Scan(dest ...any) error
}

type PGRepository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *PGRepository {
	return &PGRepository{pool: pool}
}

func (r *PGRepository) FindByIdempotencyKey(ctx context.Context, organizationID, idempotencyKey string) (*StoredRequest, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}

	cleanKey := strings.TrimSpace(idempotencyKey)
	if cleanKey == "" {
		return nil, ErrNotFound
	}

	row := r.pool.QueryRow(ctx, `
SELECT `+notificationRequestColumns+`
FROM notification_requests
WHERE organization_id = $1 AND idempotency_key = $2`, strings.TrimSpace(organizationID), cleanKey)

	return scanStoredRequest(row)
}

func (r *PGRepository) FindByID(ctx context.Context, requestID string) (*StoredRequest, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}
	requestID = strings.TrimSpace(requestID)
	if requestID == "" {
		return nil, ErrNotFound
	}
	row := r.pool.QueryRow(ctx, `
SELECT `+notificationRequestColumns+`
FROM notification_requests
WHERE id = $1`, requestID)
	return scanStoredRequest(row)
}

func (r *PGRepository) Create(ctx context.Context, params CreateRequestParams) (*StoredRequest, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}

	payload, err := marshalPayload(params.Payload)
	if err != nil {
		return nil, err
	}

	row := r.pool.QueryRow(ctx, `
	INSERT INTO notification_requests (
		id,
		organization_id,
		idempotency_key,
		request_sha256,
		retention_mode,
		recipient_kind,
		recipient_id,
	type,
	payload,
	source,
	status,
	provider,
	created_at,
	updated_at
	) VALUES (
		$1,
		$2,
		NULLIF($3, ''),
		NULLIF($4, ''),
		$5,
		$6,
		$7,
		$8,
		$9::jsonb,
		$10,
		$11,
		$12,
		$13,
		$13
	)
RETURNING `+notificationRequestColumns,
		params.ID,
		params.OrganizationID,
		strings.TrimSpace(params.IdempotencyKey),
		strings.TrimSpace(params.RequestSHA256),
		params.RetentionMode,
		params.RecipientKind,
		params.RecipientID,
		params.Type,
		payload,
		params.Source,
		params.Status,
		params.Provider,
		params.OccurredAt.UTC(),
	)

	storedRequest, err := scanStoredRequest(row)
	if err == nil {
		return storedRequest, nil
	}

	var pgError *pgconn.PgError
	if errors.As(err, &pgError) && pgError.Code == "23505" {
		return nil, ErrAlreadyExists
	}

	return nil, err
}

func (r *PGRepository) MarkSubmitted(ctx context.Context, requestID string, providerRequestID string, occurredAt time.Time) (*StoredRequest, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}

	row := r.pool.QueryRow(ctx, `
UPDATE notification_requests
SET status = $2,
	provider_request_id = NULLIF($3, ''),
	submitted_at = $4,
	updated_at = $4,
	error_message = NULL
WHERE id = $1
RETURNING `+notificationRequestColumns,
		requestID,
		StatusSubmitted,
		strings.TrimSpace(providerRequestID),
		occurredAt.UTC(),
	)

	return scanStoredRequest(row)
}

func (r *PGRepository) MarkFailed(ctx context.Context, requestID string, failureMessage string, occurredAt time.Time) (*StoredRequest, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}

	row := r.pool.QueryRow(ctx, `
UPDATE notification_requests
SET status = $2,
	error_message = $3,
	failed_at = $4,
	updated_at = $4
WHERE id = $1
RETURNING `+notificationRequestColumns,
		requestID,
		StatusFailed,
		strings.TrimSpace(failureMessage),
		occurredAt.UTC(),
	)

	return scanStoredRequest(row)
}

// EnqueueDeliveryAttempt creates the first durable attempt idempotently. It
// stores no notification payload; the worker loads the parent request only
// after claiming the attempt.
func (r *PGRepository) EnqueueDeliveryAttempt(ctx context.Context, params DeliveryAttemptParams) (*DeliveryAttempt, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}
	if strings.TrimSpace(params.ID) == "" || strings.TrimSpace(params.NotificationID) == "" || params.AttemptNumber < 1 {
		return nil, fmt.Errorf("delivery attempt id, notification id, and positive attempt number are required")
	}
	occurredAt := params.OccurredAt.UTC()
	if occurredAt.IsZero() {
		occurredAt = time.Now().UTC()
	}
	row := r.pool.QueryRow(ctx, `
INSERT INTO notification_delivery_attempts (
	id, notification_id, attempt_number, status, created_at, updated_at
)
VALUES ($1, $2, $3, 'pending', $4, $4)
ON CONFLICT (notification_id, attempt_number) DO UPDATE
SET updated_at = notification_delivery_attempts.updated_at
RETURNING `+deliveryAttemptColumns,
		strings.TrimSpace(params.ID), strings.TrimSpace(params.NotificationID), params.AttemptNumber, occurredAt)
	return scanDeliveryAttempt(row)
}

// ClaimDeliveryAttempt leases one pending or expired claim. FOR UPDATE SKIP
// LOCKED makes this safe for multiple replicas; the worker id is part of every
// later mutation so a stale worker cannot finalize a newer lease.
func (r *PGRepository) ClaimDeliveryAttempt(ctx context.Context, workerID string, now time.Time, lease time.Duration) (*DeliveryAttempt, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}
	workerID = strings.TrimSpace(workerID)
	if workerID == "" || lease <= 0 {
		return nil, fmt.Errorf("delivery worker id and positive lease are required")
	}
	now = now.UTC()
	leaseUntil := now.Add(lease)
	row := r.pool.QueryRow(ctx, `
WITH candidate AS (
	SELECT id
	FROM notification_delivery_attempts
	WHERE status = 'pending'
	   OR (status = 'claimed' AND lease_expires_at IS NOT NULL AND lease_expires_at <= $2)
	ORDER BY created_at, id
	FOR UPDATE SKIP LOCKED
	LIMIT 1
)
UPDATE notification_delivery_attempts AS attempt
SET status = 'claimed', worker_id = $1, lease_expires_at = $3, updated_at = $2
FROM candidate
WHERE attempt.id = candidate.id
RETURNING `+deliveryAttemptColumnsQualified,
		workerID, now, leaseUntil)
	return scanDeliveryAttempt(row)
}

func (r *PGRepository) MarkDeliverySubmitted(ctx context.Context, attemptID, workerID, providerRequestID string, occurredAt time.Time) (*DeliveryAttempt, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}
	occurredAt = occurredAt.UTC()
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin delivery submission: %w", err)
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	row := tx.QueryRow(ctx, `
UPDATE notification_delivery_attempts
SET status = 'sent_unconfirmed',
	provider_request_id = NULLIF($3, ''),
	worker_id = '',
	lease_expires_at = NULL,
	submitted_at = $4,
	updated_at = $4
WHERE id = $1 AND worker_id = $2 AND status = 'claimed'
RETURNING `+deliveryAttemptColumns,
		strings.TrimSpace(attemptID), strings.TrimSpace(workerID), strings.TrimSpace(providerRequestID), occurredAt)
	attempt, err := scanDeliveryAttempt(row)
	if err != nil {
		return nil, err
	}

	// Keep the parent request state and the local projection obligation in the
	// same transaction as the provider correlation. A worker crash after the
	// provider accepted the request can therefore be reconciled without losing
	// the Activity/Inbox projection.
	var requestID string
	if err := tx.QueryRow(ctx, `
UPDATE notification_requests
SET status = $2,
	provider_request_id = NULLIF($3, ''),
	submitted_at = $4,
	updated_at = $4,
	error_message = NULL
WHERE id = $1
RETURNING id`, attempt.NotificationID, StatusSubmitted, strings.TrimSpace(providerRequestID), occurredAt).Scan(&requestID); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("mark notification request submitted: %w", err)
	}

	_, err = tx.Exec(ctx, `
INSERT INTO notification_feed_projection_attempts (
	id, attempt_id, notification_id, provider_request_id, status, delivery_status,
	created_at, updated_at, submitted_at, next_attempt_at
)
SELECT $1, $2, id, $3, 'pending', 'submitted', $4, $4, $4, $4
FROM notification_requests
WHERE id = $5 AND retention_mode <> $6
ON CONFLICT (attempt_id) DO UPDATE SET
	provider_request_id = EXCLUDED.provider_request_id,
	updated_at = EXCLUDED.updated_at`,
		strings.TrimSpace(attempt.ID)+":feed", strings.TrimSpace(attempt.ID), strings.TrimSpace(providerRequestID), occurredAt, requestID, RetentionModeZDR)
	if err != nil {
		return nil, fmt.Errorf("enqueue feed projection: %w", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit delivery submission: %w", err)
	}
	committed = true
	return attempt, nil
}

func (r *PGRepository) MarkDeliveryUnknown(ctx context.Context, attemptID, workerID, errorCode string, occurredAt time.Time) (*DeliveryAttempt, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}
	occurredAt = occurredAt.UTC()
	row := r.pool.QueryRow(ctx, `
UPDATE notification_delivery_attempts
SET status = 'unknown',
	error_code = NULLIF($3, ''),
	worker_id = '',
	lease_expires_at = NULL,
	updated_at = $4
WHERE id = $1 AND worker_id = $2 AND status IN ('claimed', 'sent_unconfirmed')
RETURNING `+deliveryAttemptColumns,
		strings.TrimSpace(attemptID), strings.TrimSpace(workerID), strings.TrimSpace(errorCode), occurredAt)
	return scanDeliveryAttempt(row)
}

func (r *PGRepository) MarkDeliveryAcknowledged(ctx context.Context, attemptID, providerRequestID, receiptDigest string, occurredAt time.Time) (*DeliveryAttempt, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}
	occurredAt = occurredAt.UTC()
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin delivery acknowledgement: %w", err)
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()
	row := tx.QueryRow(ctx, `
UPDATE notification_delivery_attempts
SET status = 'acknowledged',
	provider_receipt_digest = NULLIF($3, ''),
	acknowledged_at = COALESCE(acknowledged_at, $4),
	updated_at = $4
WHERE id = $1 AND status IN ('sent_unconfirmed', 'unknown')
  AND provider_request_id = NULLIF($2, '')
RETURNING `+deliveryAttemptColumns,
		strings.TrimSpace(attemptID), strings.TrimSpace(providerRequestID), strings.TrimSpace(receiptDigest), occurredAt)
	attempt, err := scanDeliveryAttempt(row)
	if err != nil {
		return nil, err
	}
	// If the feed row has not been projected yet, leave its durable outbox
	// obligation pending but carry the provider receipt forward. If it has
	// already been projected, return it to pending so the projector can
	// idempotently update the user-visible row to delivered.
	_, err = tx.Exec(ctx, `
UPDATE notification_feed_projection_attempts
SET provider_receipt_digest = $3,
	delivery_status = 'delivered',
	delivered_at = $4,
	status = CASE WHEN status = 'projected' THEN 'pending' ELSE status END,
	worker_id = CASE WHEN status = 'projected' THEN '' ELSE worker_id END,
	lease_expires_at = CASE WHEN status = 'projected' THEN NULL ELSE lease_expires_at END,
	next_attempt_at = CASE WHEN status = 'projected' THEN $4 ELSE next_attempt_at END,
	updated_at = $4
WHERE attempt_id = $1 AND provider_request_id = $2`,
		strings.TrimSpace(attemptID), strings.TrimSpace(providerRequestID), strings.TrimSpace(receiptDigest), occurredAt)
	if err != nil {
		return nil, fmt.Errorf("mark feed projection delivered: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit delivery acknowledgement: %w", err)
	}
	committed = true
	return attempt, nil
}

// ClaimFeedProjection leases one pending, unknown-due, or expired projection.
// The worker identity is required on every later transition so a stale
// replica cannot acknowledge a newer lease.
func (r *PGRepository) ClaimFeedProjection(ctx context.Context, workerID string, now time.Time, lease time.Duration) (*FeedProjection, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}
	workerID = strings.TrimSpace(workerID)
	if workerID == "" || lease <= 0 {
		return nil, fmt.Errorf("feed projection worker id and positive lease are required")
	}
	now = now.UTC()
	leaseUntil := now.Add(lease)
	row := r.pool.QueryRow(ctx, `
WITH candidate AS (
	SELECT id
	FROM notification_feed_projection_attempts
	WHERE (status IN ('pending', 'unknown') AND next_attempt_at <= $2)
	   OR (status = 'claimed' AND lease_expires_at IS NOT NULL AND lease_expires_at <= $2)
	ORDER BY next_attempt_at, created_at, id
	FOR UPDATE SKIP LOCKED
	LIMIT 1
)
UPDATE notification_feed_projection_attempts AS projection
SET status = 'claimed', worker_id = $1, lease_expires_at = $3,
	updated_at = $2
FROM candidate
WHERE projection.id = candidate.id
RETURNING `+feedProjectionColumnsQualified,
		workerID, now, leaseUntil)
	return scanFeedProjection(row)
}

func (r *PGRepository) MarkFeedProjectionProjected(ctx context.Context, projectionID, workerID string, occurredAt time.Time) (*FeedProjection, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}
	row := r.pool.QueryRow(ctx, `
UPDATE notification_feed_projection_attempts
SET status = 'projected', worker_id = '', lease_expires_at = NULL,
	error_code = '', updated_at = $3
WHERE id = $1 AND worker_id = $2 AND status = 'claimed'
RETURNING `+feedProjectionColumns,
		strings.TrimSpace(projectionID), strings.TrimSpace(workerID), occurredAt.UTC())
	return scanFeedProjection(row)
}

func (r *PGRepository) MarkFeedProjectionUnknown(ctx context.Context, projectionID, workerID, errorCode string, occurredAt time.Time) (*FeedProjection, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}
	occurredAt = occurredAt.UTC()
	row := r.pool.QueryRow(ctx, `
UPDATE notification_feed_projection_attempts
SET status = 'unknown', worker_id = '', lease_expires_at = NULL,
	error_code = NULLIF($3, ''), next_attempt_at = $4::timestamptz + INTERVAL '5 seconds', updated_at = $4
WHERE id = $1 AND worker_id = $2 AND status = 'claimed'
RETURNING `+feedProjectionColumns,
		strings.TrimSpace(projectionID), strings.TrimSpace(workerID), strings.TrimSpace(errorCode), occurredAt)
	return scanFeedProjection(row)
}

func (r *PGRepository) MarkDeliveryFailed(ctx context.Context, attemptID, workerID, errorCode string, occurredAt time.Time) (*DeliveryAttempt, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}
	occurredAt = occurredAt.UTC()
	row := r.pool.QueryRow(ctx, `
UPDATE notification_delivery_attempts
SET status = 'failed',
	error_code = NULLIF($3, ''),
	worker_id = '',
	lease_expires_at = NULL,
	updated_at = $4
WHERE id = $1 AND (worker_id = $2 OR (worker_id = '' AND status = 'unknown'))
	  AND status IN ('pending', 'claimed', 'sent_unconfirmed', 'unknown')
RETURNING `+deliveryAttemptColumns,
		strings.TrimSpace(attemptID), strings.TrimSpace(workerID), strings.TrimSpace(errorCode), occurredAt)
	return scanDeliveryAttempt(row)
}

func scanStoredRequest(scanner rowScanner) (*StoredRequest, error) {
	var storedRequest StoredRequest
	var payload []byte
	if err := scanner.Scan(
		&storedRequest.ID,
		&storedRequest.OrganizationID,
		&storedRequest.IdempotencyKey,
		&storedRequest.RequestSHA256,
		&storedRequest.RetentionMode,
		&storedRequest.RecipientKind,
		&storedRequest.RecipientID,
		&storedRequest.Type,
		&payload,
		&storedRequest.Source,
		&storedRequest.Status,
		&storedRequest.Provider,
		&storedRequest.ProviderRequestID,
		&storedRequest.ErrorMessage,
		&storedRequest.CreatedAt,
		&storedRequest.UpdatedAt,
		&storedRequest.SubmittedAt,
		&storedRequest.FailedAt,
	); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}

	if len(payload) != 0 {
		if err := json.Unmarshal(payload, &storedRequest.Payload); err != nil {
			return nil, fmt.Errorf("unmarshal notification payload: %w", err)
		}
	}

	return &storedRequest, nil
}

func scanDeliveryAttempt(scanner rowScanner) (*DeliveryAttempt, error) {
	var attempt DeliveryAttempt
	if err := scanner.Scan(
		&attempt.ID,
		&attempt.NotificationID,
		&attempt.AttemptNumber,
		&attempt.Status,
		&attempt.WorkerID,
		&attempt.LeaseExpiresAt,
		&attempt.ProviderRequestID,
		&attempt.ProviderReceiptDigest,
		&attempt.ErrorCode,
		&attempt.CreatedAt,
		&attempt.UpdatedAt,
		&attempt.SubmittedAt,
		&attempt.AcknowledgedAt,
	); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}
	return &attempt, nil
}

func scanFeedProjection(scanner rowScanner) (*FeedProjection, error) {
	var projection FeedProjection
	if err := scanner.Scan(
		&projection.ID,
		&projection.AttemptID,
		&projection.NotificationID,
		&projection.ProviderRequestID,
		&projection.ProviderReceiptDigest,
		&projection.Status,
		&projection.DeliveryStatus,
		&projection.WorkerID,
		&projection.LeaseExpiresAt,
		&projection.ErrorCode,
		&projection.CreatedAt,
		&projection.UpdatedAt,
		&projection.SubmittedAt,
		&projection.DeliveredAt,
		&projection.NextAttemptAt,
	); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}
	return &projection, nil
}

func marshalPayload(payload map[string]any) ([]byte, error) {
	if payload == nil {
		return []byte(`{}`), nil
	}

	encoded, err := json.Marshal(payload)
	if err != nil {
		return nil, fmt.Errorf("marshal notification payload: %w", err)
	}
	return encoded, nil
}
