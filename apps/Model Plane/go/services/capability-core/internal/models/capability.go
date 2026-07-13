// Package models contains the capability domain model.
package models

import "time"

// Kind values for Capability.Kind.
const (
	KindTool          = "tool"
	KindSkill         = "skill"
	KindRetrieval     = "retrieval"
	KindBrowser       = "browser"
	KindSandbox       = "sandbox"
	KindMemory        = "memory"
	KindInference     = "inference"
	KindPlugin        = "plugin"
	KindMCPServer     = "mcp_server"
	KindModel         = "model"
	KindRoutingPolicy = "routing_policy"
	KindMemoryAdapter = "memory_adapter"
	KindSafetyPolicy  = "safety_policy"
	KindCommand       = "command"
)

// RiskLevel values for Capability.RiskLevel.
const (
	RiskLow    = "low"
	RiskMedium = "medium"
	RiskHigh   = "high"
)

// IsSupportedRiskLevel reports whether raw is one of the closed set of risk
// levels understood by policy and availability evaluation. It is deliberately
// strict so malformed persisted values cannot silently inherit low-risk
// behavior.
func IsSupportedRiskLevel(raw string) bool {
	switch raw {
	case RiskLow, RiskMedium, RiskHigh:
		return true
	default:
		return false
	}
}

// Capability describes a single capability entry in the registry.
type Capability struct {
	ID               string
	Name             string
	Kind             string
	Version          string
	Description      string
	RiskLevel        string
	LazyLoad         bool
	Scope            string
	Enabled          bool
	IdempotencyKey   string
	OrgID            string
	EnabledForScopes []string
	// RolloutState is an independent operator gate. Quarantined, deprecated,
	// or unknown rollout values must never be advertised as runnable.
	RolloutState string
	// AvailabilityState is a runtime attestation, not an alias for Enabled.
	// Empty is intentionally fail-closed and derives to unavailable.
	AvailabilityState string
	ReasonCode        string
	Reason            string
	ExecutionMode     string
	CostClass         string
	HealthCheckedAt   *time.Time
}
