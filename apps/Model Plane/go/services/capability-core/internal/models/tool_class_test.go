package models

import "testing"

// classVectors are the known answers shared with the Rust twin
// (mp-contracts/src/tool_class.rs `known_answers`): the two tables are kept
// identical by hand, so a class that means something different to the registry
// than to the gate fails one of the two suites. minFloor is the floor the class
// implies by itself: `unclassified` implies `ask` (a tool nobody classified is
// never run unasked, even under `auto`), `owner_effect` implies `durable_owner`.
var classVectors = []struct {
	class      string
	rank       int
	risky      bool
	restricted bool
	rung       string
	minFloor   string
}{
	{SideEffectRead, 0, false, false, RungReadOnly, ApprovalFloorNone},
	{SideEffectSandboxCompute, 1, false, false, RungReadOnly, ApprovalFloorNone},
	{SideEffectContextWrite, 2, false, true, RungWorkspaceWrite, ApprovalFloorNone},
	{SideEffectExternalWrite, 3, true, false, RungDangerFullAccess, ApprovalFloorNone},
	{SideEffectDestructive, 4, true, false, RungDangerFullAccess, ApprovalFloorNone},
	{SideEffectOwnerEffect, 5, true, false, RungDangerFullAccess, ApprovalFloorDurableOwner},
	{SideEffectUnclassified, 6, true, false, RungDangerFullAccess, ApprovalFloorAsk},
}

func TestSideEffectClassKnownAnswers(t *testing.T) {
	t.Parallel()

	for _, v := range classVectors {
		if !IsSupportedSideEffectClass(v.class) {
			t.Errorf("%s: not a supported class", v.class)
		}
		if got := SideEffectClassRank(v.class); got != v.rank {
			t.Errorf("%s: rank = %d, want %d", v.class, got, v.rank)
		}
		if got := SideEffectClassIsRisky(v.class); got != v.risky {
			t.Errorf("%s: risky = %v, want %v", v.class, got, v.risky)
		}
		if got := SideEffectClassIsRestrictedContextWrite(v.class); got != v.restricted {
			t.Errorf("%s: restricted context write = %v, want %v", v.class, got, v.restricted)
		}
		if got := SideEffectClassRequiredRung(v.class); got != v.rung {
			t.Errorf("%s: rung = %q, want %q", v.class, got, v.rung)
		}
		if got := MinimumApprovalFloor(v.class); got != v.minFloor {
			t.Errorf("%s: minimum floor = %q, want %q", v.class, got, v.minFloor)
		}
		// A stored `none` floor reads as the class's own minimum, never below.
		if got := EffectiveApprovalFloor(v.class, ApprovalFloorNone); got != v.minFloor {
			t.Errorf("%s: effective floor of a stored none = %q, want %q", v.class, got, v.minFloor)
		}
		// An empty floor is the strictest floor for every class, the same answer
		// the Rust twin's ApprovalFloor::from_wire("") gives.
		if got := EffectiveApprovalFloor(v.class, ""); got != ApprovalFloorDurableOwner {
			t.Errorf("%s: effective floor of an empty floor = %q, want durable_owner", v.class, got)
		}
	}
}

func TestUnknownClassIsTheStrictestClass(t *testing.T) {
	t.Parallel()

	// Anything outside the closed vocabulary, including a near miss in case or
	// padding, is `unclassified`: the registry never reads a typo as a read.
	for _, raw := range []string{"", "READ", "Read", " read", "read ", "unknown", "readonly", "none"} {
		if IsSupportedSideEffectClass(raw) {
			t.Errorf("%q: accepted as a class", raw)
		}
		if got := NormalizeSideEffectClass(raw); got != SideEffectUnclassified {
			t.Errorf("%q: normalised to %q, want unclassified", raw, got)
		}
		if !SideEffectClassIsRisky(raw) {
			t.Errorf("%q: an unknown class must be risky", raw)
		}
		if got := SideEffectClassRequiredRung(raw); got != RungDangerFullAccess {
			t.Errorf("%q: unknown class needs rung %q, want the widest", raw, got)
		}
	}
}

func TestUnknownFloorIsTheStrictestFloor(t *testing.T) {
	t.Parallel()

	for _, raw := range []string{"", "ASK", "owner", "durable", "required"} {
		if IsSupportedApprovalFloor(raw) {
			t.Errorf("%q: accepted as a floor", raw)
		}
		if got := NormalizeApprovalFloor(raw); got != ApprovalFloorDurableOwner {
			t.Errorf("%q: normalised to %q, want durable_owner", raw, got)
		}
	}
	for rank, floor := range []string{ApprovalFloorNone, ApprovalFloorAsk, ApprovalFloorDurableOwner} {
		if got := ApprovalFloorRank(floor); got != rank {
			t.Errorf("%s: rank = %d, want %d", floor, got, rank)
		}
	}
}

func TestStricterPicksTheHigherRank(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct{ a, b, want string }{
		{SideEffectRead, SideEffectDestructive, SideEffectDestructive},
		{SideEffectDestructive, SideEffectRead, SideEffectDestructive},
		{SideEffectOwnerEffect, SideEffectUnclassified, SideEffectUnclassified},
		{SideEffectContextWrite, SideEffectContextWrite, SideEffectContextWrite},
		{"nonsense", SideEffectRead, SideEffectUnclassified},
		{SideEffectRead, "nonsense", SideEffectUnclassified},
	} {
		if got := StricterSideEffectClass(tc.a, tc.b); got != tc.want {
			t.Errorf("stricter(%q, %q) = %q, want %q", tc.a, tc.b, got, tc.want)
		}
	}
	for _, tc := range []struct{ a, b, want string }{
		{ApprovalFloorNone, ApprovalFloorAsk, ApprovalFloorAsk},
		{ApprovalFloorDurableOwner, ApprovalFloorAsk, ApprovalFloorDurableOwner},
		{ApprovalFloorNone, "nonsense", ApprovalFloorDurableOwner},
	} {
		if got := StricterApprovalFloor(tc.a, tc.b); got != tc.want {
			t.Errorf("stricter floor(%q, %q) = %q, want %q", tc.a, tc.b, got, tc.want)
		}
	}
}

func TestOwnerEffectImpliesDurableOwnerFloor(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		class, floor string
		ok           bool
	}{
		{SideEffectOwnerEffect, ApprovalFloorDurableOwner, true},
		{SideEffectOwnerEffect, ApprovalFloorAsk, false},
		{SideEffectOwnerEffect, ApprovalFloorNone, false},
		{SideEffectRead, ApprovalFloorNone, true},
		{SideEffectRead, ApprovalFloorAsk, true},
		// An unclassified row implies an ask, so a pair that stores it with no
		// floor is below what the class implies.
		{SideEffectUnclassified, ApprovalFloorNone, false},
		{SideEffectUnclassified, ApprovalFloorAsk, true},
		{SideEffectUnclassified, ApprovalFloorDurableOwner, true},
		{"", ApprovalFloorNone, false},
		{SideEffectRead, "", false},
		{SideEffectRead, "required", false},
	} {
		err := ValidateClassAndFloor(tc.class, tc.floor)
		if (err == nil) != tc.ok {
			t.Errorf("ValidateClassAndFloor(%q, %q) = %v, want ok=%v", tc.class, tc.floor, err, tc.ok)
		}
	}
	for _, tc := range []struct{ class, floor, want string }{
		{SideEffectOwnerEffect, "", ApprovalFloorDurableOwner},
		{SideEffectOwnerEffect, ApprovalFloorNone, ApprovalFloorDurableOwner},
		// An empty or unknown floor is the strictest floor, as in the Rust twin.
		{SideEffectRead, "", ApprovalFloorDurableOwner},
		{SideEffectRead, "required", ApprovalFloorDurableOwner},
		{"", "", ApprovalFloorDurableOwner},
		{SideEffectRead, ApprovalFloorNone, ApprovalFloorNone},
		{SideEffectRead, ApprovalFloorAsk, ApprovalFloorAsk},
		// An unclassified (or unknown) class is never reported below an ask.
		{SideEffectUnclassified, ApprovalFloorNone, ApprovalFloorAsk},
		{"readonly", ApprovalFloorNone, ApprovalFloorAsk},
		{"", ApprovalFloorNone, ApprovalFloorAsk},
		{SideEffectUnclassified, ApprovalFloorDurableOwner, ApprovalFloorDurableOwner},
	} {
		if got := EffectiveApprovalFloor(tc.class, tc.floor); got != tc.want {
			t.Errorf("EffectiveApprovalFloor(%q, %q) = %q, want %q", tc.class, tc.floor, got, tc.want)
		}
	}
}

func TestDeriveMCPClassOnlyTightens(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		declared string
		trusted  bool
		want     string
	}{
		{DeclaredReadOnly, true, SideEffectRead},
		// A read-only claim from a server the org has not chosen to trust is a
		// third-party claim, not evidence.
		{DeclaredReadOnly, false, SideEffectExternalWrite},
		{DeclaredNonDestructiveWrite, true, SideEffectExternalWrite},
		{DeclaredNonDestructiveWrite, false, SideEffectExternalWrite},
		{DeclaredDestructive, true, SideEffectDestructive},
		{DeclaredDestructive, false, SideEffectDestructive},
		{DeclaredUndeclared, true, SideEffectUnclassified},
		{DeclaredUndeclared, false, SideEffectUnclassified},
		{"", true, SideEffectUnclassified},
		{"surprise", true, SideEffectUnclassified},
	} {
		if got := DeriveMCPClass(tc.declared, tc.trusted); got != tc.want {
			t.Errorf("DeriveMCPClass(%q, trusted=%v) = %q, want %q", tc.declared, tc.trusted, got, tc.want)
		}
	}
}
