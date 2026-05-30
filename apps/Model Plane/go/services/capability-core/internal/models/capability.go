// Package models contains the capability domain model.
package models

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
}
