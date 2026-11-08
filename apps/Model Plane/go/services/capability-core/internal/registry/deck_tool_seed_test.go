package registry

import (
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// EP-5 S2: migration 0033 seeds the class of model-gateway's `deck` tool. Its build is a
// server-authored code_interpreter step, so the tool must carry exactly code_interpreter's
// capability, class and floor; a stricter or looser row would describe a different risk than
// the step it runs.

func TestDeckToolIsSeededWithTheClassOfItsCodeInterpreterStep(t *testing.T) {
	t.Parallel()

	matches := seedRowPattern.FindAllStringSubmatch(readMigration(t, "0033_deck_tool_class.up.sql"), -1)
	if len(matches) != 1 {
		t.Fatalf("0033 seeds %d rows, want exactly the deck row", len(matches))
	}
	m := matches[0]
	deck := seedRow{capabilityID: m[1], tool: m[2], class: m[3], floor: m[4], refinement: strings.Trim(m[5], "'")}
	if deck.refinement == "NULL" {
		deck.refinement = ""
	}
	want := seedFor(t, "code_interpreter")
	if deck.tool != "deck" || deck.capabilityID != want.capabilityID || deck.class != want.class || deck.floor != want.floor || deck.refinement != "" {
		t.Errorf("deck seeded %+v, want the code_interpreter row's capability, class and floor %+v", deck, want)
	}
	if deck.class != models.SideEffectSandboxCompute || deck.floor != models.ApprovalFloorNone {
		t.Errorf("deck seeded %s/%s, want sandbox_compute with no floor", deck.class, deck.floor)
	}
}

// The migration re-runs at every boot without rewriting an operator's override, and its revert
// removes only the seeded row.
func TestDeckToolMigrationIsIdempotentAndReverts(t *testing.T) {
	t.Parallel()

	up := readMigration(t, "0033_deck_tool_class.up.sql")
	for _, required := range []string{
		"ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET",
		"WHERE capability_tools.source = 'seed';",
	} {
		if !strings.Contains(up, required) {
			t.Errorf("0033 up migration missing %q", required)
		}
	}
	if strings.Contains(up, "INSERT INTO capabilities") {
		t.Error("0033 must not register a capability: deck runs under cap.command.sandbox")
	}
	down := readMigration(t, "0033_deck_tool_class.down.sql")
	for _, required := range []string{
		"DELETE FROM capability_tools",
		"capability_id = 'cap.command.sandbox'",
		"tool_name = 'deck'",
		"source = 'seed'",
	} {
		if !strings.Contains(down, required) {
			t.Errorf("0033 down migration missing %q", required)
		}
	}
}
