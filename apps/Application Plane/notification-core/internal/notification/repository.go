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
