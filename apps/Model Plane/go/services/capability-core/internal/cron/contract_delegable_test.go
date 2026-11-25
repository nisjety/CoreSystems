package cron

import (
	"fmt"
	"reflect"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// delegableFixture is one registry row a test declares.
func delegableFixture(name, capability, class, floor, refinement string) DeclaredTool {
	return DeclaredTool{ToolName: name, CapabilityID: capability, SideEffectClass: class, ApprovalFloor: floor, CallRefinement: refinement}
}

// delegableRegistry is the registry the list and the create check are tested against: one listed tool, every way a
// row can hide a tool, an org override stricter than the seed, and a tool declared under two capabilities.
func delegableRegistry() []DeclaredTool {
	read, none := models.SideEffectRead, models.ApprovalFloorNone
	return []DeclaredTool{
		// Listed: a read tool with no approval floor and no refinement, declared under one capability. The seed and an
		// org row may both declare it, and the list names it once.
		delegableFixture("web_search", "cap.tool.web_search", read, none, ""),
		delegableFixture("web_search", "cap.tool.web_search", read, none, ""),
		delegableFixture("alpha_read", "cap.alpha", read, none, ""),
		// Hidden: a write, a sandbox compute, an approval floor and a refinement.
		delegableFixture("write_tool", "cap.write", models.SideEffectExternalWrite, none, ""),
		delegableFixture("sandbox_run", "cap.sandbox", models.SideEffectSandboxCompute, none, ""),
		delegableFixture("floor_read", "cap.floor", read, models.ApprovalFloorAsk, ""),
		delegableFixture("refined_read", "cap.refined", read, none, models.CallRefinementProviderOperation),
		// Hidden: the non-delegable names and families, whatever their rows say.
		delegableFixture("browser.open", "cap.browser", read, none, ""),
		delegableFixture("mcp__github__list", "cap.mcp", read, none, ""),
		delegableFixture("memory.write", "cap.memory", read, none, ""),
		// Hidden: an org override stricter than the seed row.
		delegableFixture("org_stricter", "cap.org", read, none, ""),
		delegableFixture("org_stricter", "cap.org", read, models.ApprovalFloorAsk, ""),
		// Hidden: two capabilities, since a contract names one.
		delegableFixture("two_caps", "cap.one", read, none, ""),
		delegableFixture("two_caps", "cap.two", read, none, ""),
	}
}

// The list is the read tools a contract may name, sorted by name, each with its one capability. Every way a row can
// hide a tool is in delegableRegistry, and none of them is listed.
func TestDelegableFromRegistryListsOnlyWhatAContractMayName(t *testing.T) {
	want := []DelegableTool{
		{Tool: "alpha_read", Capability: "cap.alpha"},
		{Tool: "web_search", Capability: "cap.tool.web_search"},
	}
	if got := DelegableFromRegistry(delegableRegistry()); !reflect.DeepEqual(got, want) {
		t.Fatalf("listed %+v, want %+v", got, want)
	}
}

// A name or capability that the contract's identifier rules refuse is hidden, so the list never offers a tool that
// ParseScheduleContract would reject as malformed.
func TestDelegableFromRegistryHidesIdentifiersTheContractRefuses(t *testing.T) {
	read, none := models.SideEffectRead, models.ApprovalFloorNone
	declared := []DeclaredTool{
		delegableFixture("Bad-Name", "cap.bad", read, none, ""),
		delegableFixture("upper_capability", "Cap.Upper", read, none, ""),
		delegableFixture("ok_name_2", "cap.ok", read, none, ""),
	}
	want := []DelegableTool{{Tool: "ok_name_2", Capability: "cap.ok"}}
	if got := DelegableFromRegistry(declared); !reflect.DeepEqual(got, want) {
		t.Fatalf("listed %+v, want %+v", got, want)
	}
}

// The list holds at most MaxDelegableTools tools, and the cap keeps the first of them in name order whatever order the
// registry returns.
func TestDelegableFromRegistryKeepsTheFirstHundredInNameOrder(t *testing.T) {
	read, none := models.SideEffectRead, models.ApprovalFloorNone
	var declared []DeclaredTool
	for i := 149; i >= 0; i-- {
		declared = append(declared, delegableFixture(fmt.Sprintf("tool_%03d", i), "cap.bulk", read, none, ""))
	}
	got := DelegableFromRegistry(declared)
	if MaxDelegableTools != 100 || len(got) != 100 {
		t.Fatalf("listed %d tools, want the cap of %d", len(got), MaxDelegableTools)
	}
	if got[0].Tool != "tool_000" || got[99].Tool != "tool_099" {
		t.Fatalf("the cap kept %s to %s, want tool_000 to tool_099", got[0].Tool, got[99].Tool)
	}
}

// With no rows the list is empty and not nil, so the answer is an empty array and not null.
func TestDelegableFromRegistryIsAnEmptyListWithoutRows(t *testing.T) {
	if got := DelegableFromRegistry(nil); got == nil || len(got) != 0 {
		t.Fatalf("no rows listed %#v, want an empty list", got)
	}
}

// The shared predicate and the create check agree on every row: a row the predicate accepts is one the create check
// accepts for its own tool and capability, and a row the predicate refuses is one the create check refuses.
func TestDelegableRowAgreesWithTheCreateCheck(t *testing.T) {
	for _, row := range delegableRegistry() {
		contract := ScheduleContract{Tools: []ContractTool{{Tool: row.ToolName, Capability: row.CapabilityID, Class: models.SideEffectRead}}}
		accepted := VerifyContractAgainstRegistry(contract, []DeclaredTool{row}) == nil
		if delegableRow(row) != accepted {
			t.Errorf("%s (%s, %s, %q): delegableRow = %v, create check accepts = %v",
				row.ToolName, row.SideEffectClass, row.ApprovalFloor, row.CallRefinement, delegableRow(row), accepted)
		}
	}
}

// The list and the create check agree on every (tool, capability) pair the registry declares: a pair the list offers is
// one the create check accepts against every row of the registry, and a pair the list hides is one it refuses.
func TestDelegableFromRegistryAgreesWithTheCreateCheckForEveryPair(t *testing.T) {
	declared := delegableRegistry()
	listed := map[DelegableTool]bool{}
	for _, tool := range DelegableFromRegistry(declared) {
		listed[tool] = true
	}
	checked := map[DelegableTool]bool{}
	for _, row := range declared {
		pair := DelegableTool{Tool: row.ToolName, Capability: row.CapabilityID}
		if checked[pair] {
			continue
		}
		checked[pair] = true
		contract := ScheduleContract{Tools: []ContractTool{{Tool: pair.Tool, Capability: pair.Capability, Class: models.SideEffectRead}}}
		accepted := VerifyContractAgainstRegistry(contract, declared) == nil
		if listed[pair] != accepted {
			t.Errorf("%s under %s: listed = %v, create check accepts = %v", pair.Tool, pair.Capability, listed[pair], accepted)
		}
	}
}
