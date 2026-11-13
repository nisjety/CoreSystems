package registry

import (
	"regexp"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// W4-CC-7: migration 0041 registers cap.custom_rest.read and cap.custom_rest.write. Both start
// unavailable: no attestation names them yet, and the migration never fabricates one. These text
// checks keep the seed honest without a database; custom_rest_migration_integration_test.go
// applies it against PostgreSQL.

const customRestMigration = "0041_custom_rest_capabilities.up.sql"

// customRestCapabilityInsert returns the VALUES tuple of one capability insert in 0041.
func customRestCapabilityInsert(t *testing.T, sql, id string) string {
	t.Helper()
	start := strings.Index(sql, ") VALUES (\n    '"+id+"',")
	if start < 0 {
		t.Fatalf("0041 inserts no capability %s", id)
	}
	rest := sql[start:]
	end := strings.Index(rest, "\n)\nON CONFLICT (id)")
	if end < 0 {
		t.Fatalf("0041 capability %s has no ON CONFLICT clause", id)
	}
	return rest[:end]
}

func TestCustomRestCapabilitiesSeedStartsUnavailable(t *testing.T) {
	t.Parallel()
	sql := readMigration(t, customRestMigration)

	for id, risk := range map[string]string{
		"cap.custom_rest.read":  "'low'",
		"cap.custom_rest.write": "'high'",
	} {
		values := customRestCapabilityInsert(t, sql, id)
		// availability_state, availability_reason_code, availability_reason, execution_mode,
		// cost_class, health_checked_at: the six trailing values of the tuple.
		tail := regexp.MustCompile(`'unavailable',\s*'health_not_attested',\s*'[^']*',\s*'unavailable',\s*'bounded',\s*NULL\s*$`)
		if !tail.MatchString(values) {
			t.Errorf("%s: seed is not unavailable / health_not_attested / execution unavailable / unattested:\n%s", id, values)
		}
		if !strings.Contains(values, "\n    "+risk+",\n") {
			t.Errorf("%s: risk is not %s", id, risk)
		}
		if !strings.Contains(values, "'migration:0041_custom_rest_capabilities'") {
			t.Errorf("%s: created_by does not name the migration (the down file deletes by it)", id)
		}
	}
}

// A re-applied 0041 (every boot re-runs every up file) must not overwrite an attestation:
// the ON CONFLICT update may not touch the availability, execution mode or health columns.
func TestCustomRestSeedReapplyNeverResetsAttestedAvailability(t *testing.T) {
	t.Parallel()
	sql := readMigration(t, customRestMigration)
	for _, clause := range regexp.MustCompile(`(?s)ON CONFLICT \(id\) DO UPDATE SET(.*?);`).FindAllStringSubmatch(sql, -1) {
		for _, column := range []string{"availability_state", "availability_reason", "execution_mode", "health_checked_at", "created_by", "org_id", "kind"} {
			if regexp.MustCompile(`\b` + column + `\s*=`).MatchString(clause[1]) {
				t.Errorf("0041 re-apply rewrites %s; an attested row would be reset at every boot", column)
			}
		}
	}
}

// The declared class of each custom REST tool: read is read/none, write is external_write/ask
// (execution-core's gate pauses every write; tool_class_registry_parity.rs pins the same row).
func TestCustomRestToolClassesAreSeeded(t *testing.T) {
	t.Parallel()
	sql := readMigration(t, customRestMigration)
	want := map[string]seedRow{
		"custom_rest.read":  {"cap.custom_rest.read", "custom_rest.read", models.SideEffectRead, models.ApprovalFloorNone, ""},
		"custom_rest.write": {"cap.custom_rest.write", "custom_rest.write", "external_write", models.ApprovalFloorAsk, ""},
	}
	got := map[string]seedRow{}
	for _, m := range seedRowPattern.FindAllStringSubmatch(sql, -1) {
		refinement := strings.Trim(m[5], "'")
		if refinement == "NULL" {
			refinement = ""
		}
		got[m[2]] = seedRow{m[1], m[2], m[3], m[4], refinement}
	}
	if len(got) != len(want) {
		t.Fatalf("0041 seeds %d tool rows, want %d: %+v", len(got), len(want), got)
	}
	for tool, row := range want {
		if got[tool] != row {
			t.Errorf("%s seeded as %+v, want %+v", tool, got[tool], row)
		}
		if !models.IsSupportedSideEffectClass(row.class) {
			t.Errorf("%s: %q is not a class", tool, row.class)
		}
		if err := models.ValidateClassAndFloor(got[tool].class, got[tool].floor); err != nil {
			t.Errorf("%s: %v", tool, err)
		}
	}
	// The class re-assert touches only rows still owned by the seed, so an operator override stays.
	if !strings.Contains(sql, "WHERE capability_tools.source = 'seed';") {
		t.Error("0041 re-asserts tool classes over operator overrides")
	}
}
