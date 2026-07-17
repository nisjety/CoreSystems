package registry

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestTenantScopeAndRiskMigrationFailsClosedForLegacyRows(t *testing.T) {
	t.Parallel()

	contents, err := os.ReadFile(filepath.Join("..", "..", "migrations", "0007_tenant_scopes_and_risk_constraints.up.sql"))
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	sql := string(contents)
	for _, required := range []string{
		"ADD COLUMN IF NOT EXISTS org_id",
		"capabilities_risk_level_check",
		"risk_level NOT IN ('low', 'medium', 'high')",
		"rollout_state = 'quarantine'",
		"revoked_at = COALESCE(revoked_at, now())",
		"SET scope_kind = 'global'\nWHERE revoked_at IS NOT NULL",
		"ALTER COLUMN org_id SET NOT NULL",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
}

func TestExecutionDispatchCapabilitiesStartUnavailableUntilHealthAttested(t *testing.T) {
	t.Parallel()

	contents, err := os.ReadFile(filepath.Join("..", "..", "migrations", "0008_execution_dispatch_capabilities.up.sql"))
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	sql := string(contents)
	for _, required := range []string{
		"'cap.command.shell'",
		"'cap.agent.spawn'",
		"'cap.retrieval.query'",
		"'cap.tool.shipping.book'",
		"'cap.tool.provider.execute'",
		"'global'",
		"ARRAY['global']",
		"'unavailable'",
		"'health_not_attested'",
		"'unavailable'",
		"health_checked_at",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	if strings.Contains(sql, "availability_state = 'available'") {
		t.Fatal("migration must not fabricate a live health attestation")
	}
}
