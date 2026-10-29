package store

import (
	"context"
	"strings"
	"sync"
	"time"
)

type MemoryRepository struct {
	mu                          sync.RWMutex
	sessions                    map[string]ConnectSession
	stateIndex                  map[string]string
	connections                 map[string]Connection
	consents                    map[string]ConnectionConsent
	syncJobs                    map[string]SyncJob
	syncEvents                  map[string][]SyncEvent
	webhooks                    map[string]WebhookEvent
	tokenLeases                 map[string]TokenLease
	scimTokens                  map[string]memorySCIMToken
	audit                       []AuditEvent
	auditAttempts               map[string]int
	auditPublished              map[string]bool
	auditTerminal               map[string]bool
	auditTerminalAt             map[string]time.Time
	auditNextAttempt            map[string]time.Time
	auditProcessing             map[string]time.Time
	actionReceipts              map[string]ActionReceipt
	actionAuthorizationReceipts map[string]string

	emailSyncStates map[string]EmailSyncState
}

type memorySCIMToken struct {
	token SCIMToken
	hash  string
}

func NewMemoryRepository() *MemoryRepository {
	return &MemoryRepository{
		sessions:                    map[string]ConnectSession{},
		stateIndex:                  map[string]string{},
		connections:                 map[string]Connection{},
		consents:                    map[string]ConnectionConsent{},
		syncJobs:                    map[string]SyncJob{},
		syncEvents:                  map[string][]SyncEvent{},
		webhooks:                    map[string]WebhookEvent{},
		tokenLeases:                 map[string]TokenLease{},
		scimTokens:                  map[string]memorySCIMToken{},
		audit:                       []AuditEvent{},
		auditAttempts:               map[string]int{},
		auditPublished:              map[string]bool{},
		auditTerminal:               map[string]bool{},
		auditTerminalAt:             map[string]time.Time{},
		auditNextAttempt:            map[string]time.Time{},
		auditProcessing:             map[string]time.Time{},
		actionReceipts:              map[string]ActionReceipt{},
		actionAuthorizationReceipts: map[string]string{},

		emailSyncStates: map[string]EmailSyncState{},
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
	connection.Capabilities = nonNilStringSlice(connection.Capabilities)
	connection.Scopes = nonNilStringSlice(connection.Scopes)
	r.connections[connection.ID] = connection
	return connection, nil
}

func (r *MemoryRepository) ReconnectConnection(_ context.Context, connection Connection) (Connection, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	existing, ok := r.connections[connection.ID]
	if !ok || existing.DeletedAt != nil || (existing.Status != "active" && existing.Status != "needs_refresh") {
		return Connection{}, ErrConflict
	}
	connection.CreatedAt = existing.CreatedAt
	connection.UpdatedAt = time.Now().UTC()
	connection.ProviderContext = cloneStringMap(connection.ProviderContext)
	connection.Capabilities = nonNilStringSlice(connection.Capabilities)
	connection.Scopes = nonNilStringSlice(connection.Scopes)
	if guildID := existing.ProviderContext["guild_id"]; guildID != "" {
		connection.ProviderContext["guild_id"] = guildID
	}
	r.connections[connection.ID] = connection
	return connection, nil
}

func (r *MemoryRepository) UpdateConnectionCredentials(_ context.Context, connection Connection) (Connection, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	current, ok := r.connections[connection.ID]
	if !ok || current.DeletedAt != nil || (current.Status != "active" && current.Status != "needs_refresh") {
		return Connection{}, ErrConflict
	}
	current.EncryptedAccessToken = connection.EncryptedAccessToken
	current.EncryptedRefreshToken = connection.EncryptedRefreshToken
	current.AccessTokenExpiresAt = connection.AccessTokenExpiresAt
	current.LastRefreshedAt = connection.LastRefreshedAt
	current.Status = connection.Status
	current.Capabilities = nonNilStringSlice(connection.Capabilities)
	current.Scopes = nonNilStringSlice(connection.Scopes)
	current.UpdatedAt = time.Now().UTC()
	r.connections[connection.ID] = current
	return current, nil
}

func (r *MemoryRepository) UpdateConnectionSyncStatus(_ context.Context, connectionID, status string) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	connection, ok := r.connections[connectionID]
	if !ok || connection.DeletedAt != nil {
		return ErrNotFound
	}
	connection.LastSyncStatus = status
	connection.UpdatedAt = time.Now().UTC()
	r.connections[connectionID] = connection
	return nil
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

func (r *MemoryRepository) FindActiveConnectionByProviderAccount(_ context.Context, organizationID, connectorType, providerAccountID string) (Connection, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	for _, connection := range r.connections {
		if connection.OrganizationID != organizationID || connection.ConnectorType != connectorType || connection.ProviderAccountID != providerAccountID || connection.DeletedAt != nil {
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
	switch strings.ToLower(strings.TrimSpace(target)) {
	case "finspo-core", "email-worker":
		return "waiting_provider"
	default:
		return "handoff_data_plane"
	}
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

func (r *MemoryRepository) GetWebhookEvent(_ context.Context, organizationID, id string) (WebhookEvent, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	event, ok := r.webhooks[id]
	if !ok || (organizationID != "" && event.OrganizationID != organizationID) {
		return WebhookEvent{}, ErrNotFound
	}
	event.Payload = cloneAnyMap(event.Payload)
	return event, nil
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
	for _, existing := range r.audit {
		if existing.ID == event.ID {
			return nil
		}
	}
	r.audit = append(r.audit, event)
	r.auditNextAttempt[event.ID] = time.Now().UTC()
	return nil
}

func (r *MemoryRepository) ClaimAuditEvent(_ context.Context) (AuditEvent, bool, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	now := time.Now().UTC()
	for _, event := range r.audit {
		if r.auditPublished[event.ID] || r.auditTerminal[event.ID] || r.auditNextAttempt[event.ID].After(now) {
			continue
		}
		if processing := r.auditProcessing[event.ID]; !processing.IsZero() && processing.After(now.Add(-time.Minute)) {
			continue
		}
		r.auditAttempts[event.ID]++
		r.auditProcessing[event.ID] = now
		event.Attempts = r.auditAttempts[event.ID]
		return event, true, nil
	}
	return AuditEvent{}, false, nil
}

func (r *MemoryRepository) CompleteAuditEvent(_ context.Context, eventID string, attempts int) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.auditAttempts[eventID] != attempts || r.auditPublished[eventID] {
		return ErrConflict
	}
	r.auditPublished[eventID] = true
	delete(r.auditProcessing, eventID)
	return nil
}

func (r *MemoryRepository) FailAuditEvent(_ context.Context, eventID string, attempts int, nextAttempt time.Time, _ string, terminal bool) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.auditAttempts[eventID] != attempts || r.auditPublished[eventID] {
		return ErrConflict
	}
	delete(r.auditProcessing, eventID)
	r.auditNextAttempt[eventID] = nextAttempt
	r.auditTerminal[eventID] = terminal
	if terminal {
		r.auditTerminalAt[eventID] = time.Now().UTC()
	}
	return nil
}

func (r *MemoryRepository) AuditOutboxStats(_ context.Context) (AuditOutboxStats, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	now := time.Now().UTC()
	stats := AuditOutboxStats{}
	for _, event := range r.audit {
		if r.auditPublished[event.ID] {
			continue
		}
		if r.auditTerminal[event.ID] {
			stats.Terminal++
			terminalAt := r.auditTerminalAt[event.ID]
			if terminalAt.IsZero() {
				terminalAt = event.CreatedAt
			}
			age := now.Sub(terminalAt)
			if age > stats.OldestTerminalAge {
				stats.OldestTerminalAge = age
			}
			continue
		}
		stats.Pending++
		age := now.Sub(event.CreatedAt)
		if age > stats.OldestPendingAge {
			stats.OldestPendingAge = age
		}
	}
	return stats, nil
}

func (r *MemoryRepository) RequeueTerminalAuditEvents(_ context.Context, eventIDs []string) (int, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	requeued := 0
	for _, eventID := range eventIDs {
		if !r.auditTerminal[eventID] || r.auditPublished[eventID] {
			continue
		}
		r.auditTerminal[eventID] = false
		delete(r.auditTerminalAt, eventID)
		delete(r.auditProcessing, eventID)
		r.auditAttempts[eventID] = 0
		r.auditNextAttempt[eventID] = time.Now().UTC()
		requeued++
	}
	return requeued, nil
}

func actionReceiptKey(organizationID, idempotencyKey string) string {
	return strings.TrimSpace(organizationID) + "\x00" + strings.TrimSpace(idempotencyKey)
}

func actionAuthorizationReceiptKey(receipt ActionReceipt) string {
	return strings.TrimSpace(receipt.AttestationIssuer) + "\x00" +
		strings.TrimSpace(receipt.OrganizationID) + "\x00" + strings.TrimSpace(receipt.AuthorizationID)
}

func (r *MemoryRepository) ClaimActionReceipt(_ context.Context, receipt ActionReceipt) (ActionReceipt, bool, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	key := actionReceiptKey(receipt.OrganizationID, receipt.IdempotencyKey)
	if existing, ok := r.actionReceipts[key]; ok {
		if !sameActionReceiptBinding(existing, receipt) {
			return ActionReceipt{}, false, ErrConflict
		}
		return existing, false, nil
	}
	authorizationKey := actionAuthorizationReceiptKey(receipt)
	if existingKey, ok := r.actionAuthorizationReceipts[authorizationKey]; ok {
		existing := r.actionReceipts[existingKey]
		if !sameActionReceiptBinding(existing, receipt) {
			return ActionReceipt{}, false, ErrConflict
		}
		return existing, false, nil
	}
	now := time.Now().UTC()
	if receipt.CreatedAt.IsZero() {
		receipt.CreatedAt = now
	}
	receipt.UpdatedAt = now
	receipt.Status = "pending"
	r.actionReceipts[key] = receipt
	r.actionAuthorizationReceipts[authorizationKey] = key
	return receipt, true, nil
}

func (r *MemoryRepository) BeginActionReceiptExecution(_ context.Context, organizationID, idempotencyKey string) (ActionReceipt, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	key := actionReceiptKey(organizationID, idempotencyKey)
	receipt, ok := r.actionReceipts[key]
	if !ok {
		return ActionReceipt{}, ErrNotFound
	}
	if receipt.Status != "pending" {
		return ActionReceipt{}, ErrConflict
	}
	receipt.Status = "executing"
	receipt.UpdatedAt = time.Now().UTC()
	r.actionReceipts[key] = receipt
	return receipt, nil
}

func (r *MemoryRepository) CompleteActionReceipt(_ context.Context, organizationID, idempotencyKey, providerMessageID string) (ActionReceipt, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	key := actionReceiptKey(organizationID, idempotencyKey)
	receipt, ok := r.actionReceipts[key]
	if !ok {
		return ActionReceipt{}, ErrNotFound
	}
	if receipt.Status != "executing" {
		return ActionReceipt{}, ErrConflict
	}
	receipt.Status = "completed"
	receipt.ProviderMessageID = strings.TrimSpace(providerMessageID)
	receipt.UpdatedAt = time.Now().UTC()
	r.actionReceipts[key] = receipt
	return receipt, nil
}

func (r *MemoryRepository) MarkActionReceiptUnknown(_ context.Context, organizationID, idempotencyKey string) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	key := actionReceiptKey(organizationID, idempotencyKey)
	receipt, ok := r.actionReceipts[key]
	if !ok {
		return ErrNotFound
	}
	if receipt.Status != "executing" {
		return ErrConflict
	}
	receipt.Status = "unknown"
	receipt.UpdatedAt = time.Now().UTC()
	r.actionReceipts[key] = receipt
	return nil
}

func (r *MemoryRepository) Close() {}

func sameActionReceiptBinding(left, right ActionReceipt) bool {
	return left.OrganizationID == right.OrganizationID &&
		left.IdempotencyKey == right.IdempotencyKey &&
		left.RequestSHA256 == right.RequestSHA256 &&
		left.ConnectionID == right.ConnectionID &&
		left.ProviderKey == right.ProviderKey &&
		left.Operation == right.Operation &&
		left.AttestationIssuer == right.AttestationIssuer &&
		left.AuthorizationKind == right.AuthorizationKind &&
		left.AuthorizationID == right.AuthorizationID &&
		left.ApprovalID == right.ApprovalID &&
		left.ActionID == right.ActionID &&
		left.ActorID == right.ActorID &&
		left.PayloadSHA256 == right.PayloadSHA256
}

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

func (r *MemoryRepository) GetEmailSyncState(_ context.Context, connectionID string) (EmailSyncState, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	state, ok := r.emailSyncStates[connectionID]
	if !ok {
		return EmailSyncState{}, ErrNotFound
	}
	return state, nil
}

func (r *MemoryRepository) UpsertEmailSyncState(_ context.Context, state EmailSyncState) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.emailSyncStates == nil {
		r.emailSyncStates = map[string]EmailSyncState{}
	}
	if current, ok := r.emailSyncStates[state.ConnectionID]; ok && current.HistoryBackfillDays > state.HistoryBackfillDays {
		state.HistoryBackfillDays = current.HistoryBackfillDays
	}
	state.UpdatedAt = time.Now().UTC()
	r.emailSyncStates[state.ConnectionID] = state
	return nil
}

func (r *MemoryRepository) ExtendEmailSyncHistory(_ context.Context, connectionID, providerKey string, days, maxDays int) (EmailSyncState, error) {
	if strings.TrimSpace(connectionID) == "" || strings.TrimSpace(providerKey) == "" || days <= 0 || maxDays <= 0 {
		return EmailSyncState{}, ErrConflict
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	state := r.emailSyncStates[connectionID]
	state.ConnectionID = connectionID
	state.ProviderKey = providerKey
	state.HistoryBackfillDays = min(state.HistoryBackfillDays+days, maxDays)
	state.UpdatedAt = time.Now().UTC()
	r.emailSyncStates[connectionID] = state
	return state, nil
}

func (r *MemoryRepository) FindConnectionByWebhookAccount(_ context.Context, providerKeys []string, accountID string) (Connection, error) {
	accountID = strings.TrimSpace(accountID)
	if accountID == "" || len(providerKeys) == 0 {
		return Connection{}, ErrNotFound
	}
	r.mu.RLock()
	defer r.mu.RUnlock()
	allowed := map[string]bool{}
	for _, key := range providerKeys {
		allowed[key] = true
	}
	var matched *Connection
	for _, conn := range r.connections {
		if !allowed[conn.ProviderKey] || conn.DeletedAt != nil {
			continue
		}
		if conn.Status != "active" && conn.Status != "needs_refresh" {
			continue
		}
		if !connectionMatchesWebhookAccount(conn, accountID) {
			continue
		}
		if matched != nil {
			return Connection{}, ErrConflict
		}
		candidate := conn
		matched = &candidate
	}
	if matched == nil {
		return Connection{}, ErrNotFound
	}
	return *matched, nil
}

func connectionMatchesWebhookAccount(conn Connection, accountID string) bool {
	if conn.ProviderAccountID == accountID || conn.TenantID == accountID {
		return true
	}
	enriched := conn.ProviderContext["webhook_account_ids"]
	if enriched == "" {
		return false
	}
	for _, id := range strings.Split(enriched, ",") {
		if strings.TrimSpace(id) == accountID {
			return true
		}
	}
	return false
}

func (r *MemoryRepository) UpdateConnectionProviderContext(_ context.Context, id string, providerContext map[string]string) (Connection, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	conn, ok := r.connections[id]
	if !ok {
		return Connection{}, ErrNotFound
	}
	claimedIDs := splitWebhookAccountIDs(providerContext["webhook_account_ids"])
	metaFamily := map[string]bool{"meta": true, "facebook": true, "instagram": true, "whatsapp": true}
	if len(claimedIDs) > 0 && metaFamily[conn.ProviderKey] {
		for otherID, other := range r.connections {
			if otherID == id || !metaFamily[other.ProviderKey] || other.DeletedAt != nil || (other.Status != "active" && other.Status != "needs_refresh") {
				continue
			}
			for _, claimedID := range claimedIDs {
				if connectionMatchesWebhookAccount(other, claimedID) {
					return Connection{}, ErrConflict
				}
			}
		}
	}
	conn.ProviderContext = cloneStringMap(providerContext)
	conn.UpdatedAt = time.Now().UTC()
	r.connections[id] = conn
	return conn, nil
}

func splitWebhookAccountIDs(raw string) []string {
	parts := strings.Split(raw, ",")
	ids := make([]string, 0, len(parts))
	for _, part := range parts {
		if id := strings.TrimSpace(part); id != "" {
			ids = append(ids, id)
		}
	}
	return ids
}

func (r *MemoryRepository) BindConnectionProviderContext(_ context.Context, id, key, value string) (Connection, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	conn, ok := r.connections[id]
	if !ok || conn.DeletedAt != nil || (conn.Status != "active" && conn.Status != "needs_refresh") {
		return Connection{}, ErrConflict
	}
	if existing := conn.ProviderContext[key]; existing != "" && existing != value {
		return Connection{}, ErrConflict
	}
	providerContext := cloneStringMap(conn.ProviderContext)
	providerContext[key] = value
	conn.ProviderContext = providerContext
	conn.UpdatedAt = time.Now().UTC()
	r.connections[id] = conn
	return conn, nil
}
