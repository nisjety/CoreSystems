package config

import "testing"

func clearEnv(t *testing.T) {
	t.Helper()
	vars := []string{
		"HTTP_PORT", "GRPC_PORT", "DATABASE_URL",
		"NATS_URL", "NATS_TOKEN", "NATS_AUTH_TOKEN",
		"VELION_NATS_URL", "VELION_NATS_TOKEN",
		"NATS_SHARED_URL", "NATS_SHARED_TOKEN",
		"SERVICE_NAME",
		"DRAGONFLY_HOST", "DRAGONFLY_PORT", "DRAGONFLY_PASSWORD", "DRAGONFLY_DB", "DRAGONFLY_ENABLED",
		"CACHE_HOST", "CACHE_PORT", "CACHE_PASSWORD", "CACHE_DB", "CACHE_ENABLED",
		"REDIS_HOST", "REDIS_PORT", "REDIS_PASSWORD", "REDIS_DB", "REDIS_ENABLED",
		"PAYMENT_PROVIDER",
		"STRIPE_BASE_URL", "STRIPE_API_KEY", "STRIPE_API_KEY_SECRET",
		"HYPERSWITCH_BASE_URL", "HYPERSWITCH_API_KEY", "HYPERSWITCH_API_KEY_SECRET",
		"HYPERSWITCH_PUBLISHABLE_KEY", "HYPERSWITCH_PROFILE_ID", "HYPERSWITCH_CLIENT_URL", "HYPERSWITCH_BACKEND_URL",
		"LAGO_BASE_URL", "LAGO_API_KEY",
		"ADAPTER_TIMEOUT_SECONDS",
		"RETRY_POLL_SECONDS", "RETRY_BATCH_SIZE", "RETRY_MAX_ATTEMPTS", "RETRY_BACKOFF_SECONDS",
	}
	for _, key := range vars {
		t.Setenv(key, "")
	}
}

func TestLoad_HyperswitchConfig(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")
	t.Setenv("PAYMENT_PROVIDER", "hyperswitch")
	t.Setenv("HYPERSWITCH_BASE_URL", "https://payments.local")
	t.Setenv("HYPERSWITCH_API_KEY_SECRET", "sk_secret")
	t.Setenv("HYPERSWITCH_PUBLISHABLE_KEY", "pk_snd")
	t.Setenv("HYPERSWITCH_PROFILE_ID", "pro_123")
	t.Setenv("HYPERSWITCH_CLIENT_URL", "https://cdn.local/HyperLoader.js")
	t.Setenv("HYPERSWITCH_BACKEND_URL", "https://api.local")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.PaymentProvider != "hyperswitch" {
		t.Fatalf("PaymentProvider = %q", cfg.PaymentProvider)
	}
	if cfg.HyperswitchBaseURL != "https://payments.local" {
		t.Fatalf("HyperswitchBaseURL = %q", cfg.HyperswitchBaseURL)
	}
	if cfg.HyperswitchAPIKey != "sk_secret" {
		t.Fatalf("HyperswitchAPIKey = %q", cfg.HyperswitchAPIKey)
	}
	if cfg.HyperswitchPublishableKey != "pk_snd" {
		t.Fatalf("HyperswitchPublishableKey = %q", cfg.HyperswitchPublishableKey)
	}
	if cfg.HyperswitchProfileID != "pro_123" {
		t.Fatalf("HyperswitchProfileID = %q", cfg.HyperswitchProfileID)
	}
	if cfg.HyperswitchClientURL != "https://cdn.local/HyperLoader.js" {
		t.Fatalf("HyperswitchClientURL = %q", cfg.HyperswitchClientURL)
	}
	if cfg.HyperswitchBackendURL != "https://api.local" {
		t.Fatalf("HyperswitchBackendURL = %q", cfg.HyperswitchBackendURL)
	}
}

func TestLoad_DefaultGRPCPort(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.GRPCPort != 50013 {
		t.Fatalf("GRPCPort = %d, want 50013", cfg.GRPCPort)
	}
}

func TestLoad_GRPCPortFromEnv(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")
	t.Setenv("GRPC_PORT", "15013")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.GRPCPort != 15013 {
		t.Fatalf("GRPCPort = %d, want 15013", cfg.GRPCPort)
	}
}

func TestLoad_VelionSharedNATSOverridesLegacySharedVars(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")
	t.Setenv("NATS_SHARED_URL", "nats://legacy:4222")
	t.Setenv("NATS_SHARED_TOKEN", "legacy-token")
	t.Setenv("VELION_NATS_URL", "nats://velion-nats:4222")
	t.Setenv("VELION_NATS_TOKEN", "velion-token")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.NATSSharedURL != "nats://velion-nats:4222" {
		t.Fatalf("NATSSharedURL = %q, want %q", cfg.NATSSharedURL, "nats://velion-nats:4222")
	}
	if cfg.NATSSharedToken != "velion-token" {
		t.Fatalf("NATSSharedToken = %q, want %q", cfg.NATSSharedToken, "velion-token")
	}
}
