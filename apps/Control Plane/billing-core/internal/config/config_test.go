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
		"REDIS_HOST", "REDIS_PORT", "REDIS_PASSWORD", "REDIS_DB", "REDIS_ENABLED",
		"STRIPE_BASE_URL", "STRIPE_API_KEY", "STRIPE_API_KEY_SECRET",
		"LAGO_BASE_URL", "LAGO_API_KEY",
		"ADAPTER_TIMEOUT_SECONDS",
		"RETRY_POLL_SECONDS", "RETRY_BATCH_SIZE", "RETRY_MAX_ATTEMPTS", "RETRY_BACKOFF_SECONDS",
	}
	for _, key := range vars {
		t.Setenv(key, "")
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
