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
)

func (r *PostgresRepository) UpdateConnectionCapabilities(ctx context.Context, id string, capabilities []string) (Connection, error) {
	var connection Connection
	err := r.pool.QueryRow(ctx, `
		UPDATE integration_connections
		SET capabilities = $2, updated_at = now()
		WHERE id = $1
		RETURNING `+connectionColumns,
		id, capabilities).Scan(connectionScanDest(&connection)...)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return Connection{}, ErrNotFound
		}
		return Connection{}, fmt.Errorf("update connection capabilities: %w", err)
	}
	return connection, nil
}

func (r *PostgresRepository) UpsertConnectionConsent(ctx context.Context, consent ConnectionConsent) (ConnectionConsent, error) {
	if consent.ID == "" {
		consent.ID = "consent_" + uuid.NewString()
	}
	if consent.CreatedAt.IsZero() {
		consent.CreatedAt = time.Now().UTC()
	}
	if consent.UpdatedAt.IsZero() {
		consent.UpdatedAt = time.Now().UTC()
	}
	metadata, err := json.Marshal(consent.Metadata)
	if err != nil {
		return ConnectionConsent{}, fmt.Errorf("marshal consent metadata: %w", err)
	}
	var out ConnectionConsent
	err = r.pool.QueryRow(ctx, `
		INSERT INTO integration_connection_consents (
			id, organization_id, connection_id, user_id, provider_key, source, purpose, granted,
			metadata, expires_at, created_at, updated_at, revoked_at
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
		ON CONFLICT (connection_id, source, purpose) DO UPDATE SET
			granted = EXCLUDED.granted,
			metadata = EXCLUDED.metadata,
			expires_at = EXCLUDED.expires_at,
			updated_at = now(),
			revoked_at = EXCLUDED.revoked_at
		RETURNING id, organization_id, connection_id, user_id, provider_key, source, purpose, granted,
			metadata, expires_at, created_at, updated_at, revoked_at
	`, consent.ID, consent.OrganizationID, consent.ConnectionID, consent.UserID, consent.ProviderKey,
		consent.Source, consent.Purpose, consent.Granted, metadata, consent.ExpiresAt, consent.CreatedAt,
		consent.UpdatedAt, consent.RevokedAt).Scan(consentScanDest(&out)...)
	if err != nil {
		return ConnectionConsent{}, fmt.Errorf("upsert connection consent: %w", err)
	}
	return out, nil
}

func (r *PostgresRepository) ListConnectionConsents(ctx context.Context, connectionID string) ([]ConnectionConsent, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT id, organization_id, connection_id, user_id, provider_key, source, purpose, granted,
			metadata, expires_at, created_at, updated_at, revoked_at
		FROM integration_connection_consents
		WHERE connection_id = $1
		ORDER BY updated_at DESC
	`, connectionID)
	if err != nil {
		return nil, fmt.Errorf("list connection consents: %w", err)
	}
	defer rows.Close()
	out := []ConnectionConsent{}
	for rows.Next() {
		var consent ConnectionConsent
		if err := rows.Scan(consentScanDest(&consent)...); err != nil {
			return nil, fmt.Errorf("scan connection consent: %w", err)
		}
		out = append(out, consent)
	}
	return out, rows.Err()
}

func (r *PostgresRepository) CreateSyncJob(ctx context.Context, job SyncJob) (SyncJob, error) {
	if job.ID == "" {
		job.ID = "sync_" + uuid.NewString()
	}
	now := time.Now().UTC()
	if job.CreatedAt.IsZero() {
		job.CreatedAt = now
	}
	if job.UpdatedAt.IsZero() {
		job.UpdatedAt = now
	}
	checkpoint, err := json.Marshal(job.Checkpoint)
	if err != nil {
		return SyncJob{}, fmt.Errorf("marshal sync checkpoint: %w", err)
	}
	metadata, err := json.Marshal(job.Metadata)
	if err != nil {
		return SyncJob{}, fmt.Errorf("marshal sync metadata: %w", err)
	}
	var out SyncJob
	err = r.pool.QueryRow(ctx, `
		INSERT INTO integration_sync_jobs (
			id, organization_id, connection_id, user_id, provider_key, status, reason, mode,
			checkpoint, metadata, created_at, updated_at, started_at, completed_at
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
		RETURNING id, organization_id, connection_id, user_id, provider_key, status, reason, mode,
			checkpoint, metadata, created_at, updated_at, started_at, completed_at
	`, job.ID, job.OrganizationID, job.ConnectionID, job.UserID, job.ProviderKey, job.Status, job.Reason,
		job.Mode, checkpoint, metadata, job.CreatedAt, job.UpdatedAt, job.StartedAt, job.CompletedAt).Scan(syncJobScanDest(&out)...)
	if err != nil {
		return SyncJob{}, fmt.Errorf("create sync job: %w", err)
	}
	return out, nil
}

func (r *PostgresRepository) GetSyncJob(ctx context.Context, id string) (SyncJob, error) {
	var job SyncJob
	err := r.pool.QueryRow(ctx, `
		SELECT id, organization_id, connection_id, user_id, provider_key, status, reason, mode,
			checkpoint, metadata, created_at, updated_at, started_at, completed_at
		FROM integration_sync_jobs
		WHERE id = $1
	`, id).Scan(syncJobScanDest(&job)...)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return SyncJob{}, ErrNotFound
		}
		return SyncJob{}, fmt.Errorf("get sync job: %w", err)
	}
	return job, nil
}

func (r *PostgresRepository) ListSyncJobs(ctx context.Context, filter SyncJobFilter) ([]SyncJob, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT id, organization_id, connection_id, user_id, provider_key, status, reason, mode,
			checkpoint, metadata, created_at, updated_at, started_at, completed_at
		FROM integration_sync_jobs
		WHERE ($1 = '' OR organization_id = $1)
		  AND ($2 = '' OR connection_id = $2)
		  AND ($3 = '' OR provider_key = $3)
		  AND ($4 = '' OR status = $4)
		ORDER BY created_at DESC
	`, filter.OrganizationID, filter.ConnectionID, filter.ProviderKey, filter.Status)
	if err != nil {
		return nil, fmt.Errorf("list sync jobs: %w", err)
	}
	defer rows.Close()
	out := []SyncJob{}
	for rows.Next() {
		var job SyncJob
		if err := rows.Scan(syncJobScanDest(&job)...); err != nil {
			return nil, fmt.Errorf("scan sync job: %w", err)
		}
		out = append(out, job)
	}
	return out, rows.Err()
}

func (r *PostgresRepository) ClaimSyncJob(ctx context.Context, claim SyncJobClaim) (SyncJob, error) {
	target := strings.TrimSpace(claim.Target)
	status := syncClaimStatus(target)
	checkpoint := claim.Checkpoint
	if checkpoint == nil {
		checkpoint = map[string]any{}
	}
	metadata := claim.Metadata
	if metadata == nil {
		metadata = map[string]any{}
	}
	checkpointJSON, err := json.Marshal(checkpoint)
	if err != nil {
		return SyncJob{}, fmt.Errorf("marshal sync claim checkpoint: %w", err)
	}
	metadataJSON, err := json.Marshal(metadata)
	if err != nil {
		return SyncJob{}, fmt.Errorf("marshal sync claim metadata: %w", err)
	}
	var out SyncJob
	err = r.pool.QueryRow(ctx, `
		UPDATE integration_sync_jobs
		SET status = 'running',
			checkpoint = checkpoint || $5::jsonb,
			metadata = metadata || $6::jsonb,
			updated_at = now(),
			started_at = COALESCE(started_at, now())
		WHERE id = (
			SELECT id
			FROM integration_sync_jobs
			WHERE status = $1
			  AND ($2 = '' OR metadata->>'handoffTarget' = $2)
			  AND ($3 = '' OR organization_id = $3)
			  AND ($4 = '' OR provider_key = $4)
			ORDER BY created_at ASC
			FOR UPDATE SKIP LOCKED
			LIMIT 1
		)
		RETURNING id, organization_id, connection_id, user_id, provider_key, status, reason, mode,
			checkpoint, metadata, created_at, updated_at, started_at, completed_at
	`, status, target, strings.TrimSpace(claim.OrganizationID), strings.TrimSpace(claim.ProviderKey), checkpointJSON, metadataJSON).Scan(syncJobScanDest(&out)...)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return SyncJob{}, ErrNotFound
		}
		return SyncJob{}, fmt.Errorf("claim sync job: %w", err)
	}
	return out, nil
}

func (r *PostgresRepository) UpdateSyncJob(ctx context.Context, job SyncJob) (SyncJob, error) {
	checkpoint, err := json.Marshal(job.Checkpoint)
	if err != nil {
		return SyncJob{}, fmt.Errorf("marshal sync checkpoint: %w", err)
	}
	metadata, err := json.Marshal(job.Metadata)
	if err != nil {
		return SyncJob{}, fmt.Errorf("marshal sync metadata: %w", err)
	}
	var out SyncJob
	err = r.pool.QueryRow(ctx, `
		UPDATE integration_sync_jobs
		SET status = $2, reason = $3, mode = $4, checkpoint = $5, metadata = $6,
			updated_at = now(), started_at = $7, completed_at = $8
		WHERE id = $1
		RETURNING id, organization_id, connection_id, user_id, provider_key, status, reason, mode,
			checkpoint, metadata, created_at, updated_at, started_at, completed_at
	`, job.ID, job.Status, job.Reason, job.Mode, checkpoint, metadata, job.StartedAt, job.CompletedAt).Scan(syncJobScanDest(&out)...)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return SyncJob{}, ErrNotFound
		}
		return SyncJob{}, fmt.Errorf("update sync job: %w", err)
	}
	return out, nil
}

func (r *PostgresRepository) InsertSyncEvent(ctx context.Context, event SyncEvent) error {
	if event.ID == "" {
		event.ID = "sync_evt_" + uuid.NewString()
	}
	if event.CreatedAt.IsZero() {
		event.CreatedAt = time.Now().UTC()
	}
	metadata, err := json.Marshal(event.Metadata)
	if err != nil {
		return fmt.Errorf("marshal sync event metadata: %w", err)
	}
	_, err = r.pool.Exec(ctx, `
		INSERT INTO integration_sync_events (id, job_id, type, message, metadata, created_at)
		VALUES ($1,$2,$3,$4,$5,$6)
	`, event.ID, event.JobID, event.Type, event.Message, metadata, event.CreatedAt)
	if err != nil {
		return fmt.Errorf("insert sync event: %w", err)
	}
	return nil
}

func (r *PostgresRepository) ListSyncEvents(ctx context.Context, jobID string) ([]SyncEvent, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT id, job_id, type, message, metadata, created_at
		FROM integration_sync_events
		WHERE job_id = $1
		ORDER BY created_at ASC
	`, jobID)
	if err != nil {
		return nil, fmt.Errorf("list sync events: %w", err)
	}
	defer rows.Close()
	out := []SyncEvent{}
	for rows.Next() {
		var event SyncEvent
		if err := rows.Scan(syncEventScanDest(&event)...); err != nil {
			return nil, fmt.Errorf("scan sync event: %w", err)
		}
		out = append(out, event)
	}
	return out, rows.Err()
}

func (r *PostgresRepository) InsertWebhookEvent(ctx context.Context, event WebhookEvent) error {
	if event.ID == "" {
		event.ID = "wh_" + uuid.NewString()
	}
	if event.ReceivedAt.IsZero() {
		event.ReceivedAt = time.Now().UTC()
	}
	payload, err := json.Marshal(event.Payload)
	if err != nil {
		return fmt.Errorf("marshal webhook payload: %w", err)
	}
	commandTag, err := r.pool.Exec(ctx, `
		INSERT INTO integration_webhook_events (id, organization_id, provider_key, event_type, signature_hash, payload, received_at)
		VALUES ($1,$2,$3,$4,$5,$6,$7)
		ON CONFLICT (id) DO NOTHING
	`, event.ID, event.OrganizationID, event.ProviderKey, event.EventType, event.SignatureHash, payload, event.ReceivedAt)
	if err != nil {
		return fmt.Errorf("insert webhook event: %w", err)
	}
	if commandTag.RowsAffected() == 0 {
		return ErrConflict
	}
	return nil
}

func (r *PostgresRepository) GetWebhookEvent(ctx context.Context, organizationID, id string) (WebhookEvent, error) {
	var event WebhookEvent
	var payload []byte
	err := r.pool.QueryRow(ctx, `
		SELECT id, organization_id, provider_key, event_type, signature_hash, payload, received_at
		FROM integration_webhook_events
		WHERE id = $1 AND ($2 = '' OR organization_id = $2)
	`, id, organizationID).Scan(
		&event.ID, &event.OrganizationID, &event.ProviderKey, &event.EventType,
		&event.SignatureHash, &payload, &event.ReceivedAt,
	)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return WebhookEvent{}, ErrNotFound
		}
		return WebhookEvent{}, fmt.Errorf("get webhook event: %w", err)
	}
	if len(payload) > 0 {
		if err := json.Unmarshal(payload, &event.Payload); err != nil {
			return WebhookEvent{}, fmt.Errorf("unmarshal webhook payload: %w", err)
		}
	}
	return event, nil
}

func (r *PostgresRepository) InsertTokenLease(ctx context.Context, lease TokenLease) error {
	if lease.ID == "" {
		lease.ID = "lease_" + uuid.NewString()
	}
	if lease.CreatedAt.IsZero() {
		lease.CreatedAt = time.Now().UTC()
	}
	_, err := r.pool.Exec(ctx, `
		INSERT INTO integration_token_leases (
			id, organization_id, connection_id, user_id, provider_key, connector_type, consumer, expires_at, created_at
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
	`, lease.ID, lease.OrganizationID, lease.ConnectionID, lease.UserID, lease.ProviderKey,
		lease.ConnectorType, lease.Consumer, lease.ExpiresAt, lease.CreatedAt)
	if err != nil {
		return fmt.Errorf("insert token lease: %w", err)
	}
	return nil
}

func (r *PostgresRepository) CreateSCIMToken(ctx context.Context, token SCIMToken, tokenHash string) (SCIMToken, error) {
	if token.ID == "" {
		token.ID = "scimtok_" + uuid.NewString()
	}
	now := time.Now().UTC()
	if token.CreatedAt.IsZero() {
		token.CreatedAt = now
	}
	if token.UpdatedAt.IsZero() {
		token.UpdatedAt = now
	}
	var out SCIMToken
	err := r.pool.QueryRow(ctx, `
		INSERT INTO integration_scim_tokens (
			id, organization_id, name, token_prefix, token_hash, created_by,
			last_used_at, expires_at, revoked_at, created_at, updated_at
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
		RETURNING id, organization_id, name, token_prefix, created_by,
			last_used_at, expires_at, revoked_at, created_at, updated_at
	`, token.ID, token.OrganizationID, token.Name, token.TokenPrefix, strings.TrimSpace(tokenHash),
		token.CreatedBy, token.LastUsedAt, token.ExpiresAt, token.RevokedAt, token.CreatedAt,
		token.UpdatedAt).Scan(scimTokenScanDest(&out)...)
	if err != nil {
		return SCIMToken{}, fmt.Errorf("create scim token: %w", err)
	}
	return out, nil
}

func (r *PostgresRepository) ListSCIMTokens(ctx context.Context, organizationID string) ([]SCIMToken, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT id, organization_id, name, token_prefix, created_by,
			last_used_at, expires_at, revoked_at, created_at, updated_at
		FROM integration_scim_tokens
		WHERE organization_id = $1
		ORDER BY created_at DESC
	`, organizationID)
	if err != nil {
		return nil, fmt.Errorf("list scim tokens: %w", err)
	}
	defer rows.Close()
	out := []SCIMToken{}
	for rows.Next() {
		var token SCIMToken
		if err := rows.Scan(scimTokenScanDest(&token)...); err != nil {
			return nil, fmt.Errorf("scan scim token: %w", err)
		}
		out = append(out, token)
	}
	return out, rows.Err()
}

func (r *PostgresRepository) FindActiveSCIMTokenByHash(ctx context.Context, organizationID, tokenHash string) (SCIMToken, error) {
	var token SCIMToken
	err := r.pool.QueryRow(ctx, `
		SELECT id, organization_id, name, token_prefix, created_by,
			last_used_at, expires_at, revoked_at, created_at, updated_at
		FROM integration_scim_tokens
		WHERE organization_id = $1
		  AND token_hash = $2
		  AND revoked_at IS NULL
		  AND (expires_at IS NULL OR expires_at > now())
		LIMIT 1
	`, organizationID, strings.TrimSpace(tokenHash)).Scan(scimTokenScanDest(&token)...)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return SCIMToken{}, ErrNotFound
		}
		return SCIMToken{}, fmt.Errorf("find active scim token: %w", err)
	}
	return token, nil
}

func (r *PostgresRepository) MarkSCIMTokenUsed(ctx context.Context, id string, usedAt time.Time) error {
	tag, err := r.pool.Exec(ctx, `
		UPDATE integration_scim_tokens
		SET last_used_at = $2, updated_at = $2
		WHERE id = $1
	`, id, usedAt)
	if err != nil {
		return fmt.Errorf("mark scim token used: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

func (r *PostgresRepository) RevokeSCIMToken(ctx context.Context, organizationID, id string, revokedAt time.Time) (SCIMToken, error) {
	var out SCIMToken
	err := r.pool.QueryRow(ctx, `
		UPDATE integration_scim_tokens
		SET revoked_at = COALESCE(revoked_at, $3), updated_at = $3
		WHERE organization_id = $1 AND id = $2
		RETURNING id, organization_id, name, token_prefix, created_by,
			last_used_at, expires_at, revoked_at, created_at, updated_at
	`, organizationID, id, revokedAt).Scan(scimTokenScanDest(&out)...)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return SCIMToken{}, ErrNotFound
		}
		return SCIMToken{}, fmt.Errorf("revoke scim token: %w", err)
	}
	return out, nil
}

func consentScanDest(consent *ConnectionConsent) []any {
	return []any{
		&consent.ID,
		&consent.OrganizationID,
		&consent.ConnectionID,
		&consent.UserID,
		&consent.ProviderKey,
		&consent.Source,
		&consent.Purpose,
		&consent.Granted,
		&anyMapJSON{target: &consent.Metadata},
		&consent.ExpiresAt,
		&consent.CreatedAt,
		&consent.UpdatedAt,
		&consent.RevokedAt,
	}
}

func scimTokenScanDest(token *SCIMToken) []any {
	return []any{
		&token.ID,
		&token.OrganizationID,
		&token.Name,
		&token.TokenPrefix,
		&token.CreatedBy,
		&token.LastUsedAt,
		&token.ExpiresAt,
		&token.RevokedAt,
		&token.CreatedAt,
		&token.UpdatedAt,
	}
}

func syncJobScanDest(job *SyncJob) []any {
	return []any{
		&job.ID,
		&job.OrganizationID,
		&job.ConnectionID,
		&job.UserID,
		&job.ProviderKey,
		&job.Status,
		&job.Reason,
		&job.Mode,
		&anyMapJSON{target: &job.Checkpoint},
		&anyMapJSON{target: &job.Metadata},
		&job.CreatedAt,
		&job.UpdatedAt,
		&job.StartedAt,
		&job.CompletedAt,
	}
}

func syncEventScanDest(event *SyncEvent) []any {
	return []any{
		&event.ID,
		&event.JobID,
		&event.Type,
		&event.Message,
		&anyMapJSON{target: &event.Metadata},
		&event.CreatedAt,
	}
}

type anyMapJSON struct {
	target *map[string]any
}

func (s *anyMapJSON) Scan(value any) error {
	if s.target == nil {
		return nil
	}
	if value == nil {
		*s.target = map[string]any{}
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
		*s.target = map[string]any{}
		return nil
	}
	var decoded map[string]any
	if err := json.Unmarshal(raw, &decoded); err != nil {
		return err
	}
	if decoded == nil {
		decoded = map[string]any{}
	}
	*s.target = decoded
	return nil
}
