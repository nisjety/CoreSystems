package org

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// Identity audit events (WS05-S6). The names are single tokens because
// audit-core accepts only six-token subjects,
// verevon.audit.v2.<plane>.<producer>.<event>; the plan's dotted names
// (org.domain.added) would be rejected as an authority mismatch.
const (
	IdentityEventDomainAdded    = "org_domain_added"
	IdentityEventDomainVerified = "org_domain_verified"
	IdentityEventDomainRevoked  = "org_domain_revoked"

	identityAuditSubjectPrefix = "verevon.audit.v2.control.org-core."
)

// identityAuditResourceKind names what the audit subject refers to. Domain
// events are the default; the SSO events (WS05-S7) refer to a policy or provider.
func identityAuditResourceKind(event string) string {
	switch event {
	case IdentityEventSSOPolicyUpdated:
		return "sso_policy"
	case IdentityEventSSOProviderAdded, IdentityEventSSOProviderGone:
		return "sso_provider"
	case IdentityEventSCIMTokenCreated, IdentityEventSCIMTokenRevoked:
		return "scim_token"
	case IdentityEventSCIMUserProvisioned, IdentityEventSCIMUserDeprovisioned,
		IdentityEventSCIMUserReactivated, IdentityEventSCIMRefusedLastOwner:
		return "scim_user"
	}
	return "domain"
}

var identityAuditEvents = map[string]struct{}{
	IdentityEventDomainAdded:    {},
	IdentityEventDomainVerified: {},
	IdentityEventDomainRevoked:  {},
	// SSO policy and provider changes (WS05-S7). Extra to the plan's event list.
	IdentityEventSSOPolicyUpdated: {},
	IdentityEventSSOProviderAdded: {},
	IdentityEventSSOProviderGone:  {},
	// SCIM tokens and users (WS05-S8). Single-token names, see the note above.
	IdentityEventSCIMTokenCreated:       {},
	IdentityEventSCIMTokenRevoked:       {},
	IdentityEventSCIMUserProvisioned:    {},
	IdentityEventSCIMUserDeprovisioned:  {},
	IdentityEventSCIMUserReactivated:    {},
	IdentityEventSCIMRefusedLastOwner:   {},
}

// identityAuditChange is one identity change to record. Details hold ids,
// domain names and kinds only: no challenge token, no DNS record content.
type identityAuditChange struct {
	Event     string
	OrgID     string
	ActorID   string
	ActorRole string
	Resource  string
	Details   map[string]any
}

// identityAuditPayload builds the audit-core payload and its event id. The id
// is a digest so a long domain name stays inside the 128 byte column.
func identityAuditPayload(change identityAuditChange, occurredAt time.Time) (eventID, subject string, payload map[string]any) {
	identity := strings.Join([]string{
		change.Event, change.OrgID, change.Resource, occurredAt.UTC().Format(time.RFC3339Nano),
	}, "\x00")
	eventID = fmt.Sprintf("identity:org-core:%x", sha256.Sum256([]byte(identity)))
	subject = identityAuditSubjectPrefix + change.Event
	details := map[string]any{}
	for key, value := range change.Details {
		details[key] = value
	}
	payload = map[string]any{
		"event_id": eventID, "occurred_at": occurredAt.UTC().Format(time.RFC3339Nano),
		"org_id": change.OrgID, "user_id": change.ActorID, "actor_role": change.ActorRole,
		"plane": "control", "producer": "org-core", "event": change.Event,
		"subject": identityAuditResourceKind(change.Event) + ":" + change.Resource, "resource_id": change.Resource,
		"outcome": "ok", "details": details,
	}
	return eventID, subject, payload
}

// enqueueIdentityAudit writes the durable audit intent in the same
// transaction as the change it records.
func enqueueIdentityAudit(ctx context.Context, tx pgx.Tx, change identityAuditChange) error {
	if _, known := identityAuditEvents[change.Event]; !known {
		return fmt.Errorf("unknown identity audit event")
	}
	if strings.TrimSpace(change.OrgID) == "" || strings.TrimSpace(change.ActorID) == "" ||
		strings.TrimSpace(change.ActorRole) == "" {
		return fmt.Errorf("identity audit org, actor and role are required")
	}
	eventID, subject, payload := identityAuditPayload(change, time.Now())
	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("encode identity audit payload: %w", err)
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO organization_identity_audit_outbox (event_id, org_id, subject, payload)
VALUES ($1, $2, $3, $4)`, eventID, change.OrgID, subject, body); err != nil {
		return fmt.Errorf("record identity audit outbox: %w", err)
	}
	return nil
}

func (r *Repository) claimIdentityAuditOutbox(ctx context.Context, limit int) ([]GDPRAuditOutboxRow, error) {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin identity audit outbox claim: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	rows, err := tx.Query(ctx, `
WITH claimed AS (
  SELECT event_id
  FROM organization_identity_audit_outbox
  WHERE published_at IS NULL
    AND dead_lettered_at IS NULL
    AND attempts < $2
    AND next_attempt_at <= NOW()
    AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '1 minute')
  ORDER BY next_attempt_at, created_at, event_id
  LIMIT $1
  FOR UPDATE SKIP LOCKED
)
UPDATE organization_identity_audit_outbox o
SET processing_at = NOW(), updated_at = NOW()
FROM claimed
WHERE o.event_id = claimed.event_id
RETURNING o.event_id, o.org_id, o.subject, o.payload, o.attempts`, limit, GDPRAuditMaxAttempts)
	if err != nil {
		return nil, fmt.Errorf("claim identity audit outbox: %w", err)
	}
	defer rows.Close()
	claimed := make([]GDPRAuditOutboxRow, 0)
	for rows.Next() {
		var row GDPRAuditOutboxRow
		var body []byte
		if err := rows.Scan(&row.EventID, &row.OrgID, &row.Subject, &body, &row.Attempts); err != nil {
			return nil, fmt.Errorf("scan identity audit outbox: %w", err)
		}
		if err := json.Unmarshal(body, &row.Payload); err != nil {
			return nil, fmt.Errorf("decode identity audit outbox payload: %w", err)
		}
		claimed = append(claimed, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate identity audit outbox: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit identity audit outbox claim: %w", err)
	}
	return claimed, nil
}

func (r *Repository) markIdentityAuditPublished(ctx context.Context, eventID string) error {
	result, err := r.pool.Exec(ctx, `
UPDATE organization_identity_audit_outbox
SET published_at = NOW(), processing_at = NULL, attempts = attempts + 1,
    last_error = NULL, updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL`, eventID)
	if err != nil {
		return fmt.Errorf("mark identity audit published: %w", err)
	}
	if result.RowsAffected() != 1 {
		return fmt.Errorf("identity audit acknowledgement did not match a pending event")
	}
	return nil
}

func (r *Repository) markIdentityAuditPublishFailed(ctx context.Context, eventID string, publishErr error) (bool, error) {
	message := "unknown audit publish failure"
	if publishErr != nil {
		message = publishErr.Error()
	}
	if len(message) > 2048 {
		message = message[:2048]
	}
	var deadLettered bool
	err := r.pool.QueryRow(ctx, `
UPDATE organization_identity_audit_outbox
SET attempts = attempts + 1,
    processing_at = NULL,
    next_attempt_at = NOW() + LEAST(
      INTERVAL '5 minutes',
      INTERVAL '1 second' * power(2, LEAST(attempts, 8))
    ),
    dead_lettered_at = CASE WHEN attempts + 1 >= $2 THEN NOW() ELSE NULL END,
    last_error = $3,
    updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL
RETURNING dead_lettered_at IS NOT NULL`, eventID, GDPRAuditMaxAttempts, message).Scan(&deadLettered)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return false, fmt.Errorf("identity audit failure did not match a pending event")
		}
		return false, fmt.Errorf("record identity audit publish failure: %w", err)
	}
	return deadLettered, nil
}

// FlushIdentityAuditOutbox publishes pending identity-change audit intents,
// the same loop FlushSIEMAuditOutbox runs.
func (s *Service) FlushIdentityAuditOutbox(ctx context.Context, limit int) (GDPRAuditFlushResult, error) {
	if limit < 1 || limit > 1000 {
		return GDPRAuditFlushResult{}, fmt.Errorf("identity audit outbox limit must be between 1 and 1000")
	}
	if s.auditPublisher == nil {
		return GDPRAuditFlushResult{}, fmt.Errorf("identity audit publisher is unavailable")
	}
	if s.repo == nil || s.repo.pool == nil {
		return GDPRAuditFlushResult{}, fmt.Errorf("identity audit outbox repository is unavailable")
	}
	rows, err := s.repo.claimIdentityAuditOutbox(ctx, limit)
	if err != nil {
		return GDPRAuditFlushResult{}, err
	}
	result := GDPRAuditFlushResult{}
	var publishErrors error
	for _, row := range rows {
		if err := s.auditPublisher.PublishAudit(ctx, row.Subject, row.EventID, row.Payload); err != nil {
			deadLettered, markErr := s.repo.markIdentityAuditPublishFailed(ctx, row.EventID, err)
			if deadLettered {
				result.DeadLettered++
			}
			publishErrors = errors.Join(publishErrors, err, markErr)
			continue
		}
		if err := s.repo.markIdentityAuditPublished(ctx, row.EventID); err != nil {
			publishErrors = errors.Join(publishErrors, err)
			continue
		}
		result.Published++
	}
	return result, publishErrors
}
