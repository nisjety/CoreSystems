package store

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

type postgresPool interface {
	BeginTx(context.Context, pgx.TxOptions) (pgx.Tx, error)
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
	Close()
}

type PostgresRepository struct {
	pool postgresPool
}

func NewPostgresRepository(pool *pgxpool.Pool) *PostgresRepository {
	return &PostgresRepository{pool: pool}
}

func (r *PostgresRepository) WithConnectionRefreshLock(ctx context.Context, connectionID string, fn func(context.Context) error) error {
	tx, err := r.pool.BeginTx(ctx, pgx.TxOptions{})
	if err != nil {
		return fmt.Errorf("begin refresh lock transaction: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(3026, hashtext($1))`, connectionID); err != nil {
		return fmt.Errorf("acquire refresh lock: %w", err)
	}
	if err := fn(ctx); err != nil {
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit refresh lock transaction: %w", err)
	}
	return nil
}

func (r *PostgresRepository) CreateConnectSession(ctx context.Context, session ConnectSession) error {
	providerContext, err := json.Marshal(session.ProviderContext)
	if err != nil {
		return fmt.Errorf("marshal provider context: %w", err)
	}
	_, err = r.pool.Exec(ctx, `
		INSERT INTO integration_oauth_sessions (
			id, provider_key, connector_type, organization_id, workspace_id, user_id, user_email,
			state_hash, code_verifier_ciphertext, redirect_uri, return_url, provider_context, capabilities,
			scopes, expires_at, created_at
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
	`, session.ID, session.ProviderKey, session.ConnectorType, session.OrganizationID, session.WorkspaceID,
		session.UserID, session.UserEmail, session.StateHash, session.CodeVerifierCiphertext, session.RedirectURI,
		session.ReturnURL, providerContext, session.Capabilities, session.Scopes, session.ExpiresAt, session.CreatedAt)
	if err != nil {
		return fmt.Errorf("create connect session: %w", err)
	}
	return nil
}

func (r *PostgresRepository) GetConnectSessionByStateHash(ctx context.Context, stateHash string) (ConnectSession, error) {
	return r.scanSession(r.pool.QueryRow(ctx, `SELECT `+sessionColumns+` FROM integration_oauth_sessions WHERE state_hash = $1`, stateHash))
}

func (r *PostgresRepository) GetConnectSessionByID(ctx context.Context, id string) (ConnectSession, error) {
	return r.scanSession(r.pool.QueryRow(ctx, `SELECT `+sessionColumns+` FROM integration_oauth_sessions WHERE id = $1`, id))
}

func (r *PostgresRepository) MarkConnectSessionConsumed(ctx context.Context, id string, errorCode, errorDescription string) error {
	command, err := r.pool.Exec(ctx, `
		UPDATE integration_oauth_sessions
		SET consumed_at = now(), error_code = $2, error_description = $3
		WHERE id = $1
	`, id, errorCode, errorDescription)
	if err != nil {
		return fmt.Errorf("mark session consumed: %w", err)
	}
	if command.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

func (r *PostgresRepository) UpsertConnection(ctx context.Context, connection Connection) (Connection, error) {
	if connection.ID == "" {
		connection.ID = "conn_" + uuid.NewString()
	}
	providerContext, err := json.Marshal(connection.ProviderContext)
	if err != nil {
		return Connection{}, fmt.Errorf("marshal connection provider context: %w", err)
	}
	now := time.Now().UTC()
	if connection.CreatedAt.IsZero() {
		connection.CreatedAt = now
	}
	connection.UpdatedAt = now
	err = r.pool.QueryRow(ctx, `
		INSERT INTO integration_connections (
			id, provider_key, connector_type, organization_id, workspace_id, user_id, user_email, status,
			display_name, provider_account_id, tenant_id, provider_context, capabilities, scopes, encrypted_access_token,
			encrypted_refresh_token, access_token_expires_at, last_refreshed_at, last_sync_status,
			created_at, updated_at
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)
		ON CONFLICT (id) DO UPDATE SET
			status = EXCLUDED.status,
			display_name = EXCLUDED.display_name,
			provider_account_id = EXCLUDED.provider_account_id,
			tenant_id = EXCLUDED.tenant_id,
			provider_context = EXCLUDED.provider_context,
			capabilities = EXCLUDED.capabilities,
			scopes = EXCLUDED.scopes,
			encrypted_access_token = EXCLUDED.encrypted_access_token,
			encrypted_refresh_token = EXCLUDED.encrypted_refresh_token,
			access_token_expires_at = EXCLUDED.access_token_expires_at,
			last_refreshed_at = EXCLUDED.last_refreshed_at,
			last_sync_status = EXCLUDED.last_sync_status,
			updated_at = EXCLUDED.updated_at,
			deleted_at = NULL
		RETURNING `+connectionColumns,
		connection.ID, connection.ProviderKey, connection.ConnectorType, connection.OrganizationID, connection.WorkspaceID,
		connection.UserID, connection.UserEmail, connection.Status, connection.DisplayName, connection.ProviderAccountID,
		connection.TenantID, providerContext, connection.Capabilities, connection.Scopes, connection.EncryptedAccessToken,
		connection.EncryptedRefreshToken, connection.AccessTokenExpiresAt, nullableTime(connection.LastRefreshedAt),
		connection.LastSyncStatus, connection.CreatedAt, connection.UpdatedAt).Scan(connectionScanDest(&connection)...)
	if err != nil {
		return Connection{}, fmt.Errorf("upsert connection: %w", err)
	}
	return connection, nil
}

func (r *PostgresRepository) ReconnectConnection(ctx context.Context, connection Connection) (Connection, error) {
	providerContext, err := json.Marshal(connection.ProviderContext)
	if err != nil {
		return Connection{}, fmt.Errorf("marshal reconnect provider context: %w", err)
	}
	var saved Connection
	err = r.pool.QueryRow(ctx, `
		UPDATE integration_connections
		SET status = $2,
		    display_name = $3,
		    provider_account_id = $4,
		    tenant_id = $5,
		    provider_context = CASE
				WHEN COALESCE(provider_context, '{}'::jsonb) ? 'guild_id' THEN
					COALESCE($6::jsonb, '{}'::jsonb)
					|| jsonb_build_object('guild_id', provider_context->>'guild_id')
				ELSE COALESCE($6::jsonb, '{}'::jsonb)
			END,
		    capabilities = $7,
		    scopes = $8,
		    encrypted_access_token = $9,
		    encrypted_refresh_token = $10,
		    access_token_expires_at = $11,
		    last_refreshed_at = $12,
		    last_sync_status = $13,
		    updated_at = now()
		WHERE id = $1
		  AND deleted_at IS NULL
		  AND status IN ('active', 'needs_refresh')
		RETURNING `+connectionColumns,
		connection.ID,
		connection.Status,
		connection.DisplayName,
		connection.ProviderAccountID,
		connection.TenantID,
		providerContext,
		connection.Capabilities,
		connection.Scopes,
		connection.EncryptedAccessToken,
		connection.EncryptedRefreshToken,
		connection.AccessTokenExpiresAt,
		nullableTime(connection.LastRefreshedAt),
		connection.LastSyncStatus,
	).Scan(connectionScanDest(&saved)...)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return Connection{}, ErrConflict
		}
		return Connection{}, fmt.Errorf("reconnect connection: %w", err)
	}
	return saved, nil
}

func (r *PostgresRepository) UpdateConnectionCredentials(ctx context.Context, connection Connection) (Connection, error) {
	var saved Connection
	err := r.pool.QueryRow(ctx, `
		UPDATE integration_connections
		SET encrypted_access_token = $2,
		    encrypted_refresh_token = $3,
		    access_token_expires_at = $4,
		    last_refreshed_at = $5,
		    status = $6,
		    capabilities = $7,
		    scopes = $8,
		    updated_at = now()
		WHERE id = $1
		  AND deleted_at IS NULL
		  AND status IN ('active', 'needs_refresh')
		RETURNING `+connectionColumns,
		connection.ID,
		connection.EncryptedAccessToken,
		connection.EncryptedRefreshToken,
		connection.AccessTokenExpiresAt,
		nullableTime(connection.LastRefreshedAt),
		connection.Status,
		connection.Capabilities,
		connection.Scopes,
	).Scan(connectionScanDest(&saved)...)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return Connection{}, ErrConflict
		}
		return Connection{}, fmt.Errorf("update connection credentials: %w", err)
	}
	return saved, nil
}

func (r *PostgresRepository) ListConnections(ctx context.Context, filter ConnectionFilter) ([]Connection, error) {
	query := `SELECT ` + connectionColumns + ` FROM integration_connections WHERE ($1 = '' OR organization_id = $1) AND ($2 = '' OR provider_key = $2) AND ($3 = '' OR connector_type = $3) AND ($4 = '' OR user_id = $4) ORDER BY created_at DESC`
	rows, err := r.pool.Query(ctx, query, filter.OrganizationID, filter.ProviderKey, filter.ConnectorType, filter.UserID)
	if err != nil {
		return nil, fmt.Errorf("list connections: %w", err)
	}
	defer rows.Close()

	connections := []Connection{}
	for rows.Next() {
		var connection Connection
		if err := rows.Scan(connectionScanDest(&connection)...); err != nil {
			return nil, fmt.Errorf("scan connection: %w", err)
		}
		connections = append(connections, connection)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate connections: %w", err)
	}
	return connections, nil
}

func (r *PostgresRepository) GetConnection(ctx context.Context, id string) (Connection, error) {
	var connection Connection
	if err := r.pool.QueryRow(ctx, `SELECT `+connectionColumns+` FROM integration_connections WHERE id = $1`, id).Scan(connectionScanDest(&connection)...); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return Connection{}, ErrNotFound
		}
		return Connection{}, fmt.Errorf("get connection: %w", err)
	}
	return connection, nil
}

func (r *PostgresRepository) FindActiveConnection(ctx context.Context, organizationID, connectorType string) (Connection, error) {
	var connection Connection
	err := r.pool.QueryRow(ctx, `
		SELECT `+connectionColumns+`
		FROM integration_connections
		WHERE organization_id = $1
		  AND connector_type = $2
		  AND deleted_at IS NULL
		  AND status IN ('active', 'needs_refresh')
		ORDER BY updated_at DESC
		LIMIT 1
	`, organizationID, connectorType).Scan(connectionScanDest(&connection)...)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return Connection{}, ErrNotFound
		}
		return Connection{}, fmt.Errorf("find active connection: %w", err)
	}
	return connection, nil
}

func (r *PostgresRepository) MarkConnectionDeleted(ctx context.Context, id string) (Connection, error) {
	var connection Connection
	err := r.pool.QueryRow(ctx, `
		UPDATE integration_connections
		SET deleted_at = now(),
		    status = 'deleted',
		    encrypted_access_token = '',
		    encrypted_refresh_token = '',
		    updated_at = now()
		WHERE id = $1
		RETURNING `+connectionColumns,
		id).Scan(connectionScanDest(&connection)...)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return Connection{}, ErrNotFound
		}
		return Connection{}, fmt.Errorf("mark connection deleted: %w", err)
	}
	return connection, nil
}

func (r *PostgresRepository) InsertAuditEvent(ctx context.Context, event AuditEvent) error {
	if event.ID == "" {
		event.ID = "audit_" + uuid.NewString()
	}
	if event.CreatedAt.IsZero() {
		event.CreatedAt = time.Now().UTC()
	}
	metadata, err := json.Marshal(event.Metadata)
	if err != nil {
		return fmt.Errorf("marshal audit metadata: %w", err)
	}
	_, err = r.pool.Exec(ctx, `
		INSERT INTO integration_audit_events (
			id, organization_id, user_id, connection_id, event_type,
			provider_key, metadata, request_id, created_at
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
		ON CONFLICT (id) DO NOTHING
	`, event.ID, event.OrganizationID, event.UserID, event.ConnectionID, event.EventType, event.ProviderKey, metadata, event.RequestID, event.CreatedAt)
	if err != nil {
		return fmt.Errorf("insert audit event: %w", err)
	}
	return nil
}

func (r *PostgresRepository) ClaimAuditEvent(ctx context.Context) (AuditEvent, bool, error) {
	var event AuditEvent
	var metadata []byte
	err := r.pool.QueryRow(ctx, `
		SELECT id, organization_id, user_id, connection_id, event_type,
		       provider_key, metadata, request_id, created_at, attempts
		FROM claim_integration_audit_event()
	`).Scan(
		&event.ID, &event.OrganizationID, &event.UserID, &event.ConnectionID,
		&event.EventType, &event.ProviderKey, &metadata, &event.RequestID,
		&event.CreatedAt, &event.Attempts,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return AuditEvent{}, false, nil
	}
	if err != nil {
		return AuditEvent{}, false, fmt.Errorf("claim audit event: %w", err)
	}
	if err := json.Unmarshal(metadata, &event.Metadata); err != nil {
		return AuditEvent{}, false, fmt.Errorf("decode audit metadata: %w", err)
	}
	return event, true, nil
}

func (r *PostgresRepository) CompleteAuditEvent(ctx context.Context, eventID string, attempts int) error {
	command, err := r.pool.Exec(ctx, `
		UPDATE integration_audit_events
		SET published_at = now(), processing_at = NULL, last_error = NULL
		WHERE id = $1 AND attempts = $2 AND published_at IS NULL
	`, eventID, attempts)
	if err != nil {
		return fmt.Errorf("complete audit event: %w", err)
	}
	if command.RowsAffected() != 1 {
		return fmt.Errorf("complete audit event: lease lost")
	}
	return nil
}

func (r *PostgresRepository) FailAuditEvent(ctx context.Context, eventID string, attempts int, nextAttempt time.Time, message string, terminal bool) error {
	command, err := r.pool.Exec(ctx, `
		UPDATE integration_audit_events
		SET processing_at = NULL,
		    next_attempt_at = $3,
		    last_error = left($4, 2000),
		    terminal_at = CASE WHEN $5 THEN now() ELSE terminal_at END
		WHERE id = $1 AND attempts = $2 AND published_at IS NULL
	`, eventID, attempts, nextAttempt, message, terminal)
	if err != nil {
		return fmt.Errorf("fail audit event: %w", err)
	}
	if command.RowsAffected() != 1 {
		return fmt.Errorf("fail audit event: lease lost")
	}
	return nil
}

func (r *PostgresRepository) AuditOutboxStats(ctx context.Context) (AuditOutboxStats, error) {
	var stats AuditOutboxStats
	var pendingAgeSeconds, terminalAgeSeconds float64
	err := r.pool.QueryRow(ctx, `
		SELECT
			COUNT(*) FILTER (WHERE published_at IS NULL AND terminal_at IS NULL)::INT,
			COUNT(*) FILTER (WHERE published_at IS NULL AND terminal_at IS NOT NULL)::INT,
			COALESCE(EXTRACT(EPOCH FROM (now() - MIN(created_at) FILTER (
				WHERE published_at IS NULL AND terminal_at IS NULL
			))), 0)::DOUBLE PRECISION,
			COALESCE(EXTRACT(EPOCH FROM (now() - MIN(terminal_at) FILTER (
				WHERE published_at IS NULL AND terminal_at IS NOT NULL
			))), 0)::DOUBLE PRECISION
		FROM integration_audit_events
	`).Scan(&stats.Pending, &stats.Terminal, &pendingAgeSeconds, &terminalAgeSeconds)
	if err != nil {
		return AuditOutboxStats{}, fmt.Errorf("query audit outbox stats: %w", err)
	}
	stats.OldestPendingAge = time.Duration(pendingAgeSeconds * float64(time.Second))
	stats.OldestTerminalAge = time.Duration(terminalAgeSeconds * float64(time.Second))
	return stats, nil
}

func (r *PostgresRepository) RequeueTerminalAuditEvents(ctx context.Context, eventIDs []string) (int, error) {
	var requeued int
	if err := r.pool.QueryRow(ctx, `SELECT requeue_terminal_integration_audit_events($1::TEXT[])`, eventIDs).Scan(&requeued); err != nil {
		return 0, fmt.Errorf("requeue terminal audit events: %w", err)
	}
	return requeued, nil
}

func (r *PostgresRepository) ClaimActionReceipt(ctx context.Context, receipt ActionReceipt) (ActionReceipt, bool, error) {
	row := r.pool.QueryRow(ctx, `
		INSERT INTO integration_action_receipts (
			organization_id, idempotency_key, request_sha256, connection_id,
			provider_key, operation, attestation_issuer, attestation_kid,
			authorization_kind, authorization_id, approval_id, action_id,
			actor_id, attestation_jti, payload_sha256,
			status, provider_message_id, created_at, updated_at
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,'pending','',now(),now())
		ON CONFLICT DO NOTHING
		RETURNING organization_id, idempotency_key, request_sha256, connection_id,
			provider_key, operation, attestation_issuer, attestation_kid,
			authorization_kind, authorization_id, approval_id, action_id,
			actor_id, attestation_jti, payload_sha256,
			status, provider_message_id, created_at, updated_at
	`, receipt.OrganizationID, receipt.IdempotencyKey, receipt.RequestSHA256,
		receipt.ConnectionID, receipt.ProviderKey, receipt.Operation,
		receipt.AttestationIssuer, receipt.AttestationKeyID, receipt.AuthorizationKind,
		receipt.AuthorizationID, receipt.ApprovalID, receipt.ActionID, receipt.ActorID,
		receipt.AttestationJTI, receipt.PayloadSHA256)
	created, err := scanActionReceipt(row)
	if err == nil {
		return created, true, nil
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return ActionReceipt{}, false, fmt.Errorf("claim action receipt: %w", err)
	}
	existing, err := scanActionReceipt(r.pool.QueryRow(ctx, `
		SELECT organization_id, idempotency_key, request_sha256, connection_id,
			provider_key, operation, attestation_issuer, attestation_kid,
			authorization_kind, authorization_id, approval_id, action_id,
			actor_id, attestation_jti, payload_sha256,
			status, provider_message_id, created_at, updated_at
		FROM integration_action_receipts
		WHERE (organization_id = $1 AND idempotency_key = $2)
		   OR (organization_id = $1 AND attestation_issuer = $3 AND authorization_id = $4)
		ORDER BY CASE WHEN idempotency_key = $2 THEN 0 ELSE 1 END
		LIMIT 1
	`, receipt.OrganizationID, receipt.IdempotencyKey, receipt.AttestationIssuer, receipt.AuthorizationID))
	if err != nil {
		return ActionReceipt{}, false, fmt.Errorf("load action receipt after conflict: %w", err)
	}
	if !sameActionReceiptBinding(existing, receipt) {
		return ActionReceipt{}, false, ErrConflict
	}
	return existing, false, nil
}

func (r *PostgresRepository) BeginActionReceiptExecution(ctx context.Context, organizationID, idempotencyKey string) (ActionReceipt, error) {
	receipt, err := scanActionReceipt(r.pool.QueryRow(ctx, `
		UPDATE integration_action_receipts
		SET status = 'executing', updated_at = now()
		WHERE organization_id = $1 AND idempotency_key = $2 AND status = 'pending'
		RETURNING organization_id, idempotency_key, request_sha256, connection_id,
			provider_key, operation, attestation_issuer, attestation_kid,
			authorization_kind, authorization_id, approval_id, action_id,
			actor_id, attestation_jti, payload_sha256,
			status, provider_message_id, created_at, updated_at
	`, organizationID, idempotencyKey))
	if errors.Is(err, pgx.ErrNoRows) {
		return ActionReceipt{}, r.actionReceiptTransitionError(ctx, organizationID, idempotencyKey)
	}
	if err != nil {
		return ActionReceipt{}, fmt.Errorf("begin action receipt execution: %w", err)
	}
	return receipt, nil
}

func (r *PostgresRepository) CompleteActionReceipt(ctx context.Context, organizationID, idempotencyKey, providerMessageID string) (ActionReceipt, error) {
	receipt, err := scanActionReceipt(r.pool.QueryRow(ctx, `
		UPDATE integration_action_receipts
		SET status = 'completed', provider_message_id = $3, updated_at = now()
		WHERE organization_id = $1 AND idempotency_key = $2 AND status = 'executing'
		RETURNING organization_id, idempotency_key, request_sha256, connection_id,
			provider_key, operation, attestation_issuer, attestation_kid,
			authorization_kind, authorization_id, approval_id, action_id,
			actor_id, attestation_jti, payload_sha256,
			status, provider_message_id, created_at, updated_at
	`, organizationID, idempotencyKey, strings.TrimSpace(providerMessageID)))
	if errors.Is(err, pgx.ErrNoRows) {
		return ActionReceipt{}, r.actionReceiptTransitionError(ctx, organizationID, idempotencyKey)
	}
	if err != nil {
		return ActionReceipt{}, fmt.Errorf("complete action receipt: %w", err)
	}
	return receipt, nil
}

func (r *PostgresRepository) MarkActionReceiptUnknown(ctx context.Context, organizationID, idempotencyKey string) error {
	tag, err := r.pool.Exec(ctx, `
		UPDATE integration_action_receipts
		SET status = 'unknown', updated_at = now()
		WHERE organization_id = $1 AND idempotency_key = $2 AND status = 'executing'
	`, organizationID, idempotencyKey)
	if err != nil {
		return fmt.Errorf("mark action receipt unknown: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return r.actionReceiptTransitionError(ctx, organizationID, idempotencyKey)
	}
	return nil
}

func (r *PostgresRepository) actionReceiptTransitionError(ctx context.Context, organizationID, idempotencyKey string) error {
	var status string
	err := r.pool.QueryRow(ctx, `
		SELECT status
		FROM integration_action_receipts
		WHERE organization_id = $1 AND idempotency_key = $2
	`, organizationID, idempotencyKey).Scan(&status)
	if errors.Is(err, pgx.ErrNoRows) {
		return ErrNotFound
	}
	if err != nil {
		return fmt.Errorf("resolve action receipt transition: %w", err)
	}
	return ErrConflict
}

func scanActionReceipt(row pgx.Row) (ActionReceipt, error) {
	var receipt ActionReceipt
	err := row.Scan(
		&receipt.OrganizationID,
		&receipt.IdempotencyKey,
		&receipt.RequestSHA256,
		&receipt.ConnectionID,
		&receipt.ProviderKey,
		&receipt.Operation,
		&receipt.AttestationIssuer,
		&receipt.AttestationKeyID,
		&receipt.AuthorizationKind,
		&receipt.AuthorizationID,
		&receipt.ApprovalID,
		&receipt.ActionID,
		&receipt.ActorID,
		&receipt.AttestationJTI,
		&receipt.PayloadSHA256,
		&receipt.Status,
		&receipt.ProviderMessageID,
		&receipt.CreatedAt,
		&receipt.UpdatedAt,
	)
	return receipt, err
}

func (r *PostgresRepository) Close() {
	r.pool.Close()
}

const sessionColumns = `id, provider_key, connector_type, organization_id, workspace_id, user_id, user_email, state_hash, code_verifier_ciphertext, redirect_uri, return_url, provider_context, capabilities, scopes, expires_at, created_at, consumed_at, error_code, error_description`

func (r *PostgresRepository) scanSession(row pgx.Row) (ConnectSession, error) {
	var session ConnectSession
	var providerContext []byte
	err := row.Scan(
		&session.ID,
		&session.ProviderKey,
		&session.ConnectorType,
		&session.OrganizationID,
		&session.WorkspaceID,
		&session.UserID,
		&session.UserEmail,
		&session.StateHash,
		&session.CodeVerifierCiphertext,
		&session.RedirectURI,
		&session.ReturnURL,
		&providerContext,
		&session.Capabilities,
		&session.Scopes,
		&session.ExpiresAt,
		&session.CreatedAt,
		&session.ConsumedAt,
		&session.ErrorCode,
		&session.ErrorDescription,
	)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return ConnectSession{}, ErrNotFound
		}
		return ConnectSession{}, fmt.Errorf("scan connect session: %w", err)
	}
	if len(providerContext) > 0 {
		if err := json.Unmarshal(providerContext, &session.ProviderContext); err != nil {
			return ConnectSession{}, fmt.Errorf("decode provider context: %w", err)
		}
	}
	if session.ProviderContext == nil {
		session.ProviderContext = map[string]string{}
	}
	return session, nil
}

const connectionColumns = `id, provider_key, connector_type, organization_id, workspace_id, user_id, user_email, status, display_name, provider_account_id, tenant_id, provider_context, capabilities, scopes, encrypted_access_token, encrypted_refresh_token, access_token_expires_at, COALESCE(last_refreshed_at, '0001-01-01T00:00:00Z'::timestamptz), last_sync_status, created_at, updated_at, deleted_at`

func connectionScanDest(connection *Connection) []any {
	return []any{
		&connection.ID,
		&connection.ProviderKey,
		&connection.ConnectorType,
		&connection.OrganizationID,
		&connection.WorkspaceID,
		&connection.UserID,
		&connection.UserEmail,
		&connection.Status,
		&connection.DisplayName,
		&connection.ProviderAccountID,
		&connection.TenantID,
		&stringMapJSON{target: &connection.ProviderContext},
		&connection.Capabilities,
		&connection.Scopes,
		&connection.EncryptedAccessToken,
		&connection.EncryptedRefreshToken,
		&connection.AccessTokenExpiresAt,
		&connection.LastRefreshedAt,
		&connection.LastSyncStatus,
		&connection.CreatedAt,
		&connection.UpdatedAt,
		&connection.DeletedAt,
	}
}

type stringMapJSON struct {
	target *map[string]string
}

func (s *stringMapJSON) Scan(value any) error {
	if s.target == nil {
		return nil
	}
	if value == nil {
		*s.target = map[string]string{}
		return nil
	}
	var raw []byte
	switch typed := value.(type) {
	case []byte:
		raw = typed
	case string:
		raw = []byte(typed)
	default:
		return fmt.Errorf("unsupported json map value %T", value)
	}
	if len(raw) == 0 {
		*s.target = map[string]string{}
		return nil
	}
	var decoded map[string]string
	if err := json.Unmarshal(raw, &decoded); err != nil {
		return err
	}
	if decoded == nil {
		decoded = map[string]string{}
	}
	*s.target = decoded
	return nil
}

func nullableTime(value time.Time) any {
	if value.IsZero() {
		return nil
	}
	return value
}

func (r *PostgresRepository) GetEmailSyncState(ctx context.Context, connectionID string) (EmailSyncState, error) {
	var state EmailSyncState
	var lastSyncedAt *time.Time
	err := r.pool.QueryRow(ctx, `
		SELECT connection_id, provider_key, cursor, last_synced_at, last_error, failure_count, history_backfill_days, updated_at
		FROM email_sync_state
		WHERE connection_id = $1
	`, connectionID).Scan(
		&state.ConnectionID, &state.ProviderKey, &state.Cursor,
		&lastSyncedAt, &state.LastError, &state.FailureCount, &state.HistoryBackfillDays, &state.UpdatedAt,
	)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return EmailSyncState{}, ErrNotFound
		}
		return EmailSyncState{}, fmt.Errorf("get email sync state: %w", err)
	}
	if lastSyncedAt != nil {
		state.LastSyncedAt = *lastSyncedAt
	}
	return state, nil
}

func (r *PostgresRepository) UpsertEmailSyncState(ctx context.Context, state EmailSyncState) error {
	if _, err := r.pool.Exec(ctx, `
		INSERT INTO email_sync_state (connection_id, provider_key, cursor, last_synced_at, last_error, failure_count, history_backfill_days, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, now())
		ON CONFLICT (connection_id) DO UPDATE SET
			provider_key = EXCLUDED.provider_key,
			cursor = EXCLUDED.cursor,
			last_synced_at = EXCLUDED.last_synced_at,
			last_error = EXCLUDED.last_error,
			failure_count = EXCLUDED.failure_count,
			history_backfill_days = GREATEST(email_sync_state.history_backfill_days, EXCLUDED.history_backfill_days),
			updated_at = now()
	`, state.ConnectionID, state.ProviderKey, state.Cursor, nullableTime(state.LastSyncedAt), state.LastError, state.FailureCount, state.HistoryBackfillDays); err != nil {
		return fmt.Errorf("upsert email sync state: %w", err)
	}
	return nil
}

func (r *PostgresRepository) ExtendEmailSyncHistory(ctx context.Context, connectionID, providerKey string, days, maxDays int) (EmailSyncState, error) {
	if strings.TrimSpace(connectionID) == "" || strings.TrimSpace(providerKey) == "" || days <= 0 || maxDays <= 0 {
		return EmailSyncState{}, ErrConflict
	}
	var state EmailSyncState
	var lastSyncedAt *time.Time
	err := r.pool.QueryRow(ctx, `
		INSERT INTO email_sync_state (connection_id, provider_key, history_backfill_days)
		VALUES ($1, $2, LEAST($3::integer, $4::integer))
		ON CONFLICT (connection_id) DO UPDATE SET
			provider_key = EXCLUDED.provider_key,
			history_backfill_days = LEAST($4::integer, email_sync_state.history_backfill_days + $3::integer),
			updated_at = now()
		RETURNING connection_id, provider_key, cursor, last_synced_at, last_error, failure_count, history_backfill_days, updated_at
	`, connectionID, providerKey, days, maxDays).Scan(
		&state.ConnectionID, &state.ProviderKey, &state.Cursor, &lastSyncedAt,
		&state.LastError, &state.FailureCount, &state.HistoryBackfillDays, &state.UpdatedAt,
	)
	if err != nil {
		return EmailSyncState{}, fmt.Errorf("extend email sync history: %w", err)
	}
	if lastSyncedAt != nil {
		state.LastSyncedAt = *lastSyncedAt
	}
	return state, nil
}
