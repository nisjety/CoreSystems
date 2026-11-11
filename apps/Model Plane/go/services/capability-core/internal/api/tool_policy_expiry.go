package api

import (
	"context"
	"log/slog"
	"time"
)

// auditActionToolPolicyRuleExpired is the registry_audit_log action written once when a confirmed "Always"
// allow passes its expires_at (WS13 S7, K3-10). The row says which policy lapsed and when, nothing a person wrote.
const auditActionToolPolicyRuleExpired = "tool_policy_rule_expired"

const (
	// toolPolicyExpiryInterval is how often the sweeper looks for lapsed allows.
	toolPolicyExpiryInterval = 5 * time.Minute
	// toolPolicyExpiryBatch caps the rows one sweep stamps, so a backlog drains over several ticks.
	toolPolicyExpiryBatch = 200
)

// ToolPolicyExpirySweeper writes one content-free tool_policy_rule_expired audit row for every allow whose
// expires_at has passed, and stamps expiry_audited_at so the row is written once. It never revokes or deletes
// a lapsed policy (renewal is a re-confirm) and never touches the policy engine. Replicas stay apart through
// FOR UPDATE SKIP LOCKED, and a retried sweep cannot write twice because the audit id is deterministic.
type ToolPolicyExpirySweeper struct {
	pool     registryDatabase
	interval time.Duration
	batch    int
}

// NewToolPolicyExpirySweeper builds the sweeper with a five minute interval and a batch of 200.
func NewToolPolicyExpirySweeper(pool registryDatabase) *ToolPolicyExpirySweeper {
	return &ToolPolicyExpirySweeper{pool: pool, interval: toolPolicyExpiryInterval, batch: toolPolicyExpiryBatch}
}

// Start sweeps on a ticker until ctx is done. It logs only the count and the error class, never a row.
func (s *ToolPolicyExpirySweeper) Start(ctx context.Context) {
	ticker := time.NewTicker(s.interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			n, err := s.SweepOnce(ctx)
			if err != nil {
				if ctx.Err() != nil {
					return
				}
				slog.Error("tool policy expiry sweep failed", "error_class", "database")
				continue
			}
			if n > 0 {
				slog.Info("tool policy expiry sweep", "audited", n)
			}
		}
	}
}

// SweepOnce runs one statement: it picks up to batch lapsed, unrevoked, not yet audited allows, stamps them,
// and appends their audit rows, and returns how many it stamped. Parameters: $1 batch, $2 audit action.
func (s *ToolPolicyExpirySweeper) SweepOnce(ctx context.Context) (int, error) {
	var count int
	if err := s.pool.QueryRow(ctx, toolPolicyExpirySweepSQL, s.batch, auditActionToolPolicyRuleExpired).Scan(&count); err != nil {
		return 0, err
	}
	return count, nil
}

// toolPolicyExpirySweepSQL stamps and audits the due rows as one statement. The audit id is derived from the
// policy id and the lapse time, so a retried sweep conflicts on the primary key and writes nothing twice, while a
// renewed rule that lapses again at a new time gets a new id. The diff holds a capability id, a closed scope word
// and a time; nothing a person wrote.
const toolPolicyExpirySweepSQL = `
WITH due AS (
    SELECT id FROM tool_policies
    WHERE decision = 'allow' AND revoked_at IS NULL AND expires_at IS NOT NULL
      AND expires_at <= now() AND expiry_audited_at IS NULL
    ORDER BY expires_at
    LIMIT $1
    FOR UPDATE SKIP LOCKED
), upd AS (
    UPDATE tool_policies t SET expiry_audited_at = now()
    FROM due WHERE t.id = due.id
    RETURNING t.id, t.org_id, t.capability_id, t.scope_kind, t.expires_at
), aud AS (
    INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
    SELECT 'ral_' || md5(upd.id || ':expired:' || upd.expires_at::text), '` + auditEntityToolPolicy + `', upd.id,
           $2::text, 'system:capability-core', upd.org_id,
           jsonb_build_object('capability_id', upd.capability_id, 'scope_kind', upd.scope_kind, 'expires_at', upd.expires_at)
    FROM upd
    ON CONFLICT (id) DO NOTHING
    RETURNING 1
)
SELECT count(*) FROM upd
`
