package store

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
)

type postgresTransactionPool struct {
	pgx.Tx
}

func (postgresTransactionPool) BeginTx(context.Context, pgx.TxOptions) (pgx.Tx, error) {
	return nil, fmt.Errorf("nested repository transaction is not supported")
}

func (postgresTransactionPool) Close() {}

func (r *PostgresRepository) WithAuditTransaction(ctx context.Context, fn func(AuditTransaction) error) error {
	if fn == nil {
		return fmt.Errorf("audit transaction callback is required")
	}
	tx, err := r.pool.BeginTx(ctx, pgx.TxOptions{})
	if err != nil {
		return fmt.Errorf("begin audit transaction: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	txRepository := &PostgresRepository{pool: postgresTransactionPool{Tx: tx}}
	if err := fn(txRepository); err != nil {
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit audit transaction: %w", err)
	}
	return nil
}

func (r *MemoryRepository) WithAuditTransaction(ctx context.Context, fn func(AuditTransaction) error) error {
	if fn == nil {
		return fmt.Errorf("audit transaction callback is required")
	}
	r.mu.Lock()
	defer r.mu.Unlock()

	working := NewMemoryRepository()
	working.connections = cloneConnections(r.connections)
	working.consents = cloneConsents(r.consents)
	working.syncJobs = cloneSyncJobs(r.syncJobs)
	working.syncEvents = cloneSyncEvents(r.syncEvents)
	working.tokenLeases = cloneTokenLeases(r.tokenLeases)
	working.scimTokens = cloneSCIMTokens(r.scimTokens)
	working.audit = cloneAuditEvents(r.audit)
	working.auditAttempts = cloneMap(r.auditAttempts)
	working.auditPublished = cloneMap(r.auditPublished)
	working.auditTerminal = cloneMap(r.auditTerminal)
	working.auditNextAttempt = cloneMap(r.auditNextAttempt)
	working.auditProcessing = cloneMap(r.auditProcessing)
	working.actionReceipts = cloneMap(r.actionReceipts)
	working.actionAuthorizationReceipts = cloneMap(r.actionAuthorizationReceipts)

	if err := fn(working); err != nil {
		return err
	}
	r.connections = working.connections
	r.consents = working.consents
	r.syncJobs = working.syncJobs
	r.syncEvents = working.syncEvents
	r.tokenLeases = working.tokenLeases
	r.scimTokens = working.scimTokens
	r.audit = working.audit
	r.auditAttempts = working.auditAttempts
	r.auditPublished = working.auditPublished
	r.auditTerminal = working.auditTerminal
	r.auditNextAttempt = working.auditNextAttempt
	r.auditProcessing = working.auditProcessing
	r.actionReceipts = working.actionReceipts
	r.actionAuthorizationReceipts = working.actionAuthorizationReceipts
	return nil
}

func cloneConnections(input map[string]Connection) map[string]Connection {
	output := make(map[string]Connection, len(input))
	for key, value := range input {
		value.ProviderContext = cloneStringMap(value.ProviderContext)
		value.Capabilities = nonNilStringSlice(value.Capabilities)
		value.Scopes = nonNilStringSlice(value.Scopes)
		output[key] = value
	}
	return output
}

func cloneConsents(input map[string]ConnectionConsent) map[string]ConnectionConsent {
	output := make(map[string]ConnectionConsent, len(input))
	for key, value := range input {
		value.Metadata = cloneAnyMap(value.Metadata)
		output[key] = value
	}
	return output
}

func cloneTokenLeases(input map[string]TokenLease) map[string]TokenLease {
	return cloneMap(input)
}

func cloneSyncJobs(input map[string]SyncJob) map[string]SyncJob {
	output := make(map[string]SyncJob, len(input))
	for key, value := range input {
		value.Checkpoint = cloneAnyMap(value.Checkpoint)
		value.Metadata = cloneAnyMap(value.Metadata)
		output[key] = value
	}
	return output
}

func cloneSyncEvents(input map[string][]SyncEvent) map[string][]SyncEvent {
	output := make(map[string][]SyncEvent, len(input))
	for key, values := range input {
		cloned := make([]SyncEvent, len(values))
		for index, value := range values {
			value.Metadata = cloneAnyMap(value.Metadata)
			cloned[index] = value
		}
		output[key] = cloned
	}
	return output
}

func cloneSCIMTokens(input map[string]memorySCIMToken) map[string]memorySCIMToken {
	return cloneMap(input)
}

func cloneAuditEvents(input []AuditEvent) []AuditEvent {
	output := make([]AuditEvent, len(input))
	for index, event := range input {
		event.Metadata = cloneAnyMap(event.Metadata)
		output[index] = event
	}
	return output
}

func cloneMap[K comparable, V any](input map[K]V) map[K]V {
	output := make(map[K]V, len(input))
	for key, value := range input {
		output[key] = value
	}
	return output
}
