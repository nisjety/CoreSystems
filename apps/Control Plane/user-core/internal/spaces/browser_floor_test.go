package spaces

import "testing"

// F-f03-01: the browser floor is Control's own deny-by-default bit for a Space. These tests
// pin its catalogue row, that no other bit implies it, and that an organisation under Space
// Zero Data Retention cannot newly enable it. No database is needed.

// TestBrowserFloorCatalogueRowIsItsOwnFloor pins the row: its column and key, its default,
// the Get and Set round trip, and that no other entitlement implies it.
func TestBrowserFloorCatalogueRowIsItsOwnFloor(t *testing.T) {
	spec, ok := EntitlementSpecByKey("browser")
	if !ok {
		t.Fatal("the catalogue lacks the browser floor row")
	}
	if spec.Column != "browser_entitled" {
		t.Fatalf("browser column = %q, want browser_entitled", spec.Column)
	}
	if spec.ZDR != ZDRUnproven {
		t.Fatalf("browser ZDR class = %q, want %q", spec.ZDR, ZDRUnproven)
	}
	if len(spec.Decisions) != 0 {
		t.Fatalf("browser is read by org-core and gates no Control decision, got %v", spec.Decisions)
	}

	var policy EffectPolicy
	if spec.Get(policy) {
		t.Fatal("the browser floor must default off")
	}
	spec.Set(&policy, true)
	if !policy.BrowserEntitled || !spec.Get(policy) {
		t.Fatal("the browser floor Set/Get do not round-trip")
	}

	// Every other bit on: the browser floor is still off, never inherited.
	everythingElse := EffectPolicy{
		ThreadCreateEntitled: true, ThreadReadEntitled: true, RetrievalReadEntitled: true,
		ImportWriteEntitled: true, AgentActionEntitled: true, ScheduleFireEntitled: true,
		SandboxCapabilityEntitled: true, ProcessRegistryEntitled: true, RunControlEntitled: true,
		ApprovalGrantEntitled: true, DMCreateEntitled: true, MessageReviseEntitled: true,
		MessageModerateEntitled: true,
	}
	if EntitlementValues(everythingElse)["browser"] {
		t.Fatal("the browser floor must never be inherited from another entitlement")
	}
}

// TestBrowserFloorCannotBeEnabledUnderSpaceZDR pins the write path: a Space's organisation
// under Zero Data Retention is refused enabling the browser floor, with the zdr_unproven
// reason and the key named, and switching it off is always allowed.
func TestBrowserFloorCannotBeEnabledUnderSpaceZDR(t *testing.T) {
	zdr := EffectPolicy{OrgID: "org-1", ZeroDataRetention: true}
	err := ValidateEntitlementChange(zdr, withKeys("browser"))
	policyErr := asOrgPolicyError(t, err)
	if policyErr.Code != OrgPolicyCodeZDRConflict {
		t.Fatalf("code = %q, want %q", policyErr.Code, OrgPolicyCodeZDRConflict)
	}
	if policyErr.Details["reason"] != OrgPolicyReasonZDRUnproven {
		t.Fatalf("reason = %v, want %q", policyErr.Details["reason"], OrgPolicyReasonZDRUnproven)
	}
	keys, _ := policyErr.Details["keys"].([]string)
	if len(keys) != 1 || keys[0] != "browser" {
		t.Fatalf("refused keys = %v, want [browser]", policyErr.Details["keys"])
	}

	// Without ZDR the same change is allowed.
	if err := ValidateEntitlementChange(EffectPolicy{OrgID: "org-1"}, withKeys("browser")); err != nil {
		t.Fatalf("a non-ZDR organisation may enable the browser floor: %v", err)
	}

	// Under ZDR, switching the floor off is always allowed.
	onUnderZDR := EffectPolicy{OrgID: "org-1", ZeroDataRetention: true, BrowserEntitled: true}
	if err := ValidateEntitlementChange(onUnderZDR, allEntitlements(false)); err != nil {
		t.Fatalf("switching the browser floor off under ZDR must be allowed: %v", err)
	}
}
