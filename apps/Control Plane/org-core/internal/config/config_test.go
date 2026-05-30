package config

import (
	"os"
	"strings"
	"testing"
)

func containsStr(s, sub string) bool { return strings.Contains(s, sub) }

// clearEnv removes the env vars that Load() reads so tests start clean.
func clearEnv(t *testing.T) {
	t.Helper()
	vars := []string{
		"DATABASE_URL", "HTTP_PORT", "GRPC_PORT", "METRICS_PORT",
		"NATS_URL", "NATS_TOKEN", "NATS_AUTH_TOKEN",
		"VELION_NATS_URL", "VELION_NATS_TOKEN",
		"NATS_SHARED_URL", "NATS_SHARED_TOKEN",
		"AUTH_SERVICE_URL", "USER_SERVICE_URL",
		"SERVICE_NAME",
		"REDIS_HOST", "REDIS_PORT", "REDIS_PASSWORD", "REDIS_DB", "REDIS_ENABLED",
	}
	for _, v := range vars {
		t.Setenv(v, "")
	}
}

func TestLoad_DefaultNATSURL(t *testing.T) {
	clearEnv(t)
	// Provide required DATABASE_URL so Load doesn't error.
	t.Setenv("DATABASE_URL", "postgres://localhost/test")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() returned error: %v", err)
	}
	const want = "nats://controlplane-nats:4222"
	if cfg.NATSURL != want {
		t.Errorf("default NATSURL = %q, want %q", cfg.NATSURL, want)
	}
}

func TestLoad_NATSURLFromEnv(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")
	t.Setenv("NATS_URL", "nats://localhost:4223")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() returned error: %v", err)
	}
	if cfg.NATSURL != "nats://localhost:4223" {
		t.Errorf("NATSURL = %q, want %q", cfg.NATSURL, "nats://localhost:4223")
	}
}

func TestLoad_NATSURLNoDeadHostname(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() returned error: %v", err)
	}
	const dead = "aquatiq-nats-local"
	if containsStr(cfg.NATSURL, dead) {
		t.Errorf("NATSURL contains dead hostname %q: %s", dead, cfg.NATSURL)
	}
}

func TestLoad_MissingDatabaseURL(t *testing.T) {
	clearEnv(t)
	// DATABASE_URL is empty — Load must return an error.
	_, err := Load()
	if err == nil {
		t.Fatal("expected error for missing DATABASE_URL, got nil")
	}
}

func TestLoad_DefaultHTTPPort(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.HTTPPort != 3013 {
		t.Errorf("default HTTPPort = %d, want 3013", cfg.HTTPPort)
	}
}

func TestLoad_HTTPPortFromEnv(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")
	t.Setenv("HTTP_PORT", "9090")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.HTTPPort != 9090 {
		t.Errorf("HTTPPort = %d, want 9090", cfg.HTTPPort)
	}
}

func TestLoad_DefaultGRPCAndMetricsPorts(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.GRPCPort != 9090 {
		t.Errorf("default GRPCPort = %d, want 9090", cfg.GRPCPort)
	}
	if cfg.MetricsPort != 9091 {
		t.Errorf("default MetricsPort = %d, want 9091", cfg.MetricsPort)
	}
}

func TestLoad_GRPCAndMetricsPortsFromEnv(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")
	t.Setenv("GRPC_PORT", "19090")
	t.Setenv("METRICS_PORT", "19091")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.GRPCPort != 19090 {
		t.Errorf("GRPCPort = %d, want 19090", cfg.GRPCPort)
	}
	if cfg.MetricsPort != 19091 {
		t.Errorf("MetricsPort = %d, want 19091", cfg.MetricsPort)
	}
}

func TestLoad_NATSTokenFallbackToAuthToken(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")
	t.Setenv("NATS_AUTH_TOKEN", "secret")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.NATSToken != "secret" {
		t.Errorf("NATSToken = %q, want %q (from NATS_AUTH_TOKEN fallback)", cfg.NATSToken, "secret")
	}
}

func TestLoad_NATSTokenDirectOverride(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")
	t.Setenv("NATS_TOKEN", "direct")
	t.Setenv("NATS_AUTH_TOKEN", "should-be-ignored")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.NATSToken != "direct" {
		t.Errorf("NATSToken = %q, want %q", cfg.NATSToken, "direct")
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
		t.Errorf("NATSSharedURL = %q, want %q", cfg.NATSSharedURL, "nats://velion-nats:4222")
	}
	if cfg.NATSSharedToken != "velion-token" {
		t.Errorf("NATSSharedToken = %q, want %q", cfg.NATSSharedToken, "velion-token")
	}
}

func TestLoad_RedisDisabledByDefault(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.Redis.Enabled {
		t.Error("Redis.Enabled should be false by default")
	}
}

func TestLoad_ServiceNameDefault(t *testing.T) {
	clearEnv(t)
	t.Setenv("DATABASE_URL", "postgres://localhost/test")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}
	if cfg.ServiceName != "org-core" {
		t.Errorf("ServiceName = %q, want %q", cfg.ServiceName, "org-core")
	}
}

// Ensure t.Setenv properly restores on cleanup (Go 1.17+).
var _ = os.Setenv
