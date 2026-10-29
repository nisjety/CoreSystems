// Package config provides environment-based configuration for bridge-core,
// mirroring the orchestrator-core config pattern.
package config

import (
	"os"
	"strconv"
)

// Config holds all configuration values read from the environment.
type Config struct {
	// HTTPAddr is the listen address for the HTTP API (sessions + health).
	HTTPAddr string
	// GRPCAddr is the listen address for the gRPC server.
	GRPCAddr string

	// SessionCoreAddr / ModelGatewayAddr are upstream Model Plane services
	// bridge-core may forward to. Present in compose; surfaced here so the
	// values are configured in one place.
	SessionCoreAddr  string
	ModelGatewayAddr string

	// Webhook delivery configuration. When WebhookURL is set, the named
	// WebhookChannels are wired to a real WebhookAdapter with durable delivery.
	// When unset, those channels remain on the noop adapter.
	WebhookURL         string
	WebhookChannels    []string
	WebhookMaxAttempts int
}

// Load reads configuration from environment variables with sensible defaults.
func Load() Config {
	return Config{
		HTTPAddr:           envOrDefault("BRIDGE_HTTP_ADDR", ":8091"),
		GRPCAddr:           envOrDefault("BRIDGE_GRPC_ADDR", ":9100"),
		SessionCoreAddr:    envOrDefault("SESSION_CORE_ADDR", "session-core:9091"),
		ModelGatewayAddr:   envOrDefault("MODEL_GATEWAY_ADDR", "model-gateway:9090"),
		WebhookURL:         os.Getenv("BRIDGE_WEBHOOK_URL"),
		WebhookChannels:    splitList(envOrDefault("BRIDGE_WEBHOOK_CHANNELS", "web,api")),
		WebhookMaxAttempts: envInt("BRIDGE_WEBHOOK_MAX_ATTEMPTS", 5),
	}
}

// WebhookEnabled reports whether a real webhook delivery channel is configured.
func (c Config) WebhookEnabled() bool {
	return c.WebhookURL != "" && len(c.WebhookChannels) > 0
}

func envOrDefault(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func envInt(key string, fallback int) int {
	if v := os.Getenv(key); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n > 0 {
			return n
		}
	}
	return fallback
}

// splitList splits a comma-separated env value into a trimmed, non-empty slice.
func splitList(v string) []string {
	var out []string
	start := 0
	for i := 0; i <= len(v); i++ {
		if i == len(v) || v[i] == ',' {
			item := trimSpace(v[start:i])
			if item != "" {
				out = append(out, item)
			}
			start = i + 1
		}
	}
	return out
}

// trimSpace trims ASCII spaces/tabs from both ends without importing strings,
// keeping the config package dependency-free.
func trimSpace(s string) string {
	i, j := 0, len(s)
	for i < j && (s[i] == ' ' || s[i] == '\t') {
		i++
	}
	for j > i && (s[j-1] == ' ' || s[j-1] == '\t') {
		j--
	}
	return s[i:j]
}
