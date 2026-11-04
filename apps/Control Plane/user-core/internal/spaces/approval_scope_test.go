package spaces

import (
	"strings"
	"testing"
)

// WS10-S2: scope vocabulary, ceiling comparison and the standing-rule
// eligibility rules (pure logic; no database).

func TestApprovalGrantScopeRankAndWithin(t *testing.T) {
	scopes := []ApprovalGrantScope{ApprovalScopeOnce, ApprovalScopeThread, ApprovalScopeAlways}
	for i, s := range scopes {
		if s.Rank() != i+1 {
			t.Errorf("rank(%q) = %d, want %d", s, s.Rank(), i+1)
		}
		if !s.Valid() {
			t.Errorf("%q should be valid", s)
		}
	}
	for _, bad := range []ApprovalGrantScope{"", "forever", "ALWAYS", " always"} {
		if bad.Rank() != 0 || bad.Valid() {
			t.Errorf("unknown scope %q must rank 0 and be invalid", bad)
		}
	}
	// Within is reach <= ceiling, and an unknown value on either side is never allowed.
	for i, s := range scopes {
		for j, ceiling := range scopes {
			if got, want := s.Within(ceiling), i <= j; got != want {
				t.Errorf("%q.Within(%q) = %v, want %v", s, ceiling, got, want)
			}
		}
		if s.Within("forever") || ApprovalGrantScope("forever").Within(s) {
			t.Errorf("an unknown scope must never be within or contain %q", s)
		}
	}
}

func TestStandingRuleEligibleActionAcceptsOnlyMCPTools(t *testing.T) {
	long := "mcp__" + strings.Repeat("x", maxApprovalKeyLen)
	for _, tc := range []struct {
		name, tool, action string
		want               bool
	}{
		{"mcp tool", "mcp__github__list_issues", "mcp__github__list_issues", true},
		{"mcp tool with surrounding space", " mcp__a__b ", "mcp__a__b", true},
		{"bare prefix only", "mcp__", "mcp__", false},
		{"provider write", "execute_provider_action", "execute_provider_action:gmail.send", false},
		{"provider write keyed by tool", "execute_provider_action:gmail.send", "execute_provider_action:gmail.send", false},
		{"owner effect", "tickets.create", "tickets.create", false},
		{"shell", "shell", "shell", false},
		{"browser", "browser.click", "browser.click", false},
		{"tool and action differ", "mcp__a__b", "mcp__a__c", false},
		{"mcp tool but action is a provider write", "mcp__a__b", "execute_provider_action:x", false},
		{"empty", "", "", false},
		{"case differs from prefix", "MCP__a__b", "MCP__a__b", false},
		{"oversized key", long, long, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if got := StandingRuleEligibleAction(tc.tool, tc.action); got != tc.want {
				t.Fatalf("StandingRuleEligibleAction(%q, %q) = %v, want %v", tc.tool, tc.action, got, tc.want)
			}
		})
	}
}

func TestApprovalPolicyUpdateValidate(t *testing.T) {
	valid := ApprovalPolicyUpdate{
		MaxGrantScope: "always", DefaultAddresseeKind: ApprovalAddresseeAnyManager,
		QuarantineMode: QuarantineShadow, ExpectedRevision: 0,
	}
	if err := valid.Validate(); err != nil {
		t.Fatalf("valid update refused: %v", err)
	}
	for name, mutate := range map[string]func(*ApprovalPolicyUpdate){
		"unknown scope":      func(u *ApprovalPolicyUpdate) { u.MaxGrantScope = "forever" },
		"empty scope":        func(u *ApprovalPolicyUpdate) { u.MaxGrantScope = "" },
		"unknown addressee":  func(u *ApprovalPolicyUpdate) { u.DefaultAddresseeKind = "everyone" },
		"unknown quarantine": func(u *ApprovalPolicyUpdate) { u.QuarantineMode = "paranoid" },
		"negative revision":  func(u *ApprovalPolicyUpdate) { u.ExpectedRevision = -1 },
	} {
		t.Run(name, func(t *testing.T) {
			u := valid
			mutate(&u)
			if err := u.Validate(); err == nil {
				t.Fatalf("%s was accepted", name)
			}
		})
	}
	for _, kind := range []string{ApprovalAddresseeRequester, ApprovalAddresseeAnyEditor, ApprovalAddresseeAnyManager} {
		u := valid
		u.DefaultAddresseeKind = kind
		if err := u.Validate(); err != nil {
			t.Errorf("addressee %q refused: %v", kind, err)
		}
	}
}

func TestApprovalPolicyLoosensOnlyWhenTheCeilingRises(t *testing.T) {
	def := DefaultApprovalPolicy("space-1", "org-1")
	if def.MaxGrantScope != string(ApprovalScopeOnce) || def.Revision != 0 ||
		def.DefaultAddresseeKind != ApprovalAddresseeRequester || def.QuarantineMode != QuarantineOff {
		t.Fatalf("the unstored default must be the most restrictive policy: %#v", def)
	}
	update := func(scope string) ApprovalPolicyUpdate {
		return ApprovalPolicyUpdate{MaxGrantScope: scope, DefaultAddresseeKind: ApprovalAddresseeRequester, QuarantineMode: QuarantineOff}
	}
	for _, tc := range []struct {
		current, next string
		want          bool
	}{
		{"once", "once", false},
		{"once", "thread", true},
		{"once", "always", true},
		{"thread", "once", false},
		{"thread", "thread", false},
		{"thread", "always", true},
		{"always", "once", false},
		{"always", "always", false},
	} {
		current := def
		current.MaxGrantScope = tc.current
		if got := update(tc.next).Loosens(current); got != tc.want {
			t.Errorf("%s -> %s Loosens = %v, want %v", tc.current, tc.next, got, tc.want)
		}
	}
}

func TestApprovalGateViewDigestIsStableAndBindsRevisionCeilingAndQuarantine(t *testing.T) {
	base := ApprovalPolicy{SpaceRef: "space-1", OrgID: "org-1", MaxGrantScope: "always", QuarantineMode: QuarantineShadow, Revision: 3}
	first := ApprovalGateViewDigest(base)
	if first != ApprovalGateViewDigest(base) {
		t.Fatal("digest is not deterministic")
	}
	if !validSHA256Commitment(first) {
		t.Fatalf("digest %q is not a sha256 commitment", first)
	}
	// Fields the gate does not cache must not move the digest.
	other := base
	other.SpaceRef, other.OrgID, other.UpdatedBy, other.DefaultAddresseeKind = "space-2", "org-2", "someone", ApprovalAddresseeAnyManager
	if ApprovalGateViewDigest(other) != first {
		t.Error("space, org, updater and addressee must not affect the gate view digest")
	}
	for name, mutate := range map[string]func(*ApprovalPolicy){
		"revision":   func(p *ApprovalPolicy) { p.Revision++ },
		"ceiling":    func(p *ApprovalPolicy) { p.MaxGrantScope = "once" },
		"quarantine": func(p *ApprovalPolicy) { p.QuarantineMode = QuarantineEnforce },
	} {
		changed := base
		mutate(&changed)
		if ApprovalGateViewDigest(changed) == first {
			t.Errorf("changing %s did not change the gate view digest", name)
		}
	}
}

// The two W2 entitlement bits that this slice and WS12-S1 added are their own
// deny-by-default floors: a zero policy has both off.
func TestApprovalGrantAndDMCreateEntitlementsDefaultOff(t *testing.T) {
	var zero EffectPolicy
	for _, key := range []string{"approval_grant", "dm_create"} {
		spec, ok := EntitlementSpecByKey(key)
		if !ok {
			t.Fatalf("catalogue lacks %q", key)
		}
		if spec.Get(zero) {
			t.Errorf("%s is on in a zero policy", key)
		}
		on := zero
		spec.Set(&on, true)
		if !spec.Get(on) {
			t.Errorf("%s Set(true) did not stick", key)
		}
		// Setting one bit must not set another.
		for _, other := range EntitlementCatalog() {
			if other.Key != key && other.Get(on) {
				t.Errorf("setting %s also set %s", key, other.Key)
			}
		}
	}
}
