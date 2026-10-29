package registry

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// readMigration reads a migration and normalises its line endings.
//
// These assertions are the durable record of several security postures: a
// capability that starts unavailable until health is attested, an upsert that
// cannot clobber a live attestation, cap.command.shell staying high-risk. One
// of them spans two lines, so on a CRLF checkout the file held a carriage
// return where the literal did not and the check failed for a reason that had
// nothing to do with the posture it guards. A security test that is red for a
// spurious reason gets ignored, which is the worst outcome available here, so
// the line endings are normalised once, at the read.
func readMigration(t *testing.T, name string) string {
	t.Helper()
	contents, err := os.ReadFile(filepath.Join("..", "..", "migrations", name))
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	return strings.ReplaceAll(string(contents), "\r\n", "\n")
}

func TestTenantScopeAndRiskMigrationFailsClosedForLegacyRows(t *testing.T) {
	t.Parallel()

	sql := readMigration(t, "0007_tenant_scopes_and_risk_constraints.up.sql")
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

	sql := readMigration(t, "0008_execution_dispatch_capabilities.up.sql")
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
	sql := readMigration(t, name)
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

func TestConversationTicketActionStartsUnavailableAndKeepsOwnerBoundFields(t *testing.T) {
	t.Parallel()

	sql := readMigration(t, "0011_conversation_ticket_action_capability.up.sql")
	for _, required := range []string{
		"'cap.tool.ticket.create'",
		"'tickets.create'",
		"'medium'",
		"'application'",
		"'conversation-core'",
		"'control-plane-user-core'",
		"'conversation-core-current-resource-check'",
		"'unavailable'",
		"'health_not_attested'",
		"'migration:0011_conversation_ticket_action_capability'",
		"'actor'",
		"'org'",
		"'idempotency_key'",
		"ON CONFLICT (id) DO UPDATE SET",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	if strings.Contains(sql, "availability_state = 'available'") {
		t.Fatal("ticket action migration must not fabricate a live health attestation")
	}
	upsert := sql[strings.Index(sql, "ON CONFLICT (id) DO UPDATE SET"):]
	for _, forbidden := range []string{
		"availability_state = EXCLUDED",
		"availability_reason_code = EXCLUDED",
		"execution_mode = EXCLUDED",
		"health_checked_at = EXCLUDED",
	} {
		if strings.Contains(upsert, forbidden) {
			t.Fatalf("upsert must not overwrite a runtime attestation via %q", forbidden)
		}
	}
}

// 0015 seed: cap.process.background exists as its own low-risk row that starts
// unavailable, and — unlike every capability before it — carries in the row
// itself the fact that the capability is NOT the whole gate.
//
// That last part is why this test asserts more than the 0010 one it mirrors. A
// `low` row for something that outlives its call reads permissive on its own,
// and it is only defensible because a per-Space, deny-by-default authority sits
// behind it. If that second gate is ever dropped, the honest response is to
// re-argue the risk level — so the migration has to keep naming it, and this
// test is what makes dropping it visible.
func TestBackgroundProcessCapabilityStaysLowRiskBehindASecondGate(t *testing.T) {
	t.Parallel()

	const name = "0015_background_process_capability.up.sql"
	sql := readMigration(t, name)
	for _, required := range []string{
		"'cap.process.background'",
		"'process_start'",
		"'low'",
		"'global'",
		"ARRAY['global']",
		"ARRAY['execution-dispatch']",
		"'execution-dispatch:cap.process.background:v1'",
		"'unavailable'",
		"'health_not_attested'",
		"health_checked_at",
		"ON CONFLICT (id) DO UPDATE SET",
		"'migration:0015_background_process_capability'",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	if strings.Contains(sql, "availability_state = 'available'") {
		t.Fatal("migration must not fabricate a live health attestation")
	}
	// The whole family maps to this one row, so the row has to say so. Five
	// separate capabilities would make "may start but may not stop" reachable
	// by operator error; if that decision is ever reversed, it should break
	// here rather than ship as a silent posture change.
	for _, member := range []string{
		"'process_start'",
		"'process_read'",
		"'process_stdin'",
		"'process_signal'",
		"'process_list'",
	} {
		if !strings.Contains(sql, member) {
			t.Fatalf("migration must declare the whole tool family, missing %q", member)
		}
	}
	// The second gate, named in the row rather than only in a design doc.
	// `space:processes` is granted by Control only to a Space carrying
	// process_registry_entitled (user-core migration 028, default FALSE), and
	// sandbox-manager refuses every process RPC on a lease that did not get it.
	if !strings.Contains(sql, "'space:processes'") {
		t.Fatal("migration must record the per-Space authority this capability still requires")
	}
	// risk_level='low' is inherited from cap.command.sandbox's constraints plus
	// bounds on the two axes this capability widens. Losing any of them means
	// the classification has to be re-argued, not silently kept.
	for _, constraint := range []string{
		"read-only root filesystem",
		"networking disabled",
		"secret-scrubbed",
		"never a named host command",
		"TTL-bounded",
		"count-limited concurrency",
	} {
		if !strings.Contains(sql, constraint) {
			t.Fatalf("migration must justify risk_level='low' with %q", constraint)
		}
	}
	// Re-running the entrypoint must not clobber a live attestation.
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
	// Registering a new capability must not reach into the 0008/0010 rows —
	// cap.command.shell's high-risk gate above all.
	for _, forbidden := range []string{"UPDATE capabilities", "DELETE FROM capabilities"} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("migration must not mutate existing capability rows via %q", forbidden)
		}
	}
}

// 0019 seed: cap.workspace.write is a row of its own, so an org can switch the two write tools off
// and keep the reads. It is a low-risk context write with no approval floor (the change is the run's
// own overlay), starts unattested, names only the write tools, and its down migration removes exactly
// what the up migration added.
func TestWorkspaceWriteCapabilityIsASeparateUnattestedRow(t *testing.T) {
	t.Parallel()

	up := readMigration(t, "0019_workspace_write_capability.up.sql")
	for _, required := range []string{
		"'cap.workspace.write'",
		"'workspace_edit'",
		"'workspace_put'",
		"'low'",
		"'global'",
		"ARRAY['global']",
		"ARRAY['execution-dispatch']",
		"'execution-dispatch:cap.workspace.write:v1'",
		"'unavailable'",
		"'health_not_attested'",
		"'context_write'",
		"'run_overlay_only'",
		"'migration:0019_workspace_write_capability'",
		"ON CONFLICT (id) DO UPDATE SET",
		"WHERE capability_tools.source = 'seed'",
	} {
		if !strings.Contains(up, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	if strings.Contains(up, "availability_state = 'available'") {
		t.Fatal("migration must not fabricate a live health attestation")
	}
	for _, read := range []string{"'workspace_read'", "'workspace_grep'", "'workspace_glob'", "'workspace_list'", "'cap.workspace.read'"} {
		if strings.Contains(up, read) {
			t.Fatalf("the write capability must not carry the read family, found %s", read)
		}
	}
	down := readMigration(t, "0019_workspace_write_capability.down.sql")
	for _, required := range []string{
		"DELETE FROM capability_tools",
		"DELETE FROM capabilities",
		"'cap.workspace.write'",
		"'migration:0019_workspace_write_capability'",
	} {
		if !strings.Contains(down, required) {
			t.Fatalf("down migration missing %q", required)
		}
	}
}

// 0018 seed: cap.workspace.read is one low-risk row for the four read-only workspace
// tools. It starts unattested, names no write tool (writes are a separate capability,
// WS24-S4), and its down migration removes exactly what the up migration added.
func TestWorkspaceReadCapabilityStaysReadOnlyAndUnattested(t *testing.T) {
	t.Parallel()

	up := readMigration(t, "0018_workspace_read_capability.up.sql")
	for _, required := range []string{
		"'cap.workspace.read'",
		"'workspace_read'",
		"'low'",
		"'global'",
		"ARRAY['global']",
		"ARRAY['execution-dispatch']",
		"'execution-dispatch:cap.workspace.read:v1'",
		"'unavailable'",
		"'health_not_attested'",
		"health_checked_at",
		"ON CONFLICT (id) DO UPDATE SET",
		"'migration:0018_workspace_read_capability'",
		"'writes', 'none'",
	} {
		if !strings.Contains(up, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	if strings.Contains(up, "availability_state = 'available'") {
		t.Fatal("migration must not fabricate a live health attestation")
	}
	for _, member := range []string{"'workspace_read'", "'workspace_grep'", "'workspace_glob'", "'workspace_list'"} {
		if !strings.Contains(up, member) {
			t.Fatalf("migration must declare the whole read family, missing %q", member)
		}
	}
	for _, write := range []string{"'workspace_edit'", "'workspace_put'", "'workspace_write'"} {
		if strings.Contains(up, write) {
			t.Fatalf("the read capability must not name the write tool %s", write)
		}
	}
	down := readMigration(t, "0018_workspace_read_capability.down.sql")
	for _, required := range []string{
		"DELETE FROM capability_tools",
		"DELETE FROM capabilities",
		"'cap.workspace.read'",
		"'migration:0018_workspace_read_capability'",
	} {
		if !strings.Contains(down, required) {
			t.Fatalf("down migration missing %q", required)
		}
	}
}
