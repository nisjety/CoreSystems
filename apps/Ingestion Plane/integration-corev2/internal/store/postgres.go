package store

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

type PostgresRepository struct {
	pool *pgxpool.Pool
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
		INSERT INTO integration_audit_events (id, organization_id, user_id, connection_id, event_type, provider_key, metadata, created_at)
		VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
	`, event.ID, event.OrganizationID, event.UserID, event.ConnectionID, event.EventType, event.ProviderKey, metadata, event.CreatedAt)
	if err != nil {
		return fmt.Errorf("insert audit event: %w", err)
	}
	return nil
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
		SELECT connection_id, provider_key, cursor, last_synced_at, last_error, failure_count, updated_at
		FROM email_sync_state
		WHERE connection_id = $1
	`, connectionID).Scan(
		&state.ConnectionID, &state.ProviderKey, &state.Cursor,
		&lastSyncedAt, &state.LastError, &state.FailureCount, &state.UpdatedAt,
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
		INSERT INTO email_sync_state (connection_id, provider_key, cursor, last_synced_at, last_error, failure_count, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, now())
		ON CONFLICT (connection_id) DO UPDATE SET
			provider_key = EXCLUDED.provider_key,
			cursor = EXCLUDED.cursor,
			last_synced_at = EXCLUDED.last_synced_at,
			last_error = EXCLUDED.last_error,
			failure_count = EXCLUDED.failure_count,
			updated_at = now()
	`, state.ConnectionID, state.ProviderKey, state.Cursor, nullableTime(state.LastSyncedAt), state.LastError, state.FailureCount); err != nil {
		return fmt.Errorf("upsert email sync state: %w", err)
	}
	return nil
}
