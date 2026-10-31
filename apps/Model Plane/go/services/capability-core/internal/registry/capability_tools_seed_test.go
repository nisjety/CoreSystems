package registry

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// seedRow is one line of the capability_tools seed in migration 0016. The
// migration keeps one tuple per line so this test, and execution-core's
// tool_class_registry_parity.rs, can read the declarations as text.
type seedRow struct {
	capabilityID, tool, class, floor, refinement string
}

var seedRowPattern = regexp.MustCompile(`\('global', '([^']+)', '([^']+)', '([a-z_]+)', '([a-z_]+)', (NULL|'[a-z_]+'), 'seed'\)`)

func readSeedRows(t *testing.T) []seedRow {
	t.Helper()
	// 0016 seeds the classes; 0018, 0019, 0020, 0024 and 0026 add the rows of the tools registered after it.
	sql := readMigration(t, "0016_capability_tool_classes.up.sql") +
		readMigration(t, "0018_workspace_read_capability.up.sql") +
		readMigration(t, "0019_workspace_write_capability.up.sql") +
		readMigration(t, "0020_result_query_capability.up.sql") +
		readMigration(t, "0024_provider_read_tool.up.sql") +
		readMigration(t, "0026_knowledge_list_documents_tool.up.sql")
	var rows []seedRow
	for _, m := range seedRowPattern.FindAllStringSubmatch(sql, -1) {
		refinement := strings.Trim(m[5], "'")
		if refinement == "NULL" {
			refinement = ""
		}
		rows = append(rows, seedRow{m[1], m[2], m[3], m[4], refinement})
	}
	if len(rows) == 0 {
		t.Fatal("no seed rows found in 0016_capability_tool_classes.up.sql")
	}
	return rows
}

// Every seeded tool carries a class on purpose. A row left at `unclassified`
// would be the strictest class by accident, and a typo would be rejected by the
// table's CHECK only after a deploy, so both are caught here at test time.
func TestEverySeededToolIsClassifiedOnPurpose(t *testing.T) {
	t.Parallel()

	seen := map[string]string{}
	for _, row := range readSeedRows(t) {
		id := row.capabilityID + "/" + row.tool
		if !models.IsSupportedSideEffectClass(row.class) {
			t.Errorf("%s: %q is not a class", id, row.class)
		}
		if row.class == models.SideEffectUnclassified {
			t.Errorf("%s: seeded as unclassified; classify it or take it out of the seed", id)
		}
		if err := models.ValidateClassAndFloor(row.class, row.floor); err != nil {
			t.Errorf("%s: %v", id, err)
		}
		if row.refinement != "" && (row.refinement != models.CallRefinementProviderOperation || row.tool != "execute_provider_action") {
			t.Errorf("%s: refinement %q is only for execute_provider_action", id, row.refinement)
		}
		// A tool name is bound to one capability: execution-core resolves the
		// capability from the name alone.
		if owner, dup := seen[row.tool]; dup {
			t.Errorf("tool %q is seeded under both %s and %s", row.tool, owner, row.capabilityID)
		}
		seen[row.tool] = row.capabilityID
	}
	if row := seedFor(t, "execute_provider_action"); row.refinement != models.CallRefinementProviderOperation {
		t.Errorf("execute_provider_action must carry the provider_operation refinement, got %q", row.refinement)
	}
}

func seedFor(t *testing.T, tool string) seedRow {
	t.Helper()
	for _, row := range readSeedRows(t) {
		if row.tool == tool {
			return row
		}
	}
	t.Fatalf("tool %q is not seeded", tool)
	return seedRow{}
}

// A seeded capability id must be a row the registry actually has, or the class
// would sit on a capability nothing evaluates.
func TestSeededCapabilityIDsExistInTheRegistry(t *testing.T) {
	t.Parallel()

	migrations, err := filepath.Glob(filepath.Join("..", "..", "migrations", "*.up.sql"))
	if err != nil || len(migrations) == 0 {
		t.Fatalf("list migrations: %v", err)
	}
	var known strings.Builder
	for _, path := range migrations {
		if strings.HasSuffix(path, "0016_capability_tool_classes.up.sql") {
			continue
		}
		contents, readErr := os.ReadFile(path)
		if readErr != nil {
			t.Fatalf("read %s: %v", path, readErr)
		}
		known.Write(contents)
	}
	static, err := staticSeedSource{}.Load()
	if err != nil {
		t.Fatalf("load static seed: %v", err)
	}
	for _, capability := range static {
		known.WriteString("'" + capability.ID + "'")
	}
	for _, row := range readSeedRows(t) {
		if !strings.Contains(known.String(), "'"+row.capabilityID+"'") {
			t.Errorf("%s/%s: capability %s is in no earlier migration and not in the static seed", row.capabilityID, row.tool, row.capabilityID)
		}
	}
}

// The ceiling of a capability is the strictest of its tools, so a high-risk
// seeded capability can never carry a ceiling that reads as harmless.
func TestSeededCeilingsAreTheStrictestOfTheirTools(t *testing.T) {
	t.Parallel()

	class := map[string]string{}
	floor := map[string]string{}
	for _, row := range readSeedRows(t) {
		class[row.capabilityID] = models.StricterSideEffectClass(orRead(class[row.capabilityID]), row.class)
		floor[row.capabilityID] = models.StricterApprovalFloor(orNone(floor[row.capabilityID]), row.floor)
	}
	for id, want := range map[string][2]string{
		"cap.agent.lineage.read":    {models.SideEffectRead, models.ApprovalFloorAsk},
		"cap.agent.spawn":           {models.SideEffectRead, models.ApprovalFloorNone},
		"cap.browser.open":          {models.SideEffectExternalWrite, models.ApprovalFloorNone},
		"cap.command.sandbox":       {models.SideEffectSandboxCompute, models.ApprovalFloorNone},
		"cap.command.shell":         {models.SideEffectDestructive, models.ApprovalFloorNone},
		"cap.memory.index":          {models.SideEffectContextWrite, models.ApprovalFloorNone},
		"cap.process.background":    {models.SideEffectSandboxCompute, models.ApprovalFloorNone},
		"cap.tool.provider.execute": {models.SideEffectExternalWrite, models.ApprovalFloorNone},
		"cap.tool.ticket.create":    {models.SideEffectOwnerEffect, models.ApprovalFloorDurableOwner},
		"cap.workspace.write":       {models.SideEffectContextWrite, models.ApprovalFloorNone},
	} {
		if class[id] != want[0] || floor[id] != want[1] {
			t.Errorf("%s: ceiling %s/%s, want %s/%s", id, class[id], floor[id], want[0], want[1])
		}
	}
	for _, id := range []string{"cap.command.shell", "cap.browser.open", "cap.tool.shipping.book", "cap.tool.social.publish", "cap.tool.provider.execute"} {
		if !models.IsSeededHighRiskCapability(id) {
			t.Fatalf("%s is expected to be a seeded high-risk capability", id)
		}
		if !models.SideEffectClassIsRisky(class[id]) {
			t.Errorf("%s: high-risk capability seeded with a ceiling of %s, which is not risky", id, class[id])
		}
	}
}

func orRead(class string) string {
	if class == "" {
		return models.SideEffectRead
	}
	return class
}

func orNone(floor string) string {
	if floor == "" {
		return models.ApprovalFloorNone
	}
	return floor
}

// The migration re-runs at every boot: a seed rewrite must never touch a row an
// operator overrode, the class columns must carry their CHECKs, and the revert
// must exist.
func TestToolClassMigrationIsIdempotentAndRevertible(t *testing.T) {
	t.Parallel()

	up := readMigration(t, "0016_capability_tool_classes.up.sql")
	for _, required := range []string{
		"ADD COLUMN IF NOT EXISTS side_effect_class TEXT NOT NULL DEFAULT 'unclassified'",
		"ADD COLUMN IF NOT EXISTS approval_floor TEXT NOT NULL DEFAULT 'none'",
		"capabilities_owner_effect_floor_check",
		"capability_tools_owner_effect_floor_check",
		"CREATE TABLE IF NOT EXISTS capability_tools",
		"PRIMARY KEY (org_id, capability_id, tool_name)",
		"CREATE UNIQUE INDEX IF NOT EXISTS capability_tools_qualified_name_uq",
		"WHERE capability_tools.source = 'seed'",
	} {
		if !strings.Contains(up, required) {
			t.Errorf("0016 up migration missing %q", required)
		}
	}
	down := readMigration(t, "0016_capability_tool_classes.down.sql")
	for _, required := range []string{"DROP TABLE IF EXISTS capability_tools", "DROP COLUMN IF EXISTS side_effect_class", "DROP COLUMN IF EXISTS approval_floor"} {
		if !strings.Contains(down, required) {
			t.Errorf("0016 down migration missing %q", required)
		}
	}
}
