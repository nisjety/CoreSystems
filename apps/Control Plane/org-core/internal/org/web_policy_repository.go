package org

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// WebPolicyAuditEvent and WebPolicyAuditSubject are the durable audit row written
// for every web policy write; audit-core ingests the subject as a Control event.
const (
	WebPolicyAuditEvent   = "org_web_policy_changed"
	WebPolicyAuditSubject = "verevon.audit.v2.control.org-core." + WebPolicyAuditEvent
)

// webPolicyAuditOutboxTable is a constant, never caller input: the claim and mark
// statements below are built with it.
const webPolicyAuditOutboxTable = "organization_web_policy_audit_outbox"

// webPolicyRow is one organization's stored state with the plan the entitlements
// derive from. Exists is false when no policy was ever written.
type webPolicyRow struct {
	Plan      string
	Exists    bool
	Policy    WebPolicyDoc
	Revision  int64
	RevDigest string
	UpdatedBy string
	UpdatedAt time.Time
}

// GetWebPolicy reads the stored policy and the plan in one RLS-scoped query. A
// missing or soft-deleted organization is ErrNotFound; an organization with no
// stored policy is Exists=false, revision 0.
func (r *Repository) GetWebPolicy(ctx context.Context, orgID string) (*webPolicyRow, error) {
	const q = `
SELECT o.plan, p.policy, p.revision, p.rev_digest, p.updated_by, p.updated_at
FROM organizations o
LEFT JOIN organization_web_policy p ON p.org_id = o.id
WHERE o.id = $1 AND o.deleted_at IS NULL`

	row := &webPolicyRow{}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var (
			raw       []byte
			revision  *int64
			digest    *string
			updatedBy *string
			updatedAt *time.Time
		)
		if err := tx.QueryRow(ctx, q, orgID).Scan(&row.Plan, &raw, &revision, &digest, &updatedBy, &updatedAt); err != nil {
			return err
		}
		if revision == nil {
			return nil
		}
		var doc WebPolicyDoc
		if err := json.Unmarshal(raw, &doc); err != nil {
			return fmt.Errorf("decode stored web policy: %w", err)
		}
		row.Exists = true
		row.Policy = doc
		row.Revision = *revision
		if digest != nil {
			row.RevDigest = *digest
		}
		if updatedBy != nil {
			row.UpdatedBy = *updatedBy
		}
		if updatedAt != nil {
			row.UpdatedAt = *updatedAt
		}
		return nil
	})
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("query web policy: %w", err)
	}
	return row, nil
}

// PutWebPolicy replaces the organization's policy in one RLS-scoped transaction:
//
//  1. lock the organization row (serialises every writer of the org, so two
//     concurrent first writes cannot both pass an If-Match of 0);
//  2. compare the stored revision with ifMatch (0 means "no policy yet");
//  3. refuse a document the plan does not entitle;
//  4. append the history row, upsert the policy at revision+1 and enqueue the
//     audit intent, all or nothing.
//
// doc must already be normalised and validated (the service does both). The
// returned revision is the one written.
func (r *Repository) PutWebPolicy(ctx context.Context, orgID string, doc WebPolicyDoc, ifMatch int64, changedBy, actorRole string) (*WebPolicyPutResult, error) {
	if strings.TrimSpace(changedBy) == "" {
		return nil, fmt.Errorf("web policy actor is required")
	}
	canonical, err := doc.Canonical()
	if err != nil {
		return nil, fmt.Errorf("encode web policy: %w", err)
	}
	digest := doc.Digest()

	var result *WebPolicyPutResult
	err = r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var plan string
		if err := tx.QueryRow(ctx, `
SELECT plan FROM organizations WHERE id = $1 AND deleted_at IS NULL FOR UPDATE`, orgID).Scan(&plan); err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrNotFound
			}
			return fmt.Errorf("lock organization for web policy: %w", err)
		}

		var (
			current int64
			oldRaw  []byte
		)
		err := tx.QueryRow(ctx, `
SELECT revision, policy FROM organization_web_policy WHERE org_id = $1`, orgID).Scan(&current, &oldRaw)
		switch {
		case errors.Is(err, pgx.ErrNoRows):
			current = 0
		case err != nil:
			return fmt.Errorf("read stored web policy: %w", err)
		}
		if current != ifMatch {
			return &RevisionConflictError{Current: current}
		}
		if err := doc.CheckEntitled(WebEntitlementsForPlan(plan)); err != nil {
			return err
		}

		var oldDoc WebPolicyDoc
		if len(oldRaw) > 0 {
			if err := json.Unmarshal(oldRaw, &oldDoc); err != nil {
				return fmt.Errorf("decode stored web policy: %w", err)
			}
		}
		next := current + 1
		tightened := Tightened(oldDoc, doc)

		if _, err := tx.Exec(ctx, `
INSERT INTO organization_web_policy_history (org_id, revision, policy, changed_by, tightened)
VALUES ($1, $2, $3::jsonb, $4, $5)`, orgID, next, canonical, changedBy, tightened); err != nil {
			return fmt.Errorf("append web policy history: %w", err)
		}
		if _, err := tx.Exec(ctx, `
INSERT INTO organization_web_policy (org_id, policy, revision, rev_digest, updated_by, updated_at)
VALUES ($1, $2::jsonb, $3, $4, $5, NOW())
ON CONFLICT (org_id) DO UPDATE
SET policy = EXCLUDED.policy,
    revision = EXCLUDED.revision,
    rev_digest = EXCLUDED.rev_digest,
    updated_by = EXCLUDED.updated_by,
    updated_at = EXCLUDED.updated_at`, orgID, canonical, next, digest, changedBy); err != nil {
			return fmt.Errorf("write web policy: %w", err)
		}
		if err := enqueueWebPolicyAudit(ctx, tx, webPolicyAuditChange{
			OrgID: orgID, ActorID: changedBy, ActorRole: actorRole,
			Revision: next, Rev: digest, Tightened: tightened,
			Keys: ChangedKeys(oldDoc, doc),
		}); err != nil {
			return err
		}
		result = &WebPolicyPutResult{Revision: next, Rev: digest, Tightened: tightened}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return result, nil
}

// ListWebPolicyHistory returns up to limit history rows, newest first, below the
// cursor revision (0 means from the newest). The second result is the revision to
// pass as the next cursor, or 0 when there are no more rows. A missing or
// soft-deleted organization is ErrNotFound.
func (r *Repository) ListWebPolicyHistory(ctx context.Context, orgID string, limit int, beforeRevision int64) ([]WebPolicyHistoryEntry, int64, error) {
	if limit < 1 {
		limit = 50
	}
	entries := make([]WebPolicyHistoryEntry, 0, limit)
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var one int
		if err := tx.QueryRow(ctx, `SELECT 1 FROM organizations WHERE id = $1 AND deleted_at IS NULL`, orgID).Scan(&one); err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrNotFound
			}
			return fmt.Errorf("check organization for web policy history: %w", err)
		}
		rows, err := tx.Query(ctx, `
SELECT revision, policy, changed_by, changed_at, tightened
FROM organization_web_policy_history
WHERE org_id = $1 AND ($2::bigint = 0 OR revision < $2::bigint)
ORDER BY revision DESC
LIMIT $3`, orgID, beforeRevision, limit+1)
		if err != nil {
			return fmt.Errorf("query web policy history: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			var (
				entry WebPolicyHistoryEntry
				raw   []byte
				at    time.Time
			)
			if err := rows.Scan(&entry.Revision, &raw, &entry.ChangedBy, &at, &entry.Tightened); err != nil {
				return fmt.Errorf("scan web policy history: %w", err)
			}
			if err := json.Unmarshal(raw, &entry.Policy); err != nil {
				return fmt.Errorf("decode web policy history: %w", err)
			}
			entry.Rev = entry.Policy.Digest()
			entry.ChangedAt = at.UTC().Format(time.RFC3339)
			entries = append(entries, entry)
		}
		return rows.Err()
	})
	if err != nil {
		return nil, 0, err
	}
	var next int64
	if len(entries) > limit {
		entries = entries[:limit]
		next = entries[len(entries)-1].Revision
	}
	return entries, next, nil
}

// --- audit outbox ---

type webPolicyAuditChange struct {
	OrgID     string
	ActorID   string
	ActorRole string
	Revision  int64
	Rev       string
	Tightened bool
	Keys      []string
}

// webPolicyAuditPayload is content-free by construction: the revision, its digest,
// whether the write only tightened, and the capability KEYS that changed. It never
// carries a host name or a value, so a Zero Data Retention organization's rules do
// not reach the audit stream.
func webPolicyAuditPayload(change webPolicyAuditChange, occurredAt time.Time) (eventID string, payload map[string]any) {
	identity := sha256.Sum256([]byte(change.OrgID + "\x00" + strconv.FormatInt(change.Revision, 10)))
	eventID = "webpolicy:org-core:" + hex.EncodeToString(identity[:])
	keys := change.Keys
	if keys == nil {
		keys = []string{}
	}
	return eventID, map[string]any{
		"event_id": eventID, "occurred_at": occurredAt.UTC().Format(time.RFC3339Nano),
		"org_id": change.OrgID, "user_id": change.ActorID, "actor_role": change.ActorRole,
		"plane": "control", "producer": "org-core", "event": WebPolicyAuditEvent,
		"subject": "web_policy:" + change.OrgID, "resource_id": change.OrgID, "outcome": "ok",
		"details": map[string]any{
			"revision": change.Revision, "rev": change.Rev,
			"tightened": change.Tightened, "keys": keys,
		},
	}
}

func enqueueWebPolicyAudit(ctx context.Context, tx pgx.Tx, change webPolicyAuditChange) error {
	if strings.TrimSpace(change.ActorID) == "" || strings.TrimSpace(change.ActorRole) == "" {
		return fmt.Errorf("web policy audit actor and role are required")
	}
	eventID, payload := webPolicyAuditPayload(change, time.Now())
	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("encode web policy audit payload: %w", err)
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO `+webPolicyAuditOutboxTable+` (event_id, org_id, subject, payload)
VALUES ($1, $2, $3, $4)`, eventID, change.OrgID, WebPolicyAuditSubject, body); err != nil {
		return fmt.Errorf("record web policy audit outbox: %w", err)
	}
	return nil
}

func (r *Repository) claimWebPolicyAuditOutbox(ctx context.Context, limit int) ([]GDPRAuditOutboxRow, error) {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin web policy audit outbox claim: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	rows, err := tx.Query(ctx, `
WITH claimed AS (
  SELECT event_id
  FROM `+webPolicyAuditOutboxTable+`
  WHERE published_at IS NULL
    AND dead_lettered_at IS NULL
    AND attempts < $2
    AND next_attempt_at <= NOW()
    AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '1 minute')
  ORDER BY next_attempt_at, created_at, event_id
  LIMIT $1
  FOR UPDATE SKIP LOCKED
)
UPDATE `+webPolicyAuditOutboxTable+` o
SET processing_at = NOW(), updated_at = NOW()
FROM claimed
WHERE o.event_id = claimed.event_id
RETURNING o.event_id, o.org_id, o.subject, o.payload, o.attempts`, limit, GDPRAuditMaxAttempts)
	if err != nil {
		return nil, fmt.Errorf("claim web policy audit outbox: %w", err)
	}
	defer rows.Close()
	claimed := make([]GDPRAuditOutboxRow, 0)
	for rows.Next() {
		var row GDPRAuditOutboxRow
		var body []byte
		if err := rows.Scan(&row.EventID, &row.OrgID, &row.Subject, &body, &row.Attempts); err != nil {
			return nil, fmt.Errorf("scan web policy audit outbox: %w", err)
		}
		if err := json.Unmarshal(body, &row.Payload); err != nil {
			return nil, fmt.Errorf("decode web policy audit outbox payload: %w", err)
		}
		claimed = append(claimed, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate web policy audit outbox: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit web policy audit outbox claim: %w", err)
	}
	return claimed, nil
}

func (r *Repository) markWebPolicyAuditPublished(ctx context.Context, eventID string) error {
	result, err := r.pool.Exec(ctx, `
UPDATE `+webPolicyAuditOutboxTable+`
SET published_at = NOW(), processing_at = NULL, attempts = attempts + 1,
    last_error = NULL, updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL`, eventID)
	if err != nil {
		return fmt.Errorf("mark web policy audit published: %w", err)
	}
	if result.RowsAffected() != 1 {
		return fmt.Errorf("web policy audit acknowledgement did not match a pending event")
	}
	return nil
}

func (r *Repository) markWebPolicyAuditPublishFailed(ctx context.Context, eventID string, publishErr error) (bool, error) {
	message := "unknown audit publish failure"
	if publishErr != nil {
		message = publishErr.Error()
	}
	if len(message) > 2048 {
		message = message[:2048]
	}
	var deadLettered bool
	err := r.pool.QueryRow(ctx, `
UPDATE `+webPolicyAuditOutboxTable+`
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
			return false, fmt.Errorf("web policy audit failure did not match a pending event")
		}
		return false, fmt.Errorf("record web policy audit publish failure: %w", err)
	}
	return deadLettered, nil
}

// FlushWebPolicyAuditOutbox publishes pending web policy change audit intents, the
// same loop FlushSIEMAuditOutbox runs. SetWebPolicy calls it once after a commit as
// best effort; a failed publish stays pending for a later flush.
func (s *Service) FlushWebPolicyAuditOutbox(ctx context.Context, limit int) (GDPRAuditFlushResult, error) {
	if limit < 1 || limit > 1000 {
		return GDPRAuditFlushResult{}, fmt.Errorf("web policy audit outbox limit must be between 1 and 1000")
	}
	if s.auditPublisher == nil {
		return GDPRAuditFlushResult{}, fmt.Errorf("web policy audit publisher is unavailable")
	}
	if s.repo == nil || s.repo.pool == nil {
		return GDPRAuditFlushResult{}, fmt.Errorf("web policy audit outbox repository is unavailable")
	}
	rows, err := s.repo.claimWebPolicyAuditOutbox(ctx, limit)
	if err != nil {
		return GDPRAuditFlushResult{}, err
	}
	result := GDPRAuditFlushResult{}
	var publishErrors error
	for _, row := range rows {
		if err := s.auditPublisher.PublishAudit(ctx, row.Subject, row.EventID, row.Payload); err != nil {
			deadLettered, markErr := s.repo.markWebPolicyAuditPublishFailed(ctx, row.EventID, err)
			if deadLettered {
				result.DeadLettered++
			}
			publishErrors = errors.Join(publishErrors, err, markErr)
			continue
		}
		if err := s.repo.markWebPolicyAuditPublished(ctx, row.EventID); err != nil {
			publishErrors = errors.Join(publishErrors, err)
			continue
		}
		result.Published++
	}
	return result, publishErrors
}
