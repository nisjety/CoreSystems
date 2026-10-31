//go:build integration

package registry

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

// TestAgentActionAdmissionsSeedOffersOnlyTicketsCreate applies every migration, reapplies the
// admission migration to prove it is idempotent, and checks that only the enabled tickets.create
// row is ever returned to the offer path. The declared rows exist but are never offered.
func TestAgentActionAdmissionsSeedOffersOnlyTicketsCreate(t *testing.T) {
	pool := setupToolClassDB(t)
	ctx := context.Background()

	sqlBytes, err := os.ReadFile(filepath.Join("..", "..", "migrations", "0022_agent_action_admissions.up.sql"))
	if err != nil {
		t.Fatalf("read migration 0022: %v", err)
	}
	if _, err := pool.Exec(ctx, string(sqlBytes)); err != nil {
		t.Fatalf("reapply migration 0022: %v", err)
	}

	var rows int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM agent_action_admissions`).Scan(&rows); err != nil {
		t.Fatalf("count admission rows: %v", err)
	}
	if rows != 3 {
		t.Fatalf("admission rows = %d, want the three seeded rows after a reapply", rows)
	}

	store, err := NewAgentActionAdmissionsStore(pool)
	if err != nil {
		t.Fatal(err)
	}
	offered, err := store.ListEnabled(ctx, []string{"tickets.create", "brreg.lookup_organization", "shipping.get_quotes"})
	if err != nil {
		t.Fatal(err)
	}
	if len(offered) != 1 || offered[0].ActionID != "tickets.create" || offered[0].Status != AgentActionAdmissionEnabled {
		t.Fatalf("offered = %#v, want only the enabled tickets.create row", offered)
	}
	if offered[0].ActionSchemaHash != "sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df" {
		t.Fatalf("tickets.create schema hash = %q", offered[0].ActionSchemaHash)
	}
}
