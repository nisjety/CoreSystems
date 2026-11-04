package registry

import (
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// E3.7.10 (capability-core half): migration 0031 registers cap.web.crawl, cap.web.map and
// cap.web.batch and seeds the class of their three tools. Written with the migration; the
// execution-core binding of the names (capability_policy.rs) is the Rust half.

func TestWebCrawlToolsAreSeededUnderTheirOwnCapabilities(t *testing.T) {
	t.Parallel()

	for tool, want := range map[string]seedRow{
		"knowledge.crawl_site": {capabilityID: "cap.web.crawl", class: models.SideEffectContextWrite, floor: models.ApprovalFloorNone},
		"knowledge.map":        {capabilityID: "cap.web.map", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
		"knowledge.batch":      {capabilityID: "cap.web.batch", class: models.SideEffectContextWrite, floor: models.ApprovalFloorNone},
	} {
		got := seedFor(t, tool)
		if got.capabilityID != want.capabilityID || got.class != want.class || got.floor != want.floor || got.refinement != "" {
			t.Errorf("%s: seeded %+v, want %+v", tool, got, want)
		}
	}
	// None of them rides cap.tool.http: a crawl multiplies pages and spend by a count the model
	// chooses, so it is governed by its own capability.
	for _, row := range readSeedRows(t) {
		if row.capabilityID == "cap.tool.http" && strings.HasPrefix(row.tool, "knowledge.") {
			t.Errorf("%s is seeded under cap.tool.http", row.tool)
		}
	}
}

// The migration re-runs at every boot and never fabricates a health attestation.
func TestWebCrawlMigrationIsIdempotentStartsUnavailableAndReverts(t *testing.T) {
	t.Parallel()

	up := readMigration(t, "0031_web_crawl_tool_capabilities.up.sql")
	for _, required := range []string{
		"ON CONFLICT (id) DO UPDATE SET",
		"ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET",
		"WHERE capability_tools.source = 'seed'",
		"WHEN capabilities.side_effect_class = 'unclassified' THEN EXCLUDED.side_effect_class",
		"'health_not_attested'",
	} {
		if !strings.Contains(up, required) {
			t.Errorf("0031 up migration missing %q", required)
		}
	}
	// Three capabilities, each unavailable in both availability_state and execution_mode.
	if got := strings.Count(up, "    'unavailable',\n") + strings.Count(up, "    'unavailable',\r\n"); got != 6 {
		t.Errorf("0031 marks %d columns unavailable, want 6 (availability_state and execution_mode of three rows)", got)
	}
	if strings.Contains(up, "'available'") {
		t.Error("0031 must not attest availability")
	}
	for _, id := range []string{"cap.web.crawl", "cap.web.map", "cap.web.batch"} {
		if !strings.Contains(up, "'"+id+"'") {
			t.Errorf("0031 up migration does not register %s", id)
		}
	}
	down := readMigration(t, "0031_web_crawl_tool_capabilities.down.sql")
	for _, required := range []string{
		"DELETE FROM capability_tools",
		"DELETE FROM capabilities",
		"created_by = 'migration:0031_web_crawl_tool_capabilities'",
	} {
		if !strings.Contains(down, required) {
			t.Errorf("0031 down migration missing %q", required)
		}
	}
}

// E3.6-8: the platform default of the document_pii key is off, inserted once and never rewritten.
func TestDocumentPIIDefaultPolicyMigrationSeedsOffOnce(t *testing.T) {
	t.Parallel()

	up := readMigration(t, "0032_document_pii_default_policy.up.sql")
	for _, required := range []string{
		"'sp_global_document_pii'",
		"'document_pii'",
		`{"mode": "off", "model_tier": false}`,
		"ON CONFLICT (id) DO NOTHING",
	} {
		if !strings.Contains(up, required) {
			t.Errorf("0032 up migration missing %q", required)
		}
	}
	if strings.Contains(up, "DO UPDATE") {
		t.Error("0032 must never rewrite a row an operator changed")
	}
	down := readMigration(t, "0032_document_pii_default_policy.down.sql")
	if !strings.Contains(down, "DELETE FROM safety_policies") || !strings.Contains(down, "created_by = 'migration:0032_document_pii_default_policy'") {
		t.Error("0032 down migration must delete only its own seeded row")
	}
}
