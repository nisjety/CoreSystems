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
	// cap.command.shell must remain high-risk in the durable seed. Policy
	// returns `ask` for high risk, which is the gate keeping arbitrary command
	// execution human-approved; the low-risk sandbox capability added in 0010
	// must not be achieved by softening this row.
	if !strings.Contains(sql, "('cap.command.shell', 'shell', 'Run a command in the governed execution sandbox.', 'high', 'variable')") {
		t.Fatal("cap.command.shell must stay seeded as high-risk")
	}
}

// TestSandboxCommandCapabilityMigrationSeedsLowRiskHermeticExecution guards the
// 0010 seed: cap.command.sandbox exists as its own low-risk, globally scoped,
// enabled row that starts unavailable until a health authority attests it, and
// the migration stays idempotent under the entrypoint that replays every
// *.up.sql on each container start.
func TestSandboxCommandCapabilityMigrationSeedsLowRiskHermeticExecution(t *testing.T) {
	t.Parallel()

	const name = "0010_sandbox_code_execution_capability.up.sql"
	contents, err := os.ReadFile(filepath.Join("..", "..", "migrations", name))
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	sql := string(contents)
	for _, required := range []string{
		"'cap.command.sandbox'",
		"'code_interpreter'",
		"'low'",
		"'global'",
		"ARRAY['global']",
		"ARRAY['execution-dispatch']",
		"'execution-dispatch:cap.command.sandbox:v1'",
		"'unavailable'",
		"'health_not_attested'",
		"health_checked_at",
		"ON CONFLICT (id) DO UPDATE SET",
		"'migration:0010_sandbox_code_execution_capability'",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	if strings.Contains(sql, "availability_state = 'available'") {
		t.Fatal("migration must not fabricate a live health attestation")
	}
	// The low-risk classification is only defensible because of the concrete
	// isolation constraints. If the sandbox ever loses one of them the comment
	// has to change, and that should break this test rather than silently ship
	// an auto-approved code executor.
	for _, constraint := range []string{
		"read-only root filesystem",
		"networking disabled",
		"wall-clock timeout",
		"throwaway workspace",
		"secret-scrubbed",
	} {
		if !strings.Contains(sql, constraint) {
			t.Fatalf("migration must justify risk_level='low' with %q", constraint)
		}
	}
	// Re-running the entrypoint must not clobber a live attestation, so the
	// upsert branch must leave the availability columns alone.
	upsert := sql[strings.Index(sql, "ON CONFLICT (id) DO UPDATE SET"):]
	for _, forbidden := range []string{
		"availability_state = EXCLUDED",
		"availability_reason_code = EXCLUDED",
		"execution_mode = EXCLUDED",
		"health_checked_at = EXCLUDED",
	} {
		if strings.Contains(upsert, forbidden) {
			t.Fatalf("upsert must not overwrite a live attestation via %q", forbidden)
		}
	}
	// 0010 only registers a new capability. A bare UPDATE/DELETE against
	// capabilities would let it reach into the 0008 rows, including the
	// high-risk cap.command.shell gate.
	for _, forbidden := range []string{"UPDATE capabilities", "DELETE FROM capabilities"} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("migration must not mutate existing capability rows via %q", forbidden)
		}
	}
}
