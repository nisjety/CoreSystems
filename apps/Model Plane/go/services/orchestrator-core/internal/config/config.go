// Package config provides environment-based configuration for orchestrator-core.
package config

import (
	"os"
	"strings"
)

// Config holds all configuration values read from the environment.
type Config struct {
	TemporalAddress    string
	HealthAddr         string
	SessionCoreAddr    string
	InferenceCoreAddr  string
	ExecutionCoreAddr  string
	CapabilityCoreAddr string
	SandboxManagerAddr string
	BrowserBrokerAddr  string
	LettaBridgeAddr    string
	TaskQueue          string

	// OrchestratorGRPCAddr is the listen address for the orchestrator-core
	// gRPC server (mpv1.OrchestrationCoreService).
	OrchestratorGRPCAddr string

	// --- StartWorkflow authentication -------------------------------------
	// These configure the ONLY locally-authenticated RPC on this server. The
	// read proxies keep forwarding the caller's credential to session-core and
	// are unaffected.

	// AuthIssuer / AuthJWKSURL are the Auth Core trust boundary. Absent, the
	// JWT path is unavailable (StartWorkflow refuses rather than opening up).
	AuthIssuer  string
	AuthJWKSURL string
	// AuthAudiences is the comma-separated list of accepted token audiences.
	AuthAudiences []string

	// InternalServiceToken is the Model-Plane-local shared secret accepted on
	// StartWorkflow for hops that have no live per-user bearer (the cron task
	// dispatcher). Mirrors MCP_OAUTH_SERVICE_TOKEN. Empty disables the path.
	InternalServiceToken string
	// InternalServiceOrgs bounds that secret to an explicit set of
	// organizations. There is no wildcard; an empty list disables the path even
	// when a token is set, so the credential can never be unbounded.
	InternalServiceOrgs []string

	// --- Outbound service credential (Temporal activity path) --------------
	// A Temporal activity has no inbound gRPC metadata, so it has nothing to
	// forward and must present a credential it minted itself. These configure
	// that minting; without them every activity calls its sibling
	// uncredentialed and is answered Unauthenticated.

	// AuthCoreURL is Auth Core's base URL, e.g. http://auth-core:3011.
	AuthCoreURL string
	// ServicePrincipalID is orchestrator-core's service-principal id, sent as
	// `x-service-id` when minting (ORCHESTRATOR_CORE_SERVICE_ID).
	ServicePrincipalID string
	// ServicePrincipalKey is that principal's secret (`x-service-api-key`,
	// ORCHESTRATOR_CORE_SERVICE_API_KEY). Never logged.
	ServicePrincipalKey string

	// ControlUserCoreURL is the internal Control endpoint used only by the
	// scheduled-run activity to obtain a fresh effect-time decision.
	ControlUserCoreURL string
	// ControlScheduleExecutionServiceToken is Orchestrator Core's separate
	// Control service credential. Empty intentionally disables scheduled runs
	// rather than replaying Capability Core's preparation authority.
	ControlScheduleExecutionServiceToken string
	ControlScheduleStepServiceToken      string
	ControlSpaceDecisionKeyID            string
	ControlSpaceDecisionPublicKeyBase64  string
}

// Load reads configuration from environment variables with sensible defaults.
func Load() Config {
	return Config{
		TemporalAddress:    envOrDefault("TEMPORAL_ADDRESS", "temporal:7233"),
		HealthAddr:         envOrDefault("HEALTH_ADDR", ":8082"),
		SessionCoreAddr:    envOrDefault("SESSION_CORE_ADDR", "session-core:9091"),
		InferenceCoreAddr:  envOrDefault("INFERENCE_CORE_ADDR", "inference-core:9092"),
		ExecutionCoreAddr:  envOrDefault("EXECUTION_CORE_ADDR", "execution-core:9093"),
		CapabilityCoreAddr: envOrDefault("CAPABILITY_CORE_ADDR", "capability-core:9092"),
		SandboxManagerAddr: envOrDefault("SANDBOX_MANAGER_ADDR", "sandbox-manager:9094"),
		BrowserBrokerAddr:  envOrDefault("BROWSER_BROKER_ADDR", "browser-broker:9095"),
		LettaBridgeAddr:    envOrDefault("LETTA_BRIDGE_ADDR", "letta-bridge:9096"),
		TaskQueue:          envOrDefault("TEMPORAL_TASK_QUEUE", "model-plane-orchestrator"),

		OrchestratorGRPCAddr: envOrDefault("ORCHESTRATOR_GRPC_ADDR", ":9080"),

		AuthIssuer:    strings.TrimSpace(os.Getenv("AUTH_CORE_ISSUER")),
		AuthJWKSURL:   strings.TrimSpace(os.Getenv("AUTH_CORE_JWKS_URL")),
		AuthAudiences: splitList(envOrDefault("ORCHESTRATOR_CORE_AUTH_AUDIENCES", "orchestrator-core")),

		InternalServiceToken: strings.TrimSpace(os.Getenv("ORCHESTRATOR_INTERNAL_SERVICE_TOKEN")),
		InternalServiceOrgs:  splitList(os.Getenv("ORCHESTRATOR_INTERNAL_SERVICE_ORGS")),

		AuthCoreURL:         strings.TrimSpace(os.Getenv("AUTH_CORE_URL")),
		ServicePrincipalID:  strings.TrimSpace(os.Getenv("ORCHESTRATOR_CORE_SERVICE_ID")),
		ServicePrincipalKey: strings.TrimSpace(os.Getenv("ORCHESTRATOR_CORE_SERVICE_API_KEY")),

		ControlUserCoreURL:                   strings.TrimSpace(os.Getenv("CONTROL_USER_CORE_URL")),
		ControlScheduleExecutionServiceToken: strings.TrimSpace(os.Getenv("ORCHESTRATOR_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN")),
		ControlScheduleStepServiceToken:      strings.TrimSpace(os.Getenv("ORCHESTRATOR_CORE_CONTROL_SCHEDULE_STEP_SERVICE_TOKEN")),
		ControlSpaceDecisionKeyID:            strings.TrimSpace(os.Getenv("CONTROL_SPACE_DECISION_KEY_ID")),
		ControlSpaceDecisionPublicKeyBase64:  strings.TrimSpace(os.Getenv("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64")),
	}
}

func envOrDefault(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

// splitList parses a comma-separated env value, dropping blanks so a trailing
// comma or an all-whitespace value reads as "not configured".
func splitList(raw string) []string {
	parts := strings.Split(raw, ",")
	out := make([]string, 0, len(parts))
	for _, p := range parts {
		if p = strings.TrimSpace(p); p != "" {
			out = append(out, p)
		}
	}
	return out
}
