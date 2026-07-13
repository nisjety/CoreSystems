package models

import (
	"strings"
	"time"
)

// AvailabilityState is the machine-readable product state shared by Model
// Plane capability consumers. Registry presence and Enabled alone never prove
// runtime availability.
type AvailabilityState string

const (
	AvailabilityAvailable        AvailabilityState = "available"
	AvailabilityDisabled         AvailabilityState = "disabled"
	AvailabilityUnhealthy        AvailabilityState = "unhealthy"
	AvailabilityApprovalRequired AvailabilityState = "approval_required"
	AvailabilityUnavailable      AvailabilityState = "unavailable"
	AvailabilityNotConfigured    AvailabilityState = "not_configured"
)

const (
	ExecutionDirectRead  = "direct_read"
	ExecutionAgentic     = "agentic"
	ExecutionUnavailable = "unavailable"

	CostUnknown  = "unknown"
	CostBounded  = "bounded"
	CostVariable = "variable"
)

const (
	// AvailabilityAttestationTTL bounds how long runtime health may be treated
	// as current when no newer workload attestation has arrived.
	AvailabilityAttestationTTL = 5 * time.Minute
	// AvailabilityFutureSkew tolerates ordinary clock skew while rejecting
	// health timestamps that could artificially extend the freshness window.
	AvailabilityFutureSkew = 30 * time.Second
)

// Availability is a normalized immutable view suitable for wire responses.
type Availability struct {
	State            AvailabilityState `json:"state"`
	ReasonCode       string            `json:"reason_code"`
	Reason           string            `json:"reason"`
	RequiresApproval bool              `json:"requires_approval"`
	ExecutionMode    string            `json:"execution_mode"`
	CostClass        string            `json:"cost_class"`
	HealthCheckedAt  string            `json:"health_checked_at"`
}

// DeriveAvailability normalizes a capability's explicit runtime attestation.
// Missing or invalid attestations fail closed as unavailable. High-risk or
// mutation-capable healthy entries are always exposed as approval_required.
func DeriveAvailability(capability *Capability) Availability {
	return DeriveAvailabilityAt(capability, time.Now().UTC())
}

// DeriveAvailabilityAt is the deterministic policy form used by tests and
// callers that already hold a trusted clock value.
func DeriveAvailabilityAt(capability *Capability, now time.Time) Availability {
	if capability == nil {
		return unavailable("invalid_capability", "Capability metadata is missing.")
	}
	if !IsSupportedRiskLevel(capability.RiskLevel) {
		return unavailableWithApproval(
			"invalid_risk_level",
			"Capability risk metadata is invalid and has been quarantined.",
			true,
		)
	}

	requiresApproval := riskRequiresApproval(capability.RiskLevel) ||
		strings.TrimSpace(capability.AvailabilityState) == string(AvailabilityApprovalRequired)

	if !capability.Enabled {
		return Availability{
			State:            AvailabilityDisabled,
			ReasonCode:       "capability_disabled",
			Reason:           "Capability is disabled by registry policy.",
			RequiresApproval: requiresApproval,
			ExecutionMode:    ExecutionUnavailable,
			CostClass:        normalizeCostClass(capability.CostClass),
			HealthCheckedAt:  formatHealthCheckedAt(capability.HealthCheckedAt),
		}
	}

	switch strings.TrimSpace(capability.RolloutState) {
	case "", "stable", "canary":
	case "quarantine":
		return unavailableForCapability(
			capability,
			"rollout_quarantine",
			"Capability is quarantined by rollout policy.",
			requiresApproval,
		)
	case "deprecated":
		return unavailableForCapability(
			capability,
			"rollout_deprecated",
			"Capability is deprecated by rollout policy.",
			requiresApproval,
		)
	default:
		return unavailableForCapability(
			capability,
			"invalid_rollout_state",
			"Capability rollout metadata is invalid and has been quarantined.",
			requiresApproval,
		)
	}

	state, valid := normalizeState(capability.AvailabilityState)
	if !valid {
		return unavailableWithApproval(
			"invalid_availability_attestation",
			"Capability runtime availability attestation is invalid.",
			requiresApproval,
		)
	}
	if state == "" {
		return unavailableWithApproval(
			"health_not_attested",
			"Capability runtime health has not been attested.",
			requiresApproval,
		)
	}
	if state == AvailabilityNotConfigured {
		reasonCode := strings.TrimSpace(capability.ReasonCode)
		if reasonCode == "" {
			reasonCode = defaultReasonCode(state)
		}
		return Availability{
			State:            state,
			ReasonCode:       reasonCode,
			Reason:           strings.TrimSpace(capability.Reason),
			RequiresApproval: requiresApproval,
			ExecutionMode:    ExecutionUnavailable,
			CostClass:        normalizeCostClass(capability.CostClass),
		}
	}
	if capability.HealthCheckedAt == nil {
		return unavailableForCapability(
			capability,
			"health_not_attested",
			"Capability runtime health has not been attested.",
			requiresApproval,
		)
	}
	checkedAt := capability.HealthCheckedAt.UTC()
	now = now.UTC()
	if checkedAt.After(now.Add(AvailabilityFutureSkew)) {
		return unavailableForCapability(
			capability,
			"health_attestation_in_future",
			"Capability runtime health attestation has an invalid future timestamp.",
			requiresApproval,
		)
	}
	if now.Sub(checkedAt) > AvailabilityAttestationTTL {
		return unavailableForCapability(
			capability,
			"health_attestation_stale",
			"Capability runtime health attestation has expired.",
			requiresApproval,
		)
	}

	if state == AvailabilityAvailable && requiresApproval {
		state = AvailabilityApprovalRequired
	}
	if state == AvailabilityApprovalRequired {
		requiresApproval = true
	}

	executionMode := normalizeExecutionMode(capability.ExecutionMode)
	if !isRunnableState(state) {
		executionMode = ExecutionUnavailable
	}
	if executionMode == ExecutionUnavailable && isRunnableState(state) {
		return unavailableWithApproval(
			"execution_mode_unavailable",
			"Capability has no supported execution mode.",
			requiresApproval,
		)
	}
	if requiresApproval && isRunnableState(state) && executionMode != ExecutionAgentic {
		return unavailableWithApproval(
			"approval_path_unavailable",
			"Approval-required capability has no governed agentic execution path.",
			requiresApproval,
		)
	}

	reasonCode := strings.TrimSpace(capability.ReasonCode)
	if reasonCode == "" {
		reasonCode = defaultReasonCode(state)
	}

	return Availability{
		State:            state,
		ReasonCode:       reasonCode,
		Reason:           strings.TrimSpace(capability.Reason),
		RequiresApproval: requiresApproval,
		ExecutionMode:    executionMode,
		CostClass:        normalizeCostClass(capability.CostClass),
		HealthCheckedAt:  formatHealthCheckedAt(capability.HealthCheckedAt),
	}
}

func unavailableForCapability(capability *Capability, reasonCode, reason string, requiresApproval bool) Availability {
	return Availability{
		State:            AvailabilityUnavailable,
		ReasonCode:       reasonCode,
		Reason:           reason,
		RequiresApproval: requiresApproval,
		ExecutionMode:    ExecutionUnavailable,
		CostClass:        normalizeCostClass(capability.CostClass),
		HealthCheckedAt:  formatHealthCheckedAt(capability.HealthCheckedAt),
	}
}

func unavailable(reasonCode, reason string) Availability {
	return unavailableWithApproval(reasonCode, reason, false)
}

func unavailableWithApproval(reasonCode, reason string, requiresApproval bool) Availability {
	return Availability{
		State:            AvailabilityUnavailable,
		ReasonCode:       reasonCode,
		Reason:           reason,
		RequiresApproval: requiresApproval,
		ExecutionMode:    ExecutionUnavailable,
		CostClass:        CostUnknown,
	}
}

func normalizeState(raw string) (AvailabilityState, bool) {
	state := AvailabilityState(strings.TrimSpace(raw))
	switch state {
	case "":
		return "", true
	case AvailabilityAvailable,
		AvailabilityDisabled,
		AvailabilityUnhealthy,
		AvailabilityApprovalRequired,
		AvailabilityUnavailable,
		AvailabilityNotConfigured:
		return state, true
	default:
		return "", false
	}
}

func normalizeExecutionMode(raw string) string {
	switch strings.TrimSpace(raw) {
	case ExecutionDirectRead:
		return ExecutionDirectRead
	case ExecutionAgentic:
		return ExecutionAgentic
	default:
		return ExecutionUnavailable
	}
}

func normalizeCostClass(raw string) string {
	switch strings.TrimSpace(raw) {
	case CostBounded:
		return CostBounded
	case CostVariable:
		return CostVariable
	default:
		return CostUnknown
	}
}

func riskRequiresApproval(raw string) bool {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case RiskHigh, "mutation", "dangerous":
		return true
	default:
		return false
	}
}

func isRunnableState(state AvailabilityState) bool {
	return state == AvailabilityAvailable || state == AvailabilityApprovalRequired
}

func defaultReasonCode(state AvailabilityState) string {
	switch state {
	case AvailabilityAvailable:
		return "runtime_available"
	case AvailabilityDisabled:
		return "capability_disabled"
	case AvailabilityUnhealthy:
		return "runtime_unhealthy"
	case AvailabilityApprovalRequired:
		return "human_approval_required"
	case AvailabilityNotConfigured:
		return "capability_not_configured"
	default:
		return "runtime_unavailable"
	}
}

func formatHealthCheckedAt(checkedAt *time.Time) string {
	if checkedAt == nil || checkedAt.IsZero() {
		return ""
	}
	return checkedAt.UTC().Format(time.RFC3339)
}
