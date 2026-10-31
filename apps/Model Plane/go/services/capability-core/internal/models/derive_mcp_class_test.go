package models

import (
	"testing"
	"time"
)

// The trust matrix: what a server's declared side effect derives to, for a
// server whose annotations the registrant did not trust (the default) and one
// whose annotations they did. Derivation only ever tightens: the only cell where
// trust changes the answer is a read-only claim, and silence is never a read.
func TestDeriveMCPClassTrustMatrix(t *testing.T) {
	cases := []struct {
		declared  string
		untrusted string
		trusted   string
	}{
		{DeclaredReadOnly, SideEffectExternalWrite, SideEffectRead},
		{DeclaredNonDestructiveWrite, SideEffectExternalWrite, SideEffectExternalWrite},
		{DeclaredDestructive, SideEffectDestructive, SideEffectDestructive},
		{DeclaredUndeclared, SideEffectUnclassified, SideEffectUnclassified},
		{"", SideEffectUnclassified, SideEffectUnclassified},
		{"read-only", SideEffectUnclassified, SideEffectUnclassified},
		{"READ_ONLY", SideEffectUnclassified, SideEffectUnclassified},
		{"harmless", SideEffectUnclassified, SideEffectUnclassified},
	}
	for _, c := range cases {
		if got := DeriveMCPClass(c.declared, false); got != c.untrusted {
			t.Errorf("DeriveMCPClass(%q, untrusted) = %q, want %q", c.declared, got, c.untrusted)
		}
		if got := DeriveMCPClass(c.declared, true); got != c.trusted {
			t.Errorf("DeriveMCPClass(%q, trusted) = %q, want %q", c.declared, got, c.trusted)
		}
	}
}

// Trust is the only input that can make a derived class weaker, and it can do so
// for exactly one claim; no declared value ever derives to a class weaker than
// the class derived without trust unless that claim is read-only.
func TestTrustLowersOnlyAReadOnlyClaim(t *testing.T) {
	for _, declared := range []string{DeclaredReadOnly, DeclaredNonDestructiveWrite, DeclaredDestructive, DeclaredUndeclared, "", "anything"} {
		without := DeriveMCPClass(declared, false)
		with := DeriveMCPClass(declared, true)
		lowered := SideEffectClassRank(with) < SideEffectClassRank(without)
		if lowered != (declared == DeclaredReadOnly) {
			t.Errorf("declared %q: trust lowered=%v (%s -> %s)", declared, lowered, without, with)
		}
		if SideEffectClassRank(with) > SideEffectClassRank(without) {
			t.Errorf("declared %q: trust raised the class (%s -> %s)", declared, without, with)
		}
	}
}

// Observation applies a derived class to a stored one through Stricter, so no
// pair of (stored, derived) may end below the stored class.
func TestStricterNeverEndsBelowTheStoredClassOrFloor(t *testing.T) {
	classes := []string{SideEffectRead, SideEffectSandboxCompute, SideEffectContextWrite, SideEffectExternalWrite,
		SideEffectDestructive, SideEffectOwnerEffect, SideEffectUnclassified}
	for _, stored := range classes {
		for _, declared := range []string{DeclaredReadOnly, DeclaredNonDestructiveWrite, DeclaredDestructive, DeclaredUndeclared} {
			for _, trusted := range []bool{false, true} {
				got := StricterSideEffectClass(stored, DeriveMCPClass(declared, trusted))
				if SideEffectClassRank(got) < SideEffectClassRank(stored) {
					t.Errorf("stored %s + declared %s (trusted=%v) ended at %s", stored, declared, trusted, got)
				}
			}
		}
	}
	for _, class := range classes {
		floor := MinimumApprovalFloor(class)
		if err := ValidateClassAndFloor(class, StricterApprovalFloor(ApprovalFloorNone, floor)); err != nil {
			t.Errorf("class %s with its own minimum floor is not storable: %v", class, err)
		}
	}
}

func TestMCPToolObservationTTL(t *testing.T) {
	for _, c := range []struct {
		name string
		set  *string
		want time.Duration
	}{
		{"unset", nil, time.Hour},
		{"empty", ptr(""), time.Hour},
		{"blank", ptr("   "), time.Hour},
		{"valid", ptr("90m"), 90 * time.Minute},
		{"valid with spaces", ptr(" 30m "), 30 * time.Minute},
		{"seconds", ptr("45s"), 45 * time.Second},
		{"capped at a day", ptr("240h"), MaxMCPToolObservationTTL},
		{"exactly the cap", ptr("24h"), MaxMCPToolObservationTTL},
		{"zero is not no limit", ptr("0s"), time.Hour},
		{"negative is not no limit", ptr("-5m"), time.Hour},
		{"not a duration", ptr("soon"), time.Hour},
		{"a bare number is not a duration", ptr("3600"), time.Hour},
	} {
		t.Run(c.name, func(t *testing.T) {
			if c.set != nil {
				t.Setenv(MCPToolObservationTTLEnv, *c.set)
			}
			if got := MCPToolObservationTTL(); got != c.want {
				t.Fatalf("MCPToolObservationTTL() = %v, want %v", got, c.want)
			}
		})
	}
}

func ptr(value string) *string { return &value }

func mcpToolRow(checkedAt time.Time) *Capability {
	return &Capability{
		ID: "cap.mcp_tool.abc", Kind: KindMCPTool, Enabled: true, RiskLevel: RiskHigh, RolloutState: "stable",
		AvailabilityState: string(AvailabilityAvailable), ExecutionMode: ExecutionAgentic, CostClass: CostVariable,
		HealthCheckedAt: &checkedAt,
	}
}

// An MCP tool is fresh for the observation window, not for the five minutes a
// workload health attestation gets; every other kind keeps the five minutes.
func TestMCPToolRowsUseTheObservationWindowInsteadOfTheHealthTTL(t *testing.T) {
	now := time.Date(2026, time.October, 21, 12, 0, 0, 0, time.UTC)

	t.Run("seen twenty minutes ago is still available under the default window", func(t *testing.T) {
		t.Setenv(MCPToolObservationTTLEnv, "")
		got := DeriveAvailabilityAt(mcpToolRow(now.Add(-20*time.Minute)), now)
		// High risk, so approval_required: the gate asks, it does not deny.
		if got.State != AvailabilityApprovalRequired {
			t.Fatalf("availability = %+v", got)
		}
	})
	t.Run("the same age is stale for a built-in", func(t *testing.T) {
		builtIn := mcpToolRow(now.Add(-20 * time.Minute))
		builtIn.Kind = KindTool
		got := DeriveAvailabilityAt(builtIn, now)
		if got.State != AvailabilityUnavailable || got.ReasonCode != "health_attestation_stale" {
			t.Fatalf("availability = %+v", got)
		}
	})
	t.Run("past the window it is stale and says so", func(t *testing.T) {
		t.Setenv(MCPToolObservationTTLEnv, "")
		got := DeriveAvailabilityAt(mcpToolRow(now.Add(-61*time.Minute)), now)
		if got.State != AvailabilityUnavailable || got.ReasonCode != ReasonToolObservationStale {
			t.Fatalf("availability = %+v", got)
		}
	})
	t.Run("the window follows MCP_TOOL_OBSERVATION_TTL", func(t *testing.T) {
		t.Setenv(MCPToolObservationTTLEnv, "10m")
		got := DeriveAvailabilityAt(mcpToolRow(now.Add(-20*time.Minute)), now)
		if got.State != AvailabilityUnavailable || got.ReasonCode != ReasonToolObservationStale {
			t.Fatalf("availability = %+v", got)
		}
	})
	t.Run("a future timestamp is still refused", func(t *testing.T) {
		got := DeriveAvailabilityAt(mcpToolRow(now.Add(time.Hour)), now)
		if got.State != AvailabilityUnavailable || got.ReasonCode != "health_attestation_in_future" {
			t.Fatalf("availability = %+v", got)
		}
	})
}

// A tool that is off for a reason the tool table knows names that reason, so the
// policy gate answers tool_definition_changed and not a generic "disabled".
func TestADisabledMCPToolNamesWhyItIsOff(t *testing.T) {
	now := time.Date(2026, time.October, 21, 12, 0, 0, 0, time.UTC)
	for _, c := range []struct {
		reasonCode string
		want       string
	}{
		{ReasonToolDefinitionChanged, ReasonToolDefinitionChanged},
		{ReasonToolNotApproved, ReasonToolNotApproved},
		{"capability_disabled", "capability_disabled"},
		{"", "capability_disabled"},
		{"something_else", "capability_disabled"},
	} {
		row := mcpToolRow(now)
		row.Enabled = false
		row.ReasonCode = c.reasonCode
		got := DeriveAvailabilityAt(row, now)
		if got.State != AvailabilityDisabled || got.ReasonCode != c.want {
			t.Errorf("reason %q: availability = %+v, want disabled/%s", c.reasonCode, got, c.want)
		}
	}
	// The reason code is honoured only for an MCP tool: another kind cannot use it
	// to rename its own disabled state.
	other := mcpToolRow(now)
	other.Kind = KindTool
	other.Enabled = false
	other.ReasonCode = ReasonToolDefinitionChanged
	if got := DeriveAvailabilityAt(other, now); got.ReasonCode != "capability_disabled" {
		t.Fatalf("a non-MCP row took an MCP reason code: %+v", got)
	}
}
