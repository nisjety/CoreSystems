package store

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// AuditEntry mirrors one row of the audit_log table.
type AuditEntry struct {
	ID             int64           `json:"id"`
	OrganizationID string          `json:"organization_id"`
	Actor          string          `json:"actor"`
	Action         string          `json:"action"`
	TargetKind     string          `json:"target_kind"`
	TargetID       string          `json:"target_id"`
	Payload        json.RawMessage `json:"payload"`
	CreatedAt      time.Time       `json:"created_at"`
}

// AuditInput is the write-side view of one audit entry.
type AuditInput struct {
	OrganizationID string
	Actor          string
	Action         string
	TargetKind     string
	TargetID       string
	Payload        any
}

type Audit struct {
	pool *pgxpool.Pool
}

// Write appends one row to audit_log. The payload is JSON-marshaled and
// rejected if not serializable — audit must never silently drop context.
func (a *Audit) Write(ctx context.Context, in AuditInput) (AuditEntry, error) {
	body, err := json.Marshal(in.Payload)
	if err != nil {
		return AuditEntry{}, fmt.Errorf("marshal audit payload: %w", err)
	}
	const q = `
INSERT INTO audit_log (organization_id, actor, action, target_kind, target_id, payload)
VALUES ($1, $2, $3, $4, $5, COALESCE($6::jsonb, '{}'::jsonb))
RETURNING id, organization_id, actor, action, target_kind, target_id, payload, created_at`
	var out AuditEntry
	err = a.pool.QueryRow(ctx, q, in.OrganizationID, in.Actor, in.Action, in.TargetKind, in.TargetID, body).Scan(
		&out.ID, &out.OrganizationID, &out.Actor, &out.Action, &out.TargetKind, &out.TargetID, &out.Payload, &out.CreatedAt,
	)
	if err != nil {
		return AuditEntry{}, fmt.Errorf("insert audit row: %w", err)
	}
	return out, nil
}

// ListForTarget returns every audit row associated with a specific target.
// Useful for "proposal X history" queries.
func (a *Audit) ListForTarget(ctx context.Context, organizationID, targetKind, targetID string, limit int) ([]AuditEntry, error) {
	if limit <= 0 || limit > 500 {
		limit = 100
	}
	const q = `
SELECT id, organization_id, actor, action, target_kind, target_id, payload, created_at
  FROM audit_log
 WHERE organization_id = $1 AND target_kind = $2 AND target_id = $3
 ORDER BY created_at DESC
 LIMIT $4`
	rows, err := a.pool.Query(ctx, q, organizationID, targetKind, targetID, limit)
	if err != nil {
		return nil, fmt.Errorf("list audit: %w", err)
	}
	defer rows.Close()
	var out []AuditEntry
	for rows.Next() {
		var r AuditEntry
		if err := rows.Scan(&r.ID, &r.OrganizationID, &r.Actor, &r.Action, &r.TargetKind, &r.TargetID, &r.Payload, &r.CreatedAt); err != nil {
			return nil, fmt.Errorf("scan: %w", err)
		}
		out = append(out, r)
	}
	return out, rows.Err()
}
