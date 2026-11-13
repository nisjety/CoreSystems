//go:build integration

package api

import (
	"context"
	"strings"
	"testing"
)

// W2-WS13-S7-rule-expiry against PostgreSQL: one tool_policy_rule_expired row per lapsed allow,
// written once, again after a renewal lapses, and never for an ask, a revoked or a live allow.
func TestToolPolicyExpirySweepAgainstPostgres(t *testing.T) {
	pool := setupMCPToolsDB(t)
	ctx := context.Background()
	digest := strings.Repeat("a", 64)
	insert := func(id, decision, expires string, revoked bool) {
		t.Helper()
		revokedAt := "NULL"
		if revoked {
			revokedAt = "now()"
		}
		var pinned any
		if decision == "allow" {
			pinned = digest
		}
		if _, err := pool.Exec(ctx, `
			INSERT INTO tool_policies (id, org_id, scope_kind, scope_value, capability_id, decision, pinned_digest, reason, set_by, expires_at, revoked_at)
			VALUES ($1, 'org-a', 'user', $1, 'cap.mcp_tool.'||$1, $2, $3, 'a person''s reason', 'user-a', `+expires+`, `+revokedAt+`)
		`, id, decision, pinned); err != nil {
			t.Fatalf("insert %s: %v", id, err)
		}
	}
	insert("lapsed", "allow", "now() - interval '1 minute'", false)
	insert("live", "allow", "now() + interval '1 day'", false)
	insert("forever", "allow", "NULL", false)
	insert("revoked", "allow", "now() - interval '1 minute'", true)

	sweeper := NewToolPolicyExpirySweeper(pool)
	n, err := sweeper.SweepOnce(ctx)
	if err != nil || n != 1 {
		t.Fatalf("first sweep = %d, %v; want 1", n, err)
	}
	if n, err = sweeper.SweepOnce(ctx); err != nil || n != 0 {
		t.Fatalf("second sweep = %d, %v; want 0 (written once)", n, err)
	}
	if got := policyAudit(t, pool, "lapsed", "tool_policy_rule_expired"); got != 1 {
		t.Fatalf("lapsed rule has %d expiry rows, want 1", got)
	}
	for _, id := range []string{"live", "forever", "revoked"} {
		if got := policyAudit(t, pool, id, "tool_policy_rule_expired"); got != 0 {
			t.Fatalf("%s got %d expiry rows", id, got)
		}
	}
	var diff, actor string
	if err := pool.QueryRow(ctx, `
		SELECT diff_json::text, actor FROM registry_audit_log WHERE entity_id = 'lapsed' AND action = 'tool_policy_rule_expired'
	`).Scan(&diff, &actor); err != nil {
		t.Fatal(err)
	}
	if strings.Contains(diff, "reason") || strings.Contains(diff, "user-a") || actor != "system:capability-core" {
		t.Fatalf("expiry row carries content: diff=%s actor=%s", diff, actor)
	}
	var stillThere int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM tool_policies WHERE id = 'lapsed' AND revoked_at IS NULL`).Scan(&stillThere); err != nil || stillThere != 1 {
		t.Fatalf("the sweep revoked or deleted the lapsed rule (%d, %v)", stillThere, err)
	}

	// Renewal: a re-confirm clears the stamp (setToolPolicySQL); the renewed rule lapses again at a new time.
	if _, err := pool.Exec(ctx, `UPDATE tool_policies SET expires_at = now() - interval '1 second', expiry_audited_at = NULL WHERE id = 'lapsed'`); err != nil {
		t.Fatal(err)
	}
	if n, err = sweeper.SweepOnce(ctx); err != nil || n != 1 {
		t.Fatalf("sweep after renewal = %d, %v; want 1", n, err)
	}
	if got := policyAudit(t, pool, "lapsed", "tool_policy_rule_expired"); got != 2 {
		t.Fatalf("renewed rule has %d expiry rows, want 2", got)
	}
}
