package models

import (
	"testing"
	"time"
)

func TestDeriveAvailabilityFailsClosedWithoutHealthAttestation(t *testing.T) {
	t.Parallel()

	got := DeriveAvailability(&Capability{
		ID:        "cap.memory.search",
		Enabled:   true,
		RiskLevel: RiskLow,
	})

	if got.State != AvailabilityUnavailable {
		t.Fatalf("state = %q, want %q", got.State, AvailabilityUnavailable)
	}
	if got.ReasonCode != "health_not_attested" {
		t.Fatalf("reason code = %q", got.ReasonCode)
	}
	if got.ExecutionMode != ExecutionUnavailable {
		t.Fatalf("execution mode = %q", got.ExecutionMode)
	}
}

func TestDeriveAvailabilityFailsClosedForUnknownRiskLevel(t *testing.T) {
	t.Parallel()
	checkedAt := time.Date(2026, time.July, 13, 12, 0, 0, 0, time.UTC)

	got := DeriveAvailabilityAt(&Capability{
		Enabled:           true,
		RiskLevel:         "critical-ish",
		AvailabilityState: string(AvailabilityAvailable),
		ExecutionMode:     ExecutionDirectRead,
		HealthCheckedAt:   &checkedAt,
	}, checkedAt)

	if got.State != AvailabilityUnavailable || got.ReasonCode != "invalid_risk_level" || !got.RequiresApproval {
		t.Fatalf("unknown-risk availability = %+v", got)
	}
}

func TestDeriveAvailabilityNeverAdvertisesQuarantinedOrDeprecatedRollouts(t *testing.T) {
	t.Parallel()
	checkedAt := time.Date(2026, time.July, 13, 12, 0, 0, 0, time.UTC)

	for rolloutState, wantReason := range map[string]string{
		"quarantine": "rollout_quarantine",
		"deprecated": "rollout_deprecated",
		"mystery":    "invalid_rollout_state",
	} {
		got := DeriveAvailabilityAt(&Capability{
			Enabled: true, RiskLevel: RiskLow, RolloutState: rolloutState,
			AvailabilityState: string(AvailabilityAvailable),
			ExecutionMode:     ExecutionDirectRead, HealthCheckedAt: &checkedAt,
		}, checkedAt)
		if got.State != AvailabilityUnavailable || got.ReasonCode != wantReason {
			t.Fatalf("rollout %q availability = %+v", rolloutState, got)
		}
	}
}

func TestDeriveAvailabilityDistinguishesDisabledAndUnhealthy(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, time.July, 13, 12, 0, 0, 0, time.UTC)

	tests := []struct {
		name string
		cap  Capability
		want AvailabilityState
	}{
		{
			name: "disabled always wins",
			cap: Capability{
				Enabled:           false,
				RiskLevel:         RiskLow,
				AvailabilityState: string(AvailabilityAvailable),
			},
			want: AvailabilityDisabled,
		},
		{
			name: "unhealthy remains observable",
			cap: Capability{
				Enabled:           true,
				RiskLevel:         RiskLow,
				AvailabilityState: string(AvailabilityUnhealthy),
				HealthCheckedAt:   &now,
			},
			want: AvailabilityUnhealthy,
		},
	}

	for _, test := range tests {
		test := test
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			if got := DeriveAvailabilityAt(&test.cap, now); got.State != test.want {
				t.Fatalf("state = %q, want %q", got.State, test.want)
			}
		})
	}
}

func TestDeriveAvailabilityMarksRiskyHealthyCapabilityApprovalRequired(t *testing.T) {
	t.Parallel()

	checkedAt := time.Date(2026, time.July, 13, 12, 0, 0, 0, time.UTC)
	got := DeriveAvailabilityAt(&Capability{
		Enabled:           true,
		RiskLevel:         RiskHigh,
		AvailabilityState: string(AvailabilityAvailable),
		ReasonCode:        "runtime_healthy",
		ExecutionMode:     ExecutionAgentic,
		CostClass:         CostVariable,
		HealthCheckedAt:   &checkedAt,
	}, checkedAt)

	if got.State != AvailabilityApprovalRequired {
		t.Fatalf("state = %q, want %q", got.State, AvailabilityApprovalRequired)
	}
	if !got.RequiresApproval {
		t.Fatal("expected approval requirement")
	}
	if got.HealthCheckedAt != "2026-07-13T12:00:00Z" {
		t.Fatalf("health_checked_at = %q", got.HealthCheckedAt)
	}
}

func TestDeriveAvailabilityRejectsRiskyDirectExecution(t *testing.T) {
	t.Parallel()
	checkedAt := time.Date(2026, time.July, 13, 12, 0, 0, 0, time.UTC)

	got := DeriveAvailabilityAt(&Capability{
		Enabled:           true,
		RiskLevel:         RiskHigh,
		AvailabilityState: string(AvailabilityAvailable),
		ReasonCode:        "runtime_healthy",
		ExecutionMode:     ExecutionDirectRead,
		HealthCheckedAt:   &checkedAt,
	}, checkedAt)

	if got.State != AvailabilityUnavailable || got.ReasonCode != "approval_path_unavailable" {
		t.Fatalf("risky direct availability = %+v", got)
	}
}

func TestDeriveAvailabilityRejectsUnknownStateAndExecutionMode(t *testing.T) {
	t.Parallel()

	got := DeriveAvailability(&Capability{
		Enabled:           true,
		RiskLevel:         RiskLow,
		AvailabilityState: "green-ish",
		ExecutionMode:     "run-anywhere",
	})

	if got.State != AvailabilityUnavailable || got.ReasonCode != "invalid_availability_attestation" {
		t.Fatalf("unexpected availability: %+v", got)
	}
	if got.ExecutionMode != ExecutionUnavailable {
		t.Fatalf("execution mode = %q", got.ExecutionMode)
	}
}

func TestDeriveAvailabilityRejectsMissingStaleOrFutureHealthProof(t *testing.T) {
	t.Parallel()

	now := time.Date(2026, time.July, 13, 15, 0, 0, 0, time.UTC)
	tests := []struct {
		name       string
		checkedAt  *time.Time
		wantReason string
	}{
		{name: "missing", wantReason: "health_not_attested"},
		{name: "stale", checkedAt: timePointer(now.Add(-AvailabilityAttestationTTL - time.Second)), wantReason: "health_attestation_stale"},
		{name: "future", checkedAt: timePointer(now.Add(AvailabilityFutureSkew + time.Second)), wantReason: "health_attestation_in_future"},
	}

	for _, test := range tests {
		test := test
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			got := DeriveAvailabilityAt(&Capability{
				ID:                "cap.read",
				Enabled:           true,
				RiskLevel:         RiskLow,
				AvailabilityState: string(AvailabilityAvailable),
				ReasonCode:        "runtime_healthy",
				ExecutionMode:     ExecutionDirectRead,
				HealthCheckedAt:   test.checkedAt,
			}, now)
			if got.State != AvailabilityUnavailable || got.ReasonCode != test.wantReason || got.ExecutionMode != ExecutionUnavailable {
				t.Fatalf("availability = %+v", got)
			}
		})
	}
}

func TestNotConfiguredIsStableWithoutFabricatingAHealthProbe(t *testing.T) {
	t.Parallel()

	got := DeriveAvailabilityAt(&Capability{
		ID:                "cap.mcp.visma",
		Enabled:           true,
		RiskLevel:         RiskLow,
		AvailabilityState: string(AvailabilityNotConfigured),
		ReasonCode:        "runtime_not_configured",
		ExecutionMode:     ExecutionUnavailable,
	}, time.Date(2026, time.July, 13, 15, 0, 0, 0, time.UTC))
	if got.State != AvailabilityNotConfigured || got.ReasonCode != "runtime_not_configured" {
		t.Fatalf("not-configured capability = %+v", got)
	}
}

func TestDefaultReasonCodesCoverEveryContractState(t *testing.T) {
	t.Parallel()

	tests := []struct {
		state AvailabilityState
		want  string
	}{
		{state: AvailabilityAvailable, want: "runtime_available"},
		{state: AvailabilityDisabled, want: "capability_disabled"},
		{state: AvailabilityUnhealthy, want: "runtime_unhealthy"},
		{state: AvailabilityApprovalRequired, want: "human_approval_required"},
		{state: AvailabilityNotConfigured, want: "capability_not_configured"},
		{state: AvailabilityUnavailable, want: "runtime_unavailable"},
	}

	for _, test := range tests {
		test := test
		t.Run(string(test.state), func(t *testing.T) {
			t.Parallel()
			if got := defaultReasonCode(test.state); got != test.want {
				t.Fatalf("reason code = %q, want %q", got, test.want)
			}
		})
	}
}

func timePointer(value time.Time) *time.Time {
	return &value
}
