package store

import (
	"context"
	"strings"
	"sync"
	"time"
)

type MemoryRepository struct {
	mu          sync.RWMutex
	sessions    map[string]ConnectSession
	stateIndex  map[string]string
	connections map[string]Connection
	consents    map[string]ConnectionConsent
	syncJobs    map[string]SyncJob
	syncEvents  map[string][]SyncEvent
	webhooks    map[string]WebhookEvent
	tokenLeases map[string]TokenLease
	scimTokens  map[string]memorySCIMToken
	audit       []AuditEvent
}

type memorySCIMToken struct {
	token SCIMToken
	hash  string
}

func NewMemoryRepository() *MemoryRepository {
	return &MemoryRepository{
		sessions:    map[string]ConnectSession{},
		stateIndex:  map[string]string{},
		connections: map[string]Connection{},
		consents:    map[string]ConnectionConsent{},
		syncJobs:    map[string]SyncJob{},
		syncEvents:  map[string][]SyncEvent{},
		webhooks:    map[string]WebhookEvent{},
		tokenLeases: map[string]TokenLease{},
		scimTokens:  map[string]memorySCIMToken{},
		audit:       []AuditEvent{},
	}
}

func (r *MemoryRepository) CreateConnectSession(_ context.Context, session ConnectSession) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, ok := r.sessions[session.ID]; ok {
		return ErrConflict
	}
	session.ProviderContext = cloneStringMap(session.ProviderContext)
	r.sessions[session.ID] = session
	r.stateIndex[session.StateHash] = session.ID
	return nil
}

func (r *MemoryRepository) GetConnectSessionByStateHash(_ context.Context, stateHash string) (ConnectSession, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	id, ok := r.stateIndex[stateHash]
	if !ok {
		return ConnectSession{}, ErrNotFound
	}
	session, ok := r.sessions[id]
	if !ok {
		return ConnectSession{}, ErrNotFound
	}
	session.ProviderContext = cloneStringMap(session.ProviderContext)
	return session, nil
}

func (r *MemoryRepository) GetConnectSessionByID(_ context.Context, id string) (ConnectSession, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	session, ok := r.sessions[id]
	if !ok {
		return ConnectSession{}, ErrNotFound
	}
	session.ProviderContext = cloneStringMap(session.ProviderContext)
	return session, nil
}

func (r *MemoryRepository) MarkConnectSessionConsumed(_ context.Context, id string, errorCode, errorDescription string) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	session, ok := r.sessions[id]
	if !ok {
		return ErrNotFound
	}
	now := time.Now().UTC()
	session.ConsumedAt = &now
	session.ErrorCode = errorCode
	session.ErrorDescription = errorDescription
	session.ProviderContext = cloneStringMap(session.ProviderContext)
	r.sessions[id] = session
	return nil
}

func (r *MemoryRepository) UpsertConnection(_ context.Context, connection Connection) (Connection, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	now := time.Now().UTC()
	if connection.CreatedAt.IsZero() {
		connection.CreatedAt = now
	}
	connection.UpdatedAt = now
	connection.ProviderContext = cloneStringMap(connection.ProviderContext)
	r.connections[connection.ID] = connection
	return connection, nil
}

func (r *MemoryRepository) ListConnections(_ context.Context, filter ConnectionFilter) ([]Connection, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	out := make([]Connection, 0, len(r.connections))
	for _, connection := range r.connections {
		if filter.OrganizationID != "" && connection.OrganizationID != filter.OrganizationID {
			continue
		}
		if filter.ProviderKey != "" && connection.ProviderKey != filter.ProviderKey {
			continue
		}
		if filter.ConnectorType != "" && connection.ConnectorType != filter.ConnectorType {
			continue
		}
		if filter.UserID != "" && connection.UserID != filter.UserID {
			continue
		}
		connection.ProviderContext = cloneStringMap(connection.ProviderContext)
		out = append(out, connection)
	}
	return out, nil
}

func (r *MemoryRepository) GetConnection(_ context.Context, id string) (Connection, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	connection, ok := r.connections[id]
	if !ok {
		return Connection{}, ErrNotFound
	}
	connection.ProviderContext = cloneStringMap(connection.ProviderContext)
	return connection, nil
}

func (r *MemoryRepository) FindActiveConnection(_ context.Context, organizationID, connectorType string) (Connection, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	for _, connection := range r.connections {
		if connection.OrganizationID != organizationID || connection.ConnectorType != connectorType || connection.DeletedAt != nil {
			continue
		}
		if connection.Status == "active" || connection.Status == "needs_refresh" {
			connection.ProviderContext = cloneStringMap(connection.ProviderContext)
			return connection, nil
		}
	}
	return Connection{}, ErrNotFound
}

func (r *MemoryRepository) MarkConnectionDeleted(_ context.Context, id string) (Connection, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	connection, ok := r.connections[id]
	if !ok {
		return Connection{}, ErrNotFound
	}
	now := time.Now().UTC()
	connection.DeletedAt = &now
	connection.Status = "deleted"
	connection.EncryptedAccessToken = ""
	connection.EncryptedRefreshToken = ""
	connection.UpdatedAt = now
	connection.ProviderContext = cloneStringMap(connection.ProviderContext)
	r.connections[id] = connection
	return connection, nil
}

func (r *MemoryRepository) UpdateConnectionCapabilities(_ context.Context, id string, capabilities []string) (Connection, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	connection, ok := r.connections[id]
	if !ok {
		return Connection{}, ErrNotFound
	}
	connection.Capabilities = append([]string{}, capabilities...)
	connection.UpdatedAt = time.Now().UTC()
	connection.ProviderContext = cloneStringMap(connection.ProviderContext)
	r.connections[id] = connection
	return connection, nil
}

func (r *MemoryRepository) UpsertConnectionConsent(_ context.Context, consent ConnectionConsent) (ConnectionConsent, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	now := time.Now().UTC()
	if consent.CreatedAt.IsZero() {
		consent.CreatedAt = now
	}
	consent.UpdatedAt = now
	if consent.ID == "" {
		consent.ID = consent.ConnectionID + ":" + consent.Source + ":" + consent.Purpose
	}
	consent.Metadata = cloneAnyMap(consent.Metadata)
	r.consents[consent.ID] = consent
	return consent, nil
}

func (r *MemoryRepository) ListConnectionConsents(_ context.Context, connectionID string) ([]ConnectionConsent, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	out := []ConnectionConsent{}
	for _, consent := range r.consents {
		if consent.ConnectionID != connectionID {
			continue
		}
		consent.Metadata = cloneAnyMap(consent.Metadata)
		out = append(out, consent)
	}
	return out, nil
}

func (r *MemoryRepository) CreateSyncJob(_ context.Context, job SyncJob) (SyncJob, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, ok := r.syncJobs[job.ID]; ok {
		return SyncJob{}, ErrConflict
	}
	now := time.Now().UTC()
	if job.CreatedAt.IsZero() {
		job.CreatedAt = now
	}
	if job.UpdatedAt.IsZero() {
		job.UpdatedAt = now
	}
	job.Checkpoint = cloneAnyMap(job.Checkpoint)
	job.Metadata = cloneAnyMap(job.Metadata)
	r.syncJobs[job.ID] = job
	return job, nil
}

func (r *MemoryRepository) GetSyncJob(_ context.Context, id string) (SyncJob, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	job, ok := r.syncJobs[id]
	if !ok {
		return SyncJob{}, ErrNotFound
	}
	job.Checkpoint = cloneAnyMap(job.Checkpoint)
	job.Metadata = cloneAnyMap(job.Metadata)
	return job, nil
}

func (r *MemoryRepository) ListSyncJobs(_ context.Context, filter SyncJobFilter) ([]SyncJob, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	out := []SyncJob{}
	for _, job := range r.syncJobs {
		if filter.OrganizationID != "" && job.OrganizationID != filter.OrganizationID {
			continue
		}
		if filter.ConnectionID != "" && job.ConnectionID != filter.ConnectionID {
			continue
		}
		if filter.ProviderKey != "" && job.ProviderKey != filter.ProviderKey {
			continue
		}
		if filter.Status != "" && job.Status != filter.Status {
			continue
		}
		job.Checkpoint = cloneAnyMap(job.Checkpoint)
		job.Metadata = cloneAnyMap(job.Metadata)
		out = append(out, job)
	}
	return out, nil
}

func (r *MemoryRepository) ClaimSyncJob(_ context.Context, claim SyncJobClaim) (SyncJob, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	target := strings.TrimSpace(claim.Target)
	status := syncClaimStatus(target)
	for _, job := range r.syncJobs {
		if job.Status != status {
			continue
		}
		if target != "" && stringFromAny(job.Metadata["handoffTarget"]) != target {
			continue
		}
		if claim.OrganizationID != "" && job.OrganizationID != claim.OrganizationID {
			continue
		}
		if claim.ProviderKey != "" && job.ProviderKey != claim.ProviderKey {
			continue
		}
		now := time.Now().UTC()
		if job.StartedAt == nil {
			job.StartedAt = &now
		}
		job.Status = "running"
		job.UpdatedAt = now
		job.Checkpoint = cloneAnyMap(mergeAnyMap(job.Checkpoint, claim.Checkpoint))
		job.Metadata = cloneAnyMap(mergeAnyMap(job.Metadata, claim.Metadata))
		r.syncJobs[job.ID] = job
		return job, nil
	}
	return SyncJob{}, ErrNotFound
}

func (r *MemoryRepository) UpdateSyncJob(_ context.Context, job SyncJob) (SyncJob, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	existing, ok := r.syncJobs[job.ID]
	if !ok {
		return SyncJob{}, ErrNotFound
	}
	if job.CreatedAt.IsZero() {
		job.CreatedAt = existing.CreatedAt
	}
	job.UpdatedAt = time.Now().UTC()
	job.Checkpoint = cloneAnyMap(job.Checkpoint)
	job.Metadata = cloneAnyMap(job.Metadata)
	r.syncJobs[job.ID] = job
	return job, nil
}

func syncClaimStatus(target string) string {
	if strings.EqualFold(strings.TrimSpace(target), "finspo-core") {
		return "waiting_provider"
	}
	return "handoff_data_plane"
}

func mergeAnyMap(base map[string]any, extra map[string]any) map[string]any {
	merged := cloneAnyMap(base)
	for key, value := range extra {
		merged[key] = value
	}
	return merged
}

func stringFromAny(value any) string {
	if value == nil {
		return ""
	}
	if str, ok := value.(string); ok {
		return strings.TrimSpace(str)
	}
	return ""
}

func (r *MemoryRepository) InsertSyncEvent(_ context.Context, event SyncEvent) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	event.Metadata = cloneAnyMap(event.Metadata)
	r.syncEvents[event.JobID] = append(r.syncEvents[event.JobID], event)
	return nil
}

func (r *MemoryRepository) ListSyncEvents(_ context.Context, jobID string) ([]SyncEvent, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	events := r.syncEvents[jobID]
	out := make([]SyncEvent, 0, len(events))
	for _, event := range events {
		event.Metadata = cloneAnyMap(event.Metadata)
		out = append(out, event)
	}
	return out, nil
}

func (r *MemoryRepository) InsertWebhookEvent(_ context.Context, event WebhookEvent) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, ok := r.webhooks[event.ID]; ok {
		return ErrConflict
	}
	event.Payload = cloneAnyMap(event.Payload)
	r.webhooks[event.ID] = event
	return nil
}

func (r *MemoryRepository) InsertTokenLease(_ context.Context, lease TokenLease) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, ok := r.tokenLeases[lease.ID]; ok {
		return ErrConflict
	}
	r.tokenLeases[lease.ID] = lease
	return nil
}

func (r *MemoryRepository) CreateSCIMToken(_ context.Context, token SCIMToken, tokenHash string) (SCIMToken, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, ok := r.scimTokens[token.ID]; ok {
		return SCIMToken{}, ErrConflict
	}
	now := time.Now().UTC()
	if token.CreatedAt.IsZero() {
		token.CreatedAt = now
	}
	if token.UpdatedAt.IsZero() {
		token.UpdatedAt = now
	}
	r.scimTokens[token.ID] = memorySCIMToken{token: token, hash: strings.TrimSpace(tokenHash)}
	return token, nil
}

func (r *MemoryRepository) ListSCIMTokens(_ context.Context, organizationID string) ([]SCIMToken, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	out := []SCIMToken{}
	for _, stored := range r.scimTokens {
		if stored.token.OrganizationID != organizationID {
			continue
		}
		out = append(out, stored.token)
	}
	return out, nil
}

func (r *MemoryRepository) FindActiveSCIMTokenByHash(_ context.Context, organizationID, tokenHash string) (SCIMToken, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	now := time.Now().UTC()
	for _, stored := range r.scimTokens {
		if stored.token.OrganizationID != organizationID || stored.hash != tokenHash || stored.token.RevokedAt != nil {
			continue
		}
		if stored.token.ExpiresAt != nil && stored.token.ExpiresAt.Before(now) {
			continue
		}
		return stored.token, nil
	}
	return SCIMToken{}, ErrNotFound
}

func (r *MemoryRepository) MarkSCIMTokenUsed(_ context.Context, id string, usedAt time.Time) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	stored, ok := r.scimTokens[id]
	if !ok {
		return ErrNotFound
	}
	stored.token.LastUsedAt = &usedAt
	stored.token.UpdatedAt = usedAt
	r.scimTokens[id] = stored
	return nil
}

func (r *MemoryRepository) RevokeSCIMToken(_ context.Context, organizationID, id string, revokedAt time.Time) (SCIMToken, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	stored, ok := r.scimTokens[id]
	if !ok || stored.token.OrganizationID != organizationID {
		return SCIMToken{}, ErrNotFound
	}
	stored.token.RevokedAt = &revokedAt
	stored.token.UpdatedAt = revokedAt
	r.scimTokens[id] = stored
	return stored.token, nil
}

func (r *MemoryRepository) InsertAuditEvent(_ context.Context, event AuditEvent) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.audit = append(r.audit, event)
	return nil
}

func (r *MemoryRepository) Close() {}

func cloneStringMap(input map[string]string) map[string]string {
	if len(input) == 0 {
		return map[string]string{}
	}
	out := make(map[string]string, len(input))
	for key, value := range input {
		out[key] = value
	}
	return out
}

func cloneAnyMap(input map[string]any) map[string]any {
	if len(input) == 0 {
		return map[string]any{}
	}
	out := make(map[string]any, len(input))
	for key, value := range input {
		out[key] = value
	}
	return out
}
