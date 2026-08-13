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

// seededHighRiskCapabilityIDs enumerates capability ids whose risk_level is
// set to RiskHigh by a database migration (a "seed"), never by a tenant write
// through the HTTP/store API. It is the fail-closed backstop for the
// risk-level floor enforced in registry.CapabilitiesStore.Upsert: a write
// targeting one of these ids is always treated as protected even if the row's
// current persisted state cannot be read (see POL-1 in
// apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md).
//
// Sourced by grepping `risk_level` ... 'high' across migrations/*.sql:
//   - migrations/0004_seed_self_owned_systems.up.sql
//   - migrations/0008_execution_dispatch_capabilities.up.sql
//
// Any future migration that seeds a RiskHigh capability MUST add its id here.
// This list is a floor, not the whole mechanism: Upsert also floors any
// capability (seeded or tenant-created) whose *currently persisted*
// risk_level is already high, so a future seed added here late is still
// protected from the moment its row exists.
var seededHighRiskCapabilityIDs = map[string]bool{
	"cap.command.shell":                          true,
	"cap.browser.open":                           true,
	"cap.tool.shipping.book":                     true,
	"cap.tool.social.publish":                    true,
	"cap.tool.provider.execute":                  true,
	"cap.self_owned.misp_opencti":                true,
	"cap.self_owned.quarry_url_reputation_feeds": true,
	"cap.self_owned.opensanctions_yente":         true,
}

// IsSeededHighRiskCapability reports whether id is one of the statically
// seeded RiskHigh capabilities (see seededHighRiskCapabilityIDs).
func IsSeededHighRiskCapability(id string) bool {
	return seededHighRiskCapabilityIDs[id]
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
