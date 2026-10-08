package api

import (
	"context"
	"errors"
	"fmt"

	"github.com/triodelab/integration-corev2/internal/providers"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/zdr"
)

// Assessment of model connections for zero data retention (Z-12).
//
// A model connection (a provider of category "ai") gets a ZDR capability record when it is
// created and again when it becomes connected, and every existing one gets one from the
// idempotent reconcile that runs at start. Nothing here decides what a turn may do: the record
// is the input to the acknowledgement API and the decision endpoint that ship later. A
// connection with no record is unassessed and every reader must treat it as cannot_verify.
//
// The inputs are server-side facts only: the provider key is the connection's own, the auth
// mode is the provider CATALOG's (never the connection's provider_context, which some routes
// let a client fill), and the digest is computed by package zdr and by nothing else.

// Audit event types written by the assessment. Content-free: ids, enumerations and digests.
const (
	auditEventZDRAssessed    = "subscription_connection.zdr_assessed"
	auditEventZDRInvalidated = "subscription_connection.zdr_acknowledgement_invalidated"
	auditChangeZDRCreated    = "created"
	auditChangeZDRChanged    = "changed"
)

// modelConnectionAuthMode reports whether the connection is a model connection (its provider is
// in the catalog under category "ai" with the same connector type) and, if so, the catalog's
// auth mode for it. Only an exact provider key matches, so an alias is never promoted to the
// provider it names.
func modelConnectionAuthMode(connection store.Connection) (string, bool) {
	for _, provider := range providers.Catalog() {
		if provider.Category == "ai" && provider.Key == connection.ProviderKey && provider.ConnectorType == connection.ConnectorType {
			return provider.AuthType, true
		}
	}
	return "", false
}

// applyConnectionZDR assesses connection and records the outcome inside the caller's audit
// transaction: the record, any acknowledgement revoked because the digest moved, and the audit
// rows that say so commit together or not at all. It does nothing for a connection that is not
// a model connection (applied false). An error means the assessment could not be recorded and
// the caller's transaction must roll back: a connection is never left half-assessed.
func applyConnectionZDR(
	ctx context.Context,
	tx store.AuditTransaction,
	persist func(store.AuditEvent) error,
	connection store.Connection,
) (result store.ConnectionZDRApplyResult, applied bool, err error) {
	authMode, model := modelConnectionAuthMode(connection)
	if !model {
		return store.ConnectionZDRApplyResult{}, false, nil
	}
	assessor, err := zdr.Default()
	if err != nil {
		return store.ConnectionZDRApplyResult{}, false, fmt.Errorf("load connection ZDR capability contract: %w", err)
	}
	assessment, err := assessor.Assess(connection.ProviderKey, authMode)
	if err != nil {
		return store.ConnectionZDRApplyResult{}, false, fmt.Errorf("assess connection %s: %w", connection.ID, err)
	}
	result, err = tx.ApplyConnectionZDRAssessment(ctx, store.NewConnectionZDRAssessment(connection.ID, assessment))
	if err != nil {
		return store.ConnectionZDRApplyResult{}, false, fmt.Errorf("record connection ZDR assessment: %w", err)
	}

	if result.Created || result.Changed {
		change := auditChangeZDRCreated
		metadata := map[string]any{
			"capability":     string(result.Record.Capability),
			"basis":          string(result.Record.Basis),
			"basisRef":       result.Record.BasisRef,
			"catalogVersion": result.Record.CatalogVersion,
			"digest":         result.Record.Digest,
			"waiverRoute":    assessment.WaiverRoute,
		}
		if result.Changed && result.Previous != nil {
			change = auditChangeZDRChanged
			metadata["previousCapability"] = string(result.Previous.Capability)
			metadata["previousDigest"] = result.Previous.Digest
		}
		metadata["change"] = change
		metadata["revokedAcknowledgements"] = len(result.Revoked)
		if err := persist(store.AuditEvent{
			ID:             auditMutationID(ctx, "zdr-assessed", connection.ID),
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      auditEventZDRAssessed,
			ProviderKey:    connection.ProviderKey,
			Metadata:       metadata,
		}); err != nil {
			return store.ConnectionZDRApplyResult{}, false, err
		}
	}
	for _, ack := range result.Revoked {
		if err := persist(store.AuditEvent{
			ID:             auditMutationID(ctx, "zdr-acknowledgement-invalidated", ack.ID),
			OrganizationID: ack.OrganizationID,
			UserID:         ack.UserID,
			ConnectionID:   ack.ConnectionID,
			EventType:      auditEventZDRInvalidated,
			ProviderKey:    ack.ProviderKey,
			Metadata: map[string]any{
				"acknowledgementId": ack.ID,
				"reason":            ack.RevokeReason,
				"digest":            ack.Digest,
				"currentDigest":     result.Record.Digest,
				"textVersion":       ack.TextVersion,
			},
		}); err != nil {
			return store.ConnectionZDRApplyResult{}, false, err
		}
	}
	return result, true, nil
}

// ZDRReconcileResult counts what one ReconcileConnectionZDR pass did.
type ZDRReconcileResult struct {
	// Examined is the number of live model connections looked at.
	Examined int
	// Created had no record and got one.
	Created int
	// Changed had a record whose digest moved (a catalog change); their acknowledgements were
	// revoked.
	Changed int
	// Unchanged already carried the current assessment.
	Unchanged int
	// Revoked is the number of acknowledgements revoked across the pass.
	Revoked int
	// Failed could not be assessed; each is in the returned error.
	Failed int
}

// ReconcileConnectionZDR assesses every live (not deleted) model connection. It is idempotent:
// a second pass over the same catalog writes nothing and revokes nothing, so it can run on
// every start and be re-run at will. A connection that fails does not stop the others; the
// failures are joined into the returned error. Each connection is its own transaction with its
// own audit rows.
//
// It reads Repo and AuditOutbox from cfg. It never touches a connection row, a provider token
// or the provider.
func ReconcileConnectionZDR(ctx context.Context, cfg ServerConfig) (ZDRReconcileResult, error) {
	var result ZDRReconcileResult
	if cfg.Repo == nil {
		return result, errors.New("repository is unavailable")
	}
	if _, err := zdr.Default(); err != nil {
		return result, fmt.Errorf("load connection ZDR capability contract: %w", err)
	}
	var failures []error
	for _, provider := range providers.Catalog() {
		if provider.Category != "ai" {
			continue
		}
		connections, err := cfg.Repo.ListConnections(ctx, store.ConnectionFilter{ProviderKey: provider.Key})
		if err != nil {
			failures = append(failures, fmt.Errorf("list %s connections: %w", provider.Key, err))
			result.Failed++
			continue
		}
		for _, connection := range connections {
			if connection.DeletedAt != nil || connection.ConnectorType != provider.ConnectorType {
				continue
			}
			result.Examined++
			var applied store.ConnectionZDRApplyResult
			err := withAuditTransaction(ctx, cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
				var inner error
				applied, _, inner = applyConnectionZDR(ctx, tx, persist, connection)
				return inner
			})
			if err != nil {
				result.Failed++
				failures = append(failures, fmt.Errorf("connection %s: %w", connection.ID, err))
				continue
			}
			switch {
			case applied.Created:
				result.Created++
			case applied.Changed:
				result.Changed++
			default:
				result.Unchanged++
			}
			result.Revoked += len(applied.Revoked)
		}
	}
	return result, errors.Join(failures...)
}
