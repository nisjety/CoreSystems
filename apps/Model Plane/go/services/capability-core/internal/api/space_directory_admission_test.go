package api

import (
	"os"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

const spaceDirectoryResolveActionID = "space.directory.resolve"

// A-REC-03-TOOL: migration 0048 seeds the directory resolver's capability and admission rows from the golden registry
// row: the same capability id, schema hash and parameter bytes, no approval, a read-only class, and the capability row
// unavailable until its owner attests it.
func TestSpaceDirectoryMigrationMatchesTheGoldenRow(t *testing.T) {
	golden, found := spaceSelfAPIRowFor(spaceDirectoryResolveActionID)
	if !found {
		t.Fatal("the golden registry has no space.directory.resolve row")
	}
	if golden.DecisionType != "run_action" || golden.Approval != "none" || golden.AutonomyRung != "read_only" ||
		golden.CapabilityID != spaceDirectoryResolveCapabilityID || golden.HashBasis != "parameters_json" {
		t.Fatalf("golden directory row = %+v", golden)
	}
	raw, err := os.ReadFile("../../migrations/0048_space_directory_resolve_capability.up.sql")
	if err != nil {
		t.Fatalf("read migration 0048: %v", err)
	}
	seed := string(raw)
	for name, want := range map[string]string{
		"capability id":    "'" + golden.CapabilityID + "'",
		"schema hash":      "'" + golden.SchemaSHA256 + "'",
		"parameters bytes": "'" + strings.ReplaceAll(golden.ParametersJSON, "'", "''") + "'",
		"read class":       "'read_only', 'read'",
		"no approval":      "FALSE, 'enabled'",
		"unattested":       "'unavailable',\n        'health_not_attested'",
		"admission insert": "INSERT INTO agent_action_admissions",
		"never overwrites": "ON CONFLICT (action_id) DO NOTHING",
	} {
		if !strings.Contains(seed, want) {
			t.Errorf("migration 0048 does not carry the %s %q", name, want)
		}
	}
	down, err := os.ReadFile("../../migrations/0048_space_directory_resolve_capability.down.sql")
	if err != nil {
		t.Fatalf("read migration 0048 down: %v", err)
	}
	if !strings.Contains(string(down), "action_id = 'space.directory.resolve'") ||
		!strings.Contains(string(down), "created_by = 'migration:0048_space_directory_resolve_capability'") {
		t.Fatal("migration 0048 down does not remove exactly its own rows")
	}
	if attester := ownerActionHealthAttesters[spaceDirectoryResolveCapabilityID]; attester == "" || !isOwnerActionCapability(spaceDirectoryResolveCapabilityID) {
		t.Fatal("the directory capability is not an owner-action id with an attester")
	}
}

// The directory admission row, as 0048 seeds it, joins the golden in a signed v2 view; a row whose parameters differ from
// the golden (a model-supplied room, say) refuses the whole view.
func TestSpaceDirectoryAdmissionJoinsTheGoldenInTheSpaceView(t *testing.T) {
	golden, _ := spaceSelfAPIRowFor(spaceDirectoryResolveActionID)
	row := &registry.AgentActionAdmission{
		ActionID: spaceDirectoryResolveActionID, ToolName: spaceDirectoryResolveActionID, CapabilityID: golden.CapabilityID,
		ModelAccess: "read_only", SideEffectClass: "read", ApprovalFloor: "none", Authority: "control_run_decision",
		HashBasis: "owner_contract", ActionSchemaHash: golden.SchemaSHA256, ParametersJSON: golden.ParametersJSON,
		Description: "Look up a person", RequiresApproval: false, Status: registry.AgentActionAdmissionEnabled,
	}
	view := verifiedModelActionView{
		signedActions: []controlModelActionViewV2Action{{ActionID: spaceDirectoryResolveActionID, ActionSchemaHash: golden.SchemaSHA256, Level: "allow"}},
	}
	candidates, err := spaceModelActionCandidates(view, []*registry.AgentActionAdmission{row})
	if err != nil || len(candidates) != 1 || candidates[0].autonomyRung != "read_only" || candidates[0].level != "allow" {
		t.Fatalf("candidates = %+v, err = %v", candidates, err)
	}
	widened := *row
	widened.ParametersJSON = `{"type":"object","properties":{"query":{"type":"string"},"room":{"type":"string"}},"required":["query"],"additionalProperties":false}`
	if _, err := spaceModelActionCandidates(view, []*registry.AgentActionAdmission{&widened}); err == nil {
		t.Fatal("a directory row that disagrees with the golden was offered")
	}
}
