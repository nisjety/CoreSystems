// Package config provides environment-based configuration for orchestrator-core.
package config

import "os"

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
	}
}

func envOrDefault(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}
